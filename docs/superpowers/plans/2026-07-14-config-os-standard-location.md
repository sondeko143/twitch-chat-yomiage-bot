# 秘密・設定を OS 標準ユーザーディレクトリへ Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `client_secret` を含む設定・秘密・トークン DB を CWD の `.env` からリポジトリ外の OS 標準ユーザーディレクトリ（Windows は Roaming AppData 配下）へ移し、CWD 非依存かつ git へ構造的にリークしない構成にする。

**Architecture:** 新モジュール `paths` が `directories` クレートで OS 標準の設定/データパスを解決する（テスト用に `TCYB_CONFIG_DIR` で差し替え可）。`settings` モジュールに (1) 初回起動時のテンプレ生成 `scaffold_config` と (2) デフォルト→OS 設定ファイル→env→`--config` の順で重ねる `load` を追加。`main` は `dotenvy` を廃し、パス解決→（無ければ）scaffold して終了→`load` の流れに置き換える。

**Tech Stack:** Rust 2021 / `directories`（新規）/ `config` 0.13（既存）/ `tempfile`（dev, 新規）。`dotenvy` は削除。

## Global Constraints

- 対象クレートは `tcyb` のみ（`igdb` 等の他クレートは本 plan のスコープ外）。
- 決定は [ADR-0013](../../adr/0013-config-secret-in-os-standard-user-dir.md) に従う（平文のまま場所だけ移す。keyring・暗号化はやらない）。
- ドキュメント/コードに個人・マシン依存の絶対パス（ホーム / AppData のリテラル）を書かない。`%APPDATA%` や `<OS 設定ディレクトリ>` 等のプレースホルダを使う（`just check-env-leak` が緑であること）。
- PR/マージ前に `just ci` 全緑（exit code 0）。fmt-check の赤は `just fmt` で解消可。
- 環境は Windows 前提。justfile は `set windows-shell := ["cmd.exe","/c"]` 済み。

---

### Task 1: `paths` モジュール（OS 標準パス解決）

**Files:**
- Modify: `tcyb/Cargo.toml`（`directories` 依存追加）
- Create: `tcyb/src/paths.rs`
- Modify: `tcyb/src/main.rs`（`mod paths;` 追加）

**Interfaces:**
- Produces:
  - `pub struct AppPaths { pub config_file: std::path::PathBuf, pub db_dir: std::path::PathBuf }`
  - `pub fn app_paths() -> anyhow::Result<AppPaths>` — 本番用。`TCYB_CONFIG_DIR` があればその配下、無ければ `directories::ProjectDirs::from("", "", "tcyb")` の config/data ディレクトリを使う。
  - `fn app_paths_from(base_override: Option<std::path::PathBuf>, proj: Option<directories::ProjectDirs>) -> anyhow::Result<AppPaths>` — 純粋関数（テスト対象）。

- [ ] **Step 1: `directories` 依存を追加**

Run: `cargo add -p tcyb directories`
Expected: `tcyb/Cargo.toml` の `[dependencies]` に `directories = "…"` が追記され、`Cargo.lock` が更新される。

- [ ] **Step 2: 失敗するテストを書く**

Create `tcyb/src/paths.rs`:

```rust
use anyhow::Context;
use directories::ProjectDirs;
use std::path::PathBuf;

const APP_DIR_ENV: &str = "TCYB_CONFIG_DIR";

pub struct AppPaths {
    pub config_file: PathBuf,
    pub db_dir: PathBuf,
}

fn app_paths_from(base_override: Option<PathBuf>, proj: Option<ProjectDirs>) -> anyhow::Result<AppPaths> {
    if let Some(base) = base_override {
        return Ok(AppPaths {
            config_file: base.join("config.toml"),
            db_dir: base.join("db"),
        });
    }
    let proj = proj.context("OS のユーザーディレクトリを解決できませんでした")?;
    Ok(AppPaths {
        config_file: proj.config_dir().join("config.toml"),
        db_dir: proj.data_dir().to_path_buf(),
    })
}

pub fn app_paths() -> anyhow::Result<AppPaths> {
    let base = std::env::var_os(APP_DIR_ENV).map(PathBuf::from);
    app_paths_from(base, ProjectDirs::from("", "", "tcyb"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_dir_places_config_and_db_under_base() {
        let base = PathBuf::from("/tmp/tcyb-base");
        let p = app_paths_from(Some(base.clone()), None).unwrap();
        assert_eq!(p.config_file, base.join("config.toml"));
        assert_eq!(p.db_dir, base.join("db"));
    }

    #[test]
    fn missing_project_dirs_without_override_errors() {
        assert!(app_paths_from(None, None).is_err());
    }
}
```

