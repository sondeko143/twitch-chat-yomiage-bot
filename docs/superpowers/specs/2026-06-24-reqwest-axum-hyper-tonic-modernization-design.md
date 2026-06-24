# reqwest / axum / hyper / tonic 近代化 設計

- 日付: 2026-06-24
- 対象: `twitch-chat-yomiage-bot`（本体）＋ `vstreamer-protos`（外部 git 依存、`sondeko143/vstreamer-protos`）
- 関連: RUSTSEC-2025-0134（`rustls-pemfile` unmaintained）、メモリ `reqwest-axum-hyper-modernization`

## 1. 背景とゴール

### 背景

- `rustls-pemfile v1.0.4`（unmaintained / RUSTSEC-2025-0134）が **`reqwest 0.11.27` 経由のみ** で残存している（`cargo tree -i rustls-pemfile` で確認済み。`tcyb` と `igdb` の両方が `reqwest 0.11` を持つ）。
- 現状 HTTP/gRPC スタックはすべて **hyper 0.14** に乗っている（hyper 1.x はツリーに無い）:
  - `axum 0.6`（`tcyb`）
  - `tonic 0.9` / `tonic-reflection 0.9`（`vstc`・`vstreamer_protos`・`tcyb` の未使用 direct 依存）
  - `reqwest 0.11`（`tcyb`・`igdb`）
  - `hyper 0.14`（`tcyb` の direct 依存。実使用は `auth.rs` の `hyper::Error` 型 1 箇所のみ）
- `reqwest` だけを 0.13（hyper 1.x 系）に上げると hyper が 0.14 と 1.x で二重化する。重複解消には axum・hyper・tonic も hyper 1.x 系へ揃える必要がある。

### ゴール / 成功条件

1. `cargo tree -i rustls-pemfile` が `did not match`（= 依存ツリーから消える）。
2. hyper が **1.x 単一**（`cargo tree` に 0.14 が出ない）。
3. `just ci` 全緑（fmt-check / clippy / test / deny / audit）。
4. 既存の機能・公開 API の挙動は不変（純粋な依存近代化。挙動変更はしない）。

## 2. スコープ

合意済みスコープ（ユーザー確認済み）:

- **フル近代化**: reqwest / axum / hyper に加え **tonic も** hyper 1.x 系へ。
- **付随整理**: `tcyb` の未使用 `tonic` direct 依存を削除、`tcyb` の direct `hyper` 依存を削除。

非スコープ:

- `tokio-tungstenite`（0.20→0.29）の更新。hyper とは独立で rustls-pemfile にも無関係のため今回は触らない。
- TLS バックエンドの変更。**native-tls 据え置き**（rustls は元々 reqwest 0.11 経由のみで、reqwest を上げれば消えるため rustls を導入する理由がない）。

## 3. ターゲット版

| crate | 現状 | ターゲット | 退避案 |
|---|---|---|---|
| reqwest | 0.11.27 | **0.13.x**（最新 0.13.4） | — |
| axum | 0.6.20 | **0.8.x**（最新 0.8.9） | 0.7.x |
| hyper（tcyb direct） | 0.14.32 | **削除**（不要化） | — |
| tonic | 0.9.2 | **0.14.x**（最新 0.14.6） | **0.12.x** |
| tonic-reflection | 0.9.2 | **0.14.x** | 0.12.x |
| prost（protos 側） | 0.11.9 | **0.14.x** | 0.13.x |
| tonic-build（protos 側） | 0.9.2 | **tonic-prost-build 0.14** | tonic-build 0.12 |

**tonic 0.14 を本命**とする（「フル近代化」=最新、protos 再生成はどのみち必須）。ただし 0.14 は crate 分割（`tonic` ＋ `tonic-prost`、`tonic-build` ＋ `tonic-prost-build`）を含み codegen 差分が大きい。**0.14 の codegen 移行が難航した場合は tonic 0.12 系へ退避**する（0.12 でも hyper 1.x 化＝重複解消とゴールは達成できる）。退避は protos / 本体の tonic・prost・builder 版だけ差し替える局所判断で可能。

## 4. アーキテクチャ / 変更単位

2 リポジトリにまたがる。protos が前提（先行）。

### Repo A: `vstreamer-protos`（`rust/` ワークスペース）

構造: `rust/` 自体が workspace。`vstreamer_protos`（lib。`[lib] path = "src/voicerecog.rs"` で **生成ファイルがそのまま crate root**）＋ `vstp_builder`（codegen バイナリ。`make rust` = `cargo run -p vstp_builder` で `voicerecog.rs` を再生成しコミットする運用）。

変更:

1. `vstp_builder/Cargo.toml`: `tonic-build 0.9` → `tonic-prost-build 0.14`（0.12 退避時は `tonic-build 0.12`）。
2. `vstp_builder/src/main.rs`: `tonic_build::configure().out_dir(...).compile(&[...], &[...])` → 新 API（`tonic_prost_build::configure().out_dir(...).compile_protos(&[...], &[...])`。正確なシンボルは実装時に `cargo doc`/ビルドエラーで確認）。
3. `vstreamer_protos/Cargo.toml`: `prost 0.11`→`0.14`、`tonic 0.9`→`0.14`、生成コードが要求するなら `tonic-prost 0.14` を追加。
4. `voicerecog.rs` を再生成しコミット。`version` を 0.1.1 → 0.1.2。
5. ビルド確認（`cd rust && cargo build`）。
6. push（main）→ 新しい git rev を取得。

