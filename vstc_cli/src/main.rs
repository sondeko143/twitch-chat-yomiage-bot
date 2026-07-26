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
    /// send の既定チェーンを管理する
    #[command(subcommand)]
    Chains(ChainsCmd),
}

/// `send` が操作を省略したときに送るチェーンの編集操作（ADR-0018）。
#[derive(Subcommand)]
enum ChainsCmd {
    /// route 列を 1 本のチェーンとして末尾に追加する
    Add {
        /// プロファイル名
        name: String,
        /// route ex: `//localhost:8081/transc` `transl?t=en`
        #[arg(required = true)]
        routes: Vec<String>,
    },
    /// 番号を指定してチェーンを削除する
    Del {
        /// プロファイル名
        name: String,
        /// `chains show` が表示する 1 始まりの番号
        index: usize,
    },
    /// 保存済みチェーンを表示する
    Show {
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
                    chains: None,
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
        ProfileCmd::Chains(cmd) => run_profile_chains(&path, cmd),
    }
}

/// `profile chains` の 3 操作を実行する。
///
/// `add` は保存の前に全 route を検証する。壊れた route をファイルに残すと、
/// 次に送信するまで気づけないため（ADR-0018）。
fn run_profile_chains(path: &Path, cmd: ChainsCmd) -> Result<()> {
    match cmd {
        ChainsCmd::Add { name, routes } => {
            validate_routes(&routes)?;
            let mut saved = store::load(path)?;
            saved.add_chain(&name, routes)?;
            store::save(path, &saved)?;
            println!("プロファイル '{name}' にチェーンを追加しました");
            Ok(())
        }
        ChainsCmd::Del { name, index } => {
            let mut saved = store::load(path)?;
            saved.del_chain(&name, index)?;
            store::save(path, &saved)?;
            println!("プロファイル '{name}' のチェーン {index} を削除しました");
            Ok(())
        }
        ChainsCmd::Show { name } => {
            let saved = store::load(path)?;
            println!("{}", chains_show_output(&name, saved.chains_of(&name)?));
            Ok(())
        }
    }
}

/// 保存前に全 route を検証する。1 つでも解釈できなければ保存しない。
fn validate_routes(routes: &[String]) -> Result<()> {
    for route in routes {
        vstc::parse_route(route)
            .with_context(|| format!("route '{route}' を解釈できませんでした"))?;
    }
    Ok(())
}

