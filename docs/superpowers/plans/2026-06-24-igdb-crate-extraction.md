# igdb クレート切り出し Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** artwork/IGDB 機能を新クレート `igdb` へ切り出し、ボット本体 `tcyb` から `image` 依存と IGDB コードを取り除く。

**Architecture:** ワークスペースに単一バイナリクレート `igdb` を新設し、`tcyb/src/artwork.rs` と `tcyb/src/api.rs` の IGDB 部分（`search_game`/`get_cover`/構造体/URL定数/`get_tokens_by_client_credentials`）を移設する。共有していた `auth_headers`（10行）は `igdb` 側に複製し、axum を持ち込まないため `reqwest::header` を使う。`client_id`/`client_secret` は同じ `.env`（env prefix `cb`）から読むので Twitch アプリは共有のまま。

**Tech Stack:** Rust 2021 / Cargo workspace / clap / config / dotenv / reqwest / serde / tokio / image

## Global Constraints

- 既存のクレート命名・流儀に倣う（`vstc`=lib, `vstc_cli`=bin と同じ「用途別クレート」）。
- artwork の機能・出力（カレントディレクトリに `thumbnails.jpg` 生成）の挙動は変えない。移設のみ。
- Twitch アプリは再登録しない。`igdb` は `tcyb` と同じ `.env` / 設定（env prefix `cb`、`--config`）から `client_id`/`client_secret` を読む。
- `igdb` には axum を入れない。`auth_headers` は `reqwest::header::{HeaderMap, HeaderValue}` を使う。
- 各ステップのビルド確認はワークスペースルート `c:\Users\me_my\vstreamer\twitch-chat-yomiage-bot` で行う。
- このリファクタは挙動不変・既存テストなしのため、各ステップの検証は `cargo build`（および最終タスクで `cargo tree` と手動実行）で行う。

---

### Task 1: `igdb` クレートを新設し artwork/IGDB を移設（完全動作する状態まで）

**Files:**
- Modify: `Cargo.toml`（workspace members に `igdb` を追加）
- Create: `igdb/Cargo.toml`
- Create: `igdb/src/main.rs`
- Create: `igdb/src/settings.rs`
- Create: `igdb/src/api.rs`
- Create: `igdb/src/artwork.rs`

**Interfaces:**
- Produces:
  - `igdb::settings::Settings { client_id: String, client_secret: String }`
  - `igdb::api::get_tokens_by_client_credentials(client_id: &str, client_secret: &str) -> Result<String, reqwest::Error>`
  - `igdb::api::search_game(access_token: &str, client_id: &str, name: &str) -> Result<Vec<SearchGame>, reqwest::Error>`
  - `igdb::api::get_cover(access_token: &str, client_id: &str, games: &[i64]) -> Result<Vec<Artwork>, reqwest::Error>`
  - `igdb::api::SearchGame { id: i64, cover: i64, name: String }`、`igdb::api::Artwork { game: i64, image_id: String }`
  - `igdb::artwork::get_artwork(client_id: &str, client_secret: &str, names: &[String]) -> anyhow::Result<()>`
  - バイナリ CLI: `igdb get-artwork <names...>`
- Consumes: なし（独立した新クレート）

- [ ] **Step 1: workspace members に `igdb` を追加**

`Cargo.toml`（ルート）を次に変更:

```toml
[workspace]
resolver = "2"
members = ["tcyb", "vstc", "vstc_cli", "igdb"]
```

- [ ] **Step 2: `igdb/Cargo.toml` を作成**

```toml
[package]
name = "igdb"
version = "0.1.0"
edition = "2021"
description = "IGDB artwork thumbnail generator"

[dependencies]
anyhow = "1.0.71"
clap = { version = "4.2.7", features = ["derive"] }
config = { version = "0.13.3", features = ["toml"] }
const_format = "0.2.30"
dotenv = "0.15.0"
image = "0.24.6"
log = "0.4.17"
reqwest = { version = "0.11.17", features = ["json"] }
serde = { version = "1.0.162", features = ["derive"] }
simple_logger = "4.1.0"
tokio = { version = "1.28.1", features = ["full"] }
```

- [ ] **Step 3: `igdb/src/settings.rs` を作成**

```rust
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Clone)]
pub struct Settings {
    pub client_id: String,
    pub client_secret: String,
}
```

- [ ] **Step 4: `igdb/src/api.rs` を作成**

`tcyb/src/api.rs` の IGDB 部分を移設。`auth_headers` は `reqwest::header` を使う形に複製する。

