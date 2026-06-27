# vstc_gui クライアント GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** vstreamer-protos ベースで、テキストとパイプライン（宛先/コマンド/パラメーター）を入力し gRPC 送信できる GUI クライアント crate `vstc_gui` を追加する。

**Architecture:** egui/eframe の単一ウィンドウアプリ。送信は `vstc` ライブラリの新規 `process_routes` を tokio ランタイム上で非同期実行し、結果をチャネル経由で UI に反映。前回入力は eframe 標準の Storage で OS ユーザーごとに自動保存/復元。パラメーターはサーバー `Params` 由来の 7 キーカタログ＋コマンド別 relevance マップでラベル付き入力する。

**Tech Stack:** Rust 2021 / eframe 0.35 (egui, glow, persistence) / tokio / serde / vstc(path) / vstreamer_protos(git tag v0.1.2)。

## Global Constraints

- **Windows 前提**。justfile は `set windows-shell := ["cmd.exe","/c"]`。コマンドは cmd 経由。
- **PR/マージ前に `just ci` が全緑（exit 0）必須**。`just clippy` は `cargo clippy --workspace --all-targets -- -D warnings`（**全 clippy 警告がエラー**）。
- 複雑度閾値（`-D warnings` で違反は失敗）: cognitive-complexity **25** / too-many-lines **120** / too-many-arguments **7**。関数は小さく保つ。
- `deny.toml` のライセンス allowlist は厳格。新規依存で未許可ライセンスが出たら、CLAUDE.md 方針に従い**原因調査の上、正当に必要なライセンスのみ allowlist に追記**（deny.toml 自身が想定する手順）。**確認の上で行い、lint スコープを緩めて黙らせない**。
- docs/spec/コードに**個人・マシン依存の絶対パスを書かない**（`check-env-leak`。検出対象は home ディレクトリ・AppData・Claude 内部プロジェクトパス等。`C:\Windows\Fonts\...` は対象外で可）。
- `vstreamer_protos` は **git tag `v0.1.2`**（`vstc` と同一タグ）。型同一性のためバージョンを一致させる。
- `eframe` は `default-features = false, features = ["glow", "persistence"]`（既定フォント crate を引かず、自前の日本語フォントを使う）。
- `vstc` クレートは `#![warn(missing_docs)]` + `clippy::pedantic`。新規 **pub** 関数には `///` ドキュメントと `## Errors` 節が必須。
- コミットメッセージは日本語 conventional commits。末尾に `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` を付与。
- pre-commit フック（`check-env-leak` + `gitleaks --staged`）が走る。`just setup-hooks` 済み前提。

## File Structure

- `Cargo.toml`（ルート）: workspace members に `"vstc_gui"` 追加。
- `vstc/src/lib.rs`: `build_operand` / `build_command`（private）と `process_routes`（pub）を追加。
- `vstc_gui/Cargo.toml`: 新規。
- `vstc_gui/src/main.rs`: エントリ。モジュール宣言、tokio ランタイム生成、`eframe::run_native`。
- `vstc_gui/src/opkind.rs`: `OpKind` enum（proto `Operation` のローカル写し）＋ `to_proto`/`label`/`ALL`/serde。
- `vstc_gui/src/catalog.rs`: `ParamKind`/`ParamSpec`/`PARAMS`(7件)/`spec`/`relevant_keys`。
- `vstc_gui/src/state.rs`: `AppState`/`PipelineStep`（serde、永続化スキーマ）。
- `vstc_gui/src/params.rs`: `validate_param`/`build_queries`/`build_routes`。
- `vstc_gui/src/fonts.rs`: `font_candidates`/`install_japanese_font`。
- `vstc_gui/src/app.rs`: `GuiApp`（eframe::App 実装）、UI 描画ヘルパ、非同期送信。

---

### Task 1: vstc_gui クレート雛形 + 依存/ライセンスゲート

eframe を引く新クレートを作り、ビルドと依存監査（とくにライセンス）を最初に通す。
新規依存ツリーが大きいため、ライセンス問題はここで早期に表面化させる。

**Files:**
- Modify: `Cargo.toml`（ルート、members に追加）
- Create: `vstc_gui/Cargo.toml`
- Create: `vstc_gui/src/main.rs`

**Interfaces:**
- Consumes: なし
- Produces: ビルド可能な `vstc_gui` バイナリ（空ウィンドウを表示する placeholder アプリ）。

- [ ] **Step 1: ルート Cargo.toml の members に vstc_gui を追加**

`Cargo.toml` の members 行を次に置き換える:

```toml
members = ["tcyb", "vstc", "vstc_cli", "igdb", "xtask", "vstc_gui"]
```

- [ ] **Step 2: vstc_gui/Cargo.toml を作成**

```toml
[package]
name = "vstc_gui"
version = "0.1.0"
edition = "2021"
description = "vstreamer client GUI"
publish = false

[lints]
workspace = true

[dependencies]
eframe = { version = "0.35", default-features = false, features = ["glow", "persistence"] }
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.28", features = ["rt-multi-thread"] }
vstc = { path = "../vstc" }
vstreamer_protos = { git = "https://github.com/sondeko143/vstreamer-protos", tag = "v0.1.2" }

[dev-dependencies]
serde_json = "1.0"
```

