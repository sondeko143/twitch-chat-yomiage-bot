//! `read-chat` のローカル配信口（`ws://127.0.0.1:<port>/feed`）。
//!
//! 接続直後に `snapshot` を 1 件送り、以降はハブの新着をそのまま送る。受信が追いつかず
//! 取りこぼした接続には、切らずに `snapshot` を送り直す。

use crate::feed::{FeedHub, FeedMessage, FeedRecvError};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use log::{info, warn};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// WebSocket のパス。
pub const FEED_PATH: &str = "/feed";

/// 1 フレームの送信をこれ以上待たない。読まないまま居座る相手で接続タスクが残らないように切る。
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// `127.0.0.1:<port>` だけで待ち受ける（`0.0.0.0` や名前解決はしない）。
pub async fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// 待ち受けを始めて配信口を別タスクで動かす。bind に失敗したら警告して `None`
/// （読み上げは配信口なしで続ける）。
pub async fn start(port: u16, hub: FeedHub) -> Option<JoinHandle<()>> {
    let listener = match bind(port).await {
        Ok(listener) => listener,
        Err(e) => {
            warn!("monitor: failed to listen on 127.0.0.1:{port} ({e}); continue without the feed");
            return None;
        }
    };
    info!("monitor: feed on ws://127.0.0.1:{port}{FEED_PATH}");
    Some(tokio::spawn(async move {
        if let Err(e) = serve(listener, hub).await {
            warn!("monitor: feed server stopped: {e}");
        }
    }))
}

/// `listener` で配信口を動かす。
pub async fn serve(listener: TcpListener, hub: FeedHub) -> std::io::Result<()> {
    let port = listener.local_addr()?.port();
    let state = AppState {
        hub,
        host: format!("127.0.0.1:{port}"),
    };
    let app = Router::new()
        .route(FEED_PATH, get(upgrade))
        .with_state(state);
    axum::serve(listener, app).await
}

#[derive(Clone)]
struct AppState {
    hub: FeedHub,
    /// 受け付ける `Host` ヘッダの値（DNS rebinding 対策）。
    host: String,
}

/// ブラウザ由来（`Origin` あり）や `Host` が違う要求を弾く。`tcyb monitor` は `Origin` を送らず、
/// `Host` は `127.0.0.1:<port>` になる。
fn is_allowed(headers: &HeaderMap, expected_host: &str) -> bool {
    if headers.contains_key(header::ORIGIN) {
        return false;
    }
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == expected_host)
}

async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !is_allowed(&headers, &state.host) {
        warn!("monitor: rejected a feed request (unexpected Origin or Host)");
        return StatusCode::FORBIDDEN.into_response();
    }
    let hub = state.hub;
    ws.on_upgrade(move |socket| stream_feed(socket, hub))
}

/// 1 接続分の配信。相手が切断・close・送信不能になったら終わる。
async fn stream_feed(mut socket: WebSocket, hub: FeedHub) {
    let (snapshot, mut rx) = hub.subscribe();
    info!(
        "monitor: client connected ({} total)",
        hub.subscriber_count()
    );
    if send(&mut socket, FeedMessage::Snapshot(snapshot))
        .await
        .is_ok()
    {
        loop {
            tokio::select! {
                next = rx.recv() => {
                    let msg = match next {
                        Ok(msg) => msg,
                        Err(FeedRecvError::Lagged(n)) => {
                            warn!("monitor: client lagged by {n} messages; resending snapshot");
                            FeedMessage::Snapshot(rx.resync())
                        }
                    };
                    if send(&mut socket, msg).await.is_err() {
                        break;
                    }
                }
                incoming = socket.recv() => match incoming {
                    // ping への pong は axum が返す。それ以外の受信は使わない
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(_)) => {}
                },
            }
        }
    }
    drop(rx);
    info!("monitor: client disconnected");
}

