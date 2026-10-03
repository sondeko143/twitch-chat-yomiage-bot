use anyhow::Context;
use serde::Deserialize;
use std::{fmt::Debug, path::Path, path::PathBuf};

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
pub struct Settings {
    pub client_id: String,
    pub client_secret: String,
    pub channel: String,
    pub username: String,
    pub speech_address: String,
    pub operations: Vec<String>,
    pub listen_address: String,
    pub db_dir: PathBuf,
    pub db_name: String,
    pub translate_command: String,
    #[serde(default)]
    pub notification_speech: Vec<NotificationSpeech>,
    #[serde(default)]
    pub monitor: MonitorSettings,
}

/// `read-chat` のローカル配信口（`[monitor]`）。省略したキーは既定値になる。
#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(default)]
pub struct MonitorSettings {
    /// 配信口と視聴者一覧の周期取得を起動するか。
    pub enabled: bool,
    /// `127.0.0.1` で待ち受けるポート。
    pub port: u16,
    /// コメントと通知それぞれの保持件数。
    pub history_size: usize,
    /// 視聴者一覧を取得する間隔（秒）。
    pub chatters_interval_secs: u64,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            history_size: 500,
            chatters_interval_secs: 60,
        }
    }
}

/// 通知の種別ごとの読み上げテンプレート（`[[notification_speech]]`）。
#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
pub struct NotificationSpeech {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub notice_type: Option<String>,
    pub template: String,
}

const LEGACY_GREETING_ERROR: &str = r#"`greeting_template` は廃止されました。設定ファイルと環境変数 `cb_greeting_template` から削除し、次のように書き換えてください。

[[notification_speech]]
type = "channel.follow"
template = "{user_name} さん。フォローありがとうございます。"
"#;

const CONFIG_TEMPLATE: &str = r#"# tcyb 設定ファイル
client_id = ""
client_secret = ""
channel = "your_channel_name"
username = "your_username"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05", "o:/play?v=18"]
translate_command = "translate"
# listen_address = "localhost:8000"   # 既定値あり。変更時のみ記入
# db_dir / db_name は OS 標準データディレクトリを既定使用（変更時のみ記入）

# 通知の読み上げ。type ごとにテンプレートを書く。{フィールド名} は通知 event の値に置き換わる
# （ネストは {a.b}、波括弧そのものは {{ と }}）。notice_type は省略可（省略時はその type の全てに一致）
[[notification_speech]]
type = "channel.follow"
template = "{user_name} さん。フォローありがとうございます。"

# ローカル配信口（tcyb monitor 用）。enabled = true で ws://127.0.0.1:<port>/feed を開く
# [monitor]
# enabled = false
# port = 8765
# history_size = 500            # コメントと通知それぞれの保持件数
# chatters_interval_secs = 60   # 視聴者一覧の取得間隔
"#;

pub fn scaffold_config(config_file: &Path) -> anyhow::Result<()> {
    use std::io::Write;

    if let Some(parent) = config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config_file)?;
    file.write_all(CONFIG_TEMPLATE.as_bytes())?;
    Ok(())
}