- [ ] **Step 3: 最小の placeholder アプリを main.rs に作成**

`vstc_gui/src/main.rs`:

```rust
use eframe::egui;

struct Placeholder;

impl eframe::App for Placeholder {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("vstc_gui placeholder");
    }
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "vstc_gui",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(Placeholder))),
    )
}
```

- [ ] **Step 4: ビルドして依存を解決**

Run: `cargo build -p vstc_gui`
Expected: 初回は eframe/winit/glow 等を大量にコンパイルし、最終的に成功（warning は可）。

> もしリンク/バックエンド不足でウィンドウ生成に失敗する、または起動時に画面が出ない場合は、`vstc_gui/Cargo.toml` の eframe を `eframe = "0.35"`（default features）に変更して再ビルドし先へ進む。その場合 Task 9 のライセンスゲートでフォント crate のライセンス追記が必要になることがある。

- [ ] **Step 5: ライセンス/依存監査を通す**

Run: `cargo deny check licenses`
Expected: `licenses ok`。未許可ライセンスが出たら、出力されたクレートとライセンスを確認し、**正当に必要なもののみ** `deny.toml` の `[licenses] allow` に追記する（例: フォント由来の `OFL-1.1` 等）。判断に迷うものは止めて相談。`bans`/`sources` の warning は許容。

Run: `cargo deny check`
Expected: advisories/sources は ok（multiple-versions は warn 許容）。

- [ ] **Step 6: 起動確認（手動）**

Run: `cargo run -p vstc_gui`
Expected: 「vstc_gui placeholder」と書かれた空ウィンドウが表示される。ウィンドウを閉じて終了。

- [ ] **Step 7: コミット**

```bash
git add Cargo.toml Cargo.lock vstc_gui/Cargo.toml vstc_gui/src/main.rs deny.toml
git commit -m "feat(vstc_gui): クレート雛形と依存/ライセンスゲートを整備

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: vstc に構造化送信 process_routes を追加

GUI が URL 文字列を経由せず、構造化された route を直接送れるようにする。
接続/送信ロジックは既存 `process_command` と共通の作法を踏襲する。

**Files:**
- Modify: `vstc/src/lib.rs`

**Interfaces:**
- Consumes: `vstreamer_protos::{OperationRoute, Response}`、既存の `unix_timestamp_secs()`。
- Produces:
  - `fn build_operand(text: String) -> Operand`（private）
  - `fn build_command(routes: Vec<OperationRoute>, text: String) -> Command`（private）
  - `pub async fn process_routes(uri: &str, routes: Vec<OperationRoute>, text: String) -> Result<Response, VstcError>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc/src/lib.rs` の `mod tests { ... }` 内に追加:

```rust
    #[test]
    fn build_command_wraps_routes_in_single_chain() {
        let routes = vec![OperationRoute {
            operation: Operation::Tts as i32,
            remote: String::new(),
            queries: HashMap::new(),
        }];
        let cmd = build_command(routes.clone(), "hello".to_string());
        assert_eq!(cmd.chains.len(), 1);
        assert_eq!(cmd.chains[0].operations, routes);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hello");
        assert!(operand.sound.is_none());
        assert!(!operand.trace_id.is_empty());
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p vstc build_command_wraps_routes_in_single_chain`
Expected: コンパイルエラー（`build_command` 未定義）で FAIL。

- [ ] **Step 3: build_operand / build_command / process_routes を実装**

`vstc/src/lib.rs` の `convert_to_operation` の前（または `process_command` の後）に追加:

```rust
/// Build a text-only `Operand` with a fresh trace id and current origin timestamp.
fn build_operand(text: String) -> Operand {
    Operand {
        text,
        sound: None,
        file_path: String::new(),
        filters: Vec::new(),
        trace_id: Uuid::new_v4().to_string(),
        origin_ts: unix_timestamp_secs(),
    }
}

/// Wrap the given routes into a single-chain `Command` carrying a text-only operand.
fn build_command(routes: Vec<OperationRoute>, text: String) -> Command {
    Command {
        chains: vec![OperationChain { operations: routes }],
        operand: Some(build_operand(text)),
    }
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
    let endpoint = tonic::transport::Endpoint::new(uri.to_string())?
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS));
    let mut channel = CommanderClient::connect(endpoint).await?;
    let c = tonic::Request::new(build_command(routes, text));
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p vstc build_command_wraps_routes_in_single_chain`
Expected: PASS（test result: ok. 1 passed）。

- [ ] **Step 5: クレート全体の lint/テストを確認**

Run: `cargo clippy -p vstc --all-targets -- -D warnings`
Expected: 警告なしで成功（missing_docs / pedantic も満たす）。

Run: `cargo test -p vstc`
Expected: 既存テスト含め全て PASS。

- [ ] **Step 6: コミット**

```bash
git add vstc/src/lib.rs
git commit -m "feat(vstc): 構造化 route 送信 process_routes を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: OpKind モジュール（proto Operation のローカル写し）

`Operation` は `Serialize` を持たないため、永続化・UI 用にローカル enum を用意する。

**Files:**
- Create: `vstc_gui/src/opkind.rs`
- Modify: `vstc_gui/src/main.rs`（`mod opkind;` 追加）

