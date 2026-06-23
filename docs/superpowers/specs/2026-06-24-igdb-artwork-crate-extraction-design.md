# artwork/IGDB を `igdb` クレートへ切り出す — 設計

- 日付: 2026-06-24
- 対象コンポーネント: ワークスペース全体（`tcyb` から artwork/IGDB を分離し、新クレート `igdb` を新設）

## 背景・目的

`tcyb` は Twitch チャット読み上げ（TTS）ボットだが、ゲームのサムネイル画像を生成する
artwork 機能（`tcyb/src/artwork.rs` と `GetArtwork` サブコマンド）も同居している。
artwork は **IGDB API** を叩く機能で、チャット読み上げとは無関係。

同居している理由は当初、(1) API 認証コードを再利用したい、(2) アプリを分けると Twitch
Developer Console への再登録が面倒、という2点だった。

### 前提の整理（重要）

- IGDB API の認証は **実体として Twitch OAuth そのもの**。`get_tokens_by_client_credentials`
  は `https://id.twitch.tv/oauth2/token` に `client_credentials` で投げ（`tcyb/src/api.rs`）、
  `auth_headers` も同じ `Client-Id` を付ける（`tcyb/src/api.rs`）。だから認証が「共通」なのは必然。
- **「コードを分ける」と「Twitch アプリを分ける（= 新しい client_id/secret の登録）」は別の軸。**
  コードをクレートに分けても、同じ `client_id`/`client_secret` を設定から読めばよいだけで、
  Developer Console への再登録は不要。

目的: **artwork/IGDB を独立クレート `igdb` に切り出し、ボット本体（`tcyb`）の依存（特に重い
`image` クレートとその依存ツリー）を軽くする。認証情報は同じ Twitch アプリを共有し続ける。**

主動機はボット本体の依存削減。Cargo の依存はパッケージ単位で、同一クレート内の全バイナリ
ターゲットが同じ `[dependencies]` を共有するため、別バイナリ（`[[bin]]`）やモジュール分割では
`image` を本体から外せない。依存を本当に分離できるのは **別クレート化** だけ。

## スコープ

### 対象
- ワークスペースに新クレート `igdb`（単一バイナリクレート）を追加。
- `tcyb/src/artwork.rs` のロジックと、`tcyb/src/api.rs` の IGDB 部分を `igdb` へ移動。
- `tcyb` から artwork/IGDB のコードと `image` 依存、`GetArtwork` サブコマンドを削除。

### 対象外
- artwork の機能・出力（`thumbnails.jpg`）の挙動変更はしない。移設のみ。
- Twitch アプリの再登録はしない（同じ `client_id`/`client_secret` を共有）。
- `tcyb` の Twitch 機能（読み上げ・認証・モデレーション等）のロジック変更はしない。
- 共有 auth 用の独立クレート新設はしない（後述、`auth_headers` は複製方針）。

## 構成

[Cargo.toml](../../../Cargo.toml) の `members` に `igdb` を追加（既存の `vstc`(lib)+`vstc_cli`(bin)
と同じ「用途別クレート」の流儀に倣う）。`igdb` は単一バイナリクレートとし、`tcyb` の内部構成を
小さく真似た形にする。

```
igdb/
  Cargo.toml
  src/
    main.rs       # clap CLI: `igdb get-artwork <names...>`
    api.rs        # IGDB 専用 API + 認証
    artwork.rs    # 画像合成ロジック（thumbnails.jpg 出力）
    settings.rs   # 最小設定: client_id / client_secret
```

## 何がどこへ移るか

`tcyb` → `igdb` へ移動するもの:

- `tcyb/src/artwork.rs` 全体（`get_artwork` / `game_ids`）→ `igdb/src/artwork.rs`
- `tcyb/src/api.rs` の IGDB 部分 → `igdb/src/api.rs`:
  - 定数: `IGDB_API_HOST`、`IGDB_GAME_API_URL`、`IGDB_COVER_API_URL`
  - 関数: `search_game`、`get_cover`
  - 構造体: `SearchGame`、`Artwork`
  - `get_tokens_by_client_credentials`（**artwork でしか使われていない**ため丸ごと移動。
    `TWITCH_OAUTH2_TOKEN_URL` 相当の定数も `igdb` 側に持つ）
