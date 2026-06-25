# 起動レイテンシ計測ツール（tcyb read-chat）— 設計

- 日付: 2026-06-25
- 対象コンポーネント: `tcyb`（本体バイナリ。`read-chat` → `yomiage` の起動パス）
- 関連 justfile / Cargo feature の追加

## 背景・目的

`tcyb read-chat` の起動が遅いと感じることがあるが、どの段階で時間を食っているのかが
切り分けられていない。起動パス（[main.rs](../../../tcyb/src/main.rs) →
[yomiage.rs](../../../tcyb/src/yomiage.rs)）には次のような**ネットワーク I/O を含む段階**が並ぶ:

1. プロセス起動 → `dotenv` / `config` 構築 / `simple_logger` 初期化
2. `Store::new`（ローカル jfs ファイル DB のオープン）
3. `store.user_id(...)` — **Twitch Helix API への HTTP 呼び出し**
4. `read_chat_client_loop` の `connect_and_authorize` — **IRC WebSocket(TLS) 接続 + 認証**
5. `sub_event_client_loop` — **EventSub WebSocket(TLS) 接続 + `session_welcome` 受信 + サブスク登録**

目的: **起動〜「両 WebSocket の接続確立完了」までの wall-clock を段階ごとに可視化し、
ボトルネック（特にネットワーク await 待ち）を特定できるツールを用意する。**

### 計測方式の前提（重要）

- 段階の多くが `await` でのネットワーク待ちのため、**CPU サンプリング型 flamegraph
  （samply / perf 等）では肝心の待ち時間が見えない**（待機中はスレッドが park され
  サンプルに出ない）。よって **wall-clock を計測する `tracing` スパン方式**を採る。
- 同一のスパン計装から、**Perfetto タイムライン**（時系列ガント）と
  **wall-clock flamegraph SVG**（幅 = 実時間）の 2 成果物を出力する。

## スコープ

### 対象
- 起動パスへの `tracing` スパン計装（恒久マージ。通常ビルドはゼロ相当オーバーヘッド）。
- `profiling` cargo feature 下でのみ有効化する計測サブスクライバ（chrome layer + flame layer）。
- 「両接続が確立したら計測を止めて正常終了する」ready 判定の仕組み。
- `just profile-startup` レシピ（profiling ビルド → 実行 → inferno で SVG 描画）。

### 対象外
- 既存の `log` / `simple_logger` によるログ出力は変更しない（`tracing` と併存させる）。
- 読み上げ・翻訳・モデレーション等の機能ロジックは変更しない。
- CPU サンプリング（samply 等）は今回入れない（前提のとおりネットワーク主体のため不適）。
  必要になれば別途追補する。

## アーキテクチャ概要

```
main (profiling::init でガード取得)
 └─ #[span] startup
     ├─ #[span] config_build / logger_init      (同期, 即時)
     └─ yomiage
         ├─ #[span] store_new
         ├─ #[span] user_id_fetch               (HTTP)
         ├─ spawn read_chat_client_loop ──► #[span] irc_connect / irc_auth ─► mark_ready(Irc)
         └─ spawn sub_event_client_loop ─► #[span] event_connect / event_subscribe ─► mark_ready(Event)

両 mark_ready が揃う → profiling::wait_for_shutdown() が完了 → yomiage が select を抜けて return
 → main の ProfileGuard が drop → chrome/flame をフラッシュ → プロセス終了
```

- **計装（スパン）は常時コンパイル**。`tracing` のマクロはサブスクライバ不在時ほぼゼロ
  コストのため、`#[cfg]` で囲わず素直に書く（tracing の標準的な使い方）。
- **重い依存とサブスクライバだけを `profiling` feature に隔離**する。

## 依存と feature（[tcyb/Cargo.toml](../../../tcyb/Cargo.toml)）

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", optional = true }
tracing-chrome = { version = "0.7", optional = true }
tracing-flame = { version = "0.2", optional = true }

[features]
profiling = ["dep:tracing-subscriber", "dep:tracing-chrome", "dep:tracing-flame"]
```

- `tracing` のみ常時依存（軽量・MIT）。計測 3 クレートは optional。
- `inferno` は**バイナリの依存ツリーに入れない**。flamegraph 描画は外部 CLI
  `inferno-flamegraph`（`cargo install inferno` で導入）を justfile から呼ぶ。

## 新規モジュール `tcyb/src/profiling.rs`

profiling 関連の機構を 1 モジュールに集約し、本体コードからは feature を意識しない薄い
API（`init` / `mark_ready` / `wait_for_shutdown`）だけ使う。

### `Component`
```rust
pub enum Component { Irc, Event }
```

### `init() -> ProfileGuard`
- **非 profiling**: 何もせず ZST の `ProfileGuard`（`Drop` 無し）を返す。
- **profiling**: `tracing_subscriber` の registry に
  - `tracing_chrome::ChromeLayerBuilder` → `target/profile/trace.json`
  - `tracing_flame::FlameLayer` → `target/profile/tracing.folded`
  を載せて `init()`。両者の `FlushGuard` を保持した `ProfileGuard` を返す。
  出力ディレクトリ `target/profile/` は無ければ作成する。

### `mark_ready(Component)`
- **非 profiling**: no-op（`#[inline] fn mark_ready(_: Component) {}`）。
- **profiling**: 対応する `AtomicBool` を立てる。**Irc と Event の両方が立った瞬間**に
  グローバル `Notify` を `notify_one()` する（= 起動完了シグナル）。idempotent。

