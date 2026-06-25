# reqwest / axum / hyper / tonic 近代化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rustls-pemfile`(RUSTSEC-2025-0134) を依存ツリーから除去し、reqwest / axum / hyper / tonic を hyper 1.x 系へ揃えて hyper の二重化を解消する。

**Architecture:** 2 リポジトリにまたがる純粋な依存近代化（挙動は不変）。前提として外部 git 依存 `vstreamer-protos`(ローカル: `../vstreamer-protos`) の Rust スタブを新 tonic/prost で再生成し `v0.1.2` としてタグ・リリース。本体(`twitch-chat-yomiage-bot`)は開発中 `[patch]` でローカル protos を参照して反復し、確定時にタグ固定参照へ切替える。

**Tech Stack:** Rust workspace（`tcyb`/`vstc`/`vstc_cli`/`igdb`）、tonic gRPC、prost、axum、reqwest(native-tls)、tokio。Windows + `just`（`set windows-shell := ["cmd.exe","/c"]`）。

> **重要 — これは migration です。** 新規 behavior が無いため「先に失敗するテストを書く」TDD は適用しません。各タスクの検証は (1) `cargo build`/`cargo test` のコンパイル成功、(2) 既存テストスイートの緑、(3) `cargo tree` アサーション で行います。新規にコードを書く箇所（auth.rs、vstp_builder）は具体コードを提示し、外部 codegen に依存する箇所（再生成 `voicerecog.rs`、tonic 0.14 の生成参照、tonic-reflection/async_trait の API）は **discover-and-adapt**（コンパイラ + `cargo doc` で実シンボルを確認して追従）とし、難航時の **tonic 0.12 フォールバック** を末尾に明記します。

## Global Constraints

- ターゲット版（本命）: reqwest **0.13**、axum **0.8**、hyper(tcyb direct) **削除**、tonic **0.14**、tonic-reflection **0.14**、prost(protos 側) **0.14**、tonic-prost-build(protos 側) **0.14**。難航時は tonic/prost/builder のみ **0.12 系** へフォールバック（§フォールバック参照）。
- TLS は **native-tls 据え置き**（reqwest は `features = ["json"]`・default-features 有効のまま。rustls を導入しない）。
- 挙動・公開 API は不変。リファクタや無関係な整理はしない（例外: 合意済みの「tcyb 未使用 `tonic` 削除」「tcyb direct `hyper` 削除」）。
- protos のリリースは semver タグ `v0.1.2`（既存慣習: `v0.1.1` = rev `f1d8e7e`）。
- 完了条件: `cargo tree -i rustls-pemfile` が `did not match` / `cargo tree -i hyper` が 1.x のみ / `just ci` 全緑。
- Windows 前提。protos 再生成はコマンド `cd rust && cargo run -r -p vstp_builder`（Makefile 相当。`make` 不在環境向けに直叩きを使う）。
- `[patch]` は **commit に残さない**（確定タスクで除去）。

---

## File Structure

### Repo A: `vstreamer-protos`（`../vstreamer-protos`、別 git repo）
- Modify: `rust/vstp_builder/Cargo.toml` — codegen 依存を `tonic-prost-build 0.14` へ。
- Modify: `rust/vstp_builder/src/main.rs` — 新 codegen API 呼び出し。
- Modify: `rust/vstreamer_protos/Cargo.toml` — `prost 0.14`/`tonic 0.14`(+`tonic-prost` 必要時)、`version` 0.1.2。
- Regenerate: `rust/vstreamer_protos/src/voicerecog.rs` — 生成物（手書きしない）。

### Repo B: `twitch-chat-yomiage-bot`（CWD、branch `chore/modernize-http-stack`）
- Modify: `Cargo.toml`(workspace root) — 開発中のみ `[patch]`（最後に除去）。
- Modify: `vstc/Cargo.toml` — tonic/tonic-reflection 0.14、protos をタグ固定参照へ。
- Modify: `vstc/src/lib.rs` — tonic 0.14 追従（最小、ほぼ不変見込み）。
- Modify: `vstc/tests/test.rs` — tonic 0.14 / tonic-reflection 0.14 / async_trait 追従（最大リスク）。
- Modify: `tcyb/Cargo.toml` — reqwest 0.13、`tonic` 削除、`hyper` 削除。
- Modify: `tcyb/src/auth.rs` — `axum::Server`→`axum::serve`、戻り値型変更。
- Modify: `igdb/Cargo.toml` — reqwest 0.13。
- (reqwest 呼び出し箇所: `tcyb/src/{api,channel,chat,eventsub,store}.rs`、`igdb/src/{api,artwork}.rs` は非破壊見込み。破壊時のみ追従。)