- [ ] **Step 3: テストが失敗することを確認（`mod paths;` 未配線でコンパイルエラー）**

Run: `cargo test -p tcyb paths::`
Expected: FAIL（`main.rs` に `mod paths;` が無いためモジュール未認識、またはリンクされない）

- [ ] **Step 4: `main.rs` にモジュールを配線**

Modify `tcyb/src/main.rs` のモジュール宣言群（先頭付近）に 1 行追加:

```rust
mod paths;
```

（既存の `mod api;` … `mod yomiage;` と同じ並びに追加する。）

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p tcyb paths::`
Expected: PASS（2 tests）

- [ ] **Step 6: コミット**

```bash
git add tcyb/Cargo.toml Cargo.lock tcyb/src/paths.rs tcyb/src/main.rs
git commit -m "feat(tcyb): OS 標準パスを解決する paths モジュールを追加"
```

---

### Task 2: 設定テンプレの自動生成（`settings::scaffold_config`）

**Files:**
- Modify: `tcyb/Cargo.toml`（`tempfile` を dev-dependency に追加）
- Modify: `tcyb/src/settings.rs`

**Interfaces:**
- Consumes: なし
- Produces: `pub fn scaffold_config(config_file: &std::path::Path) -> anyhow::Result<()>` — 親ディレクトリを作成し、プレースホルダ入りテンプレ `config.toml` を書き出す。

- [ ] **Step 1: `tempfile` を dev-dependency に追加**

Run: `cargo add -p tcyb --dev tempfile`
Expected: `tcyb/Cargo.toml` に `[dev-dependencies]` の `tempfile = "…"` が追記される。

- [ ] **Step 2: 失敗するテストを書く**

Append to `tcyb/src/settings.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_writes_parseable_template_with_required_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");

        scaffold_config(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // 必須の秘密キーがテンプレに含まれる
        assert!(text.contains("client_id"));
        assert!(text.contains("client_secret"));
        // 生成物は妥当な TOML である
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert!(parsed.get("client_secret").is_some());
    }
}
```

（`toml` クレートは `config` の依存として既に解決済み。dev で明示追加が必要なら `cargo add -p tcyb --dev toml` を実行する。）

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p tcyb settings::`
Expected: FAIL（`scaffold_config` 未定義）

- [ ] **Step 4: 最小実装を書く**

`tcyb/src/settings.rs` の先頭 `use` を調整し、関数とテンプレ定数を追加:

```rust
use serde::Deserialize;
use std::{fmt::Debug, path::Path, path::PathBuf};

// （既存の struct Settings 定義はそのまま）

const CONFIG_TEMPLATE: &str = r#"# tcyb 設定ファイル
client_id = ""
client_secret = ""
channel = "your_channel_name"
username = "your_username"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05", "o:/play?v=18"]
greeting_template = "user_name さん。フォローありがとうございます。"
translate_command = "translate"
# listen_address = "localhost:8000"   # 既定値あり。変更時のみ記入
# db_dir / db_name は OS 標準データディレクトリを既定使用（変更時のみ記入）
"#;

pub fn scaffold_config(config_file: &Path) -> anyhow::Result<()> {
    if let Some(parent) = config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_file, CONFIG_TEMPLATE)?;
    Ok(())
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p tcyb settings::`
Expected: PASS（1 test）