**Interfaces:**
- Consumes: `vstreamer_protos::Operation`
- Produces:
  - `pub enum OpKind { Transcribe, Translate, Subtitle, Tts, Vc, Playback, Pause, Resume, Reload, SetFilters, Ping, Forward }`（`Copy + PartialEq + Serialize + Deserialize`、`Default = Tts`）
  - `pub const OpKind::ALL: [OpKind; 12]`
  - `pub fn OpKind::to_proto(self) -> Operation`
  - `pub fn OpKind::label(self) -> &'static str`
  - `impl Display for OpKind`

- [ ] **Step 1: main.rs にモジュール宣言を追加**

`vstc_gui/src/main.rs` の先頭（`use eframe::egui;` の上）に追加:

```rust
mod opkind;
```

- [ ] **Step 2: 失敗するテストを書く**

`vstc_gui/src/opkind.rs` を作成（テストのみ先に置き、本体は次ステップ）:

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use vstreamer_protos::Operation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_proto_maps_each_variant() {
        assert_eq!(OpKind::Tts.to_proto(), Operation::Tts);
        assert_eq!(OpKind::Translate.to_proto(), Operation::Translate);
        assert_eq!(OpKind::Forward.to_proto(), Operation::Forward);
    }

    #[test]
    fn all_has_twelve_unique_variants() {
        assert_eq!(OpKind::ALL.len(), 12);
    }

    #[test]
    fn label_is_non_empty_for_all() {
        for op in OpKind::ALL {
            assert!(!op.label().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        for op in OpKind::ALL {
            let json = serde_json::to_string(&op).expect("serialize");
            let back: OpKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, op);
        }
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p vstc_gui opkind`
Expected: コンパイルエラー（`OpKind` 未定義）で FAIL。

- [ ] **Step 4: OpKind 本体を実装**

`vstc_gui/src/opkind.rs` の `#[cfg(test)]` ブロックの上に追加:

```rust
/// Local, serializable mirror of proto `Operation` (which lacks `Serialize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    Transcribe,
    Translate,
    Subtitle,
    Tts,
    Vc,
    Playback,
    Pause,
    Resume,
    Reload,
    SetFilters,
    Ping,
    Forward,
}

impl OpKind {
    /// Every variant, for populating the command dropdown.
    pub const ALL: [OpKind; 12] = [
        OpKind::Transcribe,
        OpKind::Translate,
        OpKind::Subtitle,
        OpKind::Tts,
        OpKind::Vc,
        OpKind::Playback,
        OpKind::Pause,
        OpKind::Resume,
        OpKind::Reload,
        OpKind::SetFilters,
        OpKind::Ping,
        OpKind::Forward,
    ];

    /// Convert to the proto enum used on the wire.
    pub fn to_proto(self) -> Operation {
        match self {
            OpKind::Transcribe => Operation::Transcribe,
            OpKind::Translate => Operation::Translate,
            OpKind::Subtitle => Operation::Subtitle,
            OpKind::Tts => Operation::Tts,
            OpKind::Vc => Operation::Vc,
            OpKind::Playback => Operation::Playback,
            OpKind::Pause => Operation::Pause,
            OpKind::Resume => Operation::Resume,
            OpKind::Reload => Operation::Reload,
            OpKind::SetFilters => Operation::SetFilters,
            OpKind::Ping => Operation::Ping,
            OpKind::Forward => Operation::Forward,
        }
    }

    /// Human-facing dropdown label.
    pub fn label(self) -> &'static str {
        match self {
            OpKind::Transcribe => "文字起こし (transcribe)",
            OpKind::Translate => "翻訳 (translate)",
            OpKind::Subtitle => "字幕 (subtitle)",
            OpKind::Tts => "読み上げ (tts)",
            OpKind::Vc => "声質変換 (vc)",
            OpKind::Playback => "再生 (playback)",
            OpKind::Pause => "一時停止 (pause)",
            OpKind::Resume => "再開 (resume)",
            OpKind::Reload => "リロード (reload)",
            OpKind::SetFilters => "フィルタ設定 (set_filters)",
            OpKind::Ping => "ping",
            OpKind::Forward => "転送 (forward)",
        }
    }
}

impl Default for OpKind {
    fn default() -> Self {
        OpKind::Tts
    }
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p vstc_gui opkind`
Expected: PASS（4 tests）。

- [ ] **Step 6: コミット**

```bash
git add vstc_gui/src/opkind.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): OpKind（Operation のローカル写し）を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: パラメーターカタログモジュール

サーバー `Params`（shared_context.py）由来の 7 キーと、コマンド別 relevance マップ。

**Files:**
- Create: `vstc_gui/src/catalog.rs`
- Modify: `vstc_gui/src/main.rs`（`mod catalog;` 追加）

**Interfaces:**
- Consumes: `vstreamer_protos::Operation`
- Produces:
  - `pub enum ParamKind { Text, Int, Float, Enum(&'static [&'static str]) }`（`Copy + PartialEq + Eq`）
  - `pub struct ParamSpec { pub key: &'static str, pub label: &'static str, pub kind: ParamKind }`（`Copy`）
  - `pub const PARAMS: &[ParamSpec]`（7 件）
  - `pub fn spec(key: &str) -> Option<&'static ParamSpec>`
  - `pub fn relevant_keys(op: Operation) -> &'static [&'static str]`

