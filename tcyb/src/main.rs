mod api;
mod artwork;
mod auth;
mod ban;
mod chat;
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    ReadChat {},
    AuthCode {},
    BanBots {},
    RefreshToken {},
    GetArtwork { names: Vec<String> },
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
struct AppConfig {
    client_id: String,
    client_secret: String,
    channel: String,
    username: String,
    speech_address: String,
    operations: Vec<String>,
    listen_address: String,
    db_dir: PathBuf,
    db_name: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct DBStore {
    access_token: String,
    refresh_token: String,
    user_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    simple_logger::SimpleLogger::new()
        .env()
        .with_local_timestamps()
        .init()?;
    let config = config::Config::builder()
        .add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        )
        .set_default("listen_address", "localhost:8000")
        .expect("this must not happen")
        .build()?;
    let app_config: AppConfig = config.try_deserialize()?;
    let args = Cli::parse();

    match &args.command {
        Some(Commands::ReadChat {}) => {
            chat::read_chat(
                &app_config.db_dir,
                &app_config.db_name,
                &app_config.username,
                &app_config.channel,
                &app_config.client_id,
                &app_config.client_secret,
                &app_config.operations,
                &app_config.speech_address,
            )
            .await?;
        }
        Some(Commands::AuthCode {}) => {
            auth::auth_code_grant(
                &app_config.listen_address,
                &app_config.db_dir,
                &app_config.db_name,
                &app_config.client_id,
                &app_config.client_secret,
            )
            .await?;
        }
        Some(Commands::BanBots {}) => {
            ban::ban_bots(
                &app_config.db_dir,
                &app_config.db_name,
                &app_config.username,
                &app_config.client_id,
            )
            .await?;
        }
        Some(Commands::RefreshToken {}) => {
            auth::refresh_token_grant(
                &app_config.db_dir,
                &app_config.db_name,
                &app_config.client_id,
                &app_config.client_secret,
            )
            .await?;
        }
        Some(Commands::GetArtwork { names }) => {
            artwork::get_artwork(&app_config.client_id, &app_config.client_secret, names).await?;
        }
        None => {}
    }
    Ok(())
}
