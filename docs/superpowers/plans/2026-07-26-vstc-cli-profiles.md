# vstc_cli プロファイル / pause・resume・reload 実装プラン

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

ADR: [0014](../../adr/0014-subcommand-only-cli-surface-for-vstc-cli.md) / [0015](../../adr/0015-single-profiles-toml-in-os-user-config-dir.md) / [0016](../../adr/0016-explicit-flags-override-profile-and-set-merges.md) / [0017](../../adr/0017-extend-vstc-routes-entrypoint-with-operand-options.md)（いずれも `Proposed`。Task 7 で `Accepted` へ昇格する）

**Goal:** `vstc_cli` を全面サブコマンド化し、`pause` / `resume` / `reload` を直接実行できるようにしたうえで、宛先と reload 設定パスを名前付きプロファイルとして OS 標準ユーザー設定ディレクトリに保存・一覧・削除できるようにする。

**Architecture:** `vstc`（lib）には `Operand` の任意フィールドを束ねた `RouteOperand` と、それを受ける送信口 `process_routes_with_operand` を足すだけに留める。`vstc_cli` は 3 層に分ける — `profile.rs`（データモデルとマージ/解決/整形の**純粋関数**、I/O 無し）、`store.rs`（`ProjectDirs` によるパス解決と temp→rename の原子的読み書き）、`main.rs`（clap 定義と dispatch のみ）。テストは純粋層に集中させ、I/O 層は一時ディレクトリで往復検証する。

**Tech Stack:** Rust 2021 / clap 4（derive）/ serde 1（derive）/ toml 1.1 / directories 6 / anyhow 1 / tempfile 3（dev）/ tonic 0.14 / `vstreamer_protos` v0.1.2

## Global Constraints

- 保存先は `directories::ProjectDirs::from("", "", "vstc")` の config ディレクトリ配下の単一ファイル `profiles.toml`。環境変数 `VSTC_CONFIG_DIR` があればそのディレクトリを優先する。
- 実行時の値解決は「組み込み既定 → プロファイル → 明示フラグ」の後勝ち。既定は host=`localhost` / port=`8080`。
- `profile set` はマージ更新。渡されなかったフィールドは既存値を保持する（クリアしない）。
- 既定プロファイルは持たない。`--profile` 省略時はプロファイルファイルを読まない。
- プロファイルの `config_path` は `reload` の設定パス解決にのみ使う。`send --file-path` には影響させない。
- `--profile` に未登録名を渡した場合、および `reload` の設定パスが解決できない場合は、送信せずエラー終了する。
- `profiles.toml` の書き込みは同一ディレクトリの一時ファイルへ書いてから `rename` で置き換える。
- `send` / `pause` / `resume` / `reload` は成功時に標準出力へ何も出さない（既存 CLI の挙動を踏襲）。エラーは `anyhow` 経由で非 0 終了。
- 新規の外部クレートは追加しない。使用する `serde` / `toml` / `directories` / `tempfile` はすべて既に `Cargo.lock` にある版（`serde 1.0.162` / `toml 1.1.2` / `directories 6.0.0` / `tempfile 3.27.0`）に合わせる。
- 品質ゲートは `just ci`（`fmt-check` + `clippy -D warnings` + `test` + `check-env-leak` + `gitleaks` + `deny` + `audit`）。PR 前に全緑必須。
- ドキュメント・コミットメッセージに個人/マシン依存の絶対パス（ホーム・AppData 等）を書かない（`check-env-leak` が落ちる）。

## File Structure

| ファイル | 責務 |
|---|---|
| `vstc/src/lib.rs`（変更） | `RouteOperand` 型と `process_routes_with_operand` の追加。エンドポイント構築の共通化。 |
| `vstc/tests/test.rs`（変更） | `process_routes_with_operand` が `file_path` を運ぶことの結合テスト。 |
| `vstc_cli/Cargo.toml`（変更） | `serde` / `toml` / `directories` / dev `tempfile` の追加。 |
| `vstc_cli/src/profile.rs`（新規） | `Profile` / `ProfileStore` / `Overrides` / `Resolved`、マージ・削除・参照・解決・一覧整形。**I/O 無しの純粋層**。 |
| `vstc_cli/src/store.rs`（新規） | `profiles.toml` のパス解決（`ProjectDirs` / `VSTC_CONFIG_DIR`）と load / save（原子的）。 |
| `vstc_cli/src/main.rs`（全面改訂） | clap のサブコマンド定義と dispatch のみ。 |
| `vstc_cli/src/sound.rs` | 変更なし。 |
| `vstc_cli/README.md`（変更） | サブコマンド形式の使用例とプロファイルの説明。 |

---

### Task 1: vstc に operand オプション付きの route 送信口を足す

ADR-0017 の実装。`process_routes` の互換は保ち（`vstc_gui` を壊さない）、`file_path` を運べる新しい口を足す。

**Files:**
- Modify: `vstc/src/lib.rs`
- Test: `vstc/src/lib.rs`（`mod tests`）, `vstc/tests/test.rs`

**Interfaces:**
- Consumes: なし（先行タスクなし）
- Produces:
  - `pub struct vstc::RouteOperand { pub text: String, pub file_path: String, pub filters: Vec<String>, pub sound: Option<vstreamer_protos::Sound> }`（`Debug + Default + Clone`）
  - `pub async fn vstc::process_routes_with_operand(uri: &str, routes: Vec<OperationRoute>, operand: RouteOperand) -> Result<Response, VstcError>`
  - `pub async fn vstc::process_routes(uri: &str, routes: Vec<OperationRoute>, text: String) -> Result<Response, VstcError>`（シグネチャ据え置き）

- [ ] **Step 1: 失敗するユニットテストを書く**

`vstc/src/lib.rs` の `mod tests` にある既存テスト `build_command_wraps_routes_in_single_chain` を、新しい `build_command` シグネチャに合わせて書き換え、`file_path` を運ぶケースを追加する。既存テストの本体を次で置き換える:

