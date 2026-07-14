# 0005. TLS バックエンドは native-tls を据え置く

- Status: Accepted
- Date: 2026-06-24
- Related: [近代化 spec](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md), [ADR-0004](0004-modernize-http-stack-to-hyper-1x.md)

## Context

近代化（[ADR-0004](0004-modernize-http-stack-to-hyper-1x.md)）で reqwest を 0.11→0.13 に上げる。unmaintained だったのは `rustls-pemfile` であり、rustls への切替も選択肢に上がる。ただし rustls-pemfile は元々 reqwest 0.11 の既定経由でのみツリーに入っており、reqwest を上げれば消える。さらに **reqwest 0.13 は既定 TLS が rustls に変更されている**（0.11/0.12 は native-tls）ため、何もしないと近代化を機に逆に rustls/hyper-rustls/aws-lc-rs を引き込んでしまう。

## Decision

TLS バックエンドは native-tls のまま据え置き、rustls を新規導入しない。reqwest 0.13 は既定が rustls に変わったため、native-tls を維持するには `default-features = false` ＋ 明示 `native-tls` を指定する（使う builder メソッド分の feature（`json`/`query`/`form` 等）も feature-gate 化されたので明示する）。

## Alternatives rejected

- **rustls バックエンドへ切り替える** — rustls-pemfile は reqwest 更新で自然に消えるため、rustls を導入する動機がそもそも無い。バックエンド変更は挙動・依存の追加リスクを持ち込むだけ。
- **reqwest 0.13 の既定（rustls）に任せる** — 既定変更に気づかず rustls/aws-lc-rs を引き込む。近代化の目的（依存を減らす）に逆行する。

## Consequences

TLS 挙動は不変で移行リスクが小さい。ただし reqwest の feature 指定は明示必須になり、`default-features = false` を外すと既定 rustls が復活する罠がある（native-tls は型のみの `rustls-pki-types` を引くが `rustls-pemfile` は引かない）。将来 rustls が必要になれば別 ADR で切り替える。
