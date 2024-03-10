use std::time::Duration;

use crate::settings::Settings;
use crate::store::Store;
use crate::{eventsub::sub_event_client_loop, irc::read_chat_client_loop};
use anyhow::bail;
use log::warn;
use tokio::time::sleep;

const IRC_CONNECT_ADDR: &str = "wss://irc-ws.chat.twitch.tv:443";
const IRC_TIMEOUT_SECS: u64 = 60 * 10;
const EVENT_CONNECT_ADDR: &str = "wss://eventsub.wss.twitch.tv:443/ws";
const EVENT_TIMEOUT_SECS: u64 = 60 * 10;

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
                        loop {
                            match store.update_tokens(&settings.client_id, &settings.client_secret).await {
                                Ok(_) => {
                                    break;
                                },
                                Err(e) => {
                                    warn!("error {}: try to reconnect.", e);
                                    sleep(Duration::from_secs(30)).await;
                                }
                            }
                        }
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
                        loop {
                            match store.update_tokens(&settings.client_id, &settings.client_secret).await {
                                Ok(_) => {
                                    break;
                                },
                                Err(e) => {
                                    warn!("error {}: try to reconnect.", e);
                                    sleep(Duration::from_secs(30)).await;
                                }
                            }
                        }
                        chat_abort_handle.abort();
                    },
                    Err(e) => bail!(e)
                }
            },
        };
    }
}
