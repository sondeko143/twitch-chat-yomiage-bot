mod api;
mod artwork;
mod auth;
mod ban;
mod chat;
mod eventsub;
mod irc;
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
    GetArtwork { names: Vec<String> },
    ShowChatters {},
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let args = Cli::parse();
    let mut config_builder = config::Config::builder()
        .set_default("listen_address", "localhost:8000")?
        .set_default("greeting_template", "user_name is now following!")?;
    config_builder = config_builder.add_source(
        config::Environment::with_prefix("cb")
            .try_parsing(true)
            .list_separator(",")
            .with_list_parse_key("operations"),
    );
    if args.config.is_some() {
        config_builder = config_builder.add_source(config::File::with_name(
            args.config.unwrap().to_str().unwrap(),
        ));
    }
    let config = config_builder.build()?;
    let settings: Settings = config.try_deserialize()?;
    simple_logger::SimpleLogger::new()
        .env()
        .with_local_timestamps()
        .init()?;

    match &args.command {
        Some(Commands::ReadChat {}) => {
            yomiage::yomiage(&settings).await?;
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
            ban::ban_bots(
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
        Some(Commands::GetArtwork { names }) => {
            artwork::get_artwork(&settings.client_id, &settings.client_secret, names).await?;
        }
        Some(Commands::ShowChatters {}) => {
            chat::chatters(
                &settings.db_dir,
                &settings.db_name,
                &settings.username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        None => {}
    }
    Ok(())
}
