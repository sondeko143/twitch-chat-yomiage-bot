mod sound;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::fs::File;
use std::path::{Path, PathBuf};
use vstreamer_protos::Sound;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 8080;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 操作チェーンを送信する ex: `send 'o:/transl?t=ja' 'o:/tts' -t "hello"`
    Send(SendArgs),
}

/// 送信系サブコマンドが共有する接続オプション。
#[derive(Args)]
struct ConnArgs {
    /// 送信先ホスト
    #[arg(short = 'H', long)]
    host: Option<String>,
    /// 送信先ポート
    #[arg(short, long)]
    port: Option<u16>,
}

impl ConnArgs {
    /// 接続先 URI。プロファイル対応は Task 5 で入る。
    fn uri(&self) -> String {
        let host = self.host.as_deref().unwrap_or(DEFAULT_HOST);
        let port = self.port.unwrap_or(DEFAULT_PORT);
        format!("http://{host}:{port}")
    }
}

#[derive(Args)]
struct SendArgs {
    /// 操作 ex: `o:/trans?t=ja&s=en`
    operations: Vec<String>,
    /// テキスト入力
    #[arg(short, long)]
    text: Option<String>,
    /// 音声入力ファイル（非圧縮 PCM）
    #[arg(short, long)]
    wav: Option<PathBuf>,
    /// operand に載せる file_path
    #[arg(long)]
    file_path: Option<String>,
    /// フィルタ
    #[arg(long)]
    filters: Option<Vec<String>>,
    #[command(flatten)]
    conn: ConnArgs,
}

/// `--wav` を読み込む。未指定なら既定の空 `Sound` を返す（従来挙動）。
fn load_sound(wav: Option<&Path>) -> Result<Option<Sound>> {
    let Some(path) = wav else {
        return Ok(Some(Sound::default()));
    };
    let mut file =
        File::open(path).with_context(|| format!("'{}' を開けませんでした", path.display()))?;
    let (spec, data) = sound::read(&mut file)
        .with_context(|| format!("'{}' を WAV として読めませんでした", path.display()))?;
    Ok(Some(Sound {
        data,
        rate: spec.sample_rate,
        format: sound::convert_format(&spec),
        channels: spec.channels.into(),
    }))
}

async fn run_send(args: SendArgs) -> Result<()> {
    let uri = args.conn.uri();
    let sound = load_sound(args.wav.as_deref())?;
    vstc::process_command(
        &uri,
        &args.operations,
        args.text.unwrap_or_default(),
        sound,
        args.file_path,
        args.filters,
    )
    .await
    .with_context(|| format!("{uri} への送信に失敗しました"))?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Send(args) => run_send(args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn send_parses_operations_and_conn_flags() {
        let cli = Cli::parse_from([
            "vstc_cli", "send", "o:/tts", "-t", "hi", "-H", "example", "-p", "19829",
        ]);
        let Commands::Send(args) = cli.command;
        assert_eq!(args.operations, vec!["o:/tts".to_string()]);
        assert_eq!(args.text.as_deref(), Some("hi"));
        assert_eq!(args.conn.host.as_deref(), Some("example"));
        assert_eq!(args.conn.port, Some(19829));
    }

    #[test]
    fn bare_operations_without_subcommand_are_rejected() {
        // ADR-0014: 従来の位置引数フォームは廃止した。
        assert!(Cli::try_parse_from(["vstc_cli", "o:/tts"]).is_err());
    }
}
