# 0012. GUI 状態は eframe 標準 Storage で OS ユーザー単位に永続化する

- Status: Accepted
- Date: 2026-06-26
- Related: [vstc_gui spec](../superpowers/specs/2026-06-26-vstc-gui-client-design.md), [ADR-0011](0011-disabled-step-as-forward-route.md)

## Context

GUI は前回入力値（接続先・テキスト・パイプライン）を OS ユーザーごとに保存し、起動時に復元する要件がある。永続化の方式と粒度を決める必要があった。proto の `Operation` は `Serialize` を持たない。

## Decision

eframe 標準の `Storage`（`persistence` feature）を使う。`App::save` で `eframe::set_value(storage, eframe::APP_KEY, &state)`、起動時（`App::new`）に `eframe::get_value` で復元する。保存先は OS ユーザーの app-data ディレクトリ（プラットフォーム規定）で明示のファイル操作は不要。粒度は **OS ユーザー単位の暗黙 1 プロファイル**とする。`Operation` は `Serialize` を持たないため、写した `Copy` なローカル enum `OpKind`（proto 変換 `to_proto`/`from_proto` を実装）を保存する。

## Alternatives rejected

- **アプリ内の名前付きプロファイル切替** — YAGNI。OS ユーザー単位の暗黙 1 プロファイルで要件を満たす。
- **独自ファイル形式で明示的に保存する** — eframe が標準の保存先・シリアライズを提供するため、独自実装は不要な複雑さ。

## Consequences

プラットフォーム規定の場所に自動保存/復元される。永続化スキーマ（`AppState`）は後方互換を保つ必要があり、新フィールドは `#[serde(default)]` にする（例: [ADR-0011](0011-disabled-step-as-forward-route.md) の `enabled`）。将来 Storage 実装や `APP_KEY` を変えると保存済み状態が孤立するため、変更は慎重に行う。
