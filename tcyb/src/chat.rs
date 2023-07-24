use crate::{api, store::Store};
use anyhow::bail;
use log::warn;
use std::path::Path;

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let user_id = store.user_id(username, client_id).await?;
    loop {
        match api::get_chatters(&user_id, store.access_token(), client_id).await {
            Ok(res) => {
                res.data
                    .iter()
                    .for_each(|c| println!("{:?},{}", chrono::offset::Local::now(), c.user_name));
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