---

## Phase 0 — 開発スキャフォールド（Repo B）

### Task 0: ローカル protos を `[patch]` で参照

**Files:**
- Modify: `Cargo.toml`（workspace root）

**Interfaces:**
- Produces: 以降のタスクが、ローカルで再生成する protos を即時に取り込める状態。

- [ ] **Step 1: workspace root `Cargo.toml` に patch を追記**

末尾に追加（既存の `[workspace]`/`[workspace.lints.clippy]` はそのまま）:

```toml
# 開発中のみ: ローカル再生成 protos を参照する。確定タスク(Task 12)で削除すること。
[patch."https://github.com/sondeko143/vstreamer-protos"]
vstreamer_protos = { path = "../vstreamer-protos/rust/vstreamer_protos" }
```

- [ ] **Step 2: patch が効くことを確認**

Run: `cargo tree -i vstreamer_protos`
Expected: source がローカルパス（`(.../vstreamer-protos/rust/vstreamer_protos)` 等）になっている。現状 protos は 0.1.1(tonic 0.9) のままなのでビルドは通る（まだ変更前）。

- [ ] **Step 3: コミット（patch 一時追加）**

```bash
git add Cargo.toml
git commit -m "chore: vstreamer-protos をローカル patch 参照に一時切替（開発用）"
```

> 注: この commit は Task 12 で patch を除去して打ち消す。最終ブランチに patch を残さない。

---

## Phase 1 — protos 再生成（Repo A: vstreamer-protos）

> すべて `../vstreamer-protos` で作業。別 git repo なので commit もそちらに入る。作業前に `git -C ../vstreamer-protos status` がクリーン & `main` であることを確認。

### Task 1: codegen ツールを tonic-prost-build 0.14 へ

**Files:**
- Modify: `rust/vstp_builder/Cargo.toml`
- Modify: `rust/vstp_builder/src/main.rs`

**Interfaces:**
- Produces: `cargo run -r -p vstp_builder` が新 tonic スタイルで `voicerecog.rs` を生成できる codegen バイナリ。

- [ ] **Step 1: `vstp_builder/Cargo.toml` の依存を更新**

`[dependencies]` を:

```toml
[dependencies]
tonic-prost-build = "0.14"
```

- [ ] **Step 2: `vstp_builder/src/main.rs` を新 API へ**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .out_dir("./vstreamer_protos/src")
        .compile_protos(
            &["../protos/vstreamer_protos/commander/commander.proto"],
            &["../protos"],
        )?;
    Ok(())
}
```

> discover-and-adapt: メソッド名が `compile_protos` でなくビルドエラーになる場合は `cargo doc -p tonic-prost-build --open`（または docs.rs）で `configure()` の返す Builder の compile 系メソッド名を確認して合わせる。crate 名が解決できない場合は §フォールバック（0.12: `tonic-build` のまま `.compile_protos(...)`）。

- [ ] **Step 3: builder がビルドできることを確認**

Run: `cd ../vstreamer-protos/rust && cargo build -p vstp_builder`
Expected: コンパイル成功（まだ生成は走らせない。lib 側が旧 tonic 0.9 でも builder 単体はビルド可）。

- [ ] **Step 4: コミット（protos repo 側）**

```bash
git -C ../vstreamer-protos add rust/vstp_builder/Cargo.toml rust/vstp_builder/src/main.rs
git -C ../vstreamer-protos commit -m "build: codegen を tonic-prost-build 0.14 へ更新"
```

### Task 2: lib 依存を 0.14 にして voicerecog.rs を再生成

**Files:**
- Modify: `rust/vstreamer_protos/Cargo.toml`
- Regenerate: `rust/vstreamer_protos/src/voicerecog.rs`

**Interfaces:**
- Produces: tonic 0.14 / prost 0.14 でビルドできる `vstreamer_protos` lib（crate root = 生成 `voicerecog.rs`）。型 `Command`/`Operand`/`Sound`/`OperationChain`/`OperationRoute`/`Response`/`Operation`、`commander_client::CommanderClient`、`commander_server::{Commander, CommanderServer}` を提供（メッセージ/サービス名は不変）。

- [ ] **Step 1: `vstreamer_protos/Cargo.toml` を更新**

```toml
[package]
name = "vstreamer_protos"
version = "0.1.2"
edition = "2021"