- `auth_headers`（10行）は **複製**して `igdb/src/api.rs` にも置く（次節）。

## `tcyb` 側の変更（削除）

- `tcyb/src/main.rs`: `mod artwork;`、`Commands::GetArtwork { names }` の定義と `match` 分岐を削除。
- `tcyb/src/artwork.rs`: ファイル削除。
- `tcyb/src/api.rs`: 上記 IGDB 部分（定数・関数・構造体・`get_tokens_by_client_credentials`）を削除。
  残る `api.rs` は **Twitch Helix 専用** となる。
- `tcyb/Cargo.toml`: `image` 依存を削除。

> 検証で `image` が他から参照されていないことは確認済み（`image` の使用箇所は `artwork.rs` のみ）。

## 認証の共有方針（判断ポイント）

両クレートが共通で必要とするコードは `auth_headers`（Bearer + `Client-Id` を付けるだけの約10行）
のみ。`get_tokens_by_client_credentials` は artwork 専用なので共有不要。

方針: **複製**。`auth_headers` を `igdb/src/api.rs` にも置く。10行・依存ゼロで、共有のためだけに
4つ目のクレートを増やすより素直（DRY より凝集と単純さを優先）。

実装上の注意:
- 現在の `auth_headers` は `axum::http::HeaderMap` を使う（`tcyb` が axum 依存のため）。`igdb` には
  axum を持ち込みたくないので、`igdb` 側の `auth_headers` は **`reqwest::header::{HeaderMap, HeaderValue}`**
  を使う。reqwest が `http` クレートのヘッダ型を再エクスポートしているため追加依存なしで書ける。

## 設定・認証情報

- `igdb/src/settings.rs` は `client_id` / `client_secret` だけを持つ最小 `Settings` 構造体
  （`serde::Deserialize`）。
- `igdb/src/main.rs` は `tcyb/src/main.rs` と同様に `dotenv` + `config` で設定を読む:
  環境変数 prefix `cb`、および任意の `--config <path>`。
- これにより `tcyb` と **同じ `.env` / 設定ファイル**から `client_id`/`client_secret` を読む。
  → 同じ Twitch アプリを共有 = Developer Console の再登録は不要。

## CLI

- 現在 `tcyb get-artwork <names...>`（`Commands::GetArtwork`）で実行していたものを、
  `igdb get-artwork <names...>` に移す。
- 出力は従来通りカレントディレクトリに `thumbnails.jpg` を生成。

## `igdb` クレートの依存（`igdb/Cargo.toml`）

artwork/IGDB の実行に必要なものに絞る:

- `anyhow`、`reqwest`(features=["json"])、`serde`(features=["derive"])、`tokio`(features=["full"])、
  `log`、`simple_logger`、`clap`(features=["derive"])、`config`(features=["toml"])、`dotenv`、
  `const_format`、`image`

`tcyb` 側からは `image` が消え、それ以外（reqwest/serde/tokio など）は引き続き本体に必要なので残る。

## 検証

- `cargo build -p igdb` が通る。
- `cargo build -p tcyb` が通る。`cargo tree -p tcyb` に `image` が出てこないこと
  （本体バイナリから `image` 依存ツリーが消えたことの確認）。
- `cargo build`（ワークスペース全体）が通る。
- 動作確認: `.env` に `client_id`/`client_secret` がある状態で `igdb get-artwork <ゲーム名>` を実行し、
  `thumbnails.jpg` が生成される（移設前の `tcyb get-artwork` と同じ挙動）。

## 既知の制限・備考

- `auth_headers` を複製するため、Twitch 認証ヘッダの仕様が将来変わった場合は2箇所
  （`tcyb/src/api.rs` と `igdb/src/api.rs`）を直す必要がある。仕様が安定しており10行のため許容する。
- `igdb` と `tcyb` は同一の Twitch アプリ（client_credentials）を使う。IGDB のレートリミットも
  共有される点に留意（従来も同じ）。