### `wait_for_shutdown()`
- **非 profiling**: `std::future::pending::<()>()`（永久に完了しない＝select 分岐が発火しない）。
- **profiling**: グローバル `Notify` の `notified()` を await する future。

> この設計により、本体（yomiage / irc / eventsub）には `#[cfg]` を撒かずに済み、
> 呼び出し側は常に同じ 3 関数を呼ぶだけになる。

## 計装ポイント（具体）

### [main.rs](../../../tcyb/src/main.rs)
- `main` 冒頭で `let _profile = profiling::init();`（ガードを main 末尾まで保持）。
- `config_build`（config builder の `.build()` ＋ `try_deserialize`）と `logger_init`
  （`simple_logger ... .init()`）を同期スパン（`info_span!(...).in_scope(...)`）で囲む。
- ルートスパン `startup` を張り、上記と `yomiage` をその配下に置く。

### [yomiage.rs](../../../tcyb/src/yomiage.rs)
- `store_new`: `Store::new(...)` を同期スパンで。
- `user_id_fetch`: `store.user_id(...).await` を `.instrument(info_span!("user_id_fetch"))` で。
- `select!` に **ready 起動完了の分岐を追加**:
  ```rust
  _ = profiling::wait_for_shutdown() => {
      warn!("profiling: startup complete, shutting down");
      chat_abort_handle.abort();
      sub_event_abort_handle.abort();
      return Ok(());
  }
  ```
  非 profiling 時はこの future が `pending` なので分岐は決して発火せず、既存挙動は不変。

### [irc.rs](../../../tcyb/src/irc.rs)
- `connect_and_authorize` 内: `connect_async` を `irc_connect` スパン、PASS/NICK/JOIN/CAP の
  送信を `irc_auth` スパンで計測。
- `read_chat_client_loop` で `connect_and_authorize(...).await?` が **Ok を返した直後**に
  `profiling::mark_ready(Component::Irc)`。
  - 定義: 「IRC は TLS 接続確立＋認証フレーム送信完了」を ready とする。最大のコスト
    （TLS ハンドシェイク）を含む現実的な接続確立点。

### [eventsub.rs](../../../tcyb/src/eventsub.rs)
- `sub_event_client_loop` の `connect_async` を `event_connect` スパン。
- `process_message` の `"session_welcome"` 分岐で `sub_event(...).await?` を
  `event_subscribe` スパンで計測し、**`sub_event` が Ok を返した直後**に
  `profiling::mark_ready(Component::Event)`。
  - 定義: 「welcome 受信 → サブスクリプション登録完了」を ready とする（実際に購読が
    有効化され、待ち受け開始できた点）。

> スパンは spawn された各タスク（IRC / EventSub）上で走るため、Perfetto では別トラック、
> flamegraph では各タスクのスタック配下に現れる。並行する 2 系統の接続コストが見分けられる。

## just レシピ（[justfile](../../../justfile)）

```
# 起動レイテンシ計測: profiling ビルドで read-chat を実行し、両接続確立で自動終了 →
# Perfetto 用 trace.json と wall-clock flamegraph SVG を target/profile/ に出力。
# 事前に: cargo install inferno / 有効な認証(.env, tcyb auth-code 済み)
profile-startup:
    cargo run -p tcyb --release --features profiling -- read-chat
    inferno-flamegraph target/profile/tracing.folded > target/profile/flame.svg
```

- 成果物:
  - `target/profile/trace.json` → https://ui.perfetto.dev または `chrome://tracing` で開く（タイムライン）。
  - `target/profile/flame.svg` → ブラウザで開く（wall-clock flamegraph）。
- `set windows-shell := ["cmd.exe","/c"]` 済みのため、`>` リダイレクトは cmd で解釈される。

## 運用の前提

- **生ネットワーク接続**のため、`.env` 設定と有効なトークン（`tcyb auth-code` 済み）が必要。
- profiling 実行は両接続が確立した時点で**自動終了**する（無限ループに入らない）。
  確立できない（トークン切れ等）場合は通常の read-chat と同様に動くため、手動中断（Ctrl+C）すること。
  その場合でも `ProfileGuard` の drop でフラッシュされ、途中までのトレースは残る。

## 品質ゲート・検証

- `just ci` 全緑を維持（CLAUDE.md 規約）。新規依存（tracing 系）が cargo-deny / cargo-audit を
  通ることを確認する。
- **profiling コードパスの lint 漏れに注意**: 既定の `just clippy` は feature 無効でビルドするため、
  追加で次を実行して profiling 経路も検査する:
  ```
  cargo clippy -p tcyb --features profiling --all-targets -- -D warnings
  ```
  （必要なら `just clippy-profiling` レシピとして追加し、`just ci` から呼ぶことも検討。）
- 機能検証:
  - `cargo build -p tcyb`（feature 無し）が通る = 既存挙動に影響しない。
  - `cargo build -p tcyb --features profiling` が通る。
  - `just profile-startup` を実行し、`target/profile/trace.json` と `flame.svg` が生成され、
    Perfetto / ブラウザで各起動段階（user_id_fetch / irc_connect / event_connect 等）の
    所要時間が確認できる。

## 既知の制限・備考

- wall-clock flamegraph は集約表示のため時系列順は失われる。段階の前後関係は Perfetto
  タイムライン側で確認する（両方を出す理由）。
- spawn 跨ぎでスパンの親子は自動伝播しない。IRC / EventSub のスパンは各タスクのルート配下に
  並ぶ（本ツールの目的＝段階別 wall-clock には十分）。
- `tracing` を常時依存に加えるが、サブスクライバ未装着時のマクロ実行コストは無視できる範囲。
