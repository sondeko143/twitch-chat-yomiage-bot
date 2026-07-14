# Twitch 読み上げボット

<https://github.com/sondeko143/vstreamer-tool> に twitch chat の読み上げさせるために作ったもの  

## 使い方

設定は作業ディレクトリの `.env` ではなく、OS 標準のユーザー設定ディレクトリ配下の `config.toml` に置く。Windows では既定で `%APPDATA%\tcyb\config\config.toml`（トークン DB は `%APPDATA%\tcyb\data`）。`TCYB_CONFIG_DIR` 環境変数を設定すると、そのディレクトリ配下（`<TCYB_CONFIG_DIR>\config.toml` / `<TCYB_CONFIG_DIR>\db`）に変更できる。

どの CWD から起動しても同じ設定ファイルを参照するため、bot はリポジトリ外・任意の作業ディレクトリから実行できる。

### 初回起動

`config.toml` が存在しない状態で実行すると、テンプレートを自動生成してその絶対パスを表示し、値の記入を促して終了する（読み上げ等は開始しない）。

```sh
cargo run -p tcyb -- read-chat
# => 設定ファイルを作成しました: %APPDATA%\tcyb\config\config.toml
# => client_id / client_secret などを記入してから再実行してください。
```

生成される内容・キーの参照サンプルは [`config.toml.example`](./config.toml.example) を参照。テンプレートに `client_id` / `client_secret` / `channel` / `username` などを記入してから再度実行する。

```toml
client_id = ""
client_secret = ""
channel = "your_channel_name"
username = "your_username"
speech_address = "http://localhost:8080" # <https://github.com/sondeko143/vstreamer-tool> の待受アドレス
operations = ["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05", "o:/play?v=18"]
greeting_template = "user_name さん。フォローありがとうございます。" # フォロー通知の読み上げメッセージ
translate_command = "translate" # 翻訳に使用する外部コマンド (第一引数に原文を渡し、標準出力を翻訳結果とする)
# listen_address = "localhost:8000"    # 既定値あり。変更時のみ記入
# db_dir / db_name は OS 標準データディレクトリを既定使用（変更時のみ記入）
```

### 設定ファイルの明示指定・個別上書き

- `--config <path>` を渡すと、その TOML ファイルを追加で読み込む（優先順位: 既定値 < OS 標準 `config.toml` < `cb_` プレフィックス環境変数 < `--config` で指定したファイル）。
- 個々の値はシェルの `cb_` プレフィックス環境変数でも上書きできる（例: `cb_client_id`, `cb_operations`。`operations` はカンマ区切り文字列として渡す）。`config.toml` 内のキー自体は `cb_` プレフィックス無し。
- ログレベルは `.env` の `RUST_LOG` ではなく、シェルの環境変数で指定する。

  ```powershell
  $env:RUST_LOG = "INFO"
  cargo run -p tcyb -- read-chat
  ```

### 旧 `.env` からの移行

旧バージョンは作業ディレクトリの `.env`（`cb_` プレフィックス付きキー）を読んでいたが、現在は読まない。以下の手順で移行する。

1. 旧 `.env` の値を `config.toml` へキー名を変えて転記する（`cb_` プレフィックスを外す）。

   | 旧 `.env`（`cb_` プレフィックス） | 新 `config.toml`（プレフィックス無し） |
   | --- | --- |
   | `cb_client_id` | `client_id` |
   | `cb_client_secret` | `client_secret` |
   | `cb_channel` | `channel` |
   | `cb_username` | `username` |
   | `cb_speech_address` | `speech_address` |
   | `cb_operations`（カンマ区切り文字列） | `operations`（TOML 配列。例: `["o:/transl?t=ja", "o:/tts?i=1&spd=1.1&pit=-0.05"]`） |
   | `cb_greeting_template` | `greeting_template` |
   | `cb_translate_command` | `translate_command` |
   | `cb_db_dir` / `cb_db_name` | `db_dir` / `db_name`（省略可。既定は OS 標準データディレクトリ） |
   | `RUST_LOG`（`.env` 経由） | シェル環境変数の `RUST_LOG`（上記参照） |

2. トークン DB を引き継ぐ場合は、旧 `db/data.json` を OS 標準データディレクトリ（Windows は `%APPDATA%\tcyb\data\data.json`）へ移動する。移動しない場合は `cargo run -p tcyb -- auth-code` を実行して新しい保存先で再認証する。

### Access Token を取る方法

```sh
cargo run -p tcyb -- auth-code

## ブラウザで http://localhost:8000/auth にアクセスして後は画面の指示通り
```

### 起動

```sh
cargo run -p tcyb -- read-chat
```
