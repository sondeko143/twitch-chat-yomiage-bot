use crate::{api, store::Store};
use anyhow::bail;
use log::warn;
use std::path::Path;

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

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let user_id = store.user_id(username, client_id).await?;
    let channel_user_id;
    loop {
        match api::get_user(channel_name, store.access_token(), client_id).await {
            Ok(channel_user) => {
                if channel_user.data.is_empty() {
                    bail!("channel not found");
                }
                channel_user_id = channel_user.data[0].id.clone();
                break;
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                } else {
                    bail!(err);
                }
            }
        };
    }
    loop {
        match api::get_chatters(&channel_user_id, &user_id, store.access_token(), client_id).await {
            Ok(res) => {
                let now = chrono::Local::now().naive_local();
                println!(
                    "{}",
                    format_chatters_line(now, &res, channel_name, username)
                );
                break;
            }
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
    Ok(())
}

pub async fn show_user_info(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    loop {
        match api::get_user(username, store.access_token(), client_id).await {
            Ok(channel_user) => {
                if channel_user.data.is_empty() {
                    bail!("channel not found");
                }
                println!("{:?}", channel_user);
                break;
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                } else {
                    bail!(err);
                }
            }
        };
    }
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
