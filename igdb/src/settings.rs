use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
pub struct Settings {
    pub client_id: String,
    pub client_secret: String,
}