async fn send(socket: &mut WebSocket, msg: FeedMessage) -> Result<(), ()> {
    match tokio::time::timeout(
        SEND_TIMEOUT,
        socket.send(Message::Text(msg.to_json().into())),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::{ChatLine, FeedMessage};
    use futures_util::StreamExt;
    use std::time::Duration;
    use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

    type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    const WAIT: Duration = Duration::from_secs(10);

    fn chat(n: i64) -> ChatLine {
        ChatLine {
            received_at: chrono::DateTime::from_timestamp(n, 0).unwrap(),
            user_login: format!("u{n}"),
            display_name: format!("U{n}"),
            text: format!("msg {n}"),
            color: None,
        }
    }

    /// ポート 0 で配信口を立て、そのポートを返す。
    async fn spawn_server(hub: FeedHub) -> u16 {
        let listener = bind(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, hub));
        port
    }

    async fn connect(port: u16) -> Client {
        connect_async(format!("ws://127.0.0.1:{port}{FEED_PATH}"))
            .await
            .unwrap()
            .0
    }

    async fn connect_with(
        port: u16,
        header: (&'static str, &str),
    ) -> tokio_tungstenite::tungstenite::Error {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut req = format!("ws://127.0.0.1:{port}{FEED_PATH}")
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert(header.0, header.1.parse().unwrap());
        connect_async(req).await.unwrap_err()
    }

    fn assert_forbidden(err: tokio_tungstenite::tungstenite::Error) {
        match err {
            tokio_tungstenite::tungstenite::Error::Http(res) => {
                assert_eq!(res.status(), 403);
            }
            other => panic!("expected an HTTP 403, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_with_an_origin_is_refused() {
        let port = spawn_server(FeedHub::new(5)).await;
        assert_forbidden(connect_with(port, ("Origin", "http://evil.example")).await);
    }

    #[tokio::test]
    async fn request_with_a_wrong_host_is_refused() {
        let port = spawn_server(FeedHub::new(5)).await;
        assert_forbidden(connect_with(port, ("Host", "evil.example:80")).await);
    }

    async fn next_message(client: &mut Client) -> FeedMessage {
        loop {
            let frame = tokio::time::timeout(WAIT, client.next())
                .await
                .expect("a message within the timeout")
                .expect("stream open")
                .unwrap();
            if let Message::Text(text) = frame {
                return FeedMessage::from_json(&text).unwrap();
            }
        }
    }

    async fn wait_until(mut cond: impl FnMut() -> bool) {
        tokio::time::timeout(WAIT, async {
            while !cond() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("condition within the timeout");
    }

    #[tokio::test]
    async fn binds_only_to_loopback() {
        let listener = bind(0).await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip(), std::net::Ipv4Addr::LOCALHOST);
    }

    #[tokio::test]
    async fn start_returns_none_when_the_port_is_taken() {
        let taken = bind(0).await.unwrap();
        let port = taken.local_addr().unwrap().port();
        assert!(start(port, FeedHub::new(1)).await.is_none());
    }

    #[tokio::test]
    async fn client_gets_snapshot_first_then_new_chat() {
        let hub = FeedHub::new(10);
        hub.send_chat(chat(1));
        let port = spawn_server(hub.clone()).await;
        let mut client = connect(port).await;

        match next_message(&mut client).await {
            FeedMessage::Snapshot(snap) => assert_eq!(snap.chats, vec![chat(1)]),
            other => panic!("expected snapshot first, got {other:?}"),
        }
        hub.send_chat(chat(2));
        assert_eq!(next_message(&mut client).await, FeedMessage::Chat(chat(2)));
    }

    #[tokio::test]
    async fn lagging_connection_gets_a_fresh_snapshot_and_stays_open() {
        let hub = FeedHub::new(5);
        let port = spawn_server(hub.clone()).await;
        let mut client = connect(port).await;
        assert!(matches!(
            next_message(&mut client).await,
            FeedMessage::Snapshot(_)
        ));

        // 単一スレッドのランタイムで譲らずに送り続け、配信側を確実に取りこぼさせる
        for n in 0..10_000 {
            hub.send_chat(chat(n));
        }
        match next_message(&mut client).await {
            FeedMessage::Snapshot(snap) => {
                assert_eq!(snap.chats, (9_995..10_000).map(chat).collect::<Vec<_>>());
            }
            other => panic!("expected a resync snapshot, got {other:?}"),
        }
        hub.send_chat(chat(20_000));
        assert_eq!(
            next_message(&mut client).await,
            FeedMessage::Chat(chat(20_000))
        );
    }

    #[tokio::test]
    async fn connection_task_ends_when_the_client_goes_away() {
        let hub = FeedHub::new(5);
        let port = spawn_server(hub.clone()).await;

        let mut closing = connect(port).await;
        next_message(&mut closing).await;
        let mut dropped = connect(port).await;
        next_message(&mut dropped).await;
        wait_until(|| hub.subscriber_count() == 2).await;

        closing.close(None).await.unwrap();
        wait_until(|| hub.subscriber_count() == 1).await;
        // close フレームなしで切る
        drop(dropped);
        wait_until(|| hub.subscriber_count() == 0).await;
    }
}