- [ ] **Step 6: コミット**

```bash
git add tcyb/Cargo.toml Cargo.lock tcyb/src/settings.rs
git commit -m "feat(tcyb): 初回起動用の設定テンプレ生成 scaffold_config を追加"
```

---

### Task 3: 設定ロード（優先順位 + db_dir デフォルト）

**Files:**
- Modify: `tcyb/src/settings.rs`

**Interfaces:**
- Consumes: `Settings`, `scaffold_config`（同モジュール）
- Produces: `pub fn load(config_file: &std::path::Path, cli_config: Option<&std::path::Path>, default_db_dir: &std::path::Path) -> anyhow::Result<Settings>` — 優先順位（低→高）: 組み込みデフォルト → OS 設定ファイル（存在すれば） → `cb_` env → `--config` 明示ファイル。`db_dir` は未指定なら `default_db_dir`。

- [ ] **Step 1: 失敗するテストを書く**

`tcyb/src/settings.rs` の `mod tests` に追記:

```rust
    fn write_config(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    const FULL_CONFIG: &str = r#"
client_id = "id"
client_secret = "secret"
channel = "ch"
username = "user"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja"]
translate_command = "translate"
"#;

    #[test]
    fn load_applies_default_db_dir_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), FULL_CONFIG);
        let default_db = std::path::Path::new("/var/tcyb-data");

        let s = load(&cfg, None, default_db).unwrap();

        assert_eq!(s.db_dir, default_db);
        assert_eq!(s.client_secret, "secret");
    }

    #[test]
    fn load_config_file_overrides_default_db_dir() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("{}\ndb_dir = \"custom-db\"\n", FULL_CONFIG);
        let cfg = write_config(dir.path(), &body);

        let s = load(&cfg, None, std::path::Path::new("/var/tcyb-data")).unwrap();

        assert_eq!(s.db_dir, std::path::Path::new("custom-db"));
    }

    #[test]
    fn load_errors_when_required_secret_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = write_config(dir.path(), "client_id = \"id\"\n");

        let err = load(&cfg, None, std::path::Path::new("/var/tcyb-data"));

        assert!(err.is_err());
    }
```

（注: これらのテストは `cb_*` 環境変数が未設定であることを前提にする。CI/ローカルシェルで `cb_*` を設定しないこと。）

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p tcyb settings::`
Expected: FAIL（`load` 未定義）

- [ ] **Step 3: `load` を実装**

`tcyb/src/settings.rs` に追加（`scaffold_config` の近く）:

```rust
use anyhow::Context;

