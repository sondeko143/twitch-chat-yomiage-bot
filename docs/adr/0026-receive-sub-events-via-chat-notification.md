# 0026. サブスク等の通知は bot トークンで購読できる channel.chat.notification で受け取る

- Status: Proposed
- 効力: 制約
- Date: 2026-10-03
- Related: [monitor TUI spec](../superpowers/specs/2026-10-03-tcyb-monitor-tui-design.md), [ADR-0025](0025-unify-notification-speech-templates-by-type.md)

## Context

EventSub の購読は現在 `channel.follow` だけで、レイド・サブスク・ギフト・Bits 等は受け取っていない。

tcyb が持つトークンは bot アカウント（チャンネルのモデレーター）のユーザーアクセストークンだけである。`channel.subscribe` / `channel.subscription.gift` / `channel.cheer` などは、Twitch の仕様上、配信者本人の認可（`channel:read:subscriptions` / `bits:read`）を要する。

## Decision

- `channel.chat.notification` を購読する。購読には bot トークンの `user:read:chat` スコープを使う。これでサブスク・リサブ・ギフト・レイド・アナウンス等をまとめて受け取る。
- `channel.raid` を購読する。認可は不要。
- `channel.follow` の購読は継続する。
- 配信者本人のトークンを要する購読は行わない。
- `auth-code` が要求するスコープに `user:read:chat` を追加する。既存の利用者は再認可が必要になる。

## Alternatives rejected

- **`channel.subscribe` / `channel.cheer` 等を個別に購読する** — 配信者本人のトークンが要る。そのためには tcyb が 2 つ目のアカウントのトークン保管とリフレッシュを抱える必要があり、ストアと認可フローの構造が変わる。
- **IRC の USERNOTICE を解析して得る** — 既に IRC 接続を持っているので取れはする。しかし手書きパーサへタグ解析を足すことになる。EventSub の構造化 JSON の方が、テンプレート置換（ADR-0025）とも型が揃う。

## Consequences

- Bits（cheer）は独立した通知としては届かない。`channel.chat.notification` に含まれない種類は、今回は扱わない。
- スコープ追加のため、移行時に `auth-code` を 1 回実行し直す必要がある。
- 将来、配信者トークンを扱うようになった時点で、この制約の前提が変わる。そのときは新 ADR で購読範囲を見直す。
