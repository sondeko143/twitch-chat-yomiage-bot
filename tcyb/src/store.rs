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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DBStore {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    /// `channel_id` を引いたときの配信チャンネルの login（小文字）。後から足した項目なので、
    /// 無いストアファイルは「未保存」として読む（ADR-0027）。
    #[serde(default)]
    pub channel_login: String,
    /// 配信チャンネルのユーザ ID。空なら未保存。
    #[serde(default)]
    pub channel_id: String,
}

/// Persist freshly obtained tokens, creating the store on first use
/// (first-time `auth-code`, or after the store location moved). An existing
/// record's `user_id` is preserved so a re-auth doesn't drop it.
pub fn save_tokens(
    db_dir: &Path,
    db_name: &str,
    access_token: String,
    refresh_token: String,
) -> Result<(), std::io::Error> {
    let db = jfs::Store::new(db_dir)?;
    let obj = match db.get::<DBStore>(db_name) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DBStore::default(),
        Err(e) => return Err(e),
    };
    let updated = DBStore {
        access_token,
        refresh_token,
        ..obj
    };
    db.save_with_id(&updated, db_name)?;
    Ok(())
}

/// `read-chat` 内でトークンを持つ唯一の主体を、yomiage の再接続処理と視聴者一覧の
/// 周期取得で共有するためのハンドル。リフレッシュはこの mutex で直列化され、
/// 片方が更新したトークンはもう片方から即座に見える。
pub type SharedStore = std::sync::Arc<tokio::sync::Mutex<Store>>;

