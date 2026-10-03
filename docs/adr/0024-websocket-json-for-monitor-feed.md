# 0024. monitor 配信口は axum の WebSocket 上の JSON メッセージで実装する

- Status: Accepted
- 効力: 既定
- Date: 2026-10-03
- Related: [monitor TUI spec](../superpowers/specs/2026-10-03-tcyb-monitor-tui-design.md), [ADR-0023](0023-separate-monitor-tui-via-local-feed-from-read-chat.md), [ADR-0006](0006-pin-protos-by-tag.md)

## Context

ADR-0023 で決めた `read-chat` → `tcyb monitor` のローカル配信口について、プロトコルを決める必要がある。

流れは一方向のストリームで、接続直後にスナップショットを 1 回送り、以降は新着を 1 件ずつ送る。送り手（`read-chat`）と受け手（`monitor`）は同じ `tcyb` バイナリに入っている。

tcyb は既に、サーバ側に axum（OAuth リダイレクト受け口）を、クライアント側に tokio-tungstenite（IRC / EventSub）を依存に持っている。gRPC の proto は外部リポジトリ `vstreamer-protos` にあり、タグで固定参照している（ADR-0006）。

## Decision

- 配信口は axum の WebSocket エンドポイントにする。メッセージは serde で直列化した JSON テキストフレームで送る。
- メッセージの型は `tcyb` クレート内で 1 つ定義し、送信側と受信側で共有する。
- 各メッセージにプロトコル版を含める。版が一致しなければ、`monitor` は非互換と表示する。
- 購読者の受信が遅れて broadcast が取りこぼしたときは、接続を切らずにスナップショットを送り直して整合を取る。

## Alternatives rejected

- **gRPC（tonic）のサーバストリーミング** — proto を `vstreamer-protos` に置くと、tcyb 固有の型が外部リポジトリへ漏れ、タグを打ち直す運用が増える。tcyb 内に置くと build.rs と protoc 前提を抱える。送り手と受け手が同じクレートにある以上、言語中立のスキーマが活きる場面が無い。
- **SSE（Server-Sent Events）** — 一方向なので用途には合う。しかし SSE を受けるクライアント側の依存を新しく足すことになる。既にある tokio-tungstenite を流用できる WebSocket を採る。
- **改行区切り JSON を素の TCP で流す** — フレーミングと再接続を自前で書くことになり、`websocat` のような既製のツールでデバッグもできない。
- **取りこぼした購読者を切断する** — TUI 側の再接続が頻発し、表示がちらつく。スナップショットの再送で同じ整合が取れる。

## Consequences

- 中身を人が読める形で確認でき（`websocat ws://127.0.0.1:<port>/...`）、トラブル時の切り分けが容易になる。
- tcyb 以外の言語やクレートから購読する需要が出たら、JSON の形がそのまま事実上の契約になる。その時点で版の扱いを強めるか、gRPC への移行を新 ADR で判断する。
- スナップショットの大きさは保持件数に比例する。保持件数の既定値は、この大きさと TUI で遡れる量のトレードオフになる。
