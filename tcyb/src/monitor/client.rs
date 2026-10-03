//! 配信口へのクライアント。切れても間隔を伸ばしながら再接続し続ける。

use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// 再接続の最初の待ち時間。
const INITIAL_DELAY: Duration = Duration::from_millis(500);
/// 再接続の待ち時間の上限。
const MAX_DELAY: Duration = Duration::from_secs(10);
/// 接続の確立（TCP + WebSocket ハンドシェイク）を待つ上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// クライアントから UI へ伝える出来事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedEvent {
    Connecting,
    Connected,
    /// 受信したテキストフレーム（解釈は UI 側で行う）。
    Text(String),
    /// 接続できなかった・切れた。`retry_in` 後に再接続する。
    Disconnected {
        error: String,
        retry_in: Duration,
    },
}

/// 再接続の待ち時間。失敗のたびに倍にし、[`MAX_DELAY`] で頭打ちにする。接続できたら戻す。
#[derive(Debug)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            next: INITIAL_DELAY,
        }
    }
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(MAX_DELAY);
        delay
    }

    pub fn reset(&mut self) {
        self.next = INITIAL_DELAY;
    }
}

/// UI が終了して受け手がいなくなった。
struct Closed;

/// `url` へ接続し続け、出来事を `tx` へ送る。`tx` の受け手がいなくなったら終わる。
pub async fn run(url: String, tx: mpsc::UnboundedSender<FeedEvent>) {
    let mut backoff = Backoff::default();
    loop {
        let error = match connect_once(&url, &tx, &mut backoff).await {
            Ok(error) => error,
            Err(Closed) => return,
        };
        let retry_in = backoff.next_delay();
        if tx
            .send(FeedEvent::Disconnected { error, retry_in })
            .is_err()
        {
            return;
        }
        tokio::time::sleep(retry_in).await;
    }
}

/// 1 回接続して、切れるまで受信を流す。切れた（繋がらなかった）理由を返す。
async fn connect_once(
    url: &str,
    tx: &mpsc::UnboundedSender<FeedEvent>,
    backoff: &mut Backoff,
) -> Result<String, Closed> {
    send(tx, FeedEvent::Connecting)?;
    let mut ws = match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(url)).await {
        Ok(Ok((ws, _))) => ws,
        Ok(Err(e)) => return Ok(e.to_string()),
        Err(_) => return Ok("接続がタイムアウトしました".into()),
    };
    backoff.reset();
    send(tx, FeedEvent::Connected)?;
    while let Some(msg) = ws.next().await {
        match msg {
            Ok(Message::Text(text)) => send(tx, FeedEvent::Text(text))?,
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => return Ok(e.to_string()),
        }
    }
    Ok("read-chat が接続を閉じました".into())
}

fn send(tx: &mpsc::UnboundedSender<FeedEvent>, event: FeedEvent) -> Result<(), Closed> {
    tx.send(event).map_err(|_| Closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::{FeedHub, FeedMessage};
    use crate::feed_server;

    #[test]
    fn backoff_doubles_up_to_the_cap_and_resets() {
        let mut backoff = Backoff::default();
        let delays: Vec<_> = (0..7).map(|_| backoff.next_delay()).collect();
        let ms = |n| Duration::from_millis(n);
        assert_eq!(
            delays,
            [
                ms(500),
                ms(1000),
                ms(2000),
                ms(4000),
                ms(8000),
                ms(10_000),
                ms(10_000)
            ]
        );
        backoff.reset();
        assert_eq!(backoff.next_delay(), ms(500));
    }

    fn url(port: u16) -> String {
        format!("ws://127.0.0.1:{port}{}", feed_server::FEED_PATH)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<FeedEvent>) -> FeedEvent {
        tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out waiting for a feed event")
            .expect("client ended")
    }

    /// 空いているポートを 1 つ選ぶ（選んだ直後は誰も待ち受けていない）。
    async fn free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    async fn next_text(rx: &mut mpsc::UnboundedReceiver<FeedEvent>) -> String {
        loop {
            if let FeedEvent::Text(text) = next(rx).await {
                return text;
            }
        }
    }

    #[tokio::test]
    async fn receives_the_snapshot_from_the_feed_server() {
        let hub = FeedHub::new(10);
        let listener = feed_server::bind(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(feed_server::serve(listener, hub));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client = tokio::spawn(run(url(port), tx));

        assert_eq!(next(&mut rx).await, FeedEvent::Connecting);
        assert_eq!(next(&mut rx).await, FeedEvent::Connected);
        assert!(matches!(
            FeedMessage::from_json(&next_text(&mut rx).await).unwrap(),
            FeedMessage::Snapshot(_)
        ));
        client.abort();
        server.abort();
    }

    #[tokio::test]
    async fn reports_disconnected_and_follows_a_server_that_starts_and_stops() {
        let port = free_port().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client = tokio::spawn(run(url(port), tx));

        // read-chat が動いていない
        assert_eq!(next(&mut rx).await, FeedEvent::Connecting);
        match next(&mut rx).await {
            FeedEvent::Disconnected { retry_in, .. } => assert_eq!(retry_in, INITIAL_DELAY),
            other => panic!("expected Disconnected, got {other:?}"),
        }

        // read-chat が後から起動し、1 通送って止まる
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            futures_util::SinkExt::send(&mut ws, Message::Text("hello".into()))
                .await
                .unwrap();
            // listener ごと落とす（プロセス終了相当）
        });

        assert_eq!(next_text(&mut rx).await, "hello");
        server.await.unwrap();

        // 止まると未接続になり、接続できていたので再試行は最初の間隔から始まる
        let retry_in = loop {
            if let FeedEvent::Disconnected { retry_in, .. } = next(&mut rx).await {
                break retry_in;
            }
        };
        assert_eq!(retry_in, INITIAL_DELAY);
        client.abort();
    }

    #[tokio::test]
    async fn ends_when_the_receiver_is_gone() {
        let port = free_port().await;
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        tokio::time::timeout(Duration::from_secs(10), run(url(port), tx))
            .await
            .expect("client should stop when nobody listens");
    }
}
