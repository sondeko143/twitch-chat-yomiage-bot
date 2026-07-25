# vstc cli

<https://github.com/sondeko143/vstreamer-tool> の cli クライアント

## 使い方

```sh
cargo run -p vstc_cli -- --help
Usage: vstc_cli.exe <COMMAND>

Commands:
  send     操作チェーンを送信する
  pause    再生を一時停止する
  resume   再生を再開する
  reload   設定ファイルをリロードする
  profile  プロファイルを管理する
```

`send` / `pause` / `resume` / `reload` は共通で `--profile <NAME>` / `-H, --host <HOST>` / `-p, --port <PORT>` を取る。

```sh
# 例) 翻訳して読み上げ
./vstc_cli.exe send 'o:/transl?t=ja' 'o:/tts?i=1&spd=1.1&pit=-0.05' 'o:/play?v=20' -p 19829 -t "hello, world"

# 例) 一時停止 / 再開
./vstc_cli.exe pause -p 19829
./vstc_cli.exe resume -p 19829
```

## プロファイル

宛先（host / port）と `reload` が読む設定ファイルのパスを、名前付きで保存できる。

```sh
# 保存（渡したフィールドだけを更新する。他は既存値のまま）
./vstc_cli.exe profile set main --host localhost --port 19829 --config-path path/to/config.yml
./vstc_cli.exe profile set main --port 20000

# 一覧
./vstc_cli.exe profile list

# 削除
./vstc_cli.exe profile remove main
```

保存したら `--profile` で呼び出す。

```sh
./vstc_cli.exe pause --profile main
./vstc_cli.exe reload --profile main

# 明示フラグはプロファイルより優先される（この 1 回だけ別ポートへ）
./vstc_cli.exe pause --profile main --port 8080
```

値は「組み込み既定 → プロファイル → 明示フラグ」の順で後勝ちに解決される。既定は `localhost:8080`。
`--profile` を指定しない実行はプロファイルファイルを読まないので、保存が一度も無くても全コマンドが動く。

`reload` の設定パスは `--config-path` がプロファイルの `config_path` より優先される。どちらからも解決できない場合は送信せずエラーになる。

### 保存場所

プロファイルは OS 標準のユーザー設定ディレクトリ配下の `vstc/profiles.toml` 1 ファイルに保存される（Windows は Roaming AppData 配下）。正確なパスは `profile list` が末尾に表示する。

環境変数 `VSTC_CONFIG_DIR` を設定すると、そのディレクトリ配下の `profiles.toml` が使われる。

```toml
[profiles.main]
host = "localhost"
port = 19829
config_path = "path/to/config.yml"

[profiles.sub]
host = "other-host"
port = 8080
```