```rust
use const_format::formatcp;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

const IGDB_API_HOST: &str = "api.igdb.com";
const IGDB_GAME_API_URL: &str = formatcp!("https://{}/v4/games", IGDB_API_HOST);
const IGDB_COVER_API_URL: &str = formatcp!("https://{}/v4/covers", IGDB_API_HOST);
const TWITCH_ID_HOST: &str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);

#[derive(Serialize, Deserialize)]
struct ClientCredentialsToken {
    access_token: String,
    expires_in: i64,
    token_type: String,
}

pub async fn get_tokens_by_client_credentials(
    client_id: &str,
    client_secret: &str,
) -> Result<String, reqwest::Error> {
    let res: ClientCredentialsToken = reqwest::Client::new()
        .post(TWITCH_OAUTH2_TOKEN_URL)
        .query(&[
            ("client_id", client_id),
            ("grant_type", "client_credentials"),
            ("client_secret", client_secret),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(res.access_token)
}

#[derive(Serialize, Deserialize)]
pub struct SearchGame {
    pub id: i64,
    pub cover: i64,
    pub name: String,
}

pub async fn search_game(
    access_token: &str,
    client_id: &str,
    name: &str,
) -> Result<Vec<SearchGame>, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let query = match name.parse::<i64>() {
        Ok(game_id) => {
            format!("fields id,name,cover; where id = {}; limit 1;", game_id)
        }
        Err(_) => {
            format!(
                "fields id,name,cover; search \"{}\"; where cover != null; limit 1;",
                name
            )
        }
    };
    let res: Vec<SearchGame> = reqwest::Client::new()
        .post(IGDB_GAME_API_URL)
        .headers(headers)
        .body(query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

#[derive(Serialize, Deserialize)]
pub struct Artwork {
    pub game: i64,
    pub image_id: String,
}

pub async fn get_cover(
    access_token: &str,
    client_id: &str,
    games: &[i64],
) -> Result<Vec<Artwork>, reqwest::Error> {
    let headers = auth_headers(access_token, client_id);
    let query = format!(
        "fields *; where game = ({});",
        games
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let res: Vec<Artwork> = reqwest::Client::new()
        .post(IGDB_COVER_API_URL)
        .headers(headers)
        .body(query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

fn auth_headers(access_token: &str, client_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.append(
        "Authorization",
        format!("Bearer {access_token}")
            .parse::<HeaderValue>()
            .unwrap(),
    );
    headers.append("Client-Id", client_id.parse::<HeaderValue>().unwrap());
    headers
}
```

- [ ] **Step 5: `igdb/src/artwork.rs` を作成**

`tcyb/src/artwork.rs` をそのまま移設（`use crate::api;` は新クレート内の `api` を指す）。

```rust
use crate::api;
use anyhow::bail;
use image::io::Reader as ImageReader;
use image::RgbaImage;
use log::{info, warn};
use std::io::Cursor;

pub async fn get_artwork(
    client_id: &str,
    client_secret: &str,
    names: &[String],
) -> anyhow::Result<()> {
    let access_token = api::get_tokens_by_client_credentials(client_id, client_secret).await?;
    let mut games: Vec<i64> = vec![];
    for name in names {
        let game_id = match game_ids(name, &access_token, client_id).await {
            Ok(r) => r,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        };
        games.push(game_id);
    }
    let covers = api::get_cover(&access_token, client_id, &games).await?;
    let image_width: u32 = 1252;
    let mut img = RgbaImage::new(image_width, 704);
    for (idx, game) in games.iter().enumerate() {
        let cover = covers
            .iter()
            .find(|x| x.game == *game)
            .unwrap_or_else(|| panic!("game id {} not found", game));
        info!("{}: {}", cover.game, cover.image_id);
        let content = reqwest::Client::new()
            .get(format!(
                "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/{}.jpg",
                cover.image_id
            ))
            .send()
            .await?
            .bytes()
            .await?;
        let on_top = ImageReader::new(Cursor::new(content))
            .with_guessed_format()?
            .decode()?;
        let len: u32 = covers.len().try_into().unwrap();
        let index: u32 = idx.try_into().unwrap();
        let offset_x: u32 = (image_width / len) * index;
        image::imageops::overlay(&mut img, &on_top, offset_x.into(), 0);
    }
    img.save("thumbnails.jpg")?;
    Ok(())
}

async fn game_ids(name: &str, access_token: &str, client_id: &str) -> anyhow::Result<i64> {
    let games = api::search_game(access_token, client_id, name).await?;
    if games.is_empty() {
        bail!("Not found {}", name)
    }
    info!(
        "{}: {}, cover: {}",
        games[0].id, games[0].name, games[0].cover
    );
    Ok(games[0].id)
}
```