```rust
    #[test]
    fn build_command_wraps_routes_in_single_chain() {
        let routes = vec![OperationRoute {
            operation: Operation::Tts as i32,
            remote: String::new(),
            queries: HashMap::new(),
        }];
        let cmd = build_command(
            routes.clone(),
            RouteOperand {
                text: "hello".to_string(),
                ..RouteOperand::default()
            },
        );
        assert_eq!(cmd.chains.len(), 1);
        assert_eq!(cmd.chains[0].operations, routes);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hello");
        assert!(operand.sound.is_none());
        assert!(!operand.trace_id.is_empty());
    }

    #[test]
    fn build_command_carries_optional_operand_fields() {
        let cmd = build_command(
            Vec::new(),
            RouteOperand {
                file_path: "conf.yml".to_string(),
                filters: vec!["a".to_string()],
                ..RouteOperand::default()
            },
        );
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.file_path, "conf.yml");
        assert_eq!(operand.filters, vec!["a".to_string()]);
        assert!(operand.text.is_empty());
        assert!(operand.origin_ts > 0.0);
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p vstc --lib`
Expected: FAIL（`cannot find struct RouteOperand` / `build_command` の引数不一致でコンパイルエラー）

- [ ] **Step 3: `RouteOperand` と新しい送信口を実装する**

`vstc/src/lib.rs` の既存の `build_operand` / `build_command` / `process_routes` の 3 関数（現在の 99〜143 行付近）を、次の内容で置き換える:

```rust
/// Optional [`Operand`] fields for [`process_routes_with_operand`].
///
/// `trace_id` and `origin_ts` are generated inside this crate, so callers never
/// fill them in. Every field has a `Default`, letting a caller set only what its
/// operation needs — e.g. only `file_path` for `Reload`.
#[derive(Debug, Default, Clone)]
pub struct RouteOperand {
    /// Text payload.
    pub text: String,
    /// Path of the config file to act on (used by `Reload`).
    pub file_path: String,
    /// Filter names (used by `SetFilters`).
    pub filters: Vec<String>,
    /// Raw sound payload.
    pub sound: Option<Sound>,
}

/// Build a proto `Operand` from the caller-visible fields, stamping a fresh
/// trace id and the current origin timestamp.
fn build_operand(operand: RouteOperand) -> Operand {
    Operand {
        text: operand.text,
        sound: operand.sound,
        file_path: operand.file_path,
        filters: operand.filters,
        trace_id: Uuid::new_v4().to_string(),
        origin_ts: unix_timestamp_secs(),
    }
}

/// Wrap the given routes into a single-chain `Command` carrying `operand`.
fn build_command(routes: Vec<OperationRoute>, operand: RouteOperand) -> Command {
    Command {
        chains: vec![OperationChain { operations: routes }],
        operand: Some(build_operand(operand)),
    }
}

/// Connect to `uri` with this crate's connect/RPC timeouts applied.
async fn connect(uri: &str) -> Result<CommanderClient<Channel>, VstcError> {
    let endpoint = tonic::transport::Endpoint::new(uri.to_string())?
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS));
    Ok(CommanderClient::connect(endpoint).await?)
}

/// Send pre-built operation routes with a text operand to the channel.
///
/// Unlike [`process_command`], this takes already-structured [`OperationRoute`]
/// values instead of URL-style operation strings, so callers (e.g. a GUI) that
/// already have separated destination/command/parameter fields don't round-trip
/// through string parsing.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_routes(
    uri: &str,
    routes: Vec<OperationRoute>,
    text: String,
) -> Result<Response, VstcError> {
    process_routes_with_operand(
        uri,
        routes,
        RouteOperand {
            text,
            ..RouteOperand::default()
        },
    )
    .await
}

/// Send pre-built operation routes together with the operand fields in
/// `operand`.
///
/// [`process_routes`] covers the text-only case; use this when an operation
/// needs another operand field, such as `Reload` needing `file_path`.
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_routes_with_operand(
    uri: &str,
    routes: Vec<OperationRoute>,
    operand: RouteOperand,
) -> Result<Response, VstcError> {
    let mut channel = connect(uri).await?;
    let c = tonic::Request::new(build_command(routes, operand));
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}
```

続けて `process_command` の本体先頭 3 行（`let endpoint = ...` から `let mut channel = CommanderClient::connect(endpoint).await?;` まで）を 1 行に置き換える:

```rust
    let mut channel = connect(uri).await?;
```

最後に import 行（現在の 13〜16 行付近）へ `Channel` を足す。`use vstreamer_protos::{...}` の直前に次を追加:

```rust
use tonic::transport::Channel;
```

- [ ] **Step 4: ユニットテストが通ることを確認する**

Run: `cargo test -p vstc --lib`
Expected: PASS（`build_command_wraps_routes_in_single_chain` と `build_command_carries_optional_operand_fields` を含む全件）

- [ ] **Step 5: file_path を運ぶ結合テストを書く**

`vstc/tests/test.rs` の末尾に追加する。既存テストと同じモックサーバー構成で、ポートだけ `9003` にずらす:

```rust
#[tokio::test]
async fn process_routes_with_operand_carries_file_path() {
    use std::collections::HashMap;
    use std::sync::mpsc::channel;
    use std::time::Duration;
    use vstreamer_protos::{Operation, OperationRoute};

    const ADDR_STR: &str = "127.0.0.1:9003";
    let (tx, rx) = channel();
    tokio::spawn(async move {
        let mut mock = MockCommanderService::new();
        mock.expect_process_command().returning(move |req| {
            let inner = req.into_inner();
            let operand = inner.operand.expect("operand should be present");
            let op = inner.chains[0].operations[0].operation;
            tx.send((op, operand.file_path))
                .expect("test channel should accept the operand");
            Ok(tonic::Response::new(Response { result: true }))
        });
        let addr = ADDR_STR.parse().unwrap();
        build(mock).serve(addr).await.unwrap();
    });

    let routes = vec![OperationRoute {
        operation: Operation::Reload as i32,
        remote: String::new(),
        queries: HashMap::new(),
    }];
    process_routes_with_operand(
        format!("http://{ADDR_STR}").as_str(),
        routes,
        RouteOperand {
            file_path: String::from("some/config.yml"),
            ..RouteOperand::default()
        },
    )
    .await
    .unwrap();

    let (op, file_path) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server should have received the operand");
    assert_eq!(op, Operation::Reload as i32);
    assert_eq!(file_path, "some/config.yml");
}
```

- [ ] **Step 6: 結合テストが通ることを確認する**

Run: `cargo test -p vstc`
Expected: PASS（`process_routes_with_operand_carries_file_path` を含む全件）

- [ ] **Step 7: 既存の呼び出し元が無傷であることを確認する**

Run: `just clippy`
Expected: 警告なしで終了（`vstc_gui` の `vstc::process_routes` 呼び出しは無変更で通る）

- [ ] **Step 8: コミット**

```bash
git add vstc/src/lib.rs vstc/tests/test.rs
git commit -m "feat(vstc): operand オプション付きの route 送信口 process_routes_with_operand を追加"
```

---