pub struct Store {
    db: jfs::Store,
    db_name: String,
    obj: DBStore,
}
impl Store {
    pub fn new(db_dir: &Path, db_name: &str) -> Result<Self, std::io::Error> {
        let db = jfs::Store::new(db_dir)?;
        let obj = db.get::<DBStore>(db_name).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let expected = db_dir.join(db_name).with_extension("json");
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "トークンストアが見つかりません ({})。`tcyb auth-code` で認証するか、\
                         既存の db/{} をこの場所へコピーしてください。",
                        expected.display(),
                        db_name
                    ),
                )
            } else {
                e
            }
        })?;
        Ok(Self {
            db,
            db_name: String::from(db_name),
            obj,
        })
    }

    pub fn access_token(&self) -> &str {
        self.obj.access_token.as_str()
    }

    /// 他の主体がトークンを更新した状況をネットワーク無しで再現するためのテスト用。
    #[cfg(test)]
    pub fn replace_access_token_for_test(&mut self, access_token: &str) {
        self.obj.access_token = access_token.to_string();
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
            ..self.obj.clone()
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
                user_id: new_user_id,
                ..self.obj.clone()
            };
            self.db.save_with_id(&updated_obj, &self.db_name)?;
            self.obj = self.db.get::<DBStore>(&self.db_name)?;
        }
        Ok(self.obj.user_id.clone())
    }

    /// 保存済みの配信チャンネル ID。保存したときの login が `channel_login` と一致する
    /// ときだけ返す。Twitch の login は大文字小文字を区別しないので、比較もそうする。
    pub fn cached_channel_id(&self, channel_login: &str) -> Option<&str> {
        let cached = !self.obj.channel_id.is_empty()
            && self.obj.channel_login.eq_ignore_ascii_case(channel_login);
        cached.then_some(self.obj.channel_id.as_str())
    }

    /// 配信チャンネルの login と ID の組を保存する（前の組は上書き）。login は小文字で持つ。
    pub fn save_channel_id(
        &mut self,
        channel_login: &str,
        channel_id: &str,
    ) -> Result<(), std::io::Error> {
        let updated_obj = DBStore {
            channel_login: channel_login.to_ascii_lowercase(),
            channel_id: channel_id.to_string(),
            ..self.obj.clone()
        };
        self.db.save_with_id(&updated_obj, &self.db_name)?;
        self.obj = self.db.get::<DBStore>(&self.db_name)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_missing_token_store_gives_actionable_error() {
        let dir = tempfile::tempdir().unwrap();
        // No record file exists in the fresh dir, so the token store is absent.
        let err = match Store::new(dir.path(), "data.json") {
            Ok(_) => panic!("expected an error for a missing token store"),
            Err(e) => e,
        };

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let msg = err.to_string();
        assert!(
            msg.contains("auth-code"),
            "error should guide the user to `auth-code`: {msg}"
        );
        assert!(
            msg.contains("data.json"),
            "error should name the missing store: {msg}"
        );
    }

    #[test]
    fn save_tokens_bootstraps_store_when_absent() {
        let dir = tempfile::tempdir().unwrap();

        // Fresh dir, no record yet: this must create the store, not error.
        save_tokens(dir.path(), "data.json", "acc".into(), "ref".into()).unwrap();

        let saved = jfs::Store::new(dir.path())
            .unwrap()
            .get::<DBStore>("data.json")
            .unwrap();
        assert_eq!(saved.access_token, "acc");
        assert_eq!(saved.refresh_token, "ref");
        assert_eq!(saved.user_id, ""); // no user id yet on a fresh bootstrap
    }

    #[test]
    fn save_tokens_preserves_existing_user_id() {
        let dir = tempfile::tempdir().unwrap();
        let seed = jfs::Store::new(dir.path()).unwrap();
        seed.save_with_id(
            &DBStore {
                access_token: "old".into(),
                refresh_token: "oldr".into(),
                user_id: "U123".into(),
                ..DBStore::default()
            },
            "data.json",
        )
        .unwrap();

        save_tokens(dir.path(), "data.json", "new".into(), "newr".into()).unwrap();

        let saved = jfs::Store::new(dir.path())
            .unwrap()
            .get::<DBStore>("data.json")
            .unwrap();
        assert_eq!(saved.access_token, "new");
        assert_eq!(saved.refresh_token, "newr");
        assert_eq!(saved.user_id, "U123"); // preserved across re-auth
    }

    /// チャンネル ID のキャッシュ項目を足す前に書かれたストアファイル。
    fn write_legacy_store(dir: &Path) {
        std::fs::write(
            dir.join("data.json"),
            r#"{"access_token":"acc","refresh_token":"ref","user_id":"U1"}"#,
        )
        .unwrap();
    }

    #[test]
    fn legacy_store_without_channel_fields_loads_as_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        write_legacy_store(dir.path());

        let store = Store::new(dir.path(), "data.json").unwrap();

        assert_eq!(store.access_token(), "acc");
        assert_eq!(store.cached_channel_id("chan"), None);
        assert_eq!(store.cached_channel_id(""), None);
    }

    #[test]
    fn saved_channel_id_is_reused_only_for_the_same_login_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        write_legacy_store(dir.path());
        let mut store = Store::new(dir.path(), "data.json").unwrap();

        store.save_channel_id("MyChan", "C1").unwrap();

        // Twitch の login は大文字小文字を区別しない（正規形は小文字）
        assert_eq!(store.cached_channel_id("mychan"), Some("C1"));
        assert_eq!(store.cached_channel_id("MYCHAN"), Some("C1"));
        assert_eq!(store.cached_channel_id("other"), None);

        let reopened = Store::new(dir.path(), "data.json").unwrap();
        assert_eq!(reopened.cached_channel_id("mychan"), Some("C1"));
        // トークンと bot の user_id は保ったまま
        assert_eq!(reopened.access_token(), "acc");
        let saved = jfs::Store::new(dir.path())
            .unwrap()
            .get::<DBStore>("data.json")
            .unwrap();
        assert_eq!(saved.refresh_token, "ref");
        assert_eq!(saved.user_id, "U1");
        assert_eq!(saved.channel_login, "mychan");
    }

    #[test]
    fn another_channel_overwrites_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        write_legacy_store(dir.path());
        let mut store = Store::new(dir.path(), "data.json").unwrap();

        store.save_channel_id("first", "C1").unwrap();
        store.save_channel_id("second", "C2").unwrap();

        assert_eq!(store.cached_channel_id("first"), None);
        assert_eq!(store.cached_channel_id("second"), Some("C2"));
    }

    #[test]
    fn save_tokens_preserves_the_channel_cache() {
        let dir = tempfile::tempdir().unwrap();
        write_legacy_store(dir.path());
        Store::new(dir.path(), "data.json")
            .unwrap()
            .save_channel_id("chan", "C1")
            .unwrap();

        save_tokens(dir.path(), "data.json", "new".into(), "newr".into()).unwrap();

        let store = Store::new(dir.path(), "data.json").unwrap();
        assert_eq!(store.access_token(), "new");
        assert_eq!(store.cached_channel_id("chan"), Some("C1"));
    }
}
