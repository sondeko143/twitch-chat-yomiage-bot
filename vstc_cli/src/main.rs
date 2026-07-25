mod profile;
mod sound;
mod store;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use profile::{Overrides, Profile, ProfileStore, Resolved};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use vstc::RouteOperand;
use vstreamer_protos::{Operation, OperationRoute, Sound};

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
    /// 再生を一時停止する
    Pause(ConnArgs),
    /// 再生を再開する
    Resume(ConnArgs),
    /// 設定ファイルをリロードする
    Reload(ReloadArgs),
    /// プロファイルを管理する
    #[command(subcommand)]
    Profile(ProfileCmd),
}

/// 送信系サブコマンドが共有する接続オプション。
#[derive(Args)]
struct ConnArgs {
    /// 使用するプロファイル名（明示フラグの方が優先される）
    #[arg(long)]
    profile: Option<String>,
    /// 送信先ホスト
    #[arg(short = 'H', long)]
    host: Option<String>,
    /// 送信先ポート
    #[arg(short, long)]
    port: Option<u16>,
}

/// 既定 → プロファイル → 明示フラグ の順で値を解決する（ADR-0016）。
///
/// `--profile` が無い実行はプロファイルファイルを一切読まないため、保存先が
/// 未作成でも解決できない環境でも動く。
fn resolve_conn(conn: &ConnArgs, config_path: Option<String>) -> Result<Resolved> {
    let overrides = Overrides {
        host: conn.host.clone(),
        port: conn.port,
        config_path,
    };
    let Some(name) = conn.profile.as_deref() else {
        return Ok(profile::resolve(None, &overrides));
    };
    let path = store::profiles_path()?;
    let saved = store::load(&path)?;
    let found = saved.get(name)?;
    Ok(profile::resolve(Some(found), &overrides))
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

#[derive(Args)]
struct ReloadArgs {
    /// リロードする設定ファイルのパス（プロファイルの config_path より優先）
    #[arg(long)]
    config_path: Option<String>,
    #[command(flatten)]
    conn: ConnArgs,
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// プロファイルを作成・更新する（渡したフィールドのみ更新）
    Set {
        /// プロファイル名
        name: String,
        /// 送信先ホスト
        #[arg(short = 'H', long)]
        host: Option<String>,
        /// 送信先ポート
        #[arg(short, long)]
        port: Option<u16>,
        /// reload が読む設定ファイルのパス
        #[arg(long)]
        config_path: Option<String>,
    },
    /// 保存済みプロファイルを一覧表示する
    List,
    /// プロファイルを削除する
    Remove {
        /// プロファイル名
        name: String,
    },
}

fn run_profile(cmd: ProfileCmd) -> Result<()> {
    let path = store::profiles_path()?;
    match cmd {
        ProfileCmd::Set {
            name,
            host,
            port,
            config_path,
        } => {
            let mut saved = store::load(&path)?;
            saved.merge(
                &name,
                &Profile {
                    host,
                    port,
                    config_path,
                },
            );
            store::save(&path, &saved)?;
            println!("プロファイル '{name}' を保存しました: {}", path.display());
            Ok(())
        }
        ProfileCmd::List => {
            let saved = store::load(&path)?;
            print_profile_list(&saved);
            eprintln!("({})", path.display());
            Ok(())
        }
        ProfileCmd::Remove { name } => {
            let mut saved = store::load(&path)?;
            saved.remove(&name)?;
            store::save(&path, &saved)?;
            println!("プロファイル '{name}' を削除しました");
            Ok(())
        }
    }
}

/// 一覧の表示内容を組み立てる。0 件は異常ではないので、作り方を案内する。
fn profile_list_output(saved: &ProfileStore) -> String {
    let table = profile::render_list(saved);
    if table.is_empty() {
        return "保存済みプロファイルはありません\n作成: vstc_cli profile set <NAME> --host <HOST> --port <PORT>"
            .to_string();
    }
    table
}

/// 一覧を表示する。0 件は異常ではないので、作り方を案内して正常終了する。
fn print_profile_list(saved: &ProfileStore) {
    println!("{}", profile_list_output(saved));
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
    let resolved = resolve_conn(&args.conn, None)?;
    let uri = resolved.uri();
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

/// 単一の操作だけを持つ route を組む。宛先は uri 側で決まるので remote は空。
fn single_route(op: Operation) -> OperationRoute {
    OperationRoute {
        operation: op.into(),
        remote: String::new(),
        queries: HashMap::new(),
    }
}

/// 1 ステップだけのチェーンを送る。
async fn send_route(resolved: &Resolved, op: Operation, operand: RouteOperand) -> Result<()> {
    let uri = resolved.uri();
    vstc::process_routes_with_operand(&uri, vec![single_route(op)], operand)
        .await
        .with_context(|| format!("{uri} への送信に失敗しました"))?;
    Ok(())
}

/// reload が使う設定パスを取り出す。どこからも解決できなければ、両方の
/// 指定方法を示して送信前に止める。
fn reload_config_path(resolved: &Resolved) -> Result<String> {
    resolved.config_path.clone().context(
        "reload には設定ファイルのパスが必要です\n\
         --config-path <PATH> を指定するか、プロファイルに保存してください:\n\
         vstc_cli profile set <NAME> --config-path <PATH>",
    )
}

async fn run_reload(args: ReloadArgs) -> Result<()> {
    let resolved = resolve_conn(&args.conn, args.config_path)?;
    let file_path = reload_config_path(&resolved)?;
    send_route(
        &resolved,
        Operation::Reload,
        RouteOperand {
            file_path,
            ..RouteOperand::default()
        },
    )
    .await
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Send(args) => run_send(args).await,
        Commands::Pause(conn) => {
            let resolved = resolve_conn(&conn, None)?;
            send_route(&resolved, Operation::Pause, RouteOperand::default()).await
        }
        Commands::Resume(conn) => {
            let resolved = resolve_conn(&conn, None)?;
            send_route(&resolved, Operation::Resume, RouteOperand::default()).await
        }
        Commands::Reload(args) => run_reload(args).await,
        Commands::Profile(cmd) => run_profile(cmd),
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
        let Commands::Send(args) = cli.command else {
            panic!("expected send");
        };
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

    #[test]
    fn send_accepts_a_profile_flag() {
        let cli = Cli::parse_from(["vstc_cli", "send", "o:/tts", "--profile", "main"]);
        let Commands::Send(args) = cli.command else {
            panic!("expected send");
        };
        assert_eq!(args.conn.profile.as_deref(), Some("main"));
    }

    #[test]
    fn profile_set_parses_all_fields() {
        let cli = Cli::parse_from([
            "vstc_cli",
            "profile",
            "set",
            "main",
            "--host",
            "h",
            "--port",
            "19829",
            "--config-path",
            "c.yml",
        ]);
        let Commands::Profile(ProfileCmd::Set {
            name,
            host,
            port,
            config_path,
        }) = cli.command
        else {
            panic!("expected profile set");
        };
        assert_eq!(name, "main");
        assert_eq!(host.as_deref(), Some("h"));
        assert_eq!(port, Some(19829));
        assert_eq!(config_path.as_deref(), Some("c.yml"));
    }

    #[test]
    fn profile_set_allows_updating_a_single_field() {
        let cli = Cli::parse_from(["vstc_cli", "profile", "set", "main", "--port", "1"]);
        let Commands::Profile(ProfileCmd::Set {
            host,
            port,
            config_path,
            ..
        }) = cli.command
        else {
            panic!("expected profile set");
        };
        assert_eq!(host, None);
        assert_eq!(port, Some(1));
        assert_eq!(config_path, None);
    }

    #[test]
    fn profile_list_and_remove_parse() {
        assert!(matches!(
            Cli::parse_from(["vstc_cli", "profile", "list"]).command,
            Commands::Profile(ProfileCmd::List)
        ));
        let cli = Cli::parse_from(["vstc_cli", "profile", "remove", "sub"]);
        let Commands::Profile(ProfileCmd::Remove { name }) = cli.command else {
            panic!("expected profile remove");
        };
        assert_eq!(name, "sub");
    }

    #[test]
    fn resolve_conn_without_profile_uses_defaults_and_skips_the_file() {
        // --profile が無い実行は profiles.toml に触れないので、保存先が解決
        // できない環境でも成功しなければならない。
        let conn = ConnArgs {
            profile: None,
            host: None,
            port: None,
        };
        let got = resolve_conn(&conn, None).expect("no profile means no file access");
        assert_eq!(got.uri(), "http://localhost:8080");
    }

    #[test]
    fn profile_list_output_guides_when_store_is_empty() {
        let out = profile_list_output(&ProfileStore::default());
        assert!(
            out.contains("保存済みプロファイルはありません"),
            "should say there are no saved profiles: {out}"
        );
        assert!(
            out.contains("profile set"),
            "should suggest how to create one: {out}"
        );
    }

    #[test]
    fn profile_list_output_renders_the_table_when_store_is_non_empty() {
        let mut saved = ProfileStore::default();
        saved.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(1),
                config_path: None,
            },
        );
        let out = profile_list_output(&saved);
        assert!(
            out.contains("NAME"),
            "should include the table header: {out}"
        );
        assert!(out.contains("main"), "should list the profile name: {out}");
        assert!(
            !out.contains("保存済みプロファイルはありません"),
            "non-empty store must not show the empty-store guidance: {out}"
        );
    }

    #[test]
    fn pause_and_resume_take_conn_flags() {
        let cli = Cli::parse_from(["vstc_cli", "pause", "--profile", "main", "--port", "1"]);
        let Commands::Pause(conn) = cli.command else {
            panic!("expected pause");
        };
        assert_eq!(conn.profile.as_deref(), Some("main"));
        assert_eq!(conn.port, Some(1));

        let cli = Cli::parse_from(["vstc_cli", "resume", "-H", "h"]);
        let Commands::Resume(conn) = cli.command else {
            panic!("expected resume");
        };
        assert_eq!(conn.host.as_deref(), Some("h"));
    }

    #[test]
    fn reload_takes_a_config_path_flag() {
        let cli = Cli::parse_from(["vstc_cli", "reload", "--config-path", "c.yml"]);
        let Commands::Reload(args) = cli.command else {
            panic!("expected reload");
        };
        assert_eq!(args.config_path.as_deref(), Some("c.yml"));
    }

    #[test]
    fn single_route_carries_the_operation_and_no_remote() {
        let route = single_route(Operation::Pause);
        assert_eq!(route.operation, Operation::Pause as i32);
        assert!(route.remote.is_empty());
        assert!(route.queries.is_empty());
    }

    #[test]
    fn reload_config_path_falls_back_to_the_profile() {
        let profile = Profile {
            config_path: Some("from-profile.yml".to_string()),
            ..Profile::default()
        };
        let resolved = profile::resolve(Some(&profile), &Overrides::default());
        assert_eq!(
            reload_config_path(&resolved).expect("profile supplies the path"),
            "from-profile.yml"
        );
    }

    #[test]
    fn reload_without_any_config_path_errors_with_guidance() {
        let resolved = profile::resolve(None, &Overrides::default());
        let err = reload_config_path(&resolved).expect_err("no source supplies a path");
        let msg = err.to_string();
        assert!(msg.contains("--config-path"), "should name the flag: {msg}");
        assert!(
            msg.contains("profile set"),
            "should point at the profile route too: {msg}"
        );
    }
}
