# 0007. 起動レイテンシは wall-clock tracing スパンで計測する

- Status: Accepted
- Date: 2026-06-25
- Related: [起動計測 spec](../superpowers/specs/2026-06-25-startup-latency-profiling-design.md)

## Context

`tcyb read-chat` の起動が遅い原因を切り分けたい。起動パスは `user_id` 取得（HTTP）、IRC WebSocket(TLS) 接続、EventSub WebSocket(TLS) 接続＋サブスク登録など、**ネットワーク await 待ちが主体**の段階が並ぶ。計測方式を決める必要があった。

## Decision

wall-clock を計測する `tracing` スパン方式を採る。同一の計装から、Perfetto タイムライン（`target/profile/trace.json`）と wall-clock flamegraph SVG（幅＝実時間、外部 CLI `inferno-flamegraph` で描画）の 2 成果物を出す。両 WebSocket 接続の確立で計測を止めて正常終了する ready 判定を設ける。

## Alternatives rejected

- **CPU サンプリング型 flamegraph（samply / perf 等）** — 待機中はスレッドが park されサンプルに出ないため、肝心のネットワーク await 待ち時間が見えない。起動ボトルネックの可視化に不適。
- **既存 `log`/`simple_logger` 出力を計測に流用する** — 段階別 wall-clock の構造化された可視化にはならない。

## Consequences

ネットワーク待ちを含む段階別 wall-clock が可視化できる。実ネットワーク接続が要る（`.env`・有効トークン）。wall-clock flamegraph は集約表示で時系列順は失われるため、段階の前後関係は Perfetto 側で見る（両方を出す理由）。CPU バウンドなボトルネックが将来問題になれば別途 samply 等を追補する。計装コード自体の設計は [ADR-0008](0008-always-on-spans-feature-gated-subscriber.md) を参照。
