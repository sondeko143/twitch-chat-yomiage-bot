mod sound;
use anyhow::{Context, Result};
use clap::Parser;
use std::{fs::File, path::PathBuf};
use vstreamer_protos::Sound;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // Operations ex: `o:/trans?t=ja&s=en`
    operations: Vec<String>,
    /// Text input
    #[arg(short, long)]
    text: Option<String>,
    /// Sound input file (uncompressed PCM)
    #[arg(short, long)]
    wav: Option<PathBuf>,
    /// Reload config file
    #[arg(long)]
    file_path: Option<String>,
    /// Filters
    #[arg(long)]
    filters: Option<Vec<String>>,
    /// Host name
    #[arg(short = 'H', long)]
    host: Option<String>,
    /// Port
    #[arg(short, long)]
    port: Option<u16>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let host = args.host.unwrap_or(String::from("localhost"));
    let port = args.port.unwrap_or(8080);

    let sound: Option<Sound> = match args.wav {
        Some(wav_path) => {
            let filename = wav_path.as_path();
            let mut wav_file =
                File::open(filename).context(format!("unable to open '{:?}'", filename))?;
            let (header, data) = sound::read(&mut wav_file)?;
            Some(Sound {
                data,
                rate: header.sampling_rate,
                format: sound::convert_format(&header),
                channels: header.channel_count.into(),
            })
        }
        None => Some(Sound::default()),
    };

    vstc::process_command(
        format!("http://{host}:{port}").as_str(),
        &args.operations,
        args.text.unwrap_or_default(),
        sound,
        args.file_path,
        args.filters,
    )
    .await?;
    Ok(())
}