- [ ] **Step 1: main.rs にモジュール宣言を追加**

`vstc_gui/src/main.rs` の `mod opkind;` の下に追加:

```rust
mod catalog;
```

- [ ] **Step 2: 失敗するテストを書く**

`vstc_gui/src/catalog.rs` を作成（テスト先置き）:

```rust
use vstreamer_protos::Operation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_seven_params() {
        assert_eq!(PARAMS.len(), 7);
    }

    #[test]
    fn spec_lookup_returns_kind() {
        assert_eq!(spec("spd").expect("spd").kind, ParamKind::Float);
        assert_eq!(spec("i").expect("i").kind, ParamKind::Int);
        assert!(matches!(spec("p").expect("p").kind, ParamKind::Enum(_)));
        assert!(spec("unknown").is_none());
    }

    #[test]
    fn relevance_matches_design() {
        assert_eq!(relevant_keys(Operation::Translate), &["t", "s"]);
        assert_eq!(relevant_keys(Operation::Tts), &["i", "spd", "pit"]);
        assert_eq!(relevant_keys(Operation::Playback), &["v"]);
        assert!(relevant_keys(Operation::Ping).is_empty());
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p vstc_gui catalog`
Expected: コンパイルエラー（`PARAMS` 等未定義）で FAIL。

- [ ] **Step 4: カタログ本体を実装**

`vstc_gui/src/catalog.rs` の `#[cfg(test)]` ブロックの上に追加:

```rust
/// The value type of a parameter, used to render the right input widget and to validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Text,
    Int,
    Float,
    Enum(&'static [&'static str]),
}

/// One known query parameter (key/label/type), mirroring server `Params`.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
}

/// The complete, authoritative parameter set (server `Params` class). Flat: any
/// subset is valid on any route; `relevant_keys` only decides default visibility.
pub const PARAMS: &[ParamSpec] = &[
    ParamSpec { key: "t", label: "翻訳先言語 (t)", kind: ParamKind::Text },
    ParamSpec { key: "s", label: "翻訳元言語 (s)", kind: ParamKind::Text },
    ParamSpec { key: "p", label: "位置 (p)", kind: ParamKind::Enum(&["s", "n"]) },
    ParamSpec { key: "i", label: "話者ID (i)", kind: ParamKind::Int },
    ParamSpec { key: "v", label: "音量 (v)", kind: ParamKind::Int },
    ParamSpec { key: "spd", label: "速度 (spd)", kind: ParamKind::Float },
    ParamSpec { key: "pit", label: "ピッチ (pit)", kind: ParamKind::Float },
];

/// Look up a parameter spec by its query key.
pub fn spec(key: &str) -> Option<&'static ParamSpec> {
    PARAMS.iter().find(|p| p.key == key)
}

/// Keys shown by default for a command. Best-effort (README + semantics);
/// all other keys remain available via the "その他" expander.
pub fn relevant_keys(op: Operation) -> &'static [&'static str] {
    match op {
        Operation::Translate => &["t", "s"],
        Operation::Tts => &["i", "spd", "pit"],
        Operation::Playback => &["v"],
        Operation::Subtitle => &["p"],
        Operation::Vc => &["i"],
        _ => &[],
    }
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p vstc_gui catalog`
Expected: PASS（3 tests）。

- [ ] **Step 6: コミット**

