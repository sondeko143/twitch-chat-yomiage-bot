# reqwest / axum / hyper / tonic 近代化 — spec

- 日付: 2026-06-24 ／ 状態: 実装済み
- 関連 ADR: [0004 hyper 1.x へフル近代化](../../adr/0004-modernize-http-stack-to-hyper-1x.md), [0005 native-tls 据え置き](../../adr/0005-keep-native-tls-backend.md), [0006 protos をタグ固定](../../adr/0006-pin-protos-by-tag.md)
- 関連: RUSTSEC-2025-0134

## 問題

`rustls-pemfile v1.0.4`（unmaintained / RUSTSEC-2025-0134）が `reqwest 0.11.27` 経由でのみ依存ツリーに残存している。HTTP/gRPC スタックが全て hyper 0.14 に乗っており、reqwest だけ上げると hyper が 0.14 と 1.x で二重化する。

## ゴール

- 依存ツリーから `rustls-pemfile` を消す。
- hyper を 1.x 単一にする。
- 既存機能・公開 API の挙動は不変（純粋な依存近代化）。

## 非ゴール

- `tokio-tungstenite`（0.20→0.29）の更新。hyper と独立で rustls-pemfile にも無関係のため今回触らない。
- TLS バックエンドの変更（native-tls 据え置き、[0005](../../adr/0005-keep-native-tls-backend.md)）。

## 受入基準

- [ ] `cargo tree -i rustls-pemfile` が `did not match`（依存ツリーから消える）。
- [ ] `cargo tree -i hyper` が 1.x 単一（0.14 が出ない）。
- [ ] 既存機能・公開 API の挙動が不変（gRPC / Twitch HTTP / IRC・EventSub WS が疎通）。
- [ ] `vstc` が `vstreamer_protos` をタグ `v0.1.2` で参照し、開発用 `[patch]` が commit に残っていない。
- [ ] `just ci` 全緑（fmt-check / clippy / test / deny / audit）。
