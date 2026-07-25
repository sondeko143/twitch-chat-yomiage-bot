# 0017. file_path を運ぶため vstc に operand オプション付きの route 送信口を足す

- Status: Accepted
- Date: 2026-07-26
- Related: [vstc_cli プロファイル spec](../superpowers/specs/2026-07-26-vstc-cli-profiles-design.md), [ADR-0009](0009-add-process-routes-entrypoint-to-vstc.md)

## Context

`vstc` の公開送信口は 2 つある。`process_command` は `o:/tts?spd=1.1` 形式の操作文字列を受けて `Operand` の全フィールド（text / sound / file_path / filters）を運ぶ。ADR-0009 で GUI 用に足した `process_routes` は構造化済みの `OperationRoute` を受けるが、`Operand` は text 専用で `file_path` を運べない。

`vstc_cli reload` は「構造化された単一 route」と「`file_path`」を同時に必要とする。既存のどちらの口もこの組み合わせを表現できない。

`vstc` は `publish = false` で、ワークスペース内の `vstc_cli` と `vstc_gui` からしか使われない。破壊的変更のコストは呼び出し箇所の書き換えだけで、外部利用者への配慮は不要である。一方 `process_command` は既に引数 6 個で、clippy の `too-many-arguments-threshold = 7` に近い。

## Decision

`process_routes` のシグネチャは変えず、`Operand` の任意フィールドを束ねた構造体を受け取る送信口を新設する。既存の `process_routes` はその新関数への薄い委譲にする。

構造体は `Default` を実装し、呼び出し側は必要なフィールドだけを埋める。`trace_id` と `origin_ts` は引き続き `vstc` 内部で生成し、呼び出し側には露出しない。

## Alternatives rejected

- **`process_routes` に `file_path` 引数を足す（破壊的変更）** — ワークスペース内 2 箇所の書き換えで済むので現実的だが、`file_path` を使わない `vstc_gui` が毎回 `None` を渡すことになる。さらに次に `filters` や `sound` が要るたび引数が増え、`process_command` が既に踏みかけている引数肥大の道を `process_routes` にも辿らせる。構造体にしておけばフィールド追加が非破壊で済む。
- **`Operand` をそのまま公開引数にする** — proto 型をそのまま受ければ新しい型を定義せずに済むが、`trace_id` / `origin_ts` の生成責務が呼び出し側へ漏れる。ADR-0009 が `process_routes` を足した動機（呼び出し側に proto の組み立て詳細を持たせない）に反する。
- **`vstc_cli` 側で操作文字列を組み立てて `process_command` を使う** — `vstc` を一切変更せずに済むが、`reload` の route を `"o:/reload"` という文字列に落として `vstc` 側で再パースさせることになり、構造化した意味が往復で消える。GUI のために構造化した口を作った ADR-0009 の判断を CLI 側で捨てることになる。
- **`vstc_cli` に送信ロジックを複製する（tonic を直接叩く）** — 接続タイムアウト・エラー型・operand 生成が `vstc` と `vstc_cli` に二重化する。ADR-0003 で「共有 `auth_headers` は複製する」と判断した例はあるが、あちらはクレート境界をまたぐ数行の定数だった。こちらは既に共有ライブラリが存在し、そこに口を足すだけで済む。

## Consequences

`reload` が構造化 route のまま `file_path` を運べるようになり、`vstc_gui` の既存呼び出しは無変更で通る。今後 route 送信で `filters` や `sound` が必要になっても、構造体へフィールドを足すだけで既存呼び出しは壊れない。

一方、`vstc` の公開 API が 3 つになり、「文字列で送る `process_command`」「構造化で送る 2 つ」という選択肢を利用者が持つ。将来 `process_command` 側も同じ構造体に寄せて口を 2 つに戻す整理はありうるが、今回のスコープでは行わない。