```bash
git add vstc_gui/src/catalog.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): パラメーターカタログ(7キー)と relevance マップを追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: 状態モジュール（AppState / PipelineStep）

永続化される入力状態のスキーマ。serde ラウンドトリップで退行を防ぐ。

**Files:**
- Create: `vstc_gui/src/state.rs`
- Modify: `vstc_gui/src/main.rs`（`mod state;` 追加）

**Interfaces:**
- Consumes: `crate::opkind::OpKind`
- Produces:
  - `pub struct PipelineStep { pub op: OpKind, pub remote: String, pub params: BTreeMap<String, String> }`（`Clone + Serialize + Deserialize + Default`）
  - `pub struct AppState { pub host: String, pub port: u16, pub text: String, pub steps: Vec<PipelineStep> }`（`Clone + Serialize + Deserialize + Default`、default は localhost:8080・1 ステップ）

- [ ] **Step 1: main.rs にモジュール宣言を追加**

`vstc_gui/src/main.rs` の `mod catalog;` の下に追加:

```rust
mod state;
```

- [ ] **Step 2: 失敗するテストを書く**

`vstc_gui/src/state.rs` を作成（テスト先置き）:

```rust
use crate::opkind::OpKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_one_step_and_localhost() {
        let s = AppState::default();
        assert_eq!(s.host, "localhost");
        assert_eq!(s.port, 8080);
        assert_eq!(s.steps.len(), 1);
    }

    #[test]
    fn appstate_serde_round_trip() {
        let mut step = PipelineStep { op: OpKind::Tts, ..Default::default() };
        step.remote = "//localhost:8080".to_string();
        step.params.insert("spd".to_string(), "1.1".to_string());
        let state = AppState {
            host: "h".to_string(),
            port: 1234,
            text: "hi".to_string(),
            steps: vec![step],
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: AppState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.host, "h");
        assert_eq!(back.port, 1234);
        assert_eq!(back.text, "hi");
        assert_eq!(back.steps.len(), 1);
        assert_eq!(back.steps[0].op, OpKind::Tts);
        assert_eq!(back.steps[0].remote, "//localhost:8080");
        assert_eq!(back.steps[0].params.get("spd").map(String::as_str), Some("1.1"));
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p vstc_gui state`
Expected: コンパイルエラー（`AppState` 等未定義）で FAIL。

- [ ] **Step 4: 状態型を実装**

`vstc_gui/src/state.rs` の `#[cfg(test)]` ブロックの上に追加:

```rust
/// One pipeline step (maps to a proto `OperationRoute`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub op: OpKind,
    pub remote: String,
    /// Raw text buffers keyed by param key; empty / missing means unset.
    pub params: BTreeMap<String, String>,
}

impl Default for PipelineStep {
    fn default() -> Self {
        Self {
            op: OpKind::default(),
            remote: String::new(),
            params: BTreeMap::new(),
        }
    }
}

/// Persisted UI state (serialized via eframe Storage, per OS user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub host: String,
    pub port: u16,
    pub text: String,
    pub steps: Vec<PipelineStep>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
            text: String::new(),
            steps: vec![PipelineStep::default()],
        }
    }
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p vstc_gui state`
Expected: PASS（2 tests）。

- [ ] **Step 6: コミット**

```bash
git add vstc_gui/src/state.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): 永続化状態 AppState/PipelineStep を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: パラメーター検証 + route 構築モジュール

入力値の型検証と、ステップ群からの proto route 構築。送信前バリデーションの中核。

**Files:**
- Create: `vstc_gui/src/params.rs`
- Modify: `vstc_gui/src/main.rs`（`mod params;` 追加）

**Interfaces:**
- Consumes: `crate::catalog::{spec, ParamKind}`、`crate::state::PipelineStep`、`crate::opkind::OpKind`（`PipelineStep.op.to_proto()` 経由）、`vstreamer_protos::OperationRoute`
- Produces:
  - `pub fn validate_param(kind: ParamKind, raw: &str) -> Result<(), String>`
  - `pub fn build_queries(step: &PipelineStep) -> Result<HashMap<String, String>, Vec<String>>`
  - `pub fn build_routes(steps: &[PipelineStep]) -> Result<Vec<OperationRoute>, Vec<String>>`

- [ ] **Step 1: main.rs にモジュール宣言を追加**

`vstc_gui/src/main.rs` の `mod state;` の下に追加:

```rust
mod params;
```

- [ ] **Step 2: 失敗するテストを書く**

`vstc_gui/src/params.rs` を作成（テスト先置き）:

```rust
use crate::catalog::{spec, ParamKind};
use crate::state::PipelineStep;
use std::collections::HashMap;
use vstreamer_protos::OperationRoute;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opkind::OpKind;

    fn step_with(params: &[(&str, &str)]) -> PipelineStep {
        let mut s = PipelineStep::default();
        for (k, v) in params {
            s.params.insert((*k).to_string(), (*v).to_string());
        }
        s
    }

    #[test]
    fn validate_int_rejects_non_numeric() {
        assert!(validate_param(ParamKind::Int, "abc").is_err());
        assert!(validate_param(ParamKind::Int, "3").is_ok());
    }

    #[test]
    fn validate_float_accepts_decimal() {
        assert!(validate_param(ParamKind::Float, "1.1").is_ok());
        assert!(validate_param(ParamKind::Float, "x").is_err());
    }

    #[test]
    fn validate_enum_checks_allowed() {
        assert!(validate_param(ParamKind::Enum(&["s", "n"]), "s").is_ok());
        assert!(validate_param(ParamKind::Enum(&["s", "n"]), "x").is_err());
    }

    #[test]
    fn build_queries_skips_empty_and_keeps_filled() {
        let step = step_with(&[("spd", "1.1"), ("i", ""), ("pit", "  ")]);
        let q = build_queries(&step).expect("ok");
        assert_eq!(q.get("spd").map(String::as_str), Some("1.1"));
        assert!(!q.contains_key("i"));
        assert!(!q.contains_key("pit"));
    }

    #[test]
    fn build_queries_reports_type_error() {
        let step = step_with(&[("i", "abc")]);
        assert!(build_queries(&step).is_err());
    }

    #[test]
    fn build_routes_sets_operation_remote_queries() {
        let mut step = step_with(&[("spd", "1.1")]);
        step.op = OpKind::Tts;
        step.remote = "//localhost:8080".to_string();
        let routes = build_routes(&[step]).expect("ok");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].operation, vstreamer_protos::Operation::Tts as i32);
        assert_eq!(routes[0].remote, "//localhost:8080");
        assert_eq!(routes[0].queries.get("spd").map(String::as_str), Some("1.1"));
    }

    #[test]
    fn build_routes_propagates_error_with_step_index() {
        let step = step_with(&[("i", "x")]);
        let err = build_routes(&[step]).expect_err("err");
        assert!(err[0].contains("ステップ 1"));
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p vstc_gui params`
Expected: コンパイルエラー（`validate_param` 等未定義）で FAIL。

- [ ] **Step 4: 検証・構築を実装**

`vstc_gui/src/params.rs` の `#[cfg(test)]` ブロックの上に追加:

```rust
/// Validate a single non-empty raw value against its declared kind.
pub fn validate_param(kind: ParamKind, raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    match kind {
        ParamKind::Text => Ok(()),
        ParamKind::Int => raw
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| format!("整数を入力してください: '{raw}'")),
        ParamKind::Float => raw
            .parse::<f64>()
            .map(|_| ())
            .map_err(|_| format!("数値を入力してください: '{raw}'")),
        ParamKind::Enum(allowed) => {
            if allowed.contains(&raw) {
                Ok(())
            } else {
                Err(format!(
                    "{} のいずれかを入力してください: '{}'",
                    allowed.join("/"),
                    raw
                ))
            }
        }
    }
}

/// Build the proto `queries` map for one step from its non-empty params.
/// Unknown keys (not in the catalog) are passed through verbatim.
pub fn build_queries(step: &PipelineStep) -> Result<HashMap<String, String>, Vec<String>> {
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    for (key, raw) in &step.params {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match spec(key) {
            Some(s) => match validate_param(s.kind, trimmed) {
                Ok(()) => {
                    out.insert(key.clone(), trimmed.to_string());
                }
                Err(e) => errors.push(format!("{}: {}", s.label, e)),
            },
            None => {
                out.insert(key.clone(), trimmed.to_string());
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Build all proto routes from the pipeline steps. Errors are prefixed by step number.
pub fn build_routes(steps: &[PipelineStep]) -> Result<Vec<OperationRoute>, Vec<String>> {
    let mut routes = Vec::new();
    let mut errors = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        match build_queries(step) {
            Ok(queries) => routes.push(OperationRoute {
                operation: step.op.to_proto().into(),
                remote: step.remote.trim().to_string(),
                queries,
            }),
            Err(errs) => {
                for e in errs {
                    errors.push(format!("ステップ {}: {}", idx + 1, e));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(routes)
    } else {
        Err(errors)
    }
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p vstc_gui params`
Expected: PASS（7 tests）。

- [ ] **Step 6: コミット**

```bash
git add vstc_gui/src/params.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): パラメーター検証と route 構築を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: 日本語フォント読み込みモジュール

egui の既定フォントは日本語非対応のため、Windows 同梱フォントを最優先で読み込む。

**Files:**
- Create: `vstc_gui/src/fonts.rs`
- Modify: `vstc_gui/src/main.rs`（`mod fonts;` 追加）

**Interfaces:**
- Consumes: `eframe::egui`
- Produces:
  - `pub fn font_candidates() -> &'static [&'static str]`
  - `pub fn install_japanese_font(ctx: &egui::Context)`

- [ ] **Step 1: main.rs にモジュール宣言を追加**

`vstc_gui/src/main.rs` の `mod params;` の下に追加:

```rust
mod fonts;
```

- [ ] **Step 2: 失敗するテストを書く**

`vstc_gui/src/fonts.rs` を作成（テスト先置き）:

```rust
use eframe::egui;
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_candidates_non_empty_and_windows_fonts() {
        let c = font_candidates();
        assert!(!c.is_empty());
        assert!(c.iter().all(|p| p.contains("Fonts")));
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p vstc_gui fonts`
Expected: コンパイルエラー（`font_candidates` 未定義）で FAIL。

- [ ] **Step 4: フォント読み込みを実装**

`vstc_gui/src/fonts.rs` の `#[cfg(test)]` ブロックの上に追加:

```rust
/// Japanese-capable fonts bundled with Windows, in preference order.
pub fn font_candidates() -> &'static [&'static str] {
    &[
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ]
}

/// Install a Japanese system font as the top-priority family. If none is found,
/// logs a warning and leaves egui's (empty) default fonts in place.
pub fn install_japanese_font(ctx: &egui::Context) {
    let Some((path, bytes)) = font_candidates()
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (*p, b)))
    else {
        eprintln!("warning: 日本語システムフォントが見つかりません。日本語が表示されない可能性があります");
        return;
    };
    eprintln!("loaded Japanese font: {path}");
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("jp".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "jp".to_owned());
    }
    ctx.set_fonts(fonts);
}
```

> 補足: `.ttc`（フォントコレクション）は egui が index 0 で読み込む。万一グリフが出ない場合は `egui::FontData::from_owned` を index 指定版に置き換える（troubleshooting）。

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p vstc_gui fonts`
Expected: PASS（1 test）。

- [ ] **Step 6: コミット**

```bash
git add vstc_gui/src/fonts.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): 日本語フォント読込を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: GuiApp（UI + 非同期送信）と main の結線

