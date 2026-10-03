use std::time::Duration;

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
async fn resolve_broadcaster_id(store: &mut Store, settings: &Settings) -> anyhow::Result<String> {
    crate::chat::resolve_channel_user_id(
        store,
        &settings.channel,
        &settings.client_id,
        &settings.client_secret,
    )
    .instrument(tracing::info_span!("channel_id_fetch"))
    .await
}

#[allow(clippy::too_many_arguments)]
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
    let broadcaster_id = resolve_broadcaster_id(&mut *store.lock().await, settings).await?;
    loop {
        let access_token = store.lock().await.access_token().to_string();
        let chat_t = tokio::spawn(read_chat_client_loop(
            irc_url.clone(),
            access_token.clone(),
            settings.username.clone(),
            settings.channel.clone(),
            settings.speech_address.clone(),
            settings.operations.clone(),
            IRC_TIMEOUT_SECS,
            settings.translate_command.clone(),
        ));
        let sub_event_t = tokio::spawn(sub_event_client_loop(
            event_url.clone(),
            access_token.clone(),
            broadcaster_id.clone(),
            user_id.clone(),
            settings.client_id.clone(),
            settings.speech_address.clone(),
            settings.operations.clone(),
            settings.notification_speech.clone(),
            EVENT_TIMEOUT_SECS,
            // 通知の配信口（ハブ）はまだ無い。
            None,
        ));
        let chat_abort_handle = chat_t.abort_handle();
        let sub_event_abort_handle = sub_event_t.abort_handle();
        tokio::select! {
            r = chat_t => {
                match r {
                    Ok(Ok(_)) => {
                        warn!("connection closed.");
                        sub_event_abort_handle.abort();
                    },
                    Ok(Err(e)) => {
                        warn!("error {}: try to reconnect.", e);
                        refresh_shared_tokens(&store, settings).await?;
                        sub_event_abort_handle.abort();
                    },
                    Err(e) => bail!(e)
                }
            },
            r = sub_event_t => {
                match r {
                    Ok(Ok(_)) => {
                        warn!("connection closed.");
                        chat_abort_handle.abort();
                    },
                    Ok(Err(e)) => {
                        warn!("error {}: try to reconnect.", e);
                        refresh_shared_tokens(&store, settings).await?;
                        chat_abort_handle.abort();
                    },
                    Err(e) => bail!(e)
                }
            },
            _ = crate::profiling::wait_for_shutdown() => {
                warn!("profiling: startup complete, shutting down");
                chat_abort_handle.abort();
                sub_event_abort_handle.abort();
                return Ok(());
            },
        };
    }
}