### Task 2: vstc_cli を send サブコマンドへ移行する

ADR-0014 の実装。この時点ではプロファイル機能は入れず、CLI の表面だけをサブコマンド化する。

**Files:**
- Modify: `vstc_cli/src/main.rs`（全面改訂）
- Test: `vstc_cli/src/main.rs`（`mod tests`）

**Interfaces:**
- Consumes: なし
- Produces:
  - `enum Commands { Send(SendArgs) }`（後続タスクが variant を足す）
  - `struct ConnArgs { host: Option<String>, port: Option<u16> }`（Task 5 が `profile` フィールドを足す）
  - `struct SendArgs { operations: Vec<String>, text: Option<String>, wav: Option<PathBuf>, file_path: Option<String>, filters: Option<Vec<String>>, conn: ConnArgs }`
  - `fn load_sound(wav: Option<&Path>) -> anyhow::Result<Option<Sound>>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/main.rs` の末尾に追加する（このテストは clap の定義そのものを検証する。`debug_assert` は引数名の重複・短縮フラグ衝突・不正なグループ定義を実行時に検出する）:

```rust
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
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p vstc_cli`
Expected: FAIL（`cannot find type Commands` 等のコンパイルエラー）

- [ ] **Step 3: main.rs をサブコマンド構造へ書き換える**

`vstc_cli/src/main.rs` の 1〜64 行（`mod sound;` から `main` の終わりまで）を、次で全面的に置き換える。末尾の `mod tests` は Step 1 で足したものをそのまま残す:

```rust
mod sound;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::fs::File;
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
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p vstc_cli`
Expected: PASS（3 件）

- [ ] **Step 5: 実際のヘルプ表示を確認する**

Run: `cargo run -p vstc_cli -- --help`
Expected: `Commands:` セクションに `send` が表示され、`[OPERATIONS]...` は最上位に出ない

- [ ] **Step 6: 整形と lint を通す**

Run: `just fmt` の後 `just clippy`
Expected: どちらも警告なしで終了

- [ ] **Step 7: コミット**

```bash
git add vstc_cli/src/main.rs
git commit -m "feat(vstc_cli)!: CLI をサブコマンド化し従来の位置引数フォームを send へ移行"
```

---

### Task 3: プロファイルのモデルと純粋ロジックを足す

ADR-0015 のデータ形式と ADR-0016 の解決規則を、I/O を持たない純粋層として実装する。

**注意:** このタスクの終了時点では `profile.rs` の各項目がまだどこからも呼ばれないため、`cargo clippy` は `dead_code` を報告する。これは想定内で、Task 5 の配線で解消する。このタスクでは `cargo test -p vstc_cli` の緑のみを確認し、`just clippy` は走らせない。

**Files:**
- Create: `vstc_cli/src/profile.rs`
- Modify: `vstc_cli/Cargo.toml`, `vstc_cli/src/main.rs`（`mod profile;` の追加のみ）
- Test: `vstc_cli/src/profile.rs`（`mod tests`）

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub const profile::DEFAULT_HOST: &str = "localhost"` / `pub const profile::DEFAULT_PORT: u16 = 8080`
  - `pub struct profile::Profile { pub host: Option<String>, pub port: Option<u16>, pub config_path: Option<String> }`（`Debug + Clone + Default + PartialEq + Eq + Serialize + Deserialize`）
  - `pub struct profile::ProfileStore { pub profiles: BTreeMap<String, Profile> }`（同上の derive）
  - `pub fn ProfileStore::merge(&mut self, name: &str, patch: &Profile)`
  - `pub fn ProfileStore::remove(&mut self, name: &str) -> anyhow::Result<()>`
  - `pub fn ProfileStore::get(&self, name: &str) -> anyhow::Result<&Profile>`
  - `pub struct profile::Overrides { pub host: Option<String>, pub port: Option<u16>, pub config_path: Option<String> }`（`Debug + Clone + Default + PartialEq + Eq`）
  - `pub struct profile::Resolved { pub host: String, pub port: u16, pub config_path: Option<String> }`（`Debug + Clone + PartialEq + Eq`）／ `pub fn Resolved::uri(&self) -> String`
  - `pub fn profile::resolve(profile: Option<&Profile>, overrides: &Overrides) -> Resolved`
  - `pub fn profile::render_list(store: &ProfileStore) -> String`（空ストアなら空文字）

- [ ] **Step 1: 依存を追加する**

`vstc_cli/Cargo.toml` の `[dependencies]` 末尾（`anyhow = "1.0.71"` の次の行）に追加する:

```toml
serde = { version = "1.0.162", features = ["derive"] }
toml = "1.1.2"
directories = "6.0.0"
```

同ファイル末尾に新しいセクションを追加する:

```toml