pub fn load(
    config_file: &Path,
    cli_config: Option<&Path>,
    default_db_dir: &Path,
) -> anyhow::Result<Settings> {
    let mut builder = config::Config::builder()
        .set_default("listen_address", "localhost:8000")?
        .set_default("greeting_template", "user_name is now following!")?
        .set_default("db_dir", default_db_dir.to_string_lossy().into_owned())?
        .set_default("db_name", "data.json")?;
    builder = builder.add_source(config::File::from(config_file).required(false));
    builder = builder.add_source(
        config::Environment::with_prefix("cb")
            .try_parsing(true)
            .list_separator(",")
            .with_list_parse_key("operations"),
    );
    if let Some(path) = cli_config {
        let name = path.to_str().context("--config path is not valid UTF-8")?;
        builder = builder.add_source(config::File::with_name(name));
    }
    let cfg = builder.build()?;
    Ok(cfg.try_deserialize()?)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb settings::`
Expected: PASS（scaffold + load の計 4 tests）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/settings.rs
git commit -m "feat(tcyb): 優先順位付き設定ロード load と db_dir デフォルトを追加"
```

---

### Task 4: `main.rs` 配線（dotenvy 廃止・scaffold/load 接続）

**Files:**
- Modify: `tcyb/src/main.rs:36-58`（`main` 冒頭の dotenvy + インライン config ビルド部）
- Modify: `tcyb/Cargo.toml`（`dotenvy` 依存を削除）

**Interfaces:**
- Consumes: `paths::app_paths`, `settings::scaffold_config`, `settings::load`
- Produces: なし（バイナリ挙動）

- [ ] **Step 1: dotenvy 自動読み込みを削除し、パス解決＋scaffold/load へ置換**

`tcyb/src/main.rs` の `main` 冒頭を次のように変更する。

置換前:

```rust
    let _profile = profiling::init();
    dotenvy::dotenv().ok();
    let args = Cli::parse();

    let settings: Settings = {
        let _span = tracing::info_span!("config_build").entered();
        let mut config_builder = config::Config::builder()
            .set_default("listen_address", "localhost:8000")?
            .set_default("greeting_template", "user_name is now following!")?;
        config_builder = config_builder.add_source(
            config::Environment::with_prefix("cb")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("operations"),
        );
        if let Some(path) = args.config.as_ref() {
            config_builder =
                config_builder.add_source(config::File::with_name(path.to_str().unwrap()));
        }
        let config = config_builder.build()?;
        config.try_deserialize()?
    };
```

置換後:

```rust
    let _profile = profiling::init();
    let args = Cli::parse();
    let app_paths = paths::app_paths()?;

    let settings: Settings = {
        let _span = tracing::info_span!("config_build").entered();
        if !app_paths.config_file.exists() && args.config.is_none() {
            settings::scaffold_config(&app_paths.config_file)?;
            println!(
                "設定ファイルを作成しました: {}",
                app_paths.config_file.display()
            );
            println!("client_id / client_secret などを記入してから再実行してください。");
            return Ok(());
        }
        settings::load(&app_paths.config_file, args.config.as_deref(), &app_paths.db_dir)?
    };
```

- [ ] **Step 2: `dotenvy` 依存を削除**

Run: `cargo remove -p tcyb dotenvy`
Expected: `tcyb/Cargo.toml` から `dotenvy` 行が消え、`Cargo.lock` が更新される。

- [ ] **Step 3: ビルドと fmt/clippy を確認**

Run: `just check`
Expected: fmt-check / clippy / test / check-env-leak すべて成功（exit 0）。`dotenvy` 未使用エラーが残らないこと。

- [ ] **Step 4: 初回起動（scaffold）を手動検証**

Windows PowerShell 例（一時ディレクトリを使い実 AppData を汚さない）:

```powershell
$env:TCYB_CONFIG_DIR = "$env:TEMP\tcyb-verify"
Remove-Item -Recurse -Force $env:TCYB_CONFIG_DIR -ErrorAction SilentlyContinue
cargo run -p tcyb -- read-chat
```

Expected: 「設定ファイルを作成しました: …\tcyb-verify\config.toml」と編集案内が表示され、`$env:TEMP\tcyb-verify\config.toml` が生成されて正常終了する（token 接続まで進まない）。

- [ ] **Step 5: 2 回目起動（load）を手動検証**

生成された `config.toml` の `client_id` / `client_secret` にダミー値を入れて再実行し、scaffold メッセージが出ずに設定読み込みまで進むこと（その後の Twitch 認証エラーは想定内）を確認する。確認後 `$env:TCYB_CONFIG_DIR` を解除する:

```powershell
Remove-Item Env:\TCYB_CONFIG_DIR
```

- [ ] **Step 6: コミット**

```bash
git add tcyb/src/main.rs tcyb/Cargo.toml Cargo.lock
git commit -m "feat(tcyb): main を OS 標準パス+scaffold/load へ移行し dotenvy を廃止"
```

---

### Task 5: ドキュメント・移行手順・クリーンアップ

**Files:**
- Modify: `tcyb/README.md`
- Create: `tcyb/config.toml.example`
- Delete: `.env.example`
- Modify: `.gitignore`（`!.env.example` 行を削除）

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: 参照用サンプルを作成**

Create `tcyb/config.toml.example`（Task 2 のテンプレと同内容 + コメント）:

```toml
# tcyb 設定ファイルのサンプル。
# 実ファイルは OS 標準ユーザーディレクトリ（Windows は %APPDATA%\tcyb\config\config.toml）に
# 初回起動時へ自動生成される。TCYB_CONFIG_DIR を設定するとその配下に変更できる。
client_id = ""
client_secret = ""
channel = "your_channel_name"
username = "your_username"
speech_address = "http://localhost:8080"
operations = ["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05", "o:/play?v=18"]
greeting_template = "user_name さん。フォローありがとうございます。"
translate_command = "translate"
# listen_address = "localhost:8000"
# db_dir / db_name は OS 標準データディレクトリを既定使用
```

- [ ] **Step 2: README を更新**

`tcyb/README.md` の「使い方」節を書き換える。要点:
- `.env` ではなく OS 標準ユーザーディレクトリの `config.toml`（Windows は `%APPDATA%\tcyb\config\config.toml`）を使うこと。
- 初回起動でテンプレが自動生成され、パスが表示されること。`tcyb/config.toml.example` が参照サンプル。
- どの CWD からでも起動できること。
- 移行手順: 旧 `.env`（`cb_` プレフィックス）の値を新 `config.toml`（プレフィックス無しキー・`operations` は配列）へ転記。旧 `db/data.json` は OS データディレクトリ（`%APPDATA%\tcyb\data`）へ移すか、`cargo run -p tcyb -- auth-code` で再認証する。
- ログレベルは従来 `.env` の `RUST_LOG` に依存していたが、今後はシェルの環境変数 `RUST_LOG` で指定する（例: PowerShell `$env:RUST_LOG = "INFO"`）。
- `--config <path>` で明示ファイルを指定でき、`cb_*` 環境変数で個別上書きできること。

- [ ] **Step 3: 旧 `.env.example` を削除し gitignore を調整**

```bash
git rm .env.example
```

`.gitignore` から `!.env.example` の行を削除する（`.env.*` の無視はそのまま残す）。

- [ ] **Step 4: フルゲートを実行**

Run: `just ci`
Expected: fmt-check + clippy + test + check-env-leak + gitleaks + deny + audit すべて成功（exit 0）。赤が出たら CLAUDE.md の対応方針に従う（fmt は `just fmt`、それ以外は原因調査）。

- [ ] **Step 5: コミット**

```bash
git add tcyb/README.md tcyb/config.toml.example .gitignore
git commit -m "docs(tcyb): 設定を config.toml/OS 標準パスへ移行し README と移行手順を更新"
```

---

## Self-Review

**1. Spec coverage（受入基準 → タスク対応）:**
- 任意 CWD で同一動作 → Task 1（`app_paths` は CWD 非依存）+ Task 4（dotenvy 廃止で CWD `.env` 依存消滅）。
- 作業ツリー外の OS 標準ディレクトリに保存 → Task 1（config/data パス）+ Task 3（db_dir デフォルト）。
- 初回テンプレ自動生成 + パス表示 + 終了 → Task 2（scaffold）+ Task 4（存在チェック→scaffold→`return`）。
- env / `--config` オーバーライド維持 → Task 3（`load` の優先順位）。
- 移行手順の文書化 → Task 5（README）。
- `just ci` 緑 → Task 5 Step 4。

**2. Placeholder scan:** TBD/TODO・曖昧指示なし。各コード手順に実コードあり。README のみ箇条書き指示だが、内容は具体（転記元/先・キー変換・RUST_LOG）を明示済み。

**3. Type consistency:** `AppPaths { config_file, db_dir }` は Task 1 定義、Task 4 で `app_paths.config_file` / `app_paths.db_dir` として一致利用。`load(config_file, cli_config, default_db_dir)` の引数順は Task 3 定義と Task 4 呼び出し（`&app_paths.config_file, args.config.as_deref(), &app_paths.db_dir`）で一致。`scaffold_config(&Path)` も Task 2/4 で一致。