全モジュールを使う本体。eframe::App 実装、UI 描画、tokio による非同期送信、永続化。

**Files:**
- Create: `vstc_gui/src/app.rs`
- Modify: `vstc_gui/src/main.rs`（placeholder を撤去し本体に結線）

**Interfaces:**
- Consumes: `crate::catalog::{relevant_keys, spec, ParamKind, PARAMS}`、`crate::fonts::install_japanese_font`、`crate::opkind::OpKind`、`crate::params::build_routes`、`crate::state::{AppState, PipelineStep}`、`vstc::{process_routes, VstcError}`、`vstreamer_protos::Response`、`tokio::runtime::Runtime`
- Produces: `pub struct GuiApp`、`pub fn GuiApp::new(cc: &eframe::CreationContext<'_>, runtime: Runtime) -> Self`、`impl eframe::App for GuiApp`

- [ ] **Step 1: app.rs を作成（型・送信ロジック）**

`vstc_gui/src/app.rs`:

```rust
use crate::catalog::{relevant_keys, spec, ParamKind, PARAMS};
use crate::fonts::install_japanese_font;
use crate::opkind::OpKind;
use crate::params::build_routes;
use crate::state::{AppState, PipelineStep};
use eframe::egui;
use std::sync::mpsc::{Receiver, TryRecvError};
use tokio::runtime::Runtime;
use vstc::VstcError;
use vstreamer_protos::Response;

enum SendStatus {
    Idle,
    Sending,
    Success,
    Error(String),
}

pub struct GuiApp {
    state: AppState,
    runtime: Runtime,
    status: SendStatus,
    result_rx: Option<Receiver<Result<Response, VstcError>>>,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: Runtime) -> Self {
        install_japanese_font(&cc.egui_ctx);
        let state = cc
            .storage
            .and_then(|s| eframe::get_value::<AppState>(s, eframe::APP_KEY))
            .unwrap_or_default();
        Self {
            state,
            runtime,
            status: SendStatus::Idle,
            result_rx: None,
        }
    }

    fn start_send(&mut self, ctx: &egui::Context) {
        let routes = match build_routes(&self.state.steps) {
            Ok(routes) => routes,
            Err(errors) => {
                self.status = SendStatus::Error(errors.join("\n"));
                return;
            }
        };
        let uri = format!("http://{}:{}", self.state.host.trim(), self.state.port);
        let text = self.state.text.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.result_rx = Some(rx);
        self.status = SendStatus::Sending;
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = vstc::process_routes(&uri, routes, text).await;
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn poll_result(&mut self) {
        let Some(rx) = &self.result_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(resp)) => {
                self.status = if resp.result {
                    SendStatus::Success
                } else {
                    SendStatus::Error("サーバーが result=false を返しました".to_string())
                };
                self.result_rx = None;
            }
            Ok(Err(e)) => {
                self.status = SendStatus::Error(e.to_string());
                self.result_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = SendStatus::Error("送信タスクが異常終了しました".to_string());
                self.result_rx = None;
            }
        }
    }
}
```

