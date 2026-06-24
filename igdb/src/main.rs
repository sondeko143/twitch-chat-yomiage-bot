mod api;
mod artwork;
mod settings;
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
    GetArtwork { names: Vec<String> },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args = Cli::parse();
    let mut config_builder = config::Config::builder();
    config_builder =
        config_builder.add_source(config::Environment::with_prefix("cb").try_parsing(true));
    if let Some(path) = args.config {
        config_builder = config_builder.add_source(config::File::with_name(path.to_str().unwrap()));
    }
    let config = config_builder.build()?;
    let settings: Settings = config.try_deserialize()?;
    simple_logger::SimpleLogger::new()
        .env()
        .with_local_timestamps()
        .init()?;

    match &args.command {
        Some(Commands::GetArtwork { names }) => {
            artwork::get_artwork(&settings.client_id, &settings.client_secret, names).await?;
        }
        None => {}
    }
    Ok(())
}
