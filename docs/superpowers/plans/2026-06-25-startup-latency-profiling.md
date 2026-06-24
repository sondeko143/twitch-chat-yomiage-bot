# 起動レイテンシ計測ツール 実装プラン

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `tcyb read-chat` の起動パスに `tracing` スパンを計装し、`profiling` feature 下で Perfetto タイムライン(`trace.json`)と wall-clock flamegraph SVG を出力。両 WebSocket 接続確立で自動終了する計測ツールを作る。

**Architecture:** スパン計装は常時コンパイル（tracing マクロはサブスクライバ不在時ほぼゼロコスト）。重い計測依存とサブスクライバは `profiling` cargo feature に隔離。起動完了判定は `profiling` モジュールの `ReadyTracker`（IRC/EventSub の両方が ready になったら `Notify` でシャットダウン）に集約し、本体コードは `init`/`mark_ready`/`wait_for_shutdown` の薄い API だけ呼ぶ。

**Tech Stack:** Rust 2021 / tokio / tracing / tracing-subscriber / tracing-chrome / tracing-flame / 外部 CLI `inferno` / just

設計仕様: [docs/superpowers/specs/2026-06-25-startup-latency-profiling-design.md](../specs/2026-06-25-startup-latency-profiling-design.md)

## Global Constraints

- **Windows 前提**。justfile は `set windows-shell := ["cmd.exe","/c"]` 済み（既存）。
- **品質ゲート**: PR/マージ前に `just ci` 全緑（exit 0）必須。clippy は `-D warnings`。lint をゲート通過のために緩めない。
- **feature 隔離**: `tracing` のみ常時依存。`tracing-subscriber` / `tracing-chrome` / `tracing-flame` は `profiling` feature の optional 依存。`inferno` はバイナリ依存ツリーに入れない（外部 CLI）。
- **既存挙動の不変**: `log`/`simple_logger` のログ出力・読み上げ/翻訳/モデレーションのロジックは変更しない。非 profiling ビルドの実行時挙動は変えない。
- **新規依存はすべて MIT/Apache 系**。deny.toml の許可リスト（MIT, Apache-2.0, BSD-*, ISC, Unicode-*, MPL-2.0 等）内に収まるはず。未許可ライセンスが出たら deny.toml の `[licenses].allow` に追記する（既存コメントの流儀どおり）。
- **profiling 経路はデフォルトの `just clippy`/`just test` では検査されない**ため、`clippy-profiling`/`test-profiling` を用意し `check`/`ci` に組み込む（Task 5）。

---

### Task 1: `profiling` モジュール（ready 判定の中核）+ 依存・feature

**Files:**
- Modify: `tcyb/Cargo.toml`（`[dependencies]` と `[features]`）
- Create: `tcyb/src/profiling.rs`
- Modify: `tcyb/src/main.rs:1-9`（`mod profiling;` を追加）
- Test: `tcyb/src/profiling.rs` 内の `#[cfg(test)] mod tests`（profiling feature 下でのみコンパイル）

**Interfaces:**
- Produces（他タスクが依存する公開 API。すべて `crate::profiling::` 配下）:
  - `enum Component { Irc, Event }`（`#[derive(Clone, Copy, Debug)]`）
  - `fn init() -> ProfileGuard`（main 冒頭で呼びガードを保持。drop でフラッシュ）
  - `fn mark_ready(c: Component)`（接続確立点で呼ぶ。非 profiling は no-op）
  - `async fn wait_for_shutdown()`（両 ready で完了。非 profiling は永久 pending）
  - `struct ProfileGuard`（main 末尾まで保持するガード）

- [ ] **Step 1: 依存と feature を追加する**

`tcyb` ディレクトリ基準で `cargo add` を使い、現行の互換バージョンを自動取得する:

```bash
cargo add -p tcyb tracing
cargo add -p tcyb --optional tracing-subscriber tracing-chrome tracing-flame
```

続けて `tcyb/Cargo.toml` に `[features]` セクションを手で追加する（`[dependencies]` の直後あたり、`[lints]` の前後どこでもよい）:

