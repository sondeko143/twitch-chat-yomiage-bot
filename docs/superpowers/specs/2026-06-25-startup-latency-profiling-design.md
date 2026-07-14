# 起動レイテンシ計測ツール（tcyb read-chat）— spec

- 日付: 2026-06-25 ／ 状態: 実装済み
- 関連 ADR: [0007 wall-clock tracing で計測](../../adr/0007-wall-clock-tracing-for-startup-profiling.md), [0008 常時計装＋feature 隔離](../../adr/0008-always-on-spans-feature-gated-subscriber.md)

## 問題

`tcyb read-chat` の起動が遅いと感じるが、起動パスのどの段階（`user_id` 取得(HTTP) / IRC 接続 / EventSub 接続など、ネットワーク待ちを含む）が時間を食っているのか切り分けられていない。

## ゴール

起動〜「両 WebSocket の接続確立完了」までの wall-clock を段階ごとに可視化し、ボトルネック（特にネットワーク await 待ち）を特定できるツールを用意する。

## 非ゴール

- 既存 `log`/`simple_logger` のログ出力の変更（tracing と併存させる）。
- 読み上げ / 翻訳 / モデレーション等の機能ロジックの変更。
- CPU サンプリング（samply 等）の導入（[0007](../../adr/0007-wall-clock-tracing-for-startup-profiling.md)）。
- 通常ビルド（feature 無し）への計測オーバーヘッド追加。

## 受入基準

- [ ] `cargo build -p tcyb`（feature 無し）が通り、既存挙動に影響しない。
- [ ] `cargo build -p tcyb --features profiling` が通る。
- [ ] `just profile-startup` で `target/profile/trace.json`（Perfetto タイムライン）と `flame.svg`（wall-clock flamegraph）が生成される。
- [ ] 成果物から各起動段階（`user_id_fetch` / `irc_connect` / `event_connect` 等）の所要時間が確認できる。
- [ ] 両接続の確立時に計測実行が自動終了する。
- [ ] `just ci` 全緑（新規 tracing 系依存が deny/audit を通る）。
