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
}