[dev-dependencies]
tempfile = "3.27.0"
```

Run: `cargo check -p vstc_cli`
Expected: 成功し、`Cargo.lock` は更新されない（すべて既存版に一致するため）

- [ ] **Step 2: 失敗するテストを書く**

`vstc_cli/src/profile.rs` を新規作成し、まずテストだけを書く:

```rust
//! Named connection profiles: data model plus the pure merge / resolve /
//! render logic. All file access lives in [`crate::store`].

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(host: &str, port: u16) -> Profile {
        Profile {
            host: Some(host.to_string()),
            port: Some(port),
            config_path: None,
        }
    }

    #[test]
    fn merge_creates_profile_when_absent() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        assert_eq!(store.profiles["main"], profile("h", 1));
    }

    #[test]
    fn merge_keeps_fields_the_patch_leaves_unset() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(1),
                config_path: Some("c.yml".to_string()),
            },
        );
        store.merge(
            "main",
            &Profile {
                port: Some(2),
                ..Profile::default()
            },
        );
        let got = &store.profiles["main"];
        assert_eq!(got.host.as_deref(), Some("h"));
        assert_eq!(got.port, Some(2));
        assert_eq!(got.config_path.as_deref(), Some("c.yml"));
    }

    #[test]
    fn remove_existing_profile_succeeds() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.remove("main").expect("remove should succeed");
        assert!(store.profiles.is_empty());
    }

    #[test]
    fn remove_unknown_profile_errors_and_lists_known_names() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.merge("sub", &profile("h", 2));
        let err = store.remove("nope").expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(msg.contains("nope"), "message should name the profile: {msg}");
        assert!(msg.contains("main"), "message should list known names: {msg}");
        assert!(msg.contains("sub"), "message should list known names: {msg}");
    }

    #[test]
    fn get_unknown_profile_on_empty_store_suggests_creating_one() {
        let store = ProfileStore::default();
        let err = store.get("nope").expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(msg.contains("nope"), "message should name the profile: {msg}");
        assert!(
            msg.contains("profile set"),
            "message should suggest how to create one: {msg}"
        );
    }

    #[test]
    fn resolve_falls_back_to_builtin_defaults() {
        let got = resolve(None, &Overrides::default());
        assert_eq!(got.host, DEFAULT_HOST);
        assert_eq!(got.port, DEFAULT_PORT);
        assert_eq!(got.config_path, None);
    }

    #[test]
    fn resolve_prefers_profile_over_defaults() {
        let p = profile("ph", 111);
        let got = resolve(Some(&p), &Overrides::default());
        assert_eq!(got.host, "ph");
        assert_eq!(got.port, 111);
    }

    #[test]
    fn resolve_prefers_explicit_flags_over_profile() {
        let p = Profile {
            host: Some("ph".to_string()),
            port: Some(111),
            config_path: Some("pc.yml".to_string()),
        };
        let overrides = Overrides {
            host: Some("oh".to_string()),
            port: Some(222),
            config_path: Some("oc.yml".to_string()),
        };
        let got = resolve(Some(&p), &overrides);
        assert_eq!(got.host, "oh");
        assert_eq!(got.port, 222);
        assert_eq!(got.config_path.as_deref(), Some("oc.yml"));
    }

    #[test]
    fn resolve_overrides_each_field_independently() {
        // ADR-0016: 明示した項目だけが勝ち、他はプロファイル値が残る。
        let p = Profile {
            host: Some("ph".to_string()),
            port: Some(111),
            config_path: Some("pc.yml".to_string()),
        };
        let overrides = Overrides {
            port: Some(222),
            ..Overrides::default()
        };
        let got = resolve(Some(&p), &overrides);
        assert_eq!(got.host, "ph");
        assert_eq!(got.port, 222);
        assert_eq!(got.config_path.as_deref(), Some("pc.yml"));
    }

    #[test]
    fn resolved_uri_is_http_host_port() {
        let got = resolve(None, &Overrides::default());
        assert_eq!(got.uri(), "http://localhost:8080");
    }

    #[test]
    fn render_list_is_empty_for_empty_store() {
        assert_eq!(render_list(&ProfileStore::default()), "");
    }

    #[test]
    fn render_list_sorts_by_name_and_marks_unset_fields() {
        let mut store = ProfileStore::default();
        store.merge(
            "zeta",
            &Profile {
                host: Some("zh".to_string()),
                ..Profile::default()
            },
        );
        store.merge(
            "alpha",
            &Profile {
                host: Some("ah".to_string()),
                port: Some(1),
                config_path: Some("a.yml".to_string()),
            },
        );
        let lines: Vec<&str> = render_list(&store).lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[0].starts_with("NAME"));
        assert!(lines[1].starts_with("alpha"), "sorted first: {}", lines[1]);
        assert!(lines[2].starts_with("zeta"), "sorted second: {}", lines[2]);
        assert!(lines[1].contains("a.yml"));
        assert!(
            lines[2].contains('-'),
            "unset port/config_path shown as dash: {}",
            lines[2]
        );
    }

    #[test]
    fn store_round_trips_through_toml() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(19829),
                config_path: Some("c.yml".to_string()),
            },
        );
        let text = toml::to_string(&store).expect("serialize");
        let back: ProfileStore = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, store);
    }

    #[test]
    fn unset_fields_are_not_written_to_toml() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                ..Profile::default()
            },
        );
        let text = toml::to_string(&store).expect("serialize");
        assert!(text.contains("host"), "set field is written: {text}");
        assert!(!text.contains("port"), "unset field is omitted: {text}");
        assert!(
            !text.contains("config_path"),
            "unset field is omitted: {text}"
        );
    }

    #[test]
    fn empty_toml_deserializes_to_empty_store() {
        let back: ProfileStore = toml::from_str("").expect("deserialize empty");
        assert!(back.profiles.is_empty());
    }
}
```

`vstc_cli/src/main.rs` の先頭 `mod sound;` の直前に追加する:

```rust
mod profile;
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p vstc_cli`
Expected: FAIL（`cannot find type Profile` 等のコンパイルエラー）

- [ ] **Step 4: 純粋層を実装する**

`vstc_cli/src/profile.rs` の冒頭のモジュールコメント直後、`#[cfg(test)] mod tests` の前に挿入する:

