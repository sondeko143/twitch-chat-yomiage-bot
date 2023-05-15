mod api;
mod auth;
mod ban;
mod chat;
use clap::{Parser, Subcommand};
use log::error;
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
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
struct AppConfig {
    client_id: String,
    client_secret: String,
    channel: String,
    username: String,
    speech_port: i16,
    operations: Vec<String>,
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
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    let config = config::Config::builder()
        .add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        )
        .build()
        .unwrap();
    let app_config: AppConfig = config.try_deserialize().unwrap();
    let args = Cli::parse();

    match &args.command {
        Some(Commands::ReadChat {}) => {
            if let Err(e) = chat::read_chat(
                app_config.db_dir.clone(),
                &app_config.db_name,
                &app_config.username,
                &app_config.channel,
                &app_config.client_id,
                &app_config.client_secret,
                app_config.operations,
                app_config.speech_port,
            )
            .await
            {
                match e {
                    err => error!("failed: {}", err),
                }
            }
        }
        Some(Commands::AuthCode {}) => {
            match auth::auth_code_grant(
                app_config.db_dir.clone(),
                &app_config.db_name,
                &app_config.client_id,
                &app_config.client_secret,
            )
            .await
            {
                Ok(_) => (),
                Err(err) => error!("failed: {}", err),
            }
        }
        Some(Commands::BanBots {}) => {
            match ban::ban_bots(
                app_config.db_dir.clone(),
                &app_config.db_name,
                &app_config.username,
                &app_config.client_id,
            )
            .await
            {
                Ok(_) => (),
                Err(err) => error!("failed: {}", err),
            }
        }
        Some(Commands::RefreshToken {}) => {
            match auth::refresh_token_grant(
                app_config.db_dir.clone(),
                &app_config.db_name,
                &app_config.client_id,
                &app_config.client_secret,
            )
            .await
            {
                Ok(_) => (),
                Err(err) => error!("failed: {}", err),
            }
        }
        None => {}
    }
}