- [ ] **Step 6: `igdb/src/main.rs` を作成（CLI を結線）**

```rust
mod api;
mod artwork;
mod settings;
use anyhow::Result;
use clap::{Parser, Subcommand};
use settings::Settings;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    GetArtwork { names: Vec<String> },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let args = Cli::parse();
    let mut config_builder = config::Config::builder();
    config_builder = config_builder
        .add_source(config::Environment::with_prefix("cb").try_parsing(true));
    if let Some(path) = args.config {
        config_builder =
            config_builder.add_source(config::File::with_name(path.to_str().unwrap()));
    }
    let config = config_builder.build()?;
    let settings: Settings = config.try_deserialize()?;
    simple_logger::SimpleLogger::new()
        .env()
        .with_local_timestamps()
        .init()?;

    match &args.command {
        Some(Commands::GetArtwork { names }) => {
            artwork::get_artwork(&settings.client_id, &settings.client_secret, names).await?;
        }
        None => {}
    }
    Ok(())
}
```

- [ ] **Step 7: `igdb` クレートをビルドして通ることを確認**

Run: `cargo build -p igdb`
Expected: コンパイル成功（warning は許容）。

- [ ] **Step 8: CLI が起動することを確認**

Run: `cargo run -p igdb -- get-artwork --help`
Expected: clap のヘルプ（`get-artwork [NAMES]...`）が表示され、正常終了する。

- [ ] **Step 9: コミット**

```bash
git add Cargo.toml Cargo.lock igdb/
git commit -m "feat(igdb): artwork/IGDB を独立クレート igdb に切り出し"
```

---

### Task 2: `tcyb` から artwork/IGDB と `image` 依存を削除

**Files:**
- Modify: `tcyb/src/main.rs`（`mod artwork;`、`GetArtwork` サブコマンドと match 分岐を削除）
- Delete: `tcyb/src/artwork.rs`
- Modify: `tcyb/src/api.rs`（IGDB 部分を削除、Twitch Helix 専用にする）
- Modify: `tcyb/Cargo.toml`（`image` 依存を削除）

**Interfaces:**
- Consumes: Task 1 で `igdb` 側に移設済みであること（`tcyb` 側のこれらは不要になっている）。
- Produces: `tcyb` は Twitch 機能のみのクレートになる（外部インターフェース変更なし）。

- [ ] **Step 1: `tcyb/src/artwork.rs` を削除**

```bash
git rm tcyb/src/artwork.rs
```

- [ ] **Step 2: `tcyb/src/main.rs` から artwork 関連を削除**

`mod artwork;` の行（モジュール宣言、現 2 行目）を削除する。

`enum Commands` から次の variant を削除:

```rust
    GetArtwork { names: Vec<String> },
```

`match &args.command` から次の分岐を削除:

```rust
        Some(Commands::GetArtwork { names }) => {
            artwork::get_artwork(&settings.client_id, &settings.client_secret, names).await?;
        }
```

- [ ] **Step 3: `tcyb/src/api.rs` から IGDB 部分を削除**

次の項目を削除する（`auth_headers` と Twitch 系の定数・関数はすべて残す）:

- 定数 `IGDB_API_HOST`、`IGDB_GAME_API_URL`、`IGDB_COVER_API_URL`
- `struct ClientCredentialsToken` と `pub async fn get_tokens_by_client_credentials(...)`
- `pub struct SearchGame` と `pub async fn search_game(...)`
- `pub struct Artwork` と `pub async fn get_cover(...)`

削除後、`tcyb/src/api.rs` の先頭定数ブロックは次の状態になる（IGDB 行が消えている）:

```rust
use axum::http::{HeaderMap, HeaderValue};
use const_format::formatcp;
use serde::{Deserialize, Serialize};

const TWITCH_API_HOST: &str = "api.twitch.tv";
const TWITCH_USERS_API_URL: &str = formatcp!("https://{}/helix/users", TWITCH_API_HOST);
const TWITCH_BANS_API_URL: &str = formatcp!("https://{}/helix/moderation/bans", TWITCH_API_HOST);
const TWITCH_CHATTERS_API_URL: &str = formatcp!("https://{}/helix/chat/chatters", TWITCH_API_HOST);
const TWITCH_FOLLOWED_API_URL: &str =
    formatcp!("https://{}/helix/channels/followed", TWITCH_API_HOST);
const TWITCH_SUB_EVENT_API_URL: &str =
    formatcp!("https://{}/helix/eventsub/subscriptions", TWITCH_API_HOST);
const TWITCH_ID_HOST: &str = "id.twitch.tv";
const TWITCH_OAUTH2_TOKEN_URL: &str = formatcp!("https://{}/oauth2/token", TWITCH_ID_HOST);
pub const TWITCH_OAUTH2_AUTHZ_URL: &str = formatcp!("https://{}/oauth2/authorize", TWITCH_ID_HOST);
```