- [ ] **Step 2: app.rs に描画ヘルパ（自由関数）を追加**

`vstc_gui/src/app.rs` の末尾に追加:

```rust
/// Render one parameter input row, bound to `step.params[key]`.
fn param_field(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep, key: &str) {
    let Some(s) = spec(key) else {
        return;
    };
    let value = step.params.entry(key.to_string()).or_default();
    ui.horizontal(|ui| {
        ui.label(s.label);
        match s.kind {
            ParamKind::Enum(allowed) => {
                let selected = if value.is_empty() {
                    "(未設定)".to_owned()
                } else {
                    value.clone()
                };
                egui::ComboBox::from_id_salt(format!("param-{idx}-{key}"))
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), "(未設定)");
                        for opt in allowed {
                            ui.selectable_value(value, (*opt).to_string(), *opt);
                        }
                    });
            }
            _ => {
                ui.text_edit_singleline(value);
            }
        }
    });
}

/// Render the relevant + "other" parameter fields for a step.
fn step_params(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep) {
    let relevant = relevant_keys(step.op.to_proto());
    for key in relevant {
        param_field(ui, idx, step, key);
    }
    let others: Vec<&'static str> = PARAMS
        .iter()
        .map(|p| p.key)
        .filter(|k| !relevant.contains(k))
        .collect();
    if !others.is_empty() {
        ui.collapsing("その他のパラメーター", |ui| {
            for key in &others {
                param_field(ui, idx, step, key);
            }
        });
    }
}

/// Render one pipeline step card. Returns true if its delete button was clicked.
fn step_card(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep) -> bool {
    let mut delete = false;
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(format!("ステップ {}", idx + 1));
            egui::ComboBox::from_id_salt(format!("op-{idx}"))
                .selected_text(step.op.label())
                .show_ui(ui, |ui| {
                    for op in OpKind::ALL {
                        ui.selectable_value(&mut step.op, op, op.label());
                    }
                });
            if ui.button("削除").clicked() {
                delete = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("宛先 (remote):");
            ui.text_edit_singleline(&mut step.remote);
        });
        step_params(ui, idx, step);
    });
    delete
}
```

- [ ] **Step 3: app.rs に GuiApp の UI メソッドと eframe::App 実装を追加**

`vstc_gui/src/app.rs` の `impl GuiApp { ... }` ブロック内（`poll_result` の後、`}` の前）に UI メソッドを追加:

