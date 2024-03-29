use crate::api;
use crate::store::Store;
use anyhow::bail;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub async fn ban_bots(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let my_user_id = store.user_id(username, client_id).await?;
    let bot_names = get_bots_list().await?;
    for bot_name in bot_names {
        match api::get_user(&bot_name, store.access_token(), client_id).await {
            Ok(user) => {
                if !user.data.is_empty() {
                    info!("ban {}: {}", bot_name, user.data[0].id);
                    match api::ban_user(
                        &my_user_id,
                        &user.data[0].id,
                        store.access_token(),
                        client_id,
                    )
                    .await
                    {
                        Ok(response) => {
                            info!("banned {}: {} {}", bot_name, user.data[0].id, response)
                        }
                        Err(err) => {
                            if err.status() == Some(reqwest::StatusCode::BAD_REQUEST) {
                                warn!("failed to ban {}: {}", user.data[0].id, err);
                            } else {
                                bail!(err);
                            }
                        }
                    };
                } else {
                    warn!("{} has no entry", bot_name);
                }
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    warn!("username {}: {}", bot_name, err);
                } else {
                    bail!(err);
                }
            }
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct BotInfo {
    name: String,
    number: i64,
    time: i64,
}

#[derive(Serialize, Deserialize)]
struct BotList {
    bots: Vec<BotInfo>,
    _total: i64,
}

async fn get_bots_list() -> Result<Vec<String>, reqwest::Error> {
    let (all_bots_res, white_bots_res) = tokio::join!(
        reqwest::Client::new()
            .get("https://api.twitchinsights.net/v1/bots/all")
            .send()
            .await?
            .json::<BotList>(),
        reqwest::Client::new()
            .get("https://mreliasen.github.io/twitch-bot-list/whitelist.json")
            .send()
            .await?
            .json::<Vec<String>>()
    );
    let all_bots = all_bots_res?;
    let white_bots = white_bots_res?;
    let black_bot_names = all_bots
        .bots
        .iter()
        .map(|n| n.name.clone())
        .filter(|b| !white_bots.contains(b))
        .collect::<Vec<_>>();
    Ok(black_bot_names)
}
