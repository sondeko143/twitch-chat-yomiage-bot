use crate::api::{desired_subscriptions, sub_event};
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
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
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
) -> Result<(), EventSubError> {
    info!("connect event sub");
    let (mut ws_stream, _) = connect_async(url)
        .instrument(tracing::info_span!("event_connect"))
        .await?;
    while let Ok(Some(msg)) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_sec),
        ws_stream.next(),
    )
    .await
    {
        let msg = msg?;
        if let Err(e) = process_message(
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
        .await
        {
            match e {
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
            }
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
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),
    #[error(transparent)]
    SerializeError(#[from] serde_json::Error),
    #[error(transparent)]
    RequestError(#[from] reqwest::Error),
    #[error(transparent)]
    VstcError(#[from] vstc::VstcError),
}

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
) -> Result<(), MessageError> {
    if msg.is_ping() {
        debug!("ping");
        let data = msg.into_data();
        let item = Message::Pong(data);
        ws_stream.send(item).await?;
        Ok(())
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
                subscribe_all(
                    broadcaster_id,
                    user_id,
                    session_id.as_str(),
                    access_token,
                    client_id,
                )
                .instrument(tracing::info_span!("event_subscribe"))
                .await?;
                crate::profiling::mark_ready(crate::profiling::Component::Event);
                Ok(())
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
                    return Ok(());
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
                Ok(())
            }
            _ => {
                debug!("received {}", msg_str);
                Ok(())
            }
        }
    } else {
        Ok(())
    }
}

/// 購読を全種別について試みる。失敗した種別はログに出して他を続ける。
/// 401 だけは、トークン更新つきの再接続で自己回復させるためエラーとして返す。
async fn subscribe_all(
    broadcaster_id: &str,
    user_id: &str,
    session_id: &str,
    access_token: &str,
    client_id: &str,
) -> Result<(), reqwest::Error> {
    let mut unauthorized = None;
    for sub in desired_subscriptions(broadcaster_id, user_id, session_id) {
        if let Err(e) = sub_event(&sub, access_token, client_id).await {
            warn!("failed to subscribe {}: {}", sub.type_, e);
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                unauthorized.get_or_insert(e);
            }
        }
    }
    unauthorized.map_or(Ok(()), Err)
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