```rust
    fn ui_endpoint(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("接続先:");
            ui.label("host");
            ui.text_edit_singleline(&mut self.state.host);
            ui.label("port");
            ui.add(egui::DragValue::new(&mut self.state.port));
        });
    }

    fn ui_text(&mut self, ui: &mut egui::Ui) {
        ui.label("送信テキスト");
        ui.add(
            egui::TextEdit::multiline(&mut self.state.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
    }

    fn ui_pipeline(&mut self, ui: &mut egui::Ui) {
        ui.label("パイプライン");
        let mut delete_idx: Option<usize> = None;
        for (idx, step) in self.state.steps.iter_mut().enumerate() {
            if step_card(ui, idx, step) {
                delete_idx = Some(idx);
            }
        }
        if let Some(idx) = delete_idx {
            self.state.steps.remove(idx);
        }
        if ui.button("＋ ステップ追加").clicked() {
            self.state.steps.push(PipelineStep::default());
        }
    }

    fn ui_send(&mut self, ui: &mut egui::Ui) {
        let sending = matches!(self.status, SendStatus::Sending);
        let clicked = ui
            .add_enabled(!sending, egui::Button::new("送信"))
            .clicked();
        if clicked {
            let ctx = ui.ctx().clone();
            self.start_send(&ctx);
        }
        match &self.status {
            SendStatus::Idle => {
                ui.label("状態: 待機中");
            }
            SendStatus::Sending => {
                ui.label("状態: 送信中…");
            }
            SendStatus::Success => {
                ui.colored_label(egui::Color32::GREEN, "状態: 成功");
            }
            SendStatus::Error(e) => {
                ui.colored_label(egui::Color32::RED, format!("エラー: {e}"));
            }
        }
    }
```

末尾（自由関数群の後、ファイル最後）に eframe::App 実装を追加:

```rust
impl eframe::App for GuiApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_result();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("vstreamer クライアント");
            self.ui_endpoint(ui);
            ui.separator();
            self.ui_text(ui);
            ui.separator();
            self.ui_pipeline(ui);
            ui.separator();
            self.ui_send(ui);
        });
    }
}
```

- [ ] **Step 4: main.rs を本体に結線**

`vstc_gui/src/main.rs` を全置換:

```rust
mod app;
mod catalog;
mod fonts;
mod opkind;
mod params;
mod state;

use app::GuiApp;

fn main() -> eframe::Result<()> {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "vstc_gui",
        native_options,
        Box::new(move |cc| Ok(Box::new(GuiApp::new(cc, runtime)))),
    )
}
```

- [ ] **Step 5: ビルドと lint を確認**

Run: `cargo build -p vstc_gui`
Expected: 成功。

Run: `cargo clippy -p vstc_gui --all-targets -- -D warnings`
Expected: 警告ゼロで成功（複雑度・行数閾値も満たす）。失敗したら、超過した関数をさらに小さなヘルパに分割する。

- [ ] **Step 6: テスト（全モジュール）を確認**

Run: `cargo test -p vstc_gui`
Expected: 既存の全ユニットテストが PASS。

- [ ] **Step 7: 手動動作確認**

Run: `cargo run -p vstc_gui`
Expected:
- 日本語ラベル（接続先 / 送信テキスト / パイプライン / 送信 等）が文字化けせず表示される。
- コマンドのドロップダウンで TTS を選ぶと 話者ID(i)/速度(spd)/ピッチ(pit) 欄が出る。「その他のパラメーター」を開くと残りのキーが出る。
- 「＋ ステップ追加」で行が増え、「削除」で消える。
- 「送信」を押すと（サーバー未起動なら）状態が「送信中…」→赤いエラー表示になる。UI は固まらない。
- アプリを閉じて再度 `cargo run -p vstc_gui` すると、前回の host/port/テキスト/ステップが復元される。

- [ ] **Step 8: コミット**

```bash
git add vstc_gui/src/app.rs vstc_gui/src/main.rs
git commit -m "feat(vstc_gui): UI と非同期送信・永続化を実装

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: フルゲート（just ci）と最終確認

ワークスペース全体の品質ゲートを通し、必要ならライセンスを正当に追記する。

**Files:**
- Modify（必要時のみ）: `deny.toml`

**Interfaces:**
- Consumes: 全タスクの成果
- Produces: `just ci` 全緑

- [ ] **Step 1: フルゲートを実行**

Run: `just ci`
Expected: fmt-check / clippy / clippy-profiling / test / test-profiling / check-env-leak / gitleaks / deny / audit が全て成功（exit 0）。

- [ ] **Step 2: 赤への対応（出た場合のみ）**

- `fmt-check` 赤 → `just fmt` で整形して再実行。
- `clippy` 赤 → 原因の関数を分割/修正（複雑度・行数・default lint）。lint を緩めない。
- `deny`（licenses）赤 → 出力のクレートとライセンスを確認し、**正当に必要なライセンスのみ** `deny.toml` の `[licenses] allow` に追記。判断が要るものは止めて相談。
- `audit` 赤 → 脆弱性は CLAUDE.md 方針で対応（既知の `yaml-rust` unmaintained は非ブロッキング）。
- `check-env-leak` 赤 → 個人/マシン依存の絶対パスを除去（フォントの `C:\Windows\Fonts\...` は対象外で問題なし）。

修正したら `just ci` を再実行し緑を確認。

- [ ] **Step 3: deny.toml を変更した場合はコミット**

```bash
git add deny.toml
git commit -m "build(vstc_gui): フォント由来ライセンスを deny allowlist に追記

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

> deny.toml を変更しなかった場合、このタスクのコミットは不要（すべて既存コミット済み）。

- [ ] **Step 4: 完了確認**

Run: `just ci`
Expected: 全緑（exit 0）。これで PR 作成可能な状態。