```rust
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Host used when neither a profile nor an explicit flag supplies one.
pub const DEFAULT_HOST: &str = "localhost";
/// Port used when neither a profile nor an explicit flag supplies one.
pub const DEFAULT_PORT: u16 = 8080;

/// Placeholder shown by `profile list` for a field that has no value.
const UNSET: &str = "-";

/// One saved connection profile.
///
/// Every field is optional so `profile set` can update a single field without
/// clearing the rest (ADR-0016), and so unset fields stay out of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Destination host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Destination port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Config file path used by `reload`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

/// The whole `profiles.toml` file.
///
/// `BTreeMap` keeps `profile list` output stable and name-sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStore {
    /// Saved profiles keyed by name.
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

impl ProfileStore {
    /// Merge `patch` into the profile named `name`, creating it when absent.
    /// Fields left `None` in `patch` keep their existing value (ADR-0016).
    pub fn merge(&mut self, name: &str, patch: &Profile) {
        let entry = self.profiles.entry(name.to_string()).or_default();
        if patch.host.is_some() {
            entry.host.clone_from(&patch.host);
        }
        if patch.port.is_some() {
            entry.port = patch.port;
        }
        if patch.config_path.is_some() {
            entry.config_path.clone_from(&patch.config_path);
        }
    }

    /// Remove the named profile.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn remove(&mut self, name: &str) -> anyhow::Result<()> {
        if self.profiles.remove(name).is_some() {
            return Ok(());
        }
        Err(self.unknown(name))
    }

    /// Look up a profile by name.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn get(&self, name: &str) -> anyhow::Result<&Profile> {
        self.profiles.get(name).ok_or_else(|| self.unknown(name))
    }

    /// Error for an unknown profile name, listing what is available so the
    /// user can fix a typo without a second command.
    fn unknown(&self, name: &str) -> anyhow::Error {
        if self.profiles.is_empty() {
            return anyhow!(
                "プロファイル '{name}' は登録されていません（登録済みプロファイルはありません）\n\
                 作成: vstc_cli profile set {name} --host <HOST> --port <PORT>"
            );
        }
        let known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        anyhow!(
            "プロファイル '{name}' は登録されていません\n登録済み: {}",
            known.join(", ")
        )
    }
}

/// Values given explicitly on the command line. `None` means "not given".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `--host`
    pub host: Option<String>,
    /// `--port`
    pub port: Option<u16>,
    /// `--config-path`
    pub config_path: Option<String>,
}

/// The effective values for one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Destination host.
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// Config file path, if any source supplied one.
    pub config_path: Option<String>,
}

impl Resolved {
    /// gRPC endpoint URI for the resolved host and port.
    pub fn uri(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Resolve the effective values: built-in defaults, then the profile, then the
/// explicit flags — later sources win, field by field (ADR-0016).
pub fn resolve(profile: Option<&Profile>, overrides: &Overrides) -> Resolved {
    Resolved {
        host: overrides
            .host
            .clone()
            .or_else(|| profile.and_then(|p| p.host.clone()))
            .unwrap_or_else(|| DEFAULT_HOST.to_string()),
        port: overrides
            .port
            .or_else(|| profile.and_then(|p| p.port))
            .unwrap_or(DEFAULT_PORT),
        config_path: overrides
            .config_path
            .clone()
            .or_else(|| profile.and_then(|p| p.config_path.clone())),
    }
}

/// Render the aligned table shown by `profile list`.
/// Returns an empty string when there is nothing to show.
pub fn render_list(store: &ProfileStore) -> String {
    if store.profiles.is_empty() {
        return String::new();
    }
    let mut rows: Vec<[String; 4]> = vec![[
        "NAME".to_string(),
        "HOST".to_string(),
        "PORT".to_string(),
        "CONFIG_PATH".to_string(),
    ]];
    for (name, p) in &store.profiles {
        rows.push([
            name.clone(),
            p.host.clone().unwrap_or_else(|| UNSET.to_string()),
            p.port
                .map_or_else(|| UNSET.to_string(), |port| port.to_string()),
            p.config_path.clone().unwrap_or_else(|| UNSET.to_string()),
        ]);
    }
    let widths: Vec<usize> = (0..4)
        .map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}", width = *width))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p vstc_cli`
Expected: PASS（Task 2 の 3 件 + このタスクの 15 件 = 18 件）

- [ ] **Step 6: 整形する**

Run: `just fmt`
Expected: 差分があれば整形される（`just clippy` は Task 5 まで走らせない — 未配線の `dead_code` が出るため）

- [ ] **Step 7: コミット**

```bash
git add vstc_cli/Cargo.toml vstc_cli/src/profile.rs vstc_cli/src/main.rs
git commit -m "feat(vstc_cli): プロファイルのモデルとマージ/解決/整形の純粋層を追加"
```

---

### Task 4: プロファイルの保存先解決と原子的な読み書きを足す

ADR-0015 の I/O 層。

**注意:** Task 3 と同じく、このタスクの終了時点でも `store.rs` は未配線のため `cargo clippy` は `dead_code` を報告する。Task 5 で解消する。

**Files:**
- Create: `vstc_cli/src/store.rs`
- Modify: `vstc_cli/src/main.rs`（`mod store;` の追加のみ）
- Test: `vstc_cli/src/store.rs`（`mod tests`）

**Interfaces:**
- Consumes: `crate::profile::{Profile, ProfileStore}`（Task 3）
- Produces:
  - `pub fn store::profiles_path() -> anyhow::Result<PathBuf>`
  - `pub fn store::load(path: &Path) -> anyhow::Result<ProfileStore>`
  - `pub fn store::save(path: &Path, store: &ProfileStore) -> anyhow::Result<()>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/store.rs` を新規作成し、まずテストだけを書く:

```rust
//! Where `profiles.toml` lives, and how it is read and written.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    #[test]
    fn override_dir_places_the_file_under_base() {
        let base = PathBuf::from("base-dir");
        let got = profiles_path_from(Some(base.clone()), None).expect("override needs no ProjectDirs");
        assert_eq!(got, base.join("profiles.toml"));
    }

    #[test]
    fn missing_project_dirs_without_override_errors() {
        assert!(profiles_path_from(None, None).is_err());
    }

    #[test]
    fn load_returns_empty_store_when_file_is_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let got = load(&dir.path().join("profiles.toml")).expect("absent file is not an error");
        assert!(got.profiles.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(19829),
                config_path: Some("c.yml".to_string()),
            },
        );
        save(&path, &store).expect("save");
        assert_eq!(load(&path).expect("load"), store);
    }

    #[test]
    fn save_creates_the_config_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("deeper").join("profiles.toml");
        save(&path, &ProfileStore::default()).expect("save should create parents");
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        save(&path, &ProfileStore::default()).expect("save");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }

    #[test]
    fn save_over_existing_file_replaces_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        let mut first = ProfileStore::default();
        first.merge(
            "old",
            &Profile {
                host: Some("h".to_string()),
                ..Profile::default()
            },
        );
        save(&path, &first).expect("first save");
        let mut second = ProfileStore::default();
        second.merge(
            "new",
            &Profile {
                host: Some("h2".to_string()),
                ..Profile::default()
            },
        );
        save(&path, &second).expect("second save");
        let got = load(&path).expect("load");
        assert_eq!(got, second, "rename must replace the existing file");
    }

    #[test]
    fn load_reports_the_path_when_the_file_is_not_valid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.toml");
        std::fs::write(&path, "this is not = = toml").expect("write");
        let err = load(&path).expect_err("broken toml should error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("profiles.toml"),
            "error should name the file: {msg}"
        );
    }
}
```

`vstc_cli/src/main.rs` の `mod profile;` の直後に追加する:

```rust
mod store;
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p vstc_cli`
Expected: FAIL（`cannot find function profiles_path_from` 等のコンパイルエラー）

- [ ] **Step 3: I/O 層を実装する**

`vstc_cli/src/store.rs` の冒頭のモジュールコメント直後、`#[cfg(test)] mod tests` の前に挿入する:

