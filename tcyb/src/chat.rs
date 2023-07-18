use crate::{api, DBStore};
use anyhow::bail;
use chrono;
use jfs::Store;
use log::warn;
use std::path::Path;

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(db_name)?;
    let user_id = if obj.user_id.is_empty() {
        let my_user = api::get_user(username, &obj.access_token, client_id).await?;
        if my_user.data.is_empty() {
            bail!("my user not found");
        }
        let my_user_id = &my_user.data[0].id;
        let new_user_id = my_user_id.clone();
        let updated_obj = DBStore {
            user_id: new_user_id,
            ..obj
        };
        db.save_with_id(&updated_obj, db_name)?;
        updated_obj.user_id
    } else {
        obj.user_id
    };
    loop {
        let db = Store::new(db_dir)?;
        let obj = db.get::<DBStore>(db_name)?;
        match api::get_chatters(&user_id, &obj.access_token, client_id).await {
            Ok(res) => {
                res.data
                    .iter()
                    .for_each(|c| println!("{:?},{}", chrono::offset::Local::now(), c.user_name));
                break;
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    let (access_token, refresh_token) =
                        api::get_tokens_by_refresh(&obj.refresh_token, client_id, client_secret)
                            .await?;
                    let updated_obj = DBStore {
                        access_token,
                        refresh_token,
                        ..obj
                    };
                    db.save_with_id(&updated_obj, db_name)?;
                } else {
                    bail!(err);
                }
            }
        }
    }
    Ok(())
}