[dependencies]
prost = "0.14"
tonic = "0.14"
tonic-prost = "0.14"

[lib]
path = "src/voicerecog.rs"
```

> discover-and-adapt: 再生成後 `voicerecog.rs` が `tonic-prost` を参照しなければ依存から外してよい（逆に未追加でビルドエラーなら追加で正しい）。`prost` のバージョンは tonic 0.14 が要求する系列（0.14）に合わせる。

- [ ] **Step 2: スタブ再生成**

Run: `cd ../vstreamer-protos/rust && cargo run -r -p vstp_builder`
Expected: `vstreamer_protos/src/voicerecog.rs` が更新される（diff が出る）。

- [ ] **Step 3: lib をビルドして生成物を確認**

Run: `cd ../vstreamer-protos/rust && cargo build -p vstreamer_protos`
Expected: コンパイル成功。失敗時は `voicerecog.rs` の冒頭 import（`use tonic::codegen::*;` 等）と codec 参照（`tonic_prost::...` 等）を確認し、`Cargo.toml` の依存と一致させる。

- [ ] **Step 4: ワークスペース全体ビルド（builder+lib）**

Run: `cd ../vstreamer-protos/rust && cargo build`
Expected: 成功。

- [ ] **Step 5: コミット（protos repo 側、push はまだしない）**

```bash
git -C ../vstreamer-protos add rust/vstreamer_protos/Cargo.toml rust/vstreamer_protos/src/voicerecog.rs rust/Cargo.lock
git -C ../vstreamer-protos commit -m "feat: tonic 0.14/prost 0.14 で Rust スタブを再生成し 0.1.2 へ"
```

> push と `v0.1.2` タグ付けは Task 12（本体が緑になってから）で行う。

---

## Phase 2 — vstc 移行（Repo B）

### Task 3: vstc 依存バンプ + lib.rs 追従

**Files:**
- Modify: `vstc/Cargo.toml`
- Modify: `vstc/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 の `vstreamer_protos`（`[patch]` 経由でローカル 0.1.2）。
- Produces: `process_command(uri, &[String], String, Option<Sound>, Option<String>, Option<Vec<String>>) -> Result<Response, VstcError>`（シグネチャ不変）。

- [ ] **Step 1: `vstc/Cargo.toml` のバージョン更新**

`tonic = "0.9.2"` → `tonic = "0.14"`、dev-dependencies の `tonic-reflection = "0.9.2"` → `tonic-reflection = "0.14"`。`vstreamer_protos` の行は **このタスクではまだ触らない**（`[patch]` が効くので version 要件 `"0.1.1"` のままで 0.1.2 を解決可。タグ固定は Task 12）。

- [ ] **Step 2: ビルドして lib.rs の追従要否を確認**

Run: `cargo build -p vstc`
Expected: 多くの場合そのまま成功（`tonic::transport::Endpoint::new`/`.connect_timeout`/`.timeout`、`CommanderClient::connect`、`tonic::Request::new`、`tonic::transport::Error`、`tonic::Status` は 0.14 でも安定）。

- [ ] **Step 3: エラーが出た箇所のみ lib.rs を追従**

discover-and-adapt: コンパイルエラーが出た場合のみ最小修正する。想定し得る差分:
- `tonic::transport::Error`/`tonic::Status` の経路が変わった場合 → `From` 実装（`lib.rs:44-54`）の型パスを `cargo doc -p tonic` で確認し合わせる。
- それ以外（`Endpoint`/`Request`/client メソッド）は基本不変。
エラーが無ければ lib.rs は無変更でよい。

- [ ] **Step 4: lib のユニットテスト（変換ロジック）を実行**

Run: `cargo test -p vstc --lib`
Expected: `convert_without_host` / `convert_with_host` PASS（tonic と無関係の純ロジックなので回帰検出に有効）。

- [ ] **Step 5: コミット**

```bash
git add vstc/Cargo.toml vstc/src/lib.rs Cargo.lock
git commit -m "feat(vstc): tonic 0.14 へバンプし lib を追従"
```

### Task 4: vstc 統合テスト(test.rs) を tonic 0.14 へ

**Files:**
- Modify: `vstc/tests/test.rs`

**Interfaces:**
- Consumes: `vstreamer_protos::commander_server::{Commander, CommanderServer}`、`tonic_reflection`、`tonic::transport::Server`。

- [ ] **Step 1: まずビルドして壊れ方を観測**

Run: `cargo test -p vstc --no-run`
Expected: ここでコンパイルエラーが出る想定（最大リスク箇所）。出力を読んで以下を順に追従する。

