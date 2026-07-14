# 0003. 共有する `auth_headers` は複製する（共有クレートを新設しない）

- Status: Accepted
- Date: 2026-06-24
- Related: [igdb 抽出 spec](../superpowers/specs/2026-06-24-igdb-artwork-crate-extraction-design.md), [ADR-0002](0002-extract-igdb-as-separate-crate.md)

## Context

`igdb` を切り出す（[ADR-0002](0002-extract-igdb-as-separate-crate.md)）と、`tcyb` と `igdb` が共通で必要とするコードは Bearer + `Client-Id` を付けるだけの `auth_headers`（約 10 行）のみ。`get_tokens_by_client_credentials` は artwork 専用で共有不要。この共有コードをどう扱うか決める必要があった。加えて現行 `auth_headers` は `axum::http::HeaderMap` を使うが、`igdb` に axum を持ち込みたくない。

## Decision

`auth_headers` を `igdb/src/api.rs` に**複製**する。共有のためだけの 4 つ目のクレートは作らない。`igdb` 側は `reqwest::header::{HeaderMap, HeaderValue}`（reqwest が再エクスポートする `http` 型）で書き、axum 依存を避ける。

## Alternatives rejected

- **共有 auth クレートを新設する** — 10 行・依存ゼロのコードのために 4 つ目のクレートを増やすのは過剰。DRY より凝集と単純さを優先する。

## Consequences

`igdb` は axum を引き込まずに済む。Twitch 認証ヘッダの仕様が将来変わったら 2 箇所（`tcyb/src/api.rs` と `igdb/src/api.rs`）を直す必要があるが、仕様は安定・10 行のため許容する。
