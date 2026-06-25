mod api;
mod auth;
mod channel;
mod chat;
mod eventsub;
mod irc;
mod profiling;
mod settings;
mod store;
mod yomiage;
use anyhow::Result;
use clap::{Parser, Subcommand};
use settings::Settings;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    ReadChat {},
    AuthCode {},
    BanBots {},
    RefreshToken {},
    ShowChatters {},
    ShowUser { username: String },
    ShowFollowings { username: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _profile = profiling::init();
    dotenvy::dotenv().ok();
    let args = Cli::parse();

    let settings: Settings = {
        let _span = tracing::info_span!("config_build").entered();
        let mut config_builder = config::Config::builder()
            .set_default("listen_address", "localhost:8000")?
            .set_default("greeting_template", "user_name is now following!")?;
        config_builder = config_builder.add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        );
        if let Some(path) = args.config.as_ref() {
            config_builder = config_builder
                .add_source(config::File::with_name(path.to_str().unwrap()));
        }
        let config = config_builder.build()?;
        config.try_deserialize()?
    };

    {
        let _span = tracing::info_span!("logger_init").entered();
        simple_logger::SimpleLogger::new()
            .env()
            .with_local_timestamps()
            .init()?;
    }

    match &args.command {
        Some(Commands::ReadChat {}) => {
            tokio::select! {
                res = yomiage::yomiage(&settings) => res?,
                sig = tokio::signal::ctrl_c() => {
                    sig?;
                    log::warn!("Ctrl+C received, shutting down");
                }
            }
        }
        Some(Commands::AuthCode {}) => {
            auth::auth_code_grant(
                &settings.listen_address,
                &settings.db_dir,
                &settings.db_name,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::BanBots {}) => {
            channel::ban_bots(
                &settings.db_dir,
                &settings.db_name,
                &settings.username,
                &settings.client_id,
            )
            .await?;
        }
        Some(Commands::RefreshToken {}) => {
            auth::refresh_token_grant(
                &settings.db_dir,
                &settings.db_name,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::ShowChatters {}) => {
            chat::chatters(
                &settings.db_dir,
                &settings.db_name,
                &settings.channel,
                &settings.username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::ShowUser { username }) => {
            chat::show_user_info(
                &settings.db_dir,
                &settings.db_name,
                username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::ShowFollowings { username }) => {
            channel::show_following_info(
                &settings.db_dir,
                &settings.db_name,
                username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        None => {}
    }
    Ok(())
}
