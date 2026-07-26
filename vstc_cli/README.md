# vstc cli

<https://github.com/sondeko143/vstreamer-tool> の cli クライアント

## 使い方

```sh
cargo run -p vstc_cli -- --help
vstreamer client cli

Usage: vstc_cli.exe <COMMAND>

Commands:
  send     操作チェーンを送信する ex: `send 'o:/transl?t=ja' 'o:/tts' -t "hello"`
  pause    再生を一時停止する
  resume   再生を再開する
  reload   設定ファイルをリロードする
  profile  プロファイルを管理する
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
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

`profile list` は各プロファイルの保存済みチェーン本数を `CHAINS` 列に表示する（未保存は `-`）。

### 既定チェーン

`send` に操作を書かなかったときに送るチェーンを、プロファイルに保存できる。チェーンは複数本持てて、1 回の送信でまとめて送られる。

```sh
# 追加（1 コマンド = 1 本のチェーン）
./vstc_cli.exe profile chains add main '//localhost:8081/transc' '//windesk:8080/sub'
./vstc_cli.exe profile chains add main '//localhost:8081/transc' 'transl?t=en' '//windesk:8080/sub?p=s'
./vstc_cli.exe profile chains add main '//localhost:8081/transc' 'transl?t=ru' '//windesk:8082/sub'

# 確認
./vstc_cli.exe profile chains show main
[1] //localhost:8081/transc -> //windesk:8080/sub
[2] //localhost:8081/transc -> transl?t=en -> //windesk:8080/sub?p=s
[3] //localhost:8081/transc -> transl?t=ru -> //windesk:8082/sub

# 削除（show が表示する番号を指定する）
./vstc_cli.exe profile chains del main 2
```

保存したら、操作を書かずに `send` するだけで全チェーンが送られる。

```sh
./vstc_cli.exe send --profile main -t "hello"
```

**操作を書いた `send` は保存済みチェーンを送らない。** 位置引数を渡した実行ではそれだけが 1 本のチェーンとして送られ、保存済みチェーンは無視される。どちらも無い場合はエラーになり、送信は行われない。

複数チェーンは 1 つの gRPC コマンドとしてまとめて送られるので、入力（テキスト / 音声 / file_path / filters）と trace_id は全チェーンで共有される。

`profile chains add` は保存する前に全 route を検証する。解釈できない route が 1 つでもあれば、何も保存されない。未登録のプロファイル名を渡した場合もエラーになり、プロファイルは作られない（作成は `profile set` の役目）。

#### route の書き方

route は 3 つの形で書ける。

| 形 | 意味 | 例 |
|---|---|---|
| `//<HOST>:<PORT>/<OP>?<QUERY>` | 宛先つきの 1 ホップ | `//windesk:8080/sub?p=s` |
| `<OP>?<QUERY>` | 宛先を指定しない 1 ホップ | `transl?t=en` |
| `o:/<OP>` / `o://<HOST>:<PORT>/<OP>` | 従来形式（引き続き有効） | `o:/tts?spd=1.1` |

`<OP>` に書けるのは `transc`（`transcribe`）/ `transl`（`translate`）/ `tts` / `play`（`playback`）/ `sub`（`subtitle`）/ `vc` / `reload` / `pause` / `resume` / `forward`（`fwd`）。

この 3 形式は `profile chains add` と `send` の位置引数の双方で同じように解釈される。

#### 保存された TOML

```toml
[profiles.main]
host = "localhost"
port = 19829
chains = [
    ["//localhost:8081/transc", "//windesk:8080/sub"],
    ["//localhost:8081/transc", "transl?t=en", "//windesk:8080/sub?p=s"],
    ["//localhost:8081/transc", "transl?t=ru", "//windesk:8082/sub"],
]
```

`profile chains add` は `chains` を 1 行にまとめて書き出すが、上のように手で改行しても同じものとして読める。

`profile set` はチェーンに触れないので、host / port を更新しても保存済みチェーンは消えない。

### 保存場所

プロファイルは OS 標準のユーザー設定ディレクトリ配下の `profiles.toml` 1 ファイルに保存される。Windows では既定で `%APPDATA%\vstc\config\profiles.toml`。正確なパスは `profile list` が末尾に表示する。

環境変数 `VSTC_CONFIG_DIR` を設定すると、そのディレクトリ配下（`<VSTC_CONFIG_DIR>\profiles.toml`）が使われる。

```toml
[profiles.main]
host = "localhost"
port = 19829
config_path = "path/to/config.yml"

[profiles.sub]
host = "other-host"
port = 8080
```

既定チェーンを保存すると `chains` が加わる（[既定チェーン](#既定チェーン)を参照）。