- [ ] **Step 2: tonic-reflection 0.14 API へ追従**

discover-and-adapt（`cargo doc -p tonic-reflection` / docs.rs で確認）。想定変更:
- `tonic_reflection::pb::FILE_DESCRIPTOR_SET` のパスが `tonic_reflection::pb::v1::FILE_DESCRIPTOR_SET` 等へ移動している可能性 → import を実際のパスへ。
- `Builder::configure().register_encoded_file_descriptor_set(...).build()` の `.build()` が `.build_v1()`（または `.build_v1alpha()`）へ分割されている可能性 → reflection を v1 で構築。

`vstc/tests/test.rs` の `build()` 関数（30-39 行）を、確認した実 API に合わせて更新する。例（v1 の場合）:

```rust
pub fn build(cmd: impl Commander) -> Router {
    let reflection_service = Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("reflection service could not build");

    Server::builder()
        .add_service(CommanderServer::new(cmd))
        .add_service(reflection_service)
}
```

- [ ] **Step 3: `#[tonic::async_trait]` / mock / Router 型を追従**

discover-and-adapt:
- `#[tonic::async_trait]`（17 行）が 0.14 で再エクスポートされない/サーバ trait が native async fn 化している場合は、コンパイラ指示に従い属性を除去 or `async-trait` クレート直接使用 or 生成 trait の要求形に合わせる（`cargo doc -p vstreamer_protos` で `Commander` trait の正確な形を確認）。
- `pub fn build(...) -> Router`（30 行）の戻り値 `tonic::transport::server::Router` が型引数を要求するようになっていれば、戻り値型をコンパイラの提案に合わせる（必要なら `-> impl ...` 化 or 具体型注釈）。
- `Server::builder().add_service(...).add_service(...).serve(addr)`（36-39, 49, 81 行）は基本不変。

- [ ] **Step 4: テストを実行**

Run: `cargo test -p vstc`
Expected: `send_minimal` / `populates_trace_id_and_origin_ts` / `process_command_times_out_when_server_silent` の 3 つが PASS（既存の振る舞い回帰が無いことを保証）。

- [ ] **Step 5: コミット**

```bash
git add vstc/tests/test.rs Cargo.lock
git commit -m "test(vstc): 統合テストを tonic 0.14 / tonic-reflection 0.14 へ追従"
```

---

## Phase 3 — tcyb 移行（Repo B）

### Task 5: tcyb Cargo.toml バンプ + 不要依存削除 + auth.rs 追従

**Files:**
- Modify: `tcyb/Cargo.toml`
- Modify: `tcyb/src/auth.rs`

**Interfaces:**
- Consumes: axum 0.8 の `Router`/`routing::get`/extractor/`response`/`http`、`tokio::net::TcpListener`。
- Produces: `start_server(addr: SocketAddr, state: ServerState) -> Result<(), std::io::Error>`（呼び出し側 `auth_code_grant` は `JoinHandle` 結果を `.err()` するだけなので戻り値型変更を吸収）。

- [ ] **Step 1: `tcyb/Cargo.toml` を更新**

- `axum = "0.6.18"` → `axum = "0.8"`
- `hyper = "0.14.26"` の行を **削除**
- `reqwest = { version = "0.11.17", features = ["json"] }` → `reqwest = { version = "0.13", features = ["json"] }`
- `tonic = "0.9.2"` の行を **削除**（tcyb ソース未使用）

- [ ] **Step 2: `tcyb/src/auth.rs` の `start_server` を axum 0.8 へ**

`auth.rs:72-81` を置換:

