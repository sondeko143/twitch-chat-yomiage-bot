use clap::Parser;
use std::path::PathBuf;
use vstc;
use vstreamer_protos::Sound;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // Operations
    operations: Vec<String>,
    /// Text input
    #[arg(short, long)]
    text: Option<String>,
    /// Sound input
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
async fn main() {
    let args = Cli::parse();
    let host = args.host.unwrap_or(String::from("localhost"));
    let port = args.port.unwrap_or(8080);

    let sound: Option<Sound> = Some(Sound::default());

    vstc::process_command(
        format!("http://{host}:{port}").as_str(),
        &args.operations,
        args.text.unwrap_or_default(),
        sound,
        args.file_path,
        args.filters,
    )
    .await
    .unwrap();
}
