use std::time::Duration;

use crate::eventsub::NotificationSink;
use crate::feed::{FeedHub, Link, LinkStatus, NotificationLine};
use crate::irc::ChatOutputs;
use crate::settings::Settings;
use crate::store::{SharedStore, Store, StoreError};
use crate::{eventsub::sub_event_client_loop, irc::read_chat_client_loop};
use anyhow::bail;
use log::warn;
use tokio::time::sleep;
use tracing::Instrument;

const IRC_CONNECT_ADDR: &str = "wss://irc-ws.chat.twitch.tv:443";
const IRC_TIMEOUT_SECS: u64 = 180;
const EVENT_CONNECT_ADDR: &str = "wss://eventsub.wss.twitch.tv:443/ws";
const EVENT_TIMEOUT_SECS: u64 = 30;
const MAX_TOKEN_REFRESH_RETRIES: u32 = 5;
const TOKEN_REFRESH_INITIAL_BACKOFF_SECS: u64 = 5;
const TOKEN_REFRESH_MAX_BACKOFF_SECS: u64 = 300;

async fn refresh_tokens_with_backoff(
    store: &mut Store,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut attempt = 0u32;
    let mut backoff = TOKEN_REFRESH_INITIAL_BACKOFF_SECS;
    loop {
        match store
            .update_tokens(client_id, client_secret)
            .instrument(tracing::info_span!("token_refresh"))
            .await
        {
            Ok(_) => return Ok(()),
            Err(e) => {
                let is_permanent = matches!(
                    &e,
                    StoreError::RequestError(err)
                        if matches!(err.status().map(|s| s.as_u16()), Some(400) | Some(401))
                );
                if is_permanent {
                    bail!(
                        "token refresh failed permanently ({}); please re-authenticate via `tcyb auth-code`",
                        e
                    );
                }
                attempt += 1;
                if attempt >= MAX_TOKEN_REFRESH_RETRIES {
                    bail!(
                        "token refresh exceeded {} retries: {}",
                        MAX_TOKEN_REFRESH_RETRIES,
                        e
                    );
                }
                warn!(
                    "token refresh failed (attempt {}/{}): {}; retry in {}s",
                    attempt, MAX_TOKEN_REFRESH_RETRIES, e, backoff
                );
                sleep(Duration::from_secs(backoff)).await;
                backoff = backoff
                    .saturating_mul(2)
                    .min(TOKEN_REFRESH_MAX_BACKOFF_SECS);
            }
        }
    }
}

/// 共有ストアのロックを取ってからリフレッシュする（視聴者一覧の周期取得と直列化される）。
async fn refresh_shared_tokens(store: &SharedStore, settings: &Settings) -> anyhow::Result<()> {
    refresh_tokens_with_backoff(
        &mut *store.lock().await,
        &settings.client_id,
        &settings.client_secret,
    )
    .await
}

/// follow / raid / chat.notification は bot ではなく設定の channel（配信者）が対象。
async fn resolve_broadcaster_id(
    store: &SharedStore,
    settings: &Settings,
) -> anyhow::Result<String> {
    crate::chat::resolve_shared_channel_user_id(
        store,
        &settings.channel,
        &settings.client_id,
        &settings.client_secret,
    )
    .instrument(tracing::info_span!("channel_id_fetch"))
    .await
}

/// 監視用の処理（ハブ・配信口・通知の橋渡し・視聴者一覧の周期取得）。`read-chat` の 1 回の
/// 実行につき 1 度だけ起動し、IRC / EventSub の再接続をまたいで生き続ける。
///
/// 各処理は別タスクで動くので、失敗や panic が読み上げのループへ伝わらない。
/// 捨てると全タスクを止める。
struct Monitor {
    hub: FeedHub,
    sink: NotificationSink,
    _tasks: AbortOnDrop,
}

impl Monitor {
    /// `monitor.enabled = false` のときは何も起動せず `None`。
    async fn start(
        settings: &Settings,
        store: &SharedStore,
        broadcaster_id: &str,
        user_id: &str,
    ) -> Option<Self> {
        let monitor = &settings.monitor;
        if !monitor.enabled {
            return None;
        }
        let hub = FeedHub::new(monitor.history_size);
        let (sink, bridge) = notification_bridge(hub.clone());
        let poll = tokio::spawn(crate::chat::chatters_poll_loop(
            hub.clone(),
            store.clone(),
            broadcaster_id.to_string(),
            user_id.to_string(),
            // 0 秒は API を詰めて叩くことになるので 1 秒に切り上げる
            Duration::from_secs(monitor.chatters_interval_secs.max(1)),
            settings.client_id.clone(),
            settings.client_secret.clone(),
        ));
        let mut tasks = vec![bridge.abort_handle(), poll.abort_handle()];
        // bind に失敗しても読み上げは続ける（警告は start が出す）
        if let Some(server) = crate::feed_server::start(monitor.port, hub.clone()).await {
            tasks.push(server.abort_handle());
        }
        Some(Self {
            hub,
            sink,
            _tasks: AbortOnDrop(tasks),
        })
    }
}

