# 0008. 計装スパンは常時コンパイルし、計測サブスクライバのみ feature 隔離する

- Status: Accepted
- Date: 2026-06-25
- Related: [起動計測 spec](../superpowers/specs/2026-06-25-startup-latency-profiling-design.md), [ADR-0007](0007-wall-clock-tracing-for-startup-profiling.md)

## Context

計測（[ADR-0007](0007-wall-clock-tracing-for-startup-profiling.md)）を入れるにあたり、本体コード（`main` / `yomiage` / `irc` / `eventsub`）へ計装をどう混ぜるかを決める必要があった。`tracing` のマクロはサブスクライバ不在時ほぼゼロコスト。重いのは計測用サブスクライバ（`tracing-subscriber` / `tracing-chrome` / `tracing-flame`）と描画ツール `inferno`。

## Decision

計装スパン（`info_span!` 等）は `#[cfg]` で囲わず常時コンパイルする。重い計測クレートだけを `profiling` cargo feature（optional deps）に隔離する。`tcyb/src/profiling.rs` に `init` / `mark_ready` / `wait_for_shutdown` の薄い API を集約し、非 profiling 時はそれぞれ ZST ガード / no-op / 永久 `pending` future にして本体挙動を不変に保つ。`inferno` はバイナリ依存に入れず外部 CLI として justfile から呼ぶ。

## Alternatives rejected

- **本体コードに `#[cfg(feature = "profiling")]` を撒く** — 計装ポイントごとに条件コンパイルが散り可読性・保守性が落ちる。常時計装・ゼロコスト前提という tracing の標準的な使い方にも反する。
- **`inferno` をバイナリ依存に含める** — 描画は開発時のみ必要で、本体依存ツリーを不要に太らせる。

## Consequences

本体は feature を意識せず常に同じ 3 関数を呼ぶだけになり、通常ビルドはゼロ相当オーバーヘッド。一方 profiling 経路は既定の `just clippy`（feature 無効でビルド）では検査されないため、`cargo clippy -p tcyb --features profiling --all-targets` を別途走らせて lint 漏れを防ぐ必要がある。