```rust
async fn start_server(addr: SocketAddr, state: ServerState) -> Result<(), std::io::Error> {
    let app = Router::new()
        .route("/", get(auth))
        .route("/callback", get(callback))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

> discover-and-adapt: `axum::serve(listener, app)` が型不一致になる場合は `axum::serve(listener, app.into_make_service())` にする（State 付き Router の make-service 化）。`tokio::net::TcpListener` は tcyb の tokio `features=["full"]` で利用可。

- [ ] **Step 3: tcyb をビルド（reqwest 呼び出しの破壊有無もここで判明）**

Run: `cargo build -p tcyb`
Expected: 成功。reqwest 0.13 で `api.rs`/`channel.rs`/`chat.rs`/`eventsub.rs`/`store.rs` の `Client::new().get/post().header().query/json/form().send().await?.error_for_status()?.json/text/bytes()` 形・`reqwest::StatusCode`・`reqwest::Error` は非破壊見込み。エラーが出た箇所のみコンパイラ指示で最小追従する。

- [ ] **Step 4: tcyb のテスト/チェック**

Run: `cargo test -p tcyb`
Expected: 既存テストがあれば PASS（無ければビルド成功で可）。

- [ ] **Step 5: コミット**

```bash
git add tcyb/Cargo.toml tcyb/src/auth.rs Cargo.lock
git commit -m "feat(tcyb): reqwest 0.13/axum 0.8 化、未使用 tonic と hyper direct 依存を削除"
```

---

## Phase 4 — igdb 移行（Repo B）

### Task 6: igdb reqwest 0.13

**Files:**
- Modify: `igdb/Cargo.toml`

- [ ] **Step 1: `igdb/Cargo.toml` を更新**

`reqwest = { version = "0.11.17", features = ["json"] }` → `reqwest = { version = "0.13", features = ["json"] }`

- [ ] **Step 2: ビルド**

Run: `cargo build -p igdb`
Expected: 成功。`igdb/src/api.rs`（`reqwest::header::{HeaderMap, HeaderValue}`、`Client::new()...json()`）・`igdb/src/artwork.rs`（`Client::new()...bytes()`）は非破壊見込み。破壊時のみ追従。

- [ ] **Step 3: テスト**

Run: `cargo test -p igdb`
Expected: 既存テストがあれば PASS（無ければビルド成功で可）。

- [ ] **Step 4: コミット**

```bash
git add igdb/Cargo.toml Cargo.lock
git commit -m "feat(igdb): reqwest を 0.13 へ更新"
```

---

## Phase 5 — 検証と確定（Repo B + Repo A 仕上げ）

### Task 7: ワークスペース横断の中間検証

**Files:** なし（検証のみ）

- [ ] **Step 1: 開発版フルチェック**

Run: `just check`
Expected: fmt-check + clippy + test 全緑。fmt の赤は `just fmt` で整形可。clippy の赤は機械的に黙らせず原因修正。

- [ ] **Step 2: rustls-pemfile 除去アサーション**

Run: `cargo tree -i rustls-pemfile`
Expected: `package ID specification ... did not match any packages`（= 消えた）。

- [ ] **Step 3: hyper 単一化アサーション**

Run: `cargo tree -i hyper`
Expected: hyper **1.x のみ**。`0.14` が出ないこと。出る場合はその逆依存（出力に表示）が未バンプ → 該当 crate を追って解消。

- [ ] **Step 4: 中間検証 OK（コミット不要・確認のみ）**

> ここまでは `[patch]` でローカル protos を見ている状態。次タスクで本物のリリースへ切替える。

### Task 8: protos を push & タグリリース（Repo A 仕上げ）

**Files:** Repo A の push/tag のみ

- [ ] **Step 1: protos repo の状態確認**

Run: `git -C ../vstreamer-protos log --oneline -3`
Expected: Task 1/2 の 2 コミットが `main` 先端にある。

- [ ] **Step 2: push**

```bash
git -C ../vstreamer-protos push origin main
```

- [ ] **Step 3: `v0.1.2` タグを打って push**

```bash
git -C ../vstreamer-protos tag -a v0.1.2 -m "v0.1.2: tonic 0.14/prost 0.14 stubs"
git -C ../vstreamer-protos push origin v0.1.2
```

Run（確認）: `git -C ../vstreamer-protos ls-remote --tags origin v0.1.2`
Expected: リモートに `refs/tags/v0.1.2` が存在。

### Task 9: vstc をタグ固定参照へ切替 + patch 除去

**Files:**
- Modify: `vstc/Cargo.toml`
- Modify: `Cargo.toml`（workspace root、patch 除去）

- [ ] **Step 1: `vstc/Cargo.toml` の protos 参照をタグ固定へ**

```toml
vstreamer_protos = { git = "https://github.com/sondeko143/vstreamer-protos", tag = "v0.1.2" }
```

- [ ] **Step 2: workspace root `Cargo.toml` の `[patch]` ブロックを削除**

Task 0 で追加した `[patch."https://github.com/sondeko143/vstreamer-protos"]` セクションを丸ごと除去。

- [ ] **Step 3: 本物のタグから解決させる**

Run: `cargo update -p vstreamer_protos`
Then: `cargo tree -i vstreamer_protos`
Expected: source が `git+https://github.com/sondeko143/vstreamer-protos?tag=v0.1.2#<sha>` になっている（ローカルパスでない）。