/// 捨てたときに抱えているタスクを止める。
struct AbortOnDrop(Vec<tokio::task::AbortHandle>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// EventSub の通知の口をハブへつなぐ。全ての送り口が捨てられるとタスクは終わる。
fn notification_bridge(hub: FeedHub) -> (NotificationSink, tokio::task::JoinHandle<()>) {
    let (sink, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            hub.send_notification(NotificationLine::from(event));
        }
    });
    (sink, task)
}

/// 接続ループが終わった後、張り直す前の処理。エラーで終わったならトークンを更新する。
async fn prepare_reconnect<E: std::fmt::Display>(
    ended: Result<Result<(), E>, tokio::task::JoinError>,
    store: &SharedStore,
    settings: &Settings,
) -> anyhow::Result<()> {
    match ended {
        Ok(Ok(())) => warn!("connection closed."),
        Ok(Err(e)) => {
            warn!("error {}: try to reconnect.", e);
            refresh_shared_tokens(store, settings).await?;
        }
        Err(e) => bail!(e),
    }
    Ok(())
}

pub async fn yomiage(settings: &Settings) -> anyhow::Result<()> {
    let irc_url = url::Url::parse(IRC_CONNECT_ADDR)?;
    let event_url = url::Url::parse(EVENT_CONNECT_ADDR)?;
    // トークンの持ち主はここ 1 つ。視聴者一覧の周期取得も同じハンドルを共有する。
    let store: SharedStore = {
        let _span = tracing::info_span!("store_new").entered();
        std::sync::Arc::new(tokio::sync::Mutex::new(Store::new(
            &settings.db_dir,
            &settings.db_name,
        )?))
    };
    let user_id = store
        .lock()
        .await
        .user_id(&settings.username, &settings.client_id)
        .instrument(tracing::info_span!("user_id_fetch"))
        .await?;
    let broadcaster_id = resolve_broadcaster_id(&store, settings).await?;
    // 再接続のループの外で 1 度だけ起動する（ハブの保持内容を再接続で失わない）
    let monitor = Monitor::start(settings, &store, &broadcaster_id, &user_id).await;
    let hub = monitor.as_ref().map(|m| m.hub.clone());
    loop {
        let access_token = store.lock().await.access_token().to_string();
        let mut chat_t = tokio::spawn(read_chat_client_loop(
            irc_url.clone(),
            access_token.clone(),
            settings.username.clone(),
            settings.channel.clone(),
            ChatOutputs {
                speech_address: settings.speech_address.clone(),
                operations: settings.operations.clone(),
                translate_command: settings.translate_command.clone(),
                hub: hub.clone(),
            },
            IRC_TIMEOUT_SECS,
        ));
        let mut sub_event_t = tokio::spawn(sub_event_client_loop(
            event_url.clone(),
            access_token.clone(),
            broadcaster_id.clone(),
            user_id.clone(),
            settings.client_id.clone(),
            settings.speech_address.clone(),
            settings.operations.clone(),
            settings.notification_speech.clone(),
            EVENT_TIMEOUT_SECS,
            monitor.as_ref().map(|m| m.sink.clone()),
            hub.clone()
                .map(|hub| LinkStatus::connecting(hub, Link::EventSub)),
        ));
        // 片方が終わったらもう片方を止め、止まり切るまで待ってから張り直す
        // （古い接続の disconnected が新しい接続の状態を上書きしないように）。
        tokio::select! {
            r = &mut chat_t => {
                sub_event_t.abort();
                let _ = sub_event_t.await;
                prepare_reconnect(r, &store, settings).await?;
            },
            r = &mut sub_event_t => {
                chat_t.abort();
                let _ = chat_t.await;
                prepare_reconnect(r, &store, settings).await?;
            },
            _ = crate::profiling::wait_for_shutdown() => {
                warn!("profiling: startup complete, shutting down");
                chat_t.abort();
                sub_event_t.abort();
                return Ok(());
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventsub::NotificationEvent;
    use crate::feed::{FeedMessage, NotificationLine};

    #[tokio::test]
    async fn notifications_from_the_sink_are_forwarded_to_the_hub() {
        let hub = FeedHub::new(10);
        let (_, mut rx) = hub.subscribe();
        let (sink, bridge) = notification_bridge(hub.clone());
        let event = NotificationEvent {
            subscription_type: "channel.raid".into(),
            event: serde_json::json!({"from_broadcaster_user_name": "X", "viewers": 3}),
            received_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
        };
        sink.send(event.clone()).unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            FeedMessage::Notification(NotificationLine::from(event))
        );
        drop(sink);
        tokio::time::timeout(Duration::from_secs(5), bridge)
            .await
            .expect("bridge ends when every sink is dropped")
            .unwrap();
    }
}
