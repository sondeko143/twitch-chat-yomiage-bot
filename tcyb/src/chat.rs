use crate::{api, store::Store};
use anyhow::bail;
use log::warn;
use std::{path::Path, time::Duration};

fn format_chatters_line(
    now: chrono::NaiveDateTime,
    chatters: &api::Chatters,
    channel_name: &str,
    username: &str,
) -> String {
    let mut users: Vec<&str> = chatters
        .data
        .iter()
        .map(|c| c.user_login.as_str())
        .filter(|name| *name != channel_name && *name != username)
        .collect();
    users.sort_unstable();
    format!("{},{}", now.format("%Y-%m-%d %H:%M:%S"), users.join(","))
}

async fn with_token_refresh<T, F, Fut>(
    store: &mut Store,
    client_id: &str,
    client_secret: &str,
    mut call: F,
) -> anyhow::Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    loop {
        match call(store.access_token().to_string()).await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                } else {
                    bail!(err);
                }
            }
        }
    }
}

async fn resolve_channel_user_id(
    store: &mut Store,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    let channel_user = with_token_refresh(store, client_id, client_secret, |token| async move {
        api::get_user(channel_name, &token, client_id).await
    })
    .await?;
    if channel_user.data.is_empty() {
        bail!("channel not found");
    }
    Ok(channel_user.data[0].id.clone())
}

async fn chatters_tick(
    store: &mut Store,
    channel_user_id: &str,
    user_id: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let res = with_token_refresh(store, client_id, client_secret, |token| async move {
        api::get_chatters(channel_user_id, user_id, &token, client_id).await
    })
    .await?;
    let now = chrono::Local::now().naive_local();
    println!(
        "{}",
        format_chatters_line(now, &res, channel_name, username)
    );
    Ok(())
}

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
    interval: Option<Duration>,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let user_id = store.user_id(username, client_id).await?;
    let channel_user_id =
        resolve_channel_user_id(&mut store, channel_name, client_id, client_secret).await?;

    let Some(period) = interval else {
        return chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await;
    };

    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await?;
    }
}

pub async fn show_user_info(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let channel_user =
        with_token_refresh(&mut store, client_id, client_secret, |token| async move {
            api::get_user(username, &token, client_id).await
        })
        .await?;
    if channel_user.data.is_empty() {
        bail!("channel not found");
    }
    println!("{:?}", channel_user);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_chatters_line;
    use crate::api::{Chatter, Chatters};

    fn chatter(login: &str) -> Chatter {
        Chatter {
            user_id: format!("{login}-id"),
            user_login: login.to_string(),
            user_name: login.to_string(),
        }
    }

    fn at(h: u32, mi: u32, s: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 9)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    #[test]
    fn formats_timestamp_then_logins_sorted_ascending() {
        let chatters = Chatters {
            data: vec![chatter("zoe"), chatter("alice"), chatter("bob")],
        };

        let line = format_chatters_line(at(12, 34, 56), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 12:34:56,alice,bob,zoe");
    }

    #[test]
    fn excludes_channel_owner_and_bot_account() {
        let chatters = Chatters {
            data: vec![chatter("mychannel"), chatter("mybot"), chatter("viewer")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,viewer");
    }

    #[test]
    fn keeps_trailing_comma_when_every_chatter_is_excluded() {
        let chatters = Chatters {
            data: vec![chatter("mybot")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,");
    }
}
