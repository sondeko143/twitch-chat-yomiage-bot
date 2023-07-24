use crate::api::{get_tokens_by_refresh, get_user};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("user not found")]
    UserNotFound,
    #[error(transparent)]
    RequestError(#[from] reqwest::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DBStore {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
}

pub struct Store {
    db: jfs::Store,
    db_name: String,
    obj: DBStore,
}
impl Store {
    pub fn new(db_dir: &Path, db_name: &str) -> Result<Self, std::io::Error> {
        let db = jfs::Store::new(db_dir)?;
        let obj = db.get::<DBStore>(db_name)?;
        Ok(Self {
            db,
            db_name: String::from(db_name),
            obj,
        })
    }

    pub fn access_token(&self) -> &str {
        self.obj.access_token.as_str()
    }

    pub async fn update_tokens(
        &mut self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), StoreError> {
        let (access_token, refresh_token) =
            get_tokens_by_refresh(&self.obj.refresh_token, client_id, client_secret).await?;
        let updated_obj = DBStore {
            access_token,
            refresh_token,
            user_id: self.obj.user_id.clone(),
        };
        self.db.save_with_id(&updated_obj, &self.db_name)?;
        self.obj = self.db.get::<DBStore>(&self.db_name)?;
        Ok(())
    }

    pub async fn user_id(&mut self, username: &str, client_id: &str) -> Result<String, StoreError> {
        if self.obj.user_id.is_empty() {
            let my_user = get_user(username, &self.obj.access_token, client_id).await?;
            if my_user.data.is_empty() {
                return Err(StoreError::UserNotFound);
            }
            let my_user_id = &my_user.data[0].id;
            let new_user_id = my_user_id.clone();
            let updated_obj = DBStore {
                access_token: self.obj.access_token.clone(),
                refresh_token: self.obj.refresh_token.clone(),
                user_id: new_user_id,
            };
            self.db.save_with_id(&updated_obj, &self.db_name)?;
            self.obj = self.db.get::<DBStore>(&self.db_name)?;
        }
        Ok(self.obj.user_id.clone())
    }
}