pub fn load(
    config_file: &Path,
    cli_config: Option<&Path>,
    default_db_dir: &Path,
) -> anyhow::Result<Settings> {
    let mut builder = config::Config::builder()
        .set_default("listen_address", "localhost:8000")?
        .set_default("db_dir", default_db_dir.to_string_lossy().into_owned())?
        .set_default("db_name", "data.json")?;
    builder = builder.add_source(config::File::from(config_file).required(false));
    builder = builder.add_source(
        config::Environment::with_prefix("cb")
            .try_parsing(true)
            .list_separator(",")
            .with_list_parse_key("operations"),
    );
    if let Some(path) = cli_config {
        let name = path.to_str().context("--config path is not valid UTF-8")?;
        builder = builder.add_source(config::File::with_name(name));
    }
    let cfg = builder.build()?;
    if cfg.get::<config::Value>("greeting_template").is_ok() {
        anyhow::bail!(LEGACY_GREETING_ERROR);
    }
    Ok(cfg.try_deserialize()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_writes_parseable_template_with_required_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        scaffold_config(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // 必須の秘密キーがテンプレに含まれる
        assert!(text.contains("client_id"));
        assert!(text.contains("client_secret"));
        // 生成物は妥当な TOML である
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert!(parsed.get("client_secret").is_some());
    }

    #[test]
    fn scaffold_template_round_trips_through_runtime_loader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        scaffold_config(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let filled = text
            .replace("client_id = \"\"", "client_id = \"testid\"")
            .replace("client_secret = \"\"", "client_secret = \"testsecret\"");
        std::fs::write(&path, filled).unwrap();

        let default_db = std::path::Path::new("/var/tcyb-data");
        let s = load_locked(&path, None, default_db).unwrap();

        assert_eq!(s.client_id, "testid");
        assert_eq!(s.client_secret, "testsecret");
        assert_eq!(s.channel, "your_channel_name");
        assert_eq!(
            s.operations,
            vec![
                "o:/transl?t=ja".to_string(),
                "o:/tts?i=1&spd=1.1&pit=-0.05".to_string(),
                "o:/play?v=18".to_string(),
            ]
        );
        assert_eq!(s.db_dir, default_db);
    }

    fn write_config(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    const FULL_CONFIG: &str = r#"
client_id = "id"
client_secret = "secret"
channel = "ch"
username = "user"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja"]
translate_command = "translate"
"#;

    #[test]
    fn load_applies_default_db_dir_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), FULL_CONFIG);
        let default_db = std::path::Path::new("/var/tcyb-data");

        let s = load_locked(&cfg, None, default_db).unwrap();

        assert_eq!(s.db_dir, default_db);
        assert_eq!(s.client_secret, "secret");
    }

    #[test]
    fn load_config_file_overrides_default_db_dir() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("{}\ndb_dir = \"custom-db\"\n", FULL_CONFIG);
        let cfg = write_config(dir.path(), &body);

        let s = load_locked(&cfg, None, std::path::Path::new("/var/tcyb-data")).unwrap();

        assert_eq!(s.db_dir, std::path::Path::new("custom-db"));
    }

    #[test]
    fn load_errors_when_required_secret_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), "client_id = \"id\"\n");

        let err = load_locked(&cfg, None, std::path::Path::new("/var/tcyb-data"));

        assert!(err.is_err());
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 環境変数を触るテストと直列化して `load` を呼ぶ。
    fn load_locked(
        config_file: &Path,
        cli: Option<&Path>,
        default_db: &Path,
    ) -> anyhow::Result<Settings> {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        load(config_file, cli, default_db)
    }

    #[test]
    fn load_reads_multiple_notification_speech_entries() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}
[[notification_speech]]
type = \"channel.follow\"
template = \"a\"

[[notification_speech]]
type = \"channel.chat.notification\"
notice_type = \"sub\"
template = \"b\"
",
            FULL_CONFIG
        );
        let cfg = write_config(dir.path(), &body);

        let s = load_locked(&cfg, None, Path::new("/d")).unwrap();

        assert_eq!(
            s.notification_speech,
            vec![
                NotificationSpeech {
                    type_: "channel.follow".into(),
                    notice_type: None,
                    template: "a".into()
                },
                NotificationSpeech {
                    type_: "channel.chat.notification".into(),
                    notice_type: Some("sub".into()),
                    template: "b".into()
                },
            ]
        );
    }

    #[test]
    fn load_defaults_monitor_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), FULL_CONFIG);
        let s = load_locked(&cfg, None, Path::new("/d")).unwrap();
        assert_eq!(
            s.monitor,
            MonitorSettings {
                enabled: false,
                port: 8765,
                history_size: 500,
                chatters_interval_secs: 60,
            }
        );
    }

    #[test]
    fn load_reads_monitor_table_and_fills_missing_keys_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}
[monitor]
enabled = true
port = 9000
",
            FULL_CONFIG
        );
        let cfg = write_config(dir.path(), &body);
        let s = load_locked(&cfg, None, Path::new("/d")).unwrap();
        assert_eq!(
            s.monitor,
            MonitorSettings {
                enabled: true,
                port: 9000,
                history_size: 500,
                chatters_interval_secs: 60,
            }
        );
    }

    #[test]
    fn load_defaults_to_no_notification_speech() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), FULL_CONFIG);
        let s = load_locked(&cfg, None, Path::new("/d")).unwrap();
        assert!(s.notification_speech.is_empty());
    }

    fn assert_migration_hint(err: &anyhow::Error) {
        let msg = format!("{err:#}");
        assert!(msg.contains("greeting_template"), "{msg}");
        assert!(msg.contains("[[notification_speech]]"), "{msg}");
        assert!(msg.contains("type = \"channel.follow\""), "{msg}");
    }

    #[test]
    fn load_rejects_legacy_greeting_template_in_file() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}
greeting_template = \"x\"
",
            FULL_CONFIG
        );
        let cfg = write_config(dir.path(), &body);

        let err = load_locked(&cfg, None, Path::new("/d")).unwrap_err();

        assert_migration_hint(&err);
    }

    #[test]
    fn load_rejects_legacy_greeting_template_in_env() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), FULL_CONFIG);
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("cb_greeting_template", "x");

        let res = load(&cfg, None, Path::new("/d"));

        std::env::remove_var("cb_greeting_template");
        assert_migration_hint(&res.unwrap_err());
    }

    #[test]
    fn scaffold_template_has_follow_example_and_no_greeting_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        scaffold_config(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("greeting_template"));
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        let arr = parsed["notification_speech"].as_array().unwrap();
        assert_eq!(arr[0]["type"].as_str(), Some("channel.follow"));
    }
}
