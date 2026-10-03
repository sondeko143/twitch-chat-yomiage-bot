use crate::api::{desired_subscriptions, sub_event};
use crate::feed::LinkStatus;
use crate::notification::render_speech;
use crate::settings::NotificationSpeech;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use serde::Deserialize;
use thiserror::Error;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::Instrument;
use url::Url;

#[derive(Error, Debug)]
pub enum EventSubError {
    #[error("connection error")]
    MessageConnectionError,
    #[error("session reconnect")]
    SessionReconnect { reconnect_url: String },
    #[error(transparent)]
    ConnectionError(Box<tokio_tungstenite::tungstenite::Error>),
}

impl From<tokio_tungstenite::tungstenite::Error> for EventSubError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::ConnectionError(Box::new(e))
    }
}

/// EventSub で受けた通知。購読していない種別も含め、event の JSON をそのまま持つ。
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationEvent {
    pub subscription_type: String,
    pub event: serde_json::Value,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

/// 受けた通知を外（モニタ等）へ渡す口。受け側が閉じていても読み上げは止めない。
pub type NotificationSink = tokio::sync::mpsc::UnboundedSender<NotificationEvent>;

#[allow(clippy::too_many_arguments)]
pub async fn sub_event_client_loop(
    url: Url,
    access_token: String,
    broadcaster_id: String,
    user_id: String,
    client_id: String,
    address: String,
    operations: Vec<String>,
    notification_speech: Vec<NotificationSpeech>,
    timeout_sec: u64,
    sink: Option<NotificationSink>,
    // 接続状態の報告。このループが終わる（abort を含む）と捨てられ disconnected になる
    status: Option<LinkStatus>,
) -> Result<(), EventSubError> {
    info!("connect event sub");
    let (mut ws_stream, _) = connect_async(url.as_str())
        .instrument(tracing::info_span!("event_connect"))
        .await?;
    // connected にするのは session_welcome を受けて購読処理を終えたとき
    while let Ok(Some(msg)) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_sec),
        ws_stream.next(),
    )
    .await
    {
        let msg = msg?;
        let processed = process_message(
            &mut ws_stream,
            msg,
            &address,
            &operations,
            &broadcaster_id,
            &user_id,
            &access_token,
            &client_id,
            &notification_speech,
            sink.as_ref(),
        )
        .await;
        match processed {
            Ok(Some(failed_subscriptions)) => {
                if let Some(status) = &status {
                    status.subscribed(failed_subscriptions);
                }
            }
            Ok(None) => {}
            Err(e) => match e {
                MessageError::SessionReconnect { reconnect_url } => {
                    warn!("session reconnect {}: try to reconnect.", reconnect_url);
                    return Err(EventSubError::SessionReconnect { reconnect_url });
                }
                MessageError::ConnectionError(e) => {
                    warn!("connection error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::SerializeError(e) => {
                    warn!("msg serialization error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::RequestError(e) => {
                    warn!("msg request error {}: try to reconnect.", e);
                    return Err(EventSubError::MessageConnectionError);
                }
                MessageError::VstcError(e) => {
                    warn!("vstc error {}: ignore it.", e);
                }
            },
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct EventSubMessage {
    metadata: Metadata,
    payload: Payload,
}

#[derive(Deserialize)]
struct Metadata {
    message_type: String,
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Payload {
    session: Option<Session>,
    event: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    reconnect_url: Option<String>,
}

#[derive(Error, Debug)]
enum MessageError {
    #[error("session reconnect")]
    SessionReconnect { reconnect_url: String },
    #[error(transparent)]
    ConnectionError(Box<tokio_tungstenite::tungstenite::Error>),
    #[error(transparent)]
    SerializeError(#[from] serde_json::Error),
    #[error(transparent)]
    RequestError(#[from] reqwest::Error),
    #[error(transparent)]
    VstcError(#[from] vstc::VstcError),
}

impl From<tokio_tungstenite::tungstenite::Error> for MessageError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::ConnectionError(Box::new(e))
    }
}

/// 1 件のメッセージを処理する。`session_welcome` で購読処理を終えたときだけ、
/// 購読に失敗した種別を `Some` で返す。
#[allow(clippy::too_many_arguments)]
async fn process_message(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg: Message,
    address: &str,
    operations: &[String],
    broadcaster_id: &str,
    user_id: &str,
    access_token: &str,
    client_id: &str,
    notification_speech: &[NotificationSpeech],
    sink: Option<&NotificationSink>,
) -> Result<Option<Vec<String>>, MessageError> {
    if msg.is_ping() {
        debug!("ping");
        let data = msg.into_data();
        let item = Message::Pong(data);
        ws_stream.send(item).await?;
        Ok(None)
    } else if msg.is_text() || msg.is_binary() {
        let msg_str = msg.into_text()?;
        let event_msg: EventSubMessage = serde_json::from_str(&msg_str)?;
        match event_msg.metadata.message_type.as_str() {
            "session_welcome" => {
                let session_id = match event_msg.payload.session {
                    Some(s) => s.id,
                    None => String::from(""),
                };
                info!("session welcome {}", session_id);
                let failed = subscribe_all(
                    broadcaster_id,
                    user_id,
                    session_id.as_str(),
                    access_token,
                    client_id,
                )
                .instrument(tracing::info_span!("event_subscribe"))
                .await?;
                crate::profiling::mark_ready(crate::profiling::Component::Event);
                Ok(Some(failed))
            }
            "session_reconnect" => {
                let reconnect_url = match event_msg.payload.session {
                    Some(s) => s.reconnect_url.unwrap_or(String::from("")),
                    None => String::from(""),
                };
                info!("reconnect to {}", reconnect_url);
                Err(MessageError::SessionReconnect { reconnect_url })
            }
            "notification" => {
                let Some((notification, text)) =
                    interpret_notification(event_msg, notification_speech)
                else {
                    return Ok(None);
                };
                info!(
                    "received {} notification {}",
                    notification.subscription_type, notification.event
                );
                if let Some(sink) = sink {
                    // 受け側が閉じていても読み上げには影響させない
                    let _ = sink.send(notification);
                }
                if let Some(text) = text {
                    vstc::process_command(address, operations, text, None, None, None).await?;
                }
                Ok(None)
            }
            _ => {
                debug!("received {}", msg_str);
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// 購読を全種別について試み、失敗した種別を返す。失敗しても他の種別は続ける。
async fn subscribe_all(
    broadcaster_id: &str,
    user_id: &str,
    session_id: &str,
    access_token: &str,
    client_id: &str,
) -> Result<Vec<String>, reqwest::Error> {
    let mut results = Vec::new();
    for sub in desired_subscriptions(broadcaster_id, user_id, session_id) {
        let res = sub_event(&sub, access_token, client_id).await.map(drop);
        results.push((sub.type_, res));
    }
    failed_subscriptions(results)
}

/// 種別ごとの購読結果から、失敗した種別を集める。失敗はログに出す。
/// 409（同じ購読が既にある）は購読できているので失敗に数えない。
/// 401 だけは、トークン更新つきの再接続で自己回復させるためエラーとして返す。
fn failed_subscriptions(
    results: Vec<(&str, Result<(), reqwest::Error>)>,
) -> Result<Vec<String>, reqwest::Error> {
    let mut failed = Vec::new();
    let mut unauthorized = None;
    for (type_, res) in results {
        let Err(e) = res else {
            continue;
        };
        if e.status() == Some(reqwest::StatusCode::CONFLICT) {
            info!("already subscribed {}: {}", type_, e);
            continue;
        }
        warn!("failed to subscribe {}: {}", type_, e);
        failed.push(type_.to_string());
        if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
            unauthorized.get_or_insert(e);
        }
    }
    unauthorized.map_or(Ok(failed), Err)
}

/// 通知メッセージから、外へ渡す通知と読み上げ文（テンプレートが一致したときだけ）を作る。
fn interpret_notification(
    msg: EventSubMessage,
    notification_speech: &[NotificationSpeech],
) -> Option<(NotificationEvent, Option<String>)> {
    let subscription_type = msg.metadata.subscription_type?;
    let event = msg.payload.event.unwrap_or(serde_json::Value::Null);
    let text = render_speech(notification_speech, &subscription_type, &event);
    let notification = NotificationEvent {
        subscription_type,
        event,
        received_at: chrono::Utc::now(),
    };
    Some((notification, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(subscription_type: &str, event: serde_json::Value) -> EventSubMessage {
        serde_json::from_value(json!({
            "metadata": {"message_type": "notification", "subscription_type": subscription_type},
            "payload": {"event": event},
        }))
        .unwrap()
    }

    fn speech(type_: &str, notice_type: Option<&str>, template: &str) -> NotificationSpeech {
        NotificationSpeech {
            type_: type_.to_string(),
            notice_type: notice_type.map(str::to_string),
            template: template.to_string(),
        }
    }

    #[test]
    fn follow_is_spoken_only_when_a_template_matches() {
        let event = json!({"user_name": "taro"});
        let templates = [speech(
            "channel.follow",
            None,
            "{user_name}さん、フォローありがとう",
        )];

        let (n, text) =
            interpret_notification(msg("channel.follow", event.clone()), &templates).unwrap();
        assert_eq!(text.as_deref(), Some("taroさん、フォローありがとう"));
        assert_eq!(n.event, event);

        let (_, text) = interpret_notification(msg("channel.follow", event), &[]).unwrap();
        assert_eq!(text, None);
    }

    #[test]
    fn raid_uses_its_template() {
        let event = json!({"from_broadcaster_user_name": "hanako", "viewers": 42});
        let templates = [speech(
            "channel.raid",
            None,
            "{from_broadcaster_user_name}さんから{viewers}人のレイド",
        )];

        let (_, text) = interpret_notification(msg("channel.raid", event), &templates).unwrap();

        assert_eq!(text.as_deref(), Some("hanakoさんから42人のレイド"));
    }

    #[test]
    fn chat_notification_sub_picks_the_notice_type_template() {
        let event = json!({
            "notice_type": "sub",
            "chatter_user_name": "jiro",
            "sub": {"sub_tier": "1000"},
        });
        let templates = [
            speech("channel.chat.notification", None, "other"),
            speech(
                "channel.chat.notification",
                Some("sub"),
                "{chatter_user_name}がサブスク",
            ),
        ];

        let (_, text) =
            interpret_notification(msg("channel.chat.notification", event), &templates).unwrap();

        assert_eq!(text.as_deref(), Some("jiroがサブスク"));
    }

    #[test]
    fn unsubscribed_types_still_reach_the_outlet_with_their_event() {
        let event = json!({"anything": [1, 2, 3]});

        let (n, text) = interpret_notification(msg("channel.cheer", event.clone()), &[]).unwrap();

        assert_eq!(n.subscription_type, "channel.cheer");
        assert_eq!(n.event, event);
        assert_eq!(text, None);
    }

    /// `status` の HTTP 応答から作った reqwest のエラー。
    fn http_error(status: u16) -> reqwest::Error {
        reqwest::Response::from(
            axum::http::Response::builder()
                .status(status)
                .body(String::new())
                .unwrap(),
        )
        .error_for_status()
        .unwrap_err()
    }

    #[test]
    fn failed_subscription_types_are_collected_and_the_rest_continue() {
        let failed = failed_subscriptions(vec![
            ("channel.follow", Err(http_error(403))),
            ("channel.raid", Ok(())),
            ("channel.chat.notification", Err(http_error(500))),
        ]);
        assert_eq!(
            failed.unwrap(),
            ["channel.follow", "channel.chat.notification"]
        );
    }

    /// 409 は同じ購読が既にあるという応答で、購読はできている。
    #[test]
    fn already_subscribed_is_not_a_failure() {
        let failed = failed_subscriptions(vec![("channel.raid", Err(http_error(409)))]);
        assert!(failed.unwrap().is_empty());
    }

    /// 401 はトークン更新つきの再接続で直すため、エラーとして返す。
    #[test]
    fn unauthorized_is_returned_as_an_error() {
        let failed = failed_subscriptions(vec![
            ("channel.follow", Err(http_error(403))),
            ("channel.raid", Err(http_error(401))),
        ]);
        assert_eq!(
            failed.unwrap_err().status(),
            Some(reqwest::StatusCode::UNAUTHORIZED)
        );
    }

    /// WebSocket が繋がっただけでは `connecting` のまま。`session_welcome` と購読を
    /// 終えるまで `connected` にしない。
    #[tokio::test]
    async fn stays_connecting_until_session_welcome() {
        use crate::feed::{ConnectionState, FeedHub, Link};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            accepted_tx.send(()).unwrap();
            // 何も送らず、クライアントが閉じるまで待つ
            while let Some(Ok(_)) = ws.next().await {}
        });
        let hub = FeedHub::new(1);
        let client = tokio::spawn(sub_event_client_loop(
            Url::parse(&format!("ws://{addr}")).unwrap(),
            "token".into(),
            "b1".into(),
            "u1".into(),
            "cid".into(),
            "http://127.0.0.1:1".into(),
            Vec::new(),
            Vec::new(),
            30,
            None,
            Some(LinkStatus::connecting(hub.clone(), Link::EventSub)),
        ));

        accepted_rx.await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            hub.subscribe().0.status.eventsub,
            ConnectionState::Connecting
        );

        client.abort();
        let _ = client.await;
        assert_eq!(
            hub.subscribe().0.status.eventsub,
            ConnectionState::Disconnected
        );
        server.abort();
    }

    #[test]
    fn notification_without_subscription_type_is_ignored() {
        let m: EventSubMessage = serde_json::from_value(json!({
            "metadata": {"message_type": "notification"},
            "payload": {"event": {}},
        }))
        .unwrap();

        assert!(interpret_notification(m, &[]).is_none());
    }
}