- [ ] **Step 4: 実リリースに対してビルド**

Run: `cargo build`
Expected: 成功（ローカル patch 無しで、push 済みタグから取得した protos でビルド可）。

- [ ] **Step 5: コミット**

```bash
git add vstc/Cargo.toml Cargo.toml Cargo.lock
git commit -m "build: vstreamer-protos を v0.1.2 タグ固定参照へ切替え、開発用 patch を除去"
```

### Task 10: フルゲート + メモリ更新

**Files:**
- Modify: メモリ `reqwest-axum-hyper-modernization`（解消反映）

- [ ] **Step 1: 最終アサーション再確認（タグ固定後）**

Run: `cargo tree -i rustls-pemfile` → `did not match`
Run: `cargo tree -i hyper` → 1.x のみ

- [ ] **Step 2: フルゲート**

Run: `just ci`
Expected: fmt-check + clippy + test + deny + audit 全緑（exit 0）。`yaml-rust` の audit 警告は既知の allowed warning（CLAUDE.md 記載、非ブロッキング）。

- [ ] **Step 3: メモリ更新**

Claude のプロジェクトメモリ `memory/reqwest-axum-hyper-modernization.md` を「解消済み（PR で reqwest0.13/axum0.8/hyper1/tonic0.14、protos v0.1.2 リリース）」に更新し、`MEMORY.md` の該当行も追従。

- [ ] **Step 4: 確定コミット（あれば）**

```bash
git add -A
git commit -m "chore: 近代化完了の検証メモを反映"
```

- [ ] **Step 5: PR 作成（main マージ前に just ci 緑を再確認）**

`just ci` 緑を確認のうえ PR を作成。protos の `v0.1.2` リリースが前提である旨を本文に明記。

---

## フォールバック: tonic 0.12 系

Task 1/2 で tonic 0.14 codegen（`tonic-prost-build`、生成参照、async_trait、tonic-reflection の v1 分割、Router 型）が想定外に難航した場合、ゴール（hyper 1.x 化 + rustls-pemfile 除去）は tonic **0.12** でも達成できる。差し替え点のみ:

- Repo A `vstp_builder/Cargo.toml`: `tonic-build = "0.12"`（crate 分割なし）。`main.rs` は `tonic_build::configure().out_dir(...).compile_protos(&[...], &[...])`（0.12 でメソッドは `compile_protos`）。
- Repo A `vstreamer_protos/Cargo.toml`: `prost = "0.13"`、`tonic = "0.12"`（`tonic-prost` 不要）。再生成コードは 0.9 に近い `tonic::codec::ProstCodec` 系のため lib/test の差分が小さい。
- Repo B `vstc/Cargo.toml`: `tonic = "0.12"`、`tonic-reflection = "0.12"`。tonic-reflection 0.12 は `.build()`（v1 分割前）/ `pb::FILE_DESCRIPTOR_SET` が現行に近く test.rs の変更が最小。
- それ以外（axum 0.8 / reqwest 0.13 / hyper 削除 / タグ v0.1.2 リリース / 検証）は本命と同一。

フォールバック採用時はタグ版数は同じ `v0.1.2` でよい（生成系が 0.12 になるだけ）。`cargo tree -i hyper` が 1.x 単一になることを必ず確認（tonic 0.12 は hyper 1.x）。

---

## Self-Review

- **Spec coverage:** rustls-pemfile 除去(Task 7/10)・hyper 単一化(Task 7/10)・reqwest 0.13(Task 5,6)・axum 0.8(Task 5)・hyper direct 削除(Task 5)・tonic 0.14(Task 1-4)・未使用 tonic 削除(Task 5)・protos 再生成(Task 1,2)・タグ v0.1.2 リリース(Task 8)・タグ固定参照(Task 9)・patch 開発フロー(Task 0,9)・native-tls 据え置き(制約)・just ci(Task 10)・メモリ更新(Task 10)・0.12 フォールバック(末尾) を網羅。
- **Placeholder scan:** 外部 codegen 依存箇所は意図的に discover-and-adapt とし、確認手段(`cargo doc`/コンパイラ)と具体例・フォールバックを明示（盲目的 TODO ではない）。手書き箇所(auth.rs/vstp_builder/Cargo.toml 群)は完全コードを提示。
- **Type consistency:** `start_server -> Result<(), std::io::Error>`、`process_command` シグネチャ不変、protos 型名(`Command`/`Response`/`Commander`/`CommanderServer`/`CommanderClient`)は再生成後も不変、を全タスクで一貫使用。
