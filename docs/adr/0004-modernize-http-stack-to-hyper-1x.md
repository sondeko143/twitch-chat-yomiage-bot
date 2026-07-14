# 0004. HTTP/gRPC スタックを hyper 1.x へフル近代化する

- Status: Accepted
- Date: 2026-06-24
- Related: [近代化 spec](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md), RUSTSEC-2025-0134, [ADR-0005](0005-keep-native-tls-backend.md), [ADR-0006](0006-pin-protos-by-tag.md)

## Context

`rustls-pemfile v1.0.4`（unmaintained / RUSTSEC-2025-0134）が `reqwest 0.11.27` 経由でのみ依存ツリーに残存していた。HTTP/gRPC スタックは全て hyper 0.14 に乗る（axum 0.6 / tonic 0.9 / reqwest 0.11）。reqwest だけ 0.13（hyper 1.x）に上げると hyper が 0.14 と 1.x で二重化するため、重複解消には axum・tonic も揃える必要がある。tonic 0.14 は crate 分割（`tonic-prost` / `tonic-prost-build`）を含み codegen 差分が大きい。近代化は外部 protos リポ（`sondeko143/vstreamer-protos`）の再生成を伴う。

## Decision

reqwest 0.13 / axum 0.8 / tonic 0.14（+ prost 0.14 / tonic-prost-build 0.14）へ揃えて hyper を 1.x 単一化する。`tcyb` の未使用 `tonic` direct 依存と direct `hyper` 依存を削除する。挙動変更はしない純粋な依存近代化とする。tonic 0.14 の codegen 移行が難航した場合は tonic 0.12 系へ退避する（0.12 でも hyper 1.x 化＝重複解消は達成できる）。

## Alternatives rejected

- **reqwest だけ 0.13 に上げる** — hyper が 0.14 と 1.x で二重化し、依存ツリーが重くなるだけで rustls-pemfile 問題の根治にならない。
- **tonic を 0.9 のまま据え置く** — hyper 0.14 が残り単一化できない。
- **最初から tonic 0.12 を本命にする** — 「フル近代化＝最新」の方針に反する。protos 再生成はどのみち必須で、0.14 を本命に据え 0.12 は退避扱いとするのが妥当。

## Consequences

`cargo tree -i rustls-pemfile` が `did not match` になり、hyper が 1.x 単一になる。tonic 0.14 の crate 分割により protos の codegen とサーバ trait 定義（`vstc/tests/test.rs` の `#[tonic::async_trait]` ＋ mockall）の調整が必要（移行の最大リスク）。protos は再生成しタグ固定する（[ADR-0006](0006-pin-protos-by-tag.md)）。TLS は据え置く（[ADR-0005](0005-keep-native-tls-backend.md)）。近代化の実績・手順はプロジェクトメモリ `reqwest-axum-hyper-modernization` に集約。
