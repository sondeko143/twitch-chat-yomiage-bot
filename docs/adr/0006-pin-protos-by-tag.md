# 0006. `vstreamer_protos` を semver タグ v0.1.2 で固定参照する

- Status: Accepted
- Date: 2026-06-24
- Related: [近代化 spec](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md), [ADR-0004](0004-modernize-http-stack-to-hyper-1x.md)

## Context

`vstc` は外部 git 依存 `vstreamer_protos`（`sondeko143/vstreamer-protos`）を参照する。近代化前は `version = "0.1.1"` 指定で main 追従＋Cargo.lock 固定という形だった。近代化（[ADR-0004](0004-modernize-http-stack-to-hyper-1x.md)）で protos を tonic-prost-build 0.14 で再生成し 0.1.2 へ上げる。cross-repo のため push 前にローカルの生成物へ rev を固定できない。

## Decision

protos を再生成後、その commit に semver タグ `v0.1.2` を打ち、`vstc/Cargo.toml` は `{ git = "...", tag = "v0.1.2" }` で明示タグ固定参照へ切り替える。開発中はワークスペースルート `Cargo.toml` に一時 `[patch]`（ローカル protos への path 参照）を当てて両 repo を同時反復し、確定時に patch を削除する（commit に残さない）。

## Alternatives rejected

- **main 追従＋Cargo.lock 固定のみ** — どの protos rev に対応するかがタグで明示されず再現性が低い。
- **開発用 `[patch]` を commit に残す** — ローカル絶対パス依存になり他環境で壊れる。確定ステップで必ず削除する。

## Consequences

protos 版と本体の対応がタグで明示され再現性が上がる。protos 更新のたびにタグ付け＋参照更新の手順が要る。`.github` の `main-<sha>` 自動タグ（Python publish 用）とは別物である点に注意。
