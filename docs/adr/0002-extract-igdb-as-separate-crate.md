# 0002. artwork/IGDB を独立クレート `igdb` へ切り出す

- Status: Accepted
- Date: 2026-06-24
- Related: [igdb 抽出 spec](../superpowers/specs/2026-06-24-igdb-artwork-crate-extraction-design.md), [ADR-0003](0003-duplicate-auth-headers.md)

## Context

`tcyb`（TTS ボット）に、IGDB API を叩いてゲームサムネイル（`thumbnails.jpg`）を生成する artwork 機能が同居していた。artwork は重い `image` クレートとその依存ツリーを本体バイナリに引き込む。Cargo の依存はパッケージ単位で、同一クレート内の全バイナリターゲットが同じ `[dependencies]` を共有する。ボット本体の依存を軽くしたい。なお IGDB の認証は実体として Twitch OAuth そのもので、`client_credentials` を同じ `client_id`/`client_secret` で取得している。

## Decision

artwork/IGDB を新しいワークスペースメンバ `igdb`（単一バイナリクレート）へ切り出し、`tcyb` から artwork コード・`image` 依存・`GetArtwork` サブコマンドを削除する。認証は同じ Twitch アプリ（`client_id`/`client_secret` を同じ `.env`/設定から読む）を共有し、Developer Console への再登録はしない。

## Alternatives rejected

- **別バイナリ（`[[bin]]`）やモジュール分割で分ける** — Cargo の依存はパッケージ単位のため、同一クレート内に置く限り `image` を本体バイナリの依存ツリーから外せない。依存を実際に分離できるのは別クレート化だけ。
- **Twitch アプリ自体を分ける（新 `client_id`/`client_secret` を登録）** — 「コードを分ける」と「アプリを分ける」は別の軸。同じ資格情報を設定から読めばよく、再登録は不要。

## Consequences

`tcyb` の依存ツリーから `image` が消える（`cargo tree -p tcyb` で確認可）。artwork は `igdb get-artwork <names...>` として独立実行し、出力挙動は不変。`tcyb/src/api.rs` は Twitch Helix 専用になり関心が明確化する。`igdb` と `tcyb` は同一の Twitch アプリを使うため IGDB のレートリミットは従来どおり共有される。共有コード `auth_headers` の扱いは [ADR-0003](0003-duplicate-auth-headers.md) を参照。
