use serde::Deserialize;
use std::{fmt::Debug, path::PathBuf};

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