```rust
use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

use crate::profile::ProfileStore;

/// Overrides the directory holding `profiles.toml`.
/// Mirrors tcyb's `TCYB_CONFIG_DIR` so both crates behave the same way.
const CONFIG_DIR_ENV: &str = "VSTC_CONFIG_DIR";
/// Name of the single file holding every profile (ADR-0015).
const FILE_NAME: &str = "profiles.toml";

/// Pure path composition, split out so both branches are testable without
/// touching the real user directories.
fn profiles_path_from(
    base_override: Option<PathBuf>,
    proj: Option<ProjectDirs>,
) -> Result<PathBuf> {
    if let Some(base) = base_override {
        return Ok(base.join(FILE_NAME));
    }
    let proj = proj.context("OS のユーザーディレクトリを解決できませんでした")?;
    Ok(proj.config_dir().join(FILE_NAME))
}

/// Absolute path of `profiles.toml` on this machine.
///
/// ## Errors
///
/// Fails when `VSTC_CONFIG_DIR` is unset and the OS user directories cannot be
/// resolved.
pub fn profiles_path() -> Result<PathBuf> {
    let base = std::env::var_os(CONFIG_DIR_ENV).map(PathBuf::from);
    profiles_path_from(base, ProjectDirs::from("", "", "vstc"))
}

/// Read the store. A missing file means "no profiles yet", not an error, so
/// every command works on a machine that has never run `profile set`.
///
/// ## Errors
///
/// Fails when the file exists but cannot be read or is not valid TOML.
pub fn load(path: &Path) -> Result<ProfileStore> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ProfileStore::default()),
        Err(e) => {
            return Err(e).with_context(|| format!("{} を読めませんでした", path.display()))
        }
    };
    toml::from_str(&text)
        .with_context(|| format!("{} の TOML を解析できませんでした", path.display()))
}

/// Write the store atomically: serialize into a sibling temp file, then rename
/// it over the target. An interrupted write therefore cannot truncate the file
/// and lose every saved profile (ADR-0015).
///
/// ## Errors
///
/// Fails when the directory cannot be created, or the file cannot be written or
/// renamed into place.
pub fn save(path: &Path, store: &ProfileStore) -> Result<()> {
    let dir = path
        .parent()
        .context("プロファイルの保存先ディレクトリを決定できませんでした")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("{} を作成できませんでした", dir.display()))?;
    let text = toml::to_string(store).context("プロファイルを TOML へ変換できませんでした")?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)
        .with_context(|| format!("{} へ書き込めませんでした", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("{} へ反映できませんでした", path.display()))
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p vstc_cli`
Expected: PASS（Task 2 の 3 件 + Task 3 の 15 件 + このタスクの 8 件 = 26 件）

- [ ] **Step 5: 整形する**

Run: `just fmt`
Expected: 差分があれば整形される

- [ ] **Step 6: コミット**

```bash
git add vstc_cli/src/store.rs vstc_cli/src/main.rs
git commit -m "feat(vstc_cli): profiles.toml のパス解決と原子的な読み書きを追加"
```

---

### Task 5: profile サブコマンドと --profile 解決を CLI に配線する

Task 3 / 4 の層を CLI から使えるようにする。ここで `dead_code` が解消し、`just clippy` が緑に戻る。

**Files:**
- Modify: `vstc_cli/src/main.rs`
- Test: `vstc_cli/src/main.rs`（`mod tests`）

**Interfaces:**
- Consumes: `crate::profile::{resolve, render_list, Overrides, Profile, ProfileStore, Resolved}`, `crate::store::{load, profiles_path, save}`
- Produces:
  - `struct ConnArgs { profile: Option<String>, host: Option<String>, port: Option<u16> }`（`uri()` は削除し `resolve_conn` に置き換わる）
  - `fn resolve_conn(conn: &ConnArgs, config_path: Option<String>) -> anyhow::Result<Resolved>`
  - `enum ProfileCmd { Set { name, host, port, config_path }, List, Remove { name } }`
  - `fn run_profile(cmd: ProfileCmd) -> anyhow::Result<()>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/main.rs` の `mod tests` の中、`bare_operations_without_subcommand_are_rejected` の後に追加する:

```rust
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
```

あわせて、Task 2 で書いた `send_parses_operations_and_conn_flags` の分解行を差し替える。`Commands::Send` が唯一の variant でなくなるため、`let Commands::Send(args) = cli.command;` は refutable になりコンパイルが通らなくなる。次の 3 行に置き換える:

```rust
        let Commands::Send(args) = cli.command else {
            panic!("expected send");
        };
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p vstc_cli`
Expected: FAIL（`no variant Profile` / `no field profile on ConnArgs` 等のコンパイルエラー）

- [ ] **Step 3: `ConnArgs` にプロファイルを足し、解決関数を実装する**

`vstc_cli/src/main.rs` の `ConnArgs` 定義と `impl ConnArgs`（Task 2 で書いた `uri()` を含むブロック全体）を、次で置き換える:

```rust
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
```

`use` 群（Task 2 で書いた冒頭）に追加する:

```rust
use profile::{Overrides, Profile, ProfileStore, Resolved};
```

`DEFAULT_HOST` / `DEFAULT_PORT` の const 2 行は `profile.rs` へ移ったので main.rs から削除する。

- [ ] **Step 4: `run_send` を解決関数経由に切り替える**

`run_send` の先頭 1 行を差し替える:

```rust
async fn run_send(args: SendArgs) -> Result<()> {
    let resolved = resolve_conn(&args.conn, None)?;
    let uri = resolved.uri();
```

以降（`let sound = ...` から）は Task 2 のまま変更しない。

- [ ] **Step 5: profile サブコマンドを実装する**

`Commands` enum に variant を追加する（`Send(SendArgs),` の次の行）:

```rust
    /// プロファイルを管理する
    #[command(subcommand)]
    Profile(ProfileCmd),
```

`SendArgs` の定義の後に、サブコマンド定義とハンドラを追加する:

```rust
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

/// 一覧を表示する。0 件は異常ではないので、作り方を案内して正常終了する。
fn print_profile_list(saved: &ProfileStore) {
    let table = profile::render_list(saved);
    if table.is_empty() {
        println!("保存済みプロファイルはありません");
        println!("作成: vstc_cli profile set <NAME> --host <HOST> --port <PORT>");
        return;
    }
    println!("{table}");
}
```

`main` の `match` に arm を追加する:

```rust
        Commands::Profile(cmd) => run_profile(cmd),
```

- [ ] **Step 6: テストが通ることを確認する**

Run: `cargo test -p vstc_cli`
Expected: PASS（既存 26 件 + 新規 5 件 = 31 件）

- [ ] **Step 7: lint が緑に戻ったことを確認する**

Run: `just fmt` の後 `just clippy`
Expected: どちらも警告なしで終了（Task 3 / 4 で出ていた `dead_code` が解消している）

- [ ] **Step 8: 実際に保存と一覧ができることを手で確かめる**

PowerShell で、実ユーザー設定ディレクトリを汚さないよう一時ディレクトリを使う:

