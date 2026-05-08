use std::time::Duration;

use crate::settings::Settings;
use crate::store::{Store, StoreError};
use crate::{eventsub::sub_event_client_loop, irc::read_chat_client_loop};
use anyhow::bail;
use log::warn;
use tokio::time::sleep;

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
        match store.update_tokens(client_id, client_secret).await {
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

#[allow(clippy::too_many_arguments)]
pub async fn yomiage(settings: &Settings) -> anyhow::Result<()> {
    let irc_url = url::Url::parse(IRC_CONNECT_ADDR)?;
    let event_url = url::Url::parse(EVENT_CONNECT_ADDR)?;
    let mut store = Store::new(&settings.db_dir, &settings.db_name)?;
    let user_id = store
        .user_id(&settings.username, &settings.client_id)
        .await?;
    loop {
        let access_token = store.access_token();
        let chat_t = tokio::spawn(read_chat_client_loop(
            irc_url.clone(),
            String::from(access_token),
            settings.username.clone(),
            settings.channel.clone(),
            settings.speech_address.clone(),
            settings.operations.clone(),
            IRC_TIMEOUT_SECS,
            settings.translate_command.clone(),
        ));
        let sub_event_t = tokio::spawn(sub_event_client_loop(
            event_url.clone(),
            String::from(access_token),
            user_id.clone(),
            settings.client_id.clone(),
            settings.speech_address.clone(),
            settings.operations.clone(),
            settings.greeting_template.clone(),
            EVENT_TIMEOUT_SECS,
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
                        refresh_tokens_with_backoff(
                            &mut store,
                            &settings.client_id,
                            &settings.client_secret,
                        )
                        .await?;
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
                        refresh_tokens_with_backoff(
                            &mut store,
                            &settings.client_id,
                            &settings.client_secret,
                        )
                        .await?;
                        chat_abort_handle.abort();
                    },
                    Err(e) => bail!(e)
                }
            },
        };
    }
}