### Repo B: `twitch-chat-yomiage-bot`

- **`vstc`**:
  - `Cargo.toml`: `tonic 0.9`→`0.14`、dev-dep `tonic-reflection 0.9`→`0.14`、`vstreamer_protos` の git rev / version を Repo A の新版へ更新。
  - `src/lib.rs`: `tonic::transport::Endpoint::new` / `.connect_timeout` / `.timeout` / `CommanderClient::connect(endpoint)` / `tonic::Request::new` / `channel.process_command().await` の 0.14 API 追従。`tonic::transport::Error` / `tonic::Status` の `From` 実装はそのまま使える見込み。
  - `tests/test.rs`: `tonic::transport::Server::builder().add_service(...)` / `.serve(addr)`、`tonic-reflection`（`Builder::configure().register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)`）、`#[tonic::async_trait]` ＋ mockall の `Commander` 実装を 0.14 へ。**サーバ trait の async 化方式が変わると mock 定義の調整が必要**（最大のリスク。§6 参照）。
- **`tcyb`**:
  - `Cargo.toml`: `reqwest 0.11`→`0.13`（features は `["json"]` 据え置き＝native-tls）。**`tonic` direct 依存を削除**（未使用）。**`hyper` direct 依存を削除**。
  - `src/auth.rs`: `axum::Server::bind(&addr).serve(app.into_make_service())` → `axum::serve(tokio::net::TcpListener::bind(addr).await?, app)`。`start_server` の戻り値 `Result<(), hyper::Error>` → `Result<(), std::io::Error>`（または `anyhow::Result<()>`）。`Router` / extractor / `IntoResponse` / `Redirect` / `http::StatusCode` は概ね非破壊。
  - reqwest 呼び出し（`api.rs`・`channel.rs`・`chat.rs`、ほか `eventsub.rs`・`store.rs` のエラー型）: `Client::new().get/post().header().query/json/form().send().await?.error_for_status()?.json/text/bytes()` 形で 0.13 でも非破壊の見込み。
- **`igdb`**:
  - `Cargo.toml`: `reqwest 0.11`→`0.13`（`["json"]` 据え置き）。
  - `src/api.rs`・`src/artwork.rs`: reqwest 呼び出しの追従（単純形のため非破壊の見込み）。

## 5. 開発フロー / 順序

`vstc` は `vstreamer_protos` を git URL + rev 参照するため、開発中はローカルで反復し、確定後に push して rev を固定する。

1. **patch をローカルに当てて反復**: workspace ルート `Cargo.toml` に
   ```toml
   [patch."https://github.com/sondeko143/vstreamer-protos"]
   vstreamer_protos = { path = "../vstreamer-protos/rust/vstreamer_protos" }
   ```
   を一時追加し、ローカルの再生成 protos に対して両 repo を同時検証する。
2. protos 側を仕上げる（再生成・ビルド緑）。
3. 本体側（vstc → tcyb → igdb）を移行し `just check` を緑にする。
4. **確定**: protos を push（main, 0.1.2）→ `vstc/Cargo.toml` の git rev/version を新版へ固定 → **`[patch]` を削除**（commit に残さない）。
5. `cargo update` 後に `cargo tree -i rustls-pemfile`（=did not match）と `cargo tree -i hyper`（=1.x 単一）を確認。
6. `just ci` 全緑を確認して PR。

## 6. リスクと対応

- **tonic 0.14 codegen の不確実性**（最大リスク）: crate 分割（`tonic-prost` / `tonic-prost-build`）と生成コードの参照変更、サーバ trait の async 表現変更で `vstc/tests/test.rs` の `#[tonic::async_trait]` ＋ mockall 部分が要調整。実装時に protos を実際に再生成して生成物を確認し、難航するなら **tonic 0.12 退避**（生成コードが現状に近く `.compile_protos()` 改名程度）。
- **reqwest 0.13 / axum 0.8 の破壊的変更**: 呼び出し箇所が少なく局所対応可能。axum 0.8 の path param 構文変更（`/:id`→`/{id}`）は本プロジェクトに該当ルート無し（`/` と `/callback` のみ）。
- **cross-repo の取り回し**: push 前に rev を固定できないため §5 の patch フローで吸収。patch を commit に残さないことを確定ステップで担保。
- **CLAUDE.md 品質ゲート**: PR / main マージ前に `just ci` 全緑必須（Windows 前提、`set windows-shell` 済み）。

## 7. 検証

1. protos: `cd rust && cargo build`（再生成物が緑）。
2. 本体: `just check`（fmt-check + clippy + test）。
3. `cargo tree -i rustls-pemfile` → `did not match`。
4. `cargo tree -i hyper` → 1.x のみ（0.14 が出ない）。
5. `just ci`（フルゲート）全緑。
6. 完了後、メモリ `reqwest-axum-hyper-modernization` を解消済みに更新（または削除）。