```powershell
$env:VSTC_CONFIG_DIR = Join-Path $env:TEMP "vstc-plan-check"
cargo run -p vstc_cli -- profile set main --host localhost --port 19829 --config-path some/config.yml
cargo run -p vstc_cli -- profile set sub --host other-host
cargo run -p vstc_cli -- profile list
cargo run -p vstc_cli -- profile set main --port 20000
cargo run -p vstc_cli -- profile list
cargo run -p vstc_cli -- profile remove sub
cargo run -p vstc_cli -- profile list
```

Expected:
- 1 回目の `list` が `main`（localhost / 19829 / some/config.yml）と `sub`（other-host / `-` / `-`）を名前順で表示する
- `--port 20000` の後の `list` で `main` の host と config_path が保持され port だけ 20000 になっている
- `remove sub` の後の `list` に `main` だけが残る

- [ ] **Step 9: 未登録プロファイルがエラーになることを確かめる**

```powershell
cargo run -p vstc_cli -- send o:/tts --profile nope
```

Expected: 非 0 終了し、`nope` と登録済み名（`main`）を含むエラーメッセージが出る。通信は試みられない。

- [ ] **Step 10: 検証用の一時ディレクトリを片付ける**

```powershell
Remove-Item -Recurse -Force (Join-Path $env:TEMP "vstc-plan-check")
Remove-Item Env:\VSTC_CONFIG_DIR
```

- [ ] **Step 11: コミット**

```bash
git add vstc_cli/src/main.rs
git commit -m "feat(vstc_cli): profile set/list/remove と --profile による宛先解決を追加"
```

---

### Task 6: pause / resume / reload サブコマンドを足す

**Files:**
- Modify: `vstc_cli/src/main.rs`
- Test: `vstc_cli/src/main.rs`（`mod tests`）

**Interfaces:**
- Consumes: `crate::resolve_conn`（Task 5）, `vstc::{process_routes_with_operand, RouteOperand}`（Task 1）
- Produces:
  - `enum Commands { …, Pause(ConnArgs), Resume(ConnArgs), Reload(ReloadArgs) }`
  - `struct ReloadArgs { config_path: Option<String>, conn: ConnArgs }`
  - `async fn send_route(resolved: &Resolved, op: Operation, operand: RouteOperand) -> anyhow::Result<()>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/main.rs` の `mod tests` 末尾に追加する:

```rust
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
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p vstc_cli`
Expected: FAIL（`no variant Pause` / `cannot find function single_route` 等のコンパイルエラー）

- [ ] **Step 3: サブコマンドと送信処理を実装する**

`Commands` enum に variant を追加する（`Send(SendArgs),` の次、`Profile` の前）:

```rust
    /// 再生を一時停止する
    Pause(ConnArgs),
    /// 再生を再開する
    Resume(ConnArgs),
    /// 設定ファイルをリロードする
    Reload(ReloadArgs),
```

`SendArgs` の定義の直後に追加する:

```rust
#[derive(Args)]
struct ReloadArgs {
    /// リロードする設定ファイルのパス（プロファイルの config_path より優先）
    #[arg(long)]
    config_path: Option<String>,
    #[command(flatten)]
    conn: ConnArgs,
}
```

`run_send` の直後にヘルパーを追加する:

```rust
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
```

冒頭の `use` 群に追加する:

```rust
use std::collections::HashMap;
use vstc::RouteOperand;
use vstreamer_protos::{Operation, OperationRoute};
```

既存の `use vstreamer_protos::Sound;` は上の行に統合し、`use vstreamer_protos::{Operation, OperationRoute, Sound};` の 1 行にする。

`main` の `match` に arm を追加する（`Commands::Send` の次）:

```rust
        Commands::Pause(conn) => {
            let resolved = resolve_conn(&conn, None)?;
            send_route(&resolved, Operation::Pause, RouteOperand::default()).await
        }
        Commands::Resume(conn) => {
            let resolved = resolve_conn(&conn, None)?;
            send_route(&resolved, Operation::Resume, RouteOperand::default()).await
        }
        Commands::Reload(args) => run_reload(args).await,
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p vstc_cli`
Expected: PASS（既存 31 件 + 新規 5 件 = 36 件）

- [ ] **Step 5: 整形と lint を通す**

Run: `just fmt` の後 `just clippy`
Expected: どちらも警告なしで終了

- [ ] **Step 6: reload のガードを手で確かめる**

```powershell
$env:VSTC_CONFIG_DIR = Join-Path $env:TEMP "vstc-plan-check2"
cargo run -p vstc_cli -- reload
```

Expected: 非 0 終了し、`--config-path` と `profile set` の両方を案内するメッセージが出る。接続は試みられない（接続エラーではなくガードのメッセージが出る）。

```powershell
cargo run -p vstc_cli -- profile set main --config-path some/config.yml
cargo run -p vstc_cli -- reload --profile main
```

Expected: 設定パスのガードは通り、サーバー未起動なので接続エラーで終了する（`http://localhost:8080 への送信に失敗しました` を含む）。

```powershell
Remove-Item -Recurse -Force (Join-Path $env:TEMP "vstc-plan-check2")
Remove-Item Env:\VSTC_CONFIG_DIR
```

- [ ] **Step 7: ヘルプ表示を確認する**

Run: `cargo run -p vstc_cli -- --help`
Expected: `send` / `pause` / `resume` / `reload` / `profile` の 5 つが並ぶ

- [ ] **Step 8: コミット**

```bash
git add vstc_cli/src/main.rs
git commit -m "feat(vstc_cli): pause/resume/reload サブコマンドを追加"
```

---

### Task 7: README を更新し、ADR を昇格させ、フルゲートを通す

**Files:**
- Modify: `vstc_cli/README.md`
- Modify: `docs/adr/0014-subcommand-only-cli-surface-for-vstc-cli.md`, `docs/adr/0015-single-profiles-toml-in-os-user-config-dir.md`, `docs/adr/0016-explicit-flags-override-profile-and-set-merges.md`, `docs/adr/0017-extend-vstc-routes-entrypoint-with-operand-options.md`, `docs/adr/README.md`

**Interfaces:**
- Consumes: Task 1〜6 の全成果
- Produces: なし（ドキュメントと検証のみ）

- [ ] **Step 1: README を書き換える**

`vstc_cli/README.md` の 5 行目以降（`## 使い方` 以下すべて）を、次で置き換える:

````markdown
## 使い方