```toml
[features]
profiling = ["dep:tracing-subscriber", "dep:tracing-chrome", "dep:tracing-flame"]
```

`cargo add --optional` が追記した 3 行が下記の形になっていることを確認する（バージョンは取得値でよい）:

```toml
tracing-subscriber = { version = "0.3", optional = true }
tracing-chrome = { version = "0.7", optional = true }
tracing-flame = { version = "0.2", optional = true }
```

- [ ] **Step 2: `profiling.rs` を作成する（ReadyTracker の `mark`/`wait` は意図的に未実装スタブ）**

`tcyb/src/profiling.rs` を新規作成し、以下を丸ごと書く。`mark`/`wait` は **わざと誤った（何もしない）スタブ**にしておき、次の Step でテストが落ちることを確認する:

```rust
//! 起動レイテンシ計測の足回り。
//! 詳細: docs/superpowers/specs/2026-06-25-startup-latency-profiling-design.md
//!
//! スパン計装は常時コンパイルされるが、実際の計測サブスクライバと重い依存は
//! `profiling` feature 下でのみ有効化される。本体コードは init/mark_ready/
//! wait_for_shutdown の 3 関数だけを使い、feature の有無を意識しない。

/// 計測対象の接続系統。両方が ready になったら起動完了とみなす。
#[derive(Clone, Copy, Debug)]
pub enum Component {
    Irc,
    Event,
}

#[cfg(feature = "profiling")]
mod imp {
    use super::Component;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    /// IRC / EventSub の ready 状態を集約し、両方揃ったら Notify で通知する。
    pub struct ReadyTracker {
        irc: AtomicBool,
        event: AtomicBool,
        notify: Notify,
    }

    impl ReadyTracker {
        pub const fn new() -> Self {
            Self {
                irc: AtomicBool::new(false),
                event: AtomicBool::new(false),
                notify: Notify::const_new(),
            }
        }

        // --- スタブ（次 Step で実装する。今はテストを落とすための仮実装）---
        pub fn mark(&self, _c: Component) {
            // TODO(next step): フラグを立て、両方揃ったら notify する
        }

        pub async fn wait(&self) {
            // TODO(next step): notify を待つ
            std::future::pending::<()>().await
        }
    }

    static TRACKER: ReadyTracker = ReadyTracker::new();

    pub fn mark_ready(c: Component) {
        TRACKER.mark(c);
    }

    pub async fn wait_for_shutdown() {
        TRACKER.wait().await;
    }

    /// drop でトレースをフラッシュするガード。main 末尾まで保持する。
    pub struct ProfileGuard {
        _chrome: tracing_chrome::FlushGuard,
        _flame: tracing_flame::FlushGuard,
    }

    pub fn init() -> ProfileGuard {
        use tracing_subscriber::prelude::*;

        std::fs::create_dir_all("target/profile").expect("create target/profile dir");

        let (chrome_layer, chrome_guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file("target/profile/trace.json")
            .build();
        let (flame_layer, flame_guard) =
            tracing_flame::FlameLayer::with_file("target/profile/tracing.folded")
                .expect("open target/profile/tracing.folded");

        tracing_subscriber::registry()
            .with(chrome_layer)
            .with(flame_layer)
            .init();

        ProfileGuard {
            _chrome: chrome_guard,
            _flame: flame_guard,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Component, ReadyTracker};
        use std::time::Duration;

        #[tokio::test]
        async fn wait_pends_until_both_marked() {
            let t = ReadyTracker::new();
            t.mark(Component::Irc);
            // IRC だけ ready → wait は完了してはならない
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_err(), "wait should still pend with only Irc marked");
        }

        #[tokio::test]
        async fn wait_completes_when_both_marked() {
            let t = ReadyTracker::new();
            t.mark(Component::Irc);
            t.mark(Component::Event);
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_ok(), "wait should complete when both marked");
        }

        #[tokio::test]
        async fn mark_order_does_not_matter() {
            let t = ReadyTracker::new();
            t.mark(Component::Event);
            t.mark(Component::Irc);
            let res = tokio::time::timeout(Duration::from_millis(50), t.wait()).await;
            assert!(res.is_ok(), "order of marks should not matter");
        }
    }
}

#[cfg(not(feature = "profiling"))]
mod imp {
    use super::Component;

    /// 非 profiling ビルドの no-op ガード（Drop なし・フィールドなし）。
    pub struct ProfileGuard;

    pub fn init() -> ProfileGuard {
        ProfileGuard
    }

    #[inline]
    pub fn mark_ready(_c: Component) {}

    pub async fn wait_for_shutdown() {
        // 非 profiling では決して完了しない（select 分岐を発火させない）。
        std::future::pending::<()>().await
    }
}

pub use imp::{init, mark_ready, wait_for_shutdown, ProfileGuard};
```

