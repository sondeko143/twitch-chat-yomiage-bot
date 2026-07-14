# 0009. GUI 用に `vstc` へ構造化エントリポイント `process_routes` を追加する

- Status: Accepted
- Date: 2026-06-26
- Related: [vstc_gui spec](../superpowers/specs/2026-06-26-vstc-gui-client-design.md)

## Context

新 GUI クライアント `vstc_gui` は `vstc` ライブラリを再利用して commander へ gRPC 送信する。既存の公開 API は文字列ベースの `process_command`（CLI 用）。GUI は構造化された route/パラメーターを持つため、文字列 URL/コマンドを経由させると接続・送信ロジックが重複し `Operand` 構築も散る。

## Decision

`vstc` に構造化エントリポイント `pub async fn process_routes(uri, routes: Vec<OperationRoute>, text) -> Result<Response, VstcError>` を追加する。`routes` を 1 本の `OperationChain` に包み、text のみの `Operand`（sound/file_path/filters は既定、`trace_id`=UUID、`origin_ts`=現在時刻は内部生成）を構築する。既存の `process_command`（CLI 利用）はそのまま残し、`Operand` 構築は private な `build_operand(text)` に切り出して両者で共有する。

## Alternatives rejected

- **GUI から文字列 URL/コマンドを組み立てて既存 `process_command` を使う** — GUI 側で接続・送信ロジックと `Operand` 構築が重複し、文字列を介した往復で型安全性も落ちる。
- **`process_command` を構造化 API に置き換える** — CLI の既存インターフェースを壊す。両者を並存させ共通部分（`build_operand`）だけ共有するのが低リスク。

## Consequences

GUI と CLI が同じ接続・送信ロジックと `Operand` 構築を共有する。`vstc` の公開 interface が 1 つ増え、契約として維持対象になる。