```sh
cargo run -p vstc_cli -- --help
Usage: vstc_cli.exe <COMMAND>

Commands:
  send     操作チェーンを送信する
  pause    再生を一時停止する
  resume   再生を再開する
  reload   設定ファイルをリロードする
  profile  プロファイルを管理する
```

`send` / `pause` / `resume` / `reload` は共通で `--profile <NAME>` / `-H, --host <HOST>` / `-p, --port <PORT>` を取る。

```sh
# 例) 翻訳して読み上げ
./vstc_cli.exe send 'o:/transl?t=ja' 'o:/tts?i=1&spd=1.1&pit=-0.05' 'o:/play?v=20' -p 19829 -t "hello, world"

# 例) 一時停止 / 再開
./vstc_cli.exe pause -p 19829
./vstc_cli.exe resume -p 19829
```

## プロファイル

宛先（host / port）と `reload` が読む設定ファイルのパスを、名前付きで保存できる。

```sh
# 保存（渡したフィールドだけを更新する。他は既存値のまま）
./vstc_cli.exe profile set main --host localhost --port 19829 --config-path path/to/config.yml
./vstc_cli.exe profile set main --port 20000

# 一覧
./vstc_cli.exe profile list

# 削除
./vstc_cli.exe profile remove main
```

保存したら `--profile` で呼び出す。

```sh
./vstc_cli.exe pause --profile main
./vstc_cli.exe reload --profile main

# 明示フラグはプロファイルより優先される（この 1 回だけ別ポートへ）
./vstc_cli.exe pause --profile main --port 8080
```

値は「組み込み既定 → プロファイル → 明示フラグ」の順で後勝ちに解決される。既定は `localhost:8080`。
`--profile` を指定しない実行はプロファイルファイルを読まないので、保存が一度も無くても全コマンドが動く。

`reload` の設定パスは `--config-path` がプロファイルの `config_path` より優先される。どちらからも解決できない場合は送信せずエラーになる。

### 保存場所

プロファイルは OS 標準のユーザー設定ディレクトリ配下の `vstc/profiles.toml` 1 ファイルに保存される（Windows は Roaming AppData 配下）。正確なパスは `profile list` が末尾に表示する。

環境変数 `VSTC_CONFIG_DIR` を設定すると、そのディレクトリ配下の `profiles.toml` が使われる。

```toml
[profiles.main]
host = "localhost"
port = 19829
config_path = "path/to/config.yml"

[profiles.sub]
host = "other-host"
port = 8080
```
````

- [ ] **Step 2: 4 件の ADR を Accepted へ昇格させる**

各ファイルの `- Status: Proposed` の 1 行だけを `- Status: Accepted` に変える。本文は書き換えない。

```
docs/adr/0014-subcommand-only-cli-surface-for-vstc-cli.md
docs/adr/0015-single-profiles-toml-in-os-user-config-dir.md
docs/adr/0016-explicit-flags-override-profile-and-set-merges.md
docs/adr/0017-extend-vstc-routes-entrypoint-with-operand-options.md
```

`docs/adr/README.md` の索引表でも、0014〜0017 の行の `Proposed` を `Accepted` に変える。

- [ ] **Step 3: ADR と実装の突合を行う**

各 ADR の Decision 節を読み、実装と食い違っていないか確認する。

- 0014: 最上位が 5 サブコマンドのみで、位置引数フォームが残っていないこと
- 0015: 保存先が単一 `profiles.toml`、`VSTC_CONFIG_DIR` が効くこと、書き込みが temp→rename であること
- 0016: 明示フラグがフィールド単位でプロファイルに勝ち、`profile set` がマージであること、既定プロファイルが存在しないこと
- 0017: `process_routes` のシグネチャが不変で、`process_routes_with_operand` が追加されていること

Expected: 乖離なし。乖離があれば実装を直すか、実装が正しければ該当 ADR を supersede する新 ADR を起こす（Accepted 本文は書き換えない）。

- [ ] **Step 4: フルゲートを通す**

Run: `just ci`
Expected: exit code 0（`fmt-check` / `clippy` / `clippy-profiling` / `test` / `test-profiling` / `check-env-leak` / `gitleaks` / `deny` / `audit` すべて緑）

`check-env-leak` が落ちた場合は、README やプランに個人/マシン依存の絶対パスが混入している。相対パスかプレースホルダへ置換する。

- [ ] **Step 5: コミット**

```bash
git add vstc_cli/README.md docs/adr
git commit -m "docs(vstc_cli): README をサブコマンド形式へ更新し ADR 0014-0017 を Accepted へ昇格"
```

---

## 受入基準の対応表

| spec の受入基準 | 実装タスク |
|---|---|
| `--help` に 5 サブコマンド | Task 2（send）/ Task 5（profile）/ Task 6（pause・resume・reload） |
| `send` が従来と同じ内容を送る | Task 2 |
| `pause` / `resume` が単独チェーンを送る | Task 6 |
| `reload` が `file_path` を載せて送る | Task 1（運搬路）/ Task 6（呼び出し） |
| 全送信コマンドが `--profile` / `-H` / `-p` を取る | Task 5（ConnArgs へ profile 追加）/ Task 6（3 コマンドへ適用） |
| `profile set` の保存とマージ更新 | Task 3（`merge`）/ Task 5（配線） |
| `profile list` の名前順表示と未設定表示 | Task 3（`render_list`）/ Task 5（配線） |
| 0 件の `list` が案内して正常終了 | Task 5（`print_profile_list`） |
| `profile remove` と不明名エラー | Task 3（`remove`）/ Task 5（配線） |
| OS 標準ディレクトリ配下の単一 TOML | Task 4 |
| `VSTC_CONFIG_DIR` による上書き | Task 4 |
| 保存先ディレクトリの自動作成 | Task 4（`save` の `create_dir_all`） |
| 中断で壊れない書き込み | Task 4（temp→rename） |
| 既定 `localhost:8080` | Task 3（`resolve`） |
| プロファイル > 既定 | Task 3（`resolve`） |
| 明示フラグ > プロファイル（フィールド単位） | Task 3（`resolve`） |
| `reload` は `--config-path` > プロファイル | Task 3（`resolve`）/ Task 6（`reload_config_path`） |
| 設定パス未解決でエラー・送信しない | Task 6（`reload_config_path`） |
| 未登録プロファイル名でエラー・送信しない | Task 3（`get`）/ Task 5（`resolve_conn`） |
| `--profile` 無しはファイル不在でも動く | Task 5（`resolve_conn` の早期 return） |
| `config_path` は `reload` 専用 | Task 5（`run_send` は `config_path: None` で解決） |
| README 更新 | Task 7 |