- [ ] **Step 3: スタブのままテストを実行し、落ちることを確認する**

Run:
```bash
cargo test -p tcyb --features profiling profiling::imp::tests
```
Expected: `wait_pends_until_both_marked` は PASS、`wait_completes_when_both_marked` と `mark_order_does_not_matter` は **FAIL**（50ms タイムアウトで `res.is_ok()` が false → `Elapsed`）。

- [ ] **Step 4: `ReadyTracker` の `mark`/`wait` を実装する**

`tcyb/src/profiling.rs` のスタブ 2 メソッドを以下に差し替える:

```rust
        pub fn mark(&self, c: Component) {
            match c {
                Component::Irc => self.irc.store(true, Ordering::SeqCst),
                Component::Event => self.event.store(true, Ordering::SeqCst),
            }
            if self.irc.load(Ordering::SeqCst) && self.event.load(Ordering::SeqCst) {
                // wait 側がまだ待っていなくても、Notify が permit を保持するので取りこぼさない。
                self.notify.notify_one();
            }
        }

        pub async fn wait(&self) {
            self.notify.notified().await;
        }
```

- [ ] **Step 5: テストを再実行し、全 PASS を確認する**

Run:
```bash
cargo test -p tcyb --features profiling profiling::imp::tests
```
Expected: 3 件すべて PASS。

- [ ] **Step 6: `main.rs` にモジュールを配線する**

`tcyb/src/main.rs` の先頭モジュール宣言群（1-9 行目）にアルファベット位置で `mod profiling;` を追加する:

```rust
mod api;
mod auth;
mod channel;
mod chat;
mod eventsub;
mod irc;
mod profiling;
mod settings;
mod store;
mod yomiage;
```

- [ ] **Step 7: 両ビルド構成がコンパイルできることを確認する**

Run:
```bash
cargo build -p tcyb
cargo build -p tcyb --features profiling
```
Expected: 両方とも成功。非 profiling では `init`/`mark_ready`/`wait_for_shutdown`/`Component`/`ProfileGuard` がまだ未使用だが、Task 2-4 で使うため `dead_code` 警告が出る可能性がある。**この時点では `cargo build`（clippy ではない）なので warning 止まりで OK**。clippy ゲートは Task 5 で全配線後に通す。

- [ ] **Step 8: コミット**

