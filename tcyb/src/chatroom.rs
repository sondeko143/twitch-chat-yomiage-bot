use crate::{api, DBStore};
use anyhow::bail;
use jfs::Store;
use std::path::Path;

pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    username: &str,
    client_id: &str,
) -> anyhow::Result<()> {
    let db = Store::new(db_dir)?;
    let obj = db.get::<DBStore>(db_name)?;
    let access_token = obj.access_token.clone();
    let user_id = if obj.user_id.is_empty() {
        let my_user = api::get_user(username, &access_token, client_id).await?;
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
    let res = api::get_chatters(&user_id, &access_token, client_id).await?;
    res.data.iter().for_each(|c| println!("{}", c.user_name));
    Ok(())
}