（`auth_headers` 関数はファイル末尾にそのまま残す。`get_user`/`get_tokens_by_refresh`/`get_tokens_by_code`/`ban_user`/`get_chatters`/`get_followed`/`sub_event` とそれぞれの構造体も残す。）

- [ ] **Step 4: `tcyb/Cargo.toml` から `image` 依存を削除**

次の行を削除:

```toml
image = "0.24.6"
```

- [ ] **Step 5: `tcyb` をビルドして通ることを確認**

Run: `cargo build -p tcyb`
Expected: コンパイル成功。未使用 import 等の warning が出た場合は該当 import を削除して解消する。

- [ ] **Step 6: `tcyb` の依存ツリーから `image` が消えたことを確認**

Run: `cargo tree -p tcyb -i image`
Expected: `image` がツリーに存在しない旨（`package ID specification ... did not match any packages` のようなエラー、または何も出力されない）。= 本体バイナリから `image` 依存が外れた証拠。

- [ ] **Step 7: ワークスペース全体のビルド確認**

Run: `cargo build`
Expected: `tcyb` / `igdb` / `vstc` / `vstc_cli` すべてコンパイル成功。

- [ ] **Step 8: コミット**

```bash
git add tcyb/ Cargo.lock
git commit -m "refactor(tcyb): artwork/IGDB と image 依存を igdb クレートへ移し本体から削除"
```

---

### Task 3: 動作確認（artwork 生成の挙動が保たれていること）

**Files:** なし（実行確認のみ）

**Interfaces:**
- Consumes: Task 1・2 完了済み。`.env`（または `--config`）に `cb_client_id` / `cb_client_secret` 相当が設定されていること。

- [ ] **Step 1: artwork 生成を実行して `thumbnails.jpg` ができることを確認**

Run: `cargo run -p igdb -- get-artwork "Elden Ring"`
Expected: ログに検索したゲームの id / cover が出力され、カレントディレクトリに `thumbnails.jpg` が生成される（移設前の `tcyb get-artwork "Elden Ring"` と同じ結果）。

> 注: このステップは IGDB（= Twitch OAuth）の有効な `client_id`/`client_secret` とネットワークが必要。credentials やネットワークが無い実行環境では、Task 1 Step 8 の `--help` 起動確認と Task 2 のビルド確認をもって完了とし、この実機確認はユーザーが手元で行う。

- [ ] **Step 2: （任意）生成物を片付ける**

確認用に生成した `thumbnails.jpg` はコミット対象ではない。不要なら削除する。

---

## Self-Review

**1. Spec coverage（spec の各節 → 対応タスク）:**
- 「ワークスペースに `igdb` 新設」→ Task 1 Step 1-2 ✓
- 「artwork.rs と IGDB API の移設」→ Task 1 Step 4-5 ✓
- 「`get_tokens_by_client_credentials` 丸ごと移動」→ Task 1 Step 4 / Task 2 Step 3 ✓
- 「`auth_headers` 複製＋`reqwest::header` 化」→ Task 1 Step 4 ✓
- 「最小 `Settings`（client_id/client_secret）」→ Task 1 Step 3 ✓
- 「同じ `.env`（prefix `cb`、`--config`）から読む」→ Task 1 Step 6 ✓
- 「CLI `igdb get-artwork`」→ Task 1 Step 6 ✓
- 「`tcyb` から artwork コード・`GetArtwork`・IGDB 部分・`image` 削除」→ Task 2 ✓
- 「検証: `cargo build -p igdb`/`-p tcyb`/全体、`cargo tree` で image 消失、実機で thumbnails.jpg」→ Task 1 Step 7-8 / Task 2 Step 5-7 / Task 3 ✓

**2. Placeholder scan:** TBD/TODO/「適切に処理」等なし。コードを要するステップはすべて実コードを記載済み。✓

**3. Type consistency:** `get_artwork(client_id, client_secret, names: &[String])`、`get_tokens_by_client_credentials(client_id, client_secret) -> Result<String, reqwest::Error>`、`SearchGame{id,cover,name}`、`Artwork{game,image_id}`、`Settings{client_id,client_secret}` は Task 1 内で定義し、main.rs から同じシグネチャで呼んでいる。移設元 `tcyb` と同一定義。✓