```bash
git add tcyb/Cargo.toml tcyb/src/profiling.rs tcyb/src/main.rs
git commit -m "feat(profiling): ready 判定の中核と profiling feature を追加

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `main.rs` / `yomiage.rs` の起動スパン計装と自動終了配線

**Files:**
- Modify: `tcyb/src/main.rs:35-57`（`init` ガード、`config_build`/`logger_init` スパン）
- Modify: `tcyb/src/yomiage.rs:1-8, 62-68, 93-132`（`store_new`/`user_id_fetch` スパン、select の shutdown 分岐）

**Interfaces:**
- Consumes（Task 1 より）: `crate::profiling::{init, wait_for_shutdown}`
- Produces: なし（後続タスクはこの計装に依存しない。各タスク独立）

- [ ] **Step 1: `main.rs` の冒頭で profiling を初期化し、config/logger をスパンで囲む**

`tcyb/src/main.rs` の `main` 関数の冒頭〜logger 初期化（現 36-57 行）を以下に差し替える。`config` 構築ブロックは同期処理なので `info_span!(...).entered()` ガードで囲める（await を跨がない）:

```rust
async fn main() -> Result<()> {
    let _profile = profiling::init();
    dotenvy::dotenv().ok();
    let args = Cli::parse();

    let settings: Settings = {
        let _span = tracing::info_span!("config_build").entered();
        let mut config_builder = config::Config::builder()
            .set_default("listen_address", "localhost:8000")?
            .set_default("greeting_template", "user_name is now following!")?;
        config_builder = config_builder.add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        );
        if let Some(path) = args.config.as_ref() {
            config_builder = config_builder
                .add_source(config::File::with_name(path.to_str().unwrap()));
        }
        let config = config_builder.build()?;
        config.try_deserialize()?
    };

    {
        let _span = tracing::info_span!("logger_init").entered();
        simple_logger::SimpleLogger::new()
            .env()
            .with_local_timestamps()
            .init()?;
    }

    match &args.command {
        // ... 既存の match 分岐はそのまま ...
```

注意: `match &args.command { ... }` 以降は既存のまま。`settings` を `let settings: Settings = { ... };` ブロック式で束縛する点が変更の要。`if args.config.is_some() { ... unwrap() }` を `if let Some(path) = args.config.as_ref()` に置換している（clippy 配慮）。

- [ ] **Step 2: `yomiage.rs` に Instrument を import し、`store_new`/`user_id_fetch` を計装する**

`tcyb/src/yomiage.rs` の use 群（1-8 行）に追加:

```rust
use tracing::Instrument;
```

`yomiage` 関数の `store` 生成と `user_id` 取得（現 65-68 行）を以下に差し替える:

```rust
    let mut store = {
        let _span = tracing::info_span!("store_new").entered();
        Store::new(&settings.db_dir, &settings.db_name)?
    };
    let user_id = store
        .user_id(&settings.username, &settings.client_id)
        .instrument(tracing::info_span!("user_id_fetch"))
        .await?;
```

- [ ] **Step 3: `yomiage.rs` の `select!` に shutdown 分岐を追加する**

現在の `tokio::select! { r = chat_t => {...}, r = sub_event_t => {...}, };`（93-132 行）の **2 分岐の後ろ**に、3 つ目の分岐を追加する。`chat_abort_handle` / `sub_event_abort_handle` は同スコープ（91-92 行）で定義済み:

```rust
        tokio::select! {
            r = chat_t => {
                // ... 既存のまま ...
            },
            r = sub_event_t => {
                // ... 既存のまま ...
            },
            _ = crate::profiling::wait_for_shutdown() => {
                warn!("profiling: startup complete, shutting down");
                chat_abort_handle.abort();
                sub_event_abort_handle.abort();
                return Ok(());
            },
        };
```

非 profiling ビルドでは `wait_for_shutdown()` が永久 pending のためこの分岐は決して発火せず、既存挙動は不変。

- [ ] **Step 4: 両ビルド構成がコンパイルし、既存テストが緑であることを確認する**

Run:
```bash
cargo build -p tcyb
cargo build -p tcyb --features profiling
cargo test --workspace
```
Expected: ビルド両構成成功。`cargo test --workspace` は既存テスト全 PASS（挙動不変の確認）。

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/main.rs tcyb/src/yomiage.rs
git commit -m "feat(profiling): 起動段階(config/logger/store/user_id)を計装し自動終了を配線

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `irc.rs` の接続スパン計装と IRC ready マーク

**Files:**
- Modify: `tcyb/src/irc.rs:1-10`（`use tracing::Instrument;`）
- Modify: `tcyb/src/irc.rs:37`（`mark_ready(Irc)`）
- Modify: `tcyb/src/irc.rs:111-135`（`connect_and_authorize` の `irc_connect`/`irc_auth` スパン）

**Interfaces:**
- Consumes（Task 1 より）: `crate::profiling::{mark_ready, Component}`
- Produces: なし

- [ ] **Step 1: `Instrument` トレイトを import する**

`tcyb/src/irc.rs` の use 群（1-10 行）に追加:

```rust
use tracing::Instrument;
```

- [ ] **Step 2: `read_chat_client_loop` で接続確立直後に IRC ready をマークする**

現 37 行 `let mut ws_stream = connect_and_authorize(...).await?;` の直後に 1 行追加:

```rust
    let mut ws_stream = connect_and_authorize(&url, &access_token, &username, &channel).await?;
    crate::profiling::mark_ready(crate::profiling::Component::Irc);
```

- [ ] **Step 3: `connect_and_authorize` を `irc_connect`/`irc_auth` スパンで計装する**

現 `connect_and_authorize`（111-135 行）の本体を以下に差し替える（シグネチャは不変）:

```rust
    let (mut ws_stream, _) = connect_async(url)
        .instrument(tracing::info_span!("irc_connect"))
        .await?;
    info!("authorizing...");
    async {
        ws_stream
            .send(Message::Text(format!("PASS oauth:{}", access_token)))
            .await?;
        ws_stream
            .send(Message::Text(format!("NICK {}", username)))
            .await?;
        ws_stream
            .send(Message::Text(format!("JOIN #{}", channel)))
            .await?;
        ws_stream
            .send(Message::Text(String::from("CAP REQ :twitch.tv/tags")))
            .await?;
        Ok::<(), tokio_tungstenite::tungstenite::Error>(())
    }
    .instrument(tracing::info_span!("irc_auth"))
    .await?;
    Ok(ws_stream)
```

- [ ] **Step 4: 両ビルド構成がコンパイルし、irc テストが緑であることを確認する**

Run:
```bash
cargo build -p tcyb
cargo build -p tcyb --features profiling
cargo test -p tcyb irc::
```
Expected: 両構成成功。`irc::` のテスト全 PASS。

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(profiling): IRC 接続(connect/auth)を計装し ready をマーク

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `eventsub.rs` の接続スパン計装と EventSub ready マーク

**Files:**
- Modify: `tcyb/src/eventsub.rs:1-9`（`use tracing::Instrument;`）
- Modify: `tcyb/src/eventsub.rs:33`（`event_connect` スパン）
- Modify: `tcyb/src/eventsub.rs:143-151`（`session_welcome` 分岐の `event_subscribe` スパン + `mark_ready(Event)`）

**Interfaces:**
- Consumes（Task 1 より）: `crate::profiling::{mark_ready, Component}`
- Produces: なし

- [ ] **Step 1: `Instrument` トレイトを import する**

`tcyb/src/eventsub.rs` の use 群（1-9 行）に追加:

```rust
use tracing::Instrument;
```

- [ ] **Step 2: `connect_async` を `event_connect` スパンで計装する**

現 33 行 `let (mut ws_stream, _) = connect_async(url).await?;` を以下に差し替える:

```rust
    let (mut ws_stream, _) = connect_async(url)
        .instrument(tracing::info_span!("event_connect"))
        .await?;
```

- [ ] **Step 3: `session_welcome` 分岐でサブスク登録を計装し EventSub ready をマークする**

`process_message` の `"session_welcome"` 分岐（143-151 行）を以下に差し替える:

```rust
            "session_welcome" => {
                let session_id = match event_msg.payload.session {
                    Some(s) => s.id,
                    None => String::from(""),
                };
                info!("session welcome {}", session_id);
                sub_event(user_id, session_id.as_str(), access_token, client_id)
                    .instrument(tracing::info_span!("event_subscribe"))
                    .await?;
                crate::profiling::mark_ready(crate::profiling::Component::Event);
                Ok(())
            }
```

- [ ] **Step 4: 両ビルド構成がコンパイルすることを確認する**

Run:
```bash
cargo build -p tcyb
cargo build -p tcyb --features profiling
cargo test --workspace
```
Expected: 両構成成功。`cargo test --workspace` 全 PASS。

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/eventsub.rs
git commit -m "feat(profiling): EventSub 接続(connect/subscribe)を計装し ready をマーク

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: just レシピ追加・ゲート配線・フル検証

**Files:**
- Modify: `justfile`（`profile-startup` / `test-profiling` / `clippy-profiling` レシピ、`check`/`ci` への配線）

**Interfaces:**
- Consumes: Task 1-4 の成果すべて（profiling feature でビルド・実行可能なバイナリ）
- Produces: 運用コマンド `just profile-startup` と、profiling 経路を含む品質ゲート

- [ ] **Step 1: justfile に 3 レシピを追加する**

リポジトリ直下の `justfile` の `test:` レシピの後あたりに追加する:

```just
# profiling 経路のテスト（既定 test は feature 無効のため別途）
test-profiling:
    cargo test -p tcyb --features profiling

# profiling 経路の clippy（既定 clippy は feature 無効のため別途）
clippy-profiling:
    cargo clippy -p tcyb --features profiling --all-targets -- -D warnings

# 起動レイテンシ計測: profiling ビルドで read-chat を実行し、両接続確立で自動終了。
# 出力: target/profile/trace.json (→ ui.perfetto.dev) と target/profile/flame.svg。
# 事前に: cargo install inferno / 有効な認証(.env, `tcyb auth-code` 済み)。
profile-startup:
    cargo run -p tcyb --release --features profiling -- read-chat
    inferno-flamegraph target/profile/tracing.folded > target/profile/flame.svg
```

- [ ] **Step 2: `check` / `ci` に profiling 経路を組み込む**

`justfile` の `check` と `ci` の行を以下に差し替える（profiling コードが恒常的に lint/test されるようにする）:

```just
# 一括チェック（整形検査 + clippy + テスト）— 開発時用
check: fmt-check clippy clippy-profiling test test-profiling

# フルゲート（check + 依存監査）— コミット前 / CI 用
ci: fmt-check clippy clippy-profiling test test-profiling deny audit
```

- [ ] **Step 3: clippy（両 feature 構成）が全緑であることを確認する**

Run:
```bash
just clippy
just clippy-profiling
```
Expected: どちらも警告ゼロで成功（exit 0）。`-D warnings` のため未使用 import 等があれば失敗する → その場で修正。

- [ ] **Step 4: フルゲートを実行する**

Run:
```bash
just ci
```
Expected: `fmt-check / clippy / clippy-profiling / test / test-profiling / deny / audit` がすべて緑（exit 0）。
- `deny` で新規依存の未許可ライセンスが出た場合のみ、deny.toml の `[licenses].allow` に該当ライセンスを追記して再実行（Global Constraints 参照）。それ以外の赤は機械的に黙らせず原因を修正する。

- [ ] **Step 5: コミット**

```bash
git add justfile deny.toml
git commit -m "build(profiling): just レシピ追加と profiling 経路をゲートに配線

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 6: 手動の動作確認（生ネットワーク・任意）**

> CI には載せられない実ネットワーク確認。`.env` 設定済み・`tcyb auth-code` でトークン取得済みの環境で実施する。

事前に一度だけ:
```bash
cargo install inferno
```
実行:
```bash
just profile-startup
```
Expected:
- 両 WebSocket 接続確立後にプロセスが自動終了する（`profiling: startup complete, shutting down` のログが出る）。
- `target/profile/trace.json` と `target/profile/flame.svg` が生成される。
- `trace.json` を https://ui.perfetto.dev で開くと `config_build` / `logger_init` / `store_new` / `user_id_fetch` / `irc_connect` / `irc_auth` / `event_connect` / `event_subscribe` の各スパンが時系列で並び、どの段階が長いかが読める。
- `flame.svg` をブラウザで開くと各段階の wall-clock 幅が確認できる。

接続が確立しない（トークン切れ等）場合は通常の read-chat 同様に待ち続けるので Ctrl+C で中断する（途中までのトレースは drop 時にフラッシュされる）。

---

## 検証（プラン全体）

- 各 Task 末で `cargo build -p tcyb`（非 profiling）と `cargo build -p tcyb --features profiling` の両方を通す。
- Task 1 の `ReadyTracker` は profiling feature 下の単体テストで TDD 済み。
- Task 5 の `just ci` で fmt/clippy（両 feature）/test（両 feature）/deny/audit を全緑にする。
- 実ネットワークの最終確認は Task 5 Step 6（手動）。