/// `chains show` の表示内容を組み立てる。0 本は異常ではないので、
/// 追加方法を案内する。
fn chains_show_output(name: &str, chains: &[Vec<String>]) -> String {
    let rendered = profile::render_chains(chains);
    if rendered.is_empty() {
        return format!(
            "プロファイル '{name}' に保存済みチェーンはありません\n\
             追加: vstc_cli profile chains add {name} <ROUTES>..."
        );
    }
    rendered
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

/// `op` から送信する operand を組み立てる。`Reload` のときだけ設定パスを
/// 解決して `file_path` に載せ、それ以外（`Pause`/`Resume`）は空の operand を
/// 返す。送信・I/O を行わない純粋な変換なので、この対応をテストで固定できる。
///
/// ## Errors
///
/// `Reload` でどこからも設定パスを解決できなかったとき。
fn route_operand(op: Operation, resolved: &Resolved) -> Result<RouteOperand> {
    if op == Operation::Reload {
        Ok(RouteOperand {
            file_path: reload_config_path(resolved)?,
            ..RouteOperand::default()
        })
    } else {
        Ok(RouteOperand::default())
    }
}

/// 単一操作を送る。`Reload` のときだけ設定パスを解決して operand に載せる。
async fn run_route(conn: &ConnArgs, op: Operation, config_path: Option<String>) -> Result<()> {
    let resolved = resolve_conn(conn, config_path)?;
    let operand = route_operand(op, &resolved)?;
    send_route(&resolved, op, operand).await
}

/// 解析済みコマンドから決まる実行内容。送信も I/O も行わない純粋な変換で、
/// サブコマンドと Operation の対応をテストで固定できるようにしている。
enum Action {
    Send(SendArgs),
    Profile(ProfileCmd),
    /// 単一操作を送る。`config_path` には reload の明示指定のみが入る。
    Route {
        conn: ConnArgs,
        op: Operation,
        config_path: Option<String>,
    },
}

fn plan(cmd: Commands) -> Action {
    match cmd {
        Commands::Send(args) => Action::Send(args),
        Commands::Profile(cmd) => Action::Profile(cmd),
        Commands::Pause(conn) => Action::Route {
            conn,
            op: Operation::Pause,
            config_path: None,
        },
        Commands::Resume(conn) => Action::Route {
            conn,
            op: Operation::Resume,
            config_path: None,
        },
        Commands::Reload(args) => Action::Route {
            conn: args.conn,
            op: Operation::Reload,
            config_path: args.config_path,
        },
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match plan(cli.command) {
        Action::Send(args) => run_send(args).await,
        Action::Profile(cmd) => run_profile(cmd),
        Action::Route {
            conn,
            op,
            config_path,
        } => run_route(&conn, op, config_path).await,
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
    fn profile_chains_add_parses_the_name_and_every_route() {
        let cli = Cli::parse_from([
            "vstc_cli",
            "profile",
            "chains",
            "add",
            "main",
            "//localhost:8081/transc",
            "transl?t=en",
            "//windesk:8080/sub?p=s",
        ]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Add { name, routes })) = cli.command
        else {
            panic!("expected profile chains add");
        };
        assert_eq!(name, "main");
        assert_eq!(
            routes,
            vec![
                "//localhost:8081/transc".to_string(),
                "transl?t=en".to_string(),
                "//windesk:8080/sub?p=s".to_string(),
            ]
        );
    }

    #[test]
    fn profile_chains_add_requires_at_least_one_route() {
        assert!(Cli::try_parse_from(["vstc_cli", "profile", "chains", "add", "main"]).is_err());
    }

    #[test]
    fn profile_chains_del_and_show_parse() {
        let cli = Cli::parse_from(["vstc_cli", "profile", "chains", "del", "main", "2"]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Del { name, index })) = cli.command
        else {
            panic!("expected profile chains del");
        };
        assert_eq!(name, "main");
        assert_eq!(index, 2);

        let cli = Cli::parse_from(["vstc_cli", "profile", "chains", "show", "main"]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Show { name })) = cli.command else {
            panic!("expected profile chains show");
        };
        assert_eq!(name, "main");
    }

    #[test]
    fn validate_routes_accepts_every_documented_form() {
        validate_routes(&[
            "//localhost:8081/transc".to_string(),
            "transl?t=en".to_string(),
            "o:/tts".to_string(),
        ])
        .expect("all three documented forms are valid");
    }

    #[test]
    fn validate_routes_rejects_an_unparsable_route_naming_it() {
        let err = validate_routes(&["//localhost:8081/transc".to_string(), "nope".to_string()])
            .expect_err("an unknown operation must fail before saving");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("nope"),
            "should name the offending route: {msg}"
        );
    }

    #[test]
    fn chains_show_output_guides_when_the_profile_has_none() {
        let out = chains_show_output("main", &[]);
        assert!(
            out.contains("保存済みチェーンはありません"),
            "should say there are none: {out}"
        );
        assert!(
            out.contains("profile chains add main"),
            "should suggest how to add one: {out}"
        );
    }

    #[test]
    fn chains_show_output_renders_the_saved_chains() {
        let chains = vec![vec!["o:/tts".to_string()]];
        let out = chains_show_output("main", &chains);
        assert!(out.contains("[1] o:/tts"), "should number the chain: {out}");
        assert!(
            !out.contains("ありません"),
            "a non-empty profile must not show the empty guidance: {out}"
        );
    }

    #[test]
    fn resolve_conn_without_profile_uses_defaults() {
        // このテストは「既定値になる」ことだけを検証する。「ファイルに触れ
        // ない」性質自体は resolve_conn の早期 return（--profile が無ければ
        // store::profiles_path/load を呼ぶ前に return する）で成り立ってお
        // り、ここでは検証していない。
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
                chains: None,
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

    #[test]
    fn route_operand_for_reload_carries_the_resolved_config_path() {
        let resolved = profile::resolve(
            None,
            &Overrides {
                config_path: Some("c.yml".to_string()),
                ..Overrides::default()
            },
        );
        let operand = route_operand(Operation::Reload, &resolved).expect("config path is present");
        assert_eq!(operand.file_path, "c.yml");
    }

    #[test]
    fn route_operand_for_reload_without_a_config_path_errors() {
        let resolved = profile::resolve(None, &Overrides::default());
        assert!(route_operand(Operation::Reload, &resolved).is_err());
    }

    #[test]
    fn route_operand_for_pause_and_resume_is_empty() {
        let resolved = profile::resolve(None, &Overrides::default());
        for op in [Operation::Pause, Operation::Resume] {
            let operand = route_operand(op, &resolved).expect("pause/resume never error");
            assert!(operand.file_path.is_empty(), "file_path for {op:?}");
            assert!(operand.text.is_empty(), "text for {op:?}");
            assert!(operand.filters.is_empty(), "filters for {op:?}");
            assert!(operand.sound.is_none(), "sound for {op:?}");
        }
    }

    fn planned_operation(argv: &[&str]) -> Operation {
        match plan(Cli::parse_from(argv).command) {
            Action::Route { op, .. } => op,
            _ => panic!("expected a route action for {argv:?}"),
        }
    }

    #[test]
    fn pause_resume_reload_map_to_distinct_operations() {
        assert_eq!(planned_operation(&["vstc_cli", "pause"]), Operation::Pause);
        assert_eq!(
            planned_operation(&["vstc_cli", "resume"]),
            Operation::Resume
        );
        assert_eq!(
            planned_operation(&["vstc_cli", "reload"]),
            Operation::Reload
        );
    }

    #[test]
    fn plan_carries_reload_config_path_only() {
        let Action::Route { config_path, .. } =
            plan(Cli::parse_from(["vstc_cli", "reload", "--config-path", "c.yml"]).command)
        else {
            panic!("expected a route action");
        };
        assert_eq!(config_path.as_deref(), Some("c.yml"));

        let Action::Route { config_path, .. } =
            plan(Cli::parse_from(["vstc_cli", "pause"]).command)
        else {
            panic!("expected a route action");
        };
        assert_eq!(config_path, None);
    }
}
