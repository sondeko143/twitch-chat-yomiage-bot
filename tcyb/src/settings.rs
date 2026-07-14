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
    pub greeting_template: String,
    pub db_dir: PathBuf,
    pub db_name: String,
    pub translate_command: String,
}

const CONFIG_TEMPLATE: &str = r#"# tcyb 設定ファイル
client_id = ""
client_secret = ""
channel = "your_channel_name"
username = "your_username"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05", "o:/play?v=18"]
greeting_template = "user_name さん。フォローありがとうございます。"
translate_command = "translate"
# listen_address = "localhost:8000"   # 既定値あり。変更時のみ記入
# db_dir / db_name は OS 標準データディレクトリを既定使用（変更時のみ記入）
"#;

pub fn scaffold_config(config_file: &Path) -> anyhow::Result<()> {
    if let Some(parent) = config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_file, CONFIG_TEMPLATE)?;
    Ok(())
}

pub fn load(
    config_file: &Path,
    cli_config: Option<&Path>,
    default_db_dir: &Path,
) -> anyhow::Result<Settings> {
    let mut builder = config::Config::builder()
        .set_default("listen_address", "localhost:8000")?
        .set_default("greeting_template", "user_name is now following!")?
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

        let s = load(&cfg, None, default_db).unwrap();

        assert_eq!(s.db_dir, default_db);
        assert_eq!(s.client_secret, "secret");
    }

    #[test]
    fn load_config_file_overrides_default_db_dir() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("{}\ndb_dir = \"custom-db\"\n", FULL_CONFIG);
        let cfg = write_config(dir.path(), &body);

        let s = load(&cfg, None, std::path::Path::new("/var/tcyb-data")).unwrap();

        assert_eq!(s.db_dir, std::path::Path::new("custom-db"));
    }

    #[test]
    fn load_errors_when_required_secret_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), "client_id = \"id\"\n");

        let err = load(&cfg, None, std::path::Path::new("/var/tcyb-data"));

        assert!(err.is_err());
    }
}
