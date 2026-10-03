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
translate_command = "translate" # 翻訳に使用する外部コマンド (第一引数に原文を渡し、標準出力を翻訳結果とする)
# listen_address = "localhost:8000"    # 既定値あり。変更時のみ記入
# db_dir / db_name は OS 標準データディレクトリを既定使用（変更時のみ記入）

[[notification_speech]]
type = "channel.follow"
template = "{user_name} さん。フォローありがとうございます。"
```

通知の読み上げ（`[[notification_speech]]`）と `[monitor]` は後述。

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
   | `cb_greeting_template` | 廃止。`[[notification_speech]]` に書き換える（下記「通知の読み上げ」参照） |
   | `cb_translate_command` | `translate_command` |
   | `cb_db_dir` / `cb_db_name` | `db_dir` / `db_name`（省略可。`db_dir` の既定は OS 標準データディレクトリ、`db_name` の既定は `data.json`） |
   | `RUST_LOG`（`.env` 経由） | シェル環境変数の `RUST_LOG`（上記参照） |

2. トークン DB を引き継ぐ場合は、旧 `db/data.json` を OS 標準データディレクトリ（Windows は `%APPDATA%\tcyb\data\data.json`）へ移動する。移動しない場合は `cargo run -p tcyb -- auth-code` を実行して新しい保存先で再認証する。

### Access Token を取る方法

```sh
cargo run -p tcyb -- auth-code
```

実行するとブラウザが自動で開き、Twitch の認可画面へリダイレクトされる（`force_verify` 済みのためアカウント選択を求められる）。あとは画面の指示に従う。

> **重要:** 認可は **対象チャンネルのモデレーター権限を持つ bot アカウント（設定の `username`）でブラウザにログインした状態**で行うこと。別のアカウントで認可すると、トークン自体は有効でも `moderator:read:chatters` / `moderator:read:followers` / `user:read:chat` を要する操作（`show-chatters` の Get Chatters、`read-chat` の follow・chat.notification 購読など）が 401 / 403 になる。その場合はブラウザで bot アカウントにログインし直してから `auth-code` をやり直す。

> **移行時の注意:** `channel.chat.notification` の購読のために `auth-code` のスコープへ `user:read:chat` が加わった。以前に認可済みのトークンにはこのスコープが無いので、更新後に **`auth-code` で再認可を 1 回行う**こと（`refresh-token` ではスコープは増えない）。

### 起動

```sh
cargo run -p tcyb -- read-chat
```

起動時に、EventSub の購読に使う配信チャンネル（設定の `channel`）の ID を Helix の Get Users で引き、トークンストアに保存する。2 回目以降は `channel` が保存済みの login と一致すれば（大文字小文字は区別しない）保存済みの ID を使い、API を呼ばない。`channel` を変えると引き直して上書きする。引くときの一時的な失敗（通信エラー・5xx）は間隔を空けて 3 回まで試し、それでも失敗するか、4xx・チャンネル未検出のときは起動を中止する（[ADR-0027](../docs/adr/0027-cache-channel-id-in-store-and-extend-status-additively.md)）。`show-chatters` も同じ保存済みの ID を使う。

### 通知の読み上げ

`read-chat` は EventSub で次の 3 つを購読する（配信者は設定の `channel`、bot は `username` のアカウント）。

| type | 内容 |
| --- | --- |
| `channel.follow` | フォロー |
| `channel.raid` | レイド（`channel` が受ける側） |
| `channel.chat.notification` | サブスク・リサブ・ギフト・レイド・アナウンスなど。種類は event の `notice_type`（`sub` / `resub` / `sub_gift` / `community_sub_gift` / `raid` / `announcement` / `bits_badge_tier` など）で区別される |

読み上げる文面は `config.toml` の `[[notification_speech]]` に書く。要素ごとのキーは `type`（必須）、`notice_type`（任意）、`template`（必須）。

```toml
[[notification_speech]]
type = "channel.follow"
template = "{user_name} さん。フォローありがとうございます。"

[[notification_speech]]
type = "channel.raid"
template = "{from_broadcaster_user_name} さんが {viewers} 人でレイドに来てくれました。"

# notice_type を指定すると、その種類だけに使われる
[[notification_speech]]
type = "channel.chat.notification"
notice_type = "sub"
template = "{chatter_user_name} さん。サブスクありがとうございます。"

# notice_type を省略した要素は、同じ type で他に一致する要素が無いときに使われる
[[notification_speech]]
type = "channel.chat.notification"
template = "{system_message}"
```

- `type` が一致し `notice_type` も一致する要素が優先され、無ければ `notice_type` を省略した要素が使われる。どれにも一致しない通知は読み上げない。
- `{name}` は通知 event の同名フィールドに、`{a.b}` はネストしたフィールドに置き換わる。`{{` は `{`、`}}` は `}` をそのまま出力する。存在しないフィールドは空文字になり、警告ログが出る。文字列でない値（数値など）は JSON の表記のまま読み上げる。
- 読み上げの送り先は従来のフォロー読み上げと同じ（`speech_address` / `operations`）。
- `[[notification_speech]]` は `config.toml`（と `--config` のファイル）でのみ指定できる。`cb_` 環境変数では上書きできない。

> **注意（二重読み上げ）:** レイドは `channel.raid` と、`channel.chat.notification` の `notice_type = "raid"` の両方で届く。両方にテンプレートを書くと同じレイドを二重に読み上げるので、どちらか一方だけに書く。`notice_type` を省略した `channel.chat.notification` の要素もレイドに一致する点に注意。

> **移行:** `greeting_template`（`cb_greeting_template` を含む）は廃止された。設定に残っていると、共有の設定読み込みが失敗するため `read-chat` だけでなく **すべてのサブコマンド（`auth-code` を含む）** がエラーで起動を中止する。`auth-code` を実行する前にも、このキーを削除するか上の `channel.follow` の例のように `[[notification_speech]]` へ書き換えること。
>
> また、`greeting_template` を一度も設定していなかった場合は、従来は組み込みの既定のフォロー挨拶が読み上げられていた。この変更後は `type = "channel.follow"` の `[[notification_speech]]` を追加するまでフォローは無言になり、エラーも表示されない。

### 監視 TUI（`tcyb monitor`）

配信中にコメント・通知・視聴者一覧を一望するための閲覧専用 TUI。`read-chat` が自分の中でローカル配信口を開き、`tcyb monitor` がそこへ別プロセスとして接続する。

1. `config.toml` で配信口を有効にする（既定は無効）。

   ```toml
   [monitor]
   enabled = true               # 既定 false
   port = 8765                  # 既定 8765。127.0.0.1 でのみ待ち受ける
   history_size = 500           # 既定 500。コメントと通知それぞれの保持件数
   chatters_interval_secs = 60  # 既定 60。視聴者一覧を取得する間隔（秒）
   ```

2. `read-chat` を起動したまま、別の端末で `tcyb monitor` を起動する。`port` / `history_size` は同じ設定ファイルから読む（`--config` も使える）。

   ```sh
   cargo run -p tcyb -- monitor
   ```

画面は左にコメント（上）と通知（下）、右に視聴者、最下部に read-chat・IRC・EventSub・視聴者一覧の状態行が出る。IRC はログイン完了の応答を受けてから、EventSub は購読処理を終えてから「接続」になる（それまでは「接続待ち」）。EventSub の購読に失敗した種別があれば、EventSub の状態の後ろに `（channel.chat.notification 購読失敗）` のように出る（新しいセッションで消える）。接続直後に `read-chat` が保持している直近の履歴が表示され、以降はリアルタイムで追加される。視聴者欄では、接続後に新しく現れた視聴者に印が付く。

| キー | 動作 |
| --- | --- |
| `q`（`Ctrl+C` も可） | 終了 |
| `Tab` | フォーカスするペイン（コメント / 通知 / 視聴者）を切り替える |
| `↑` / `↓` | フォーカス中のペインを 1 行スクロール |
| `PageUp` / `PageDown` | フォーカス中のペインを 1 画面スクロール |
| `End` | 最新への追従に戻る |

- **TUI を閉じても、異常終了しても、`read-chat` の読み上げは止まらない。** TUI は閲覧専用で、`read-chat` は配信口の購読者の有無や遅さに左右されない。視聴者一覧の取得や配信口の起動に失敗しても読み上げは続き、失敗は状態行に出る。
- `read-chat` が未起動・再起動中でも `monitor` は終了せず、0.5 秒から最大 10 秒まで間隔を延ばしながら再接続を試みる。再接続のたびに履歴が送り直される。
- 履歴は `read-chat` のメモリ上だけにあり、`history_size` を超えた古い分と `read-chat` 再起動前の分は残らない。
- 配信口に認証は無く、`127.0.0.1` 以外には公開しない。
- `monitor` 実行中は端末を TUI が占有するため、ログ出力（`RUST_LOG`）は行われない。
- 配信口は `ws://127.0.0.1:<port>/feed`（JSON、プロトコル版 `1`）。プロトコル版が違うと `monitor` は非互換と表示する（[ADR-0024](../docs/adr/0024-websocket-json-for-monitor-feed.md)）。

### 視聴者一覧の記録

`show-chatters` は、現在チャンネルに滞在している視聴者の login 名を 1 行の CSV で標準出力に書く。行頭はローカル時刻のタイムスタンプで、設定の `channel` と `username`（bot 自身）は除外し、残りを昇順に並べる。

```sh
cargo run -p tcyb -- show-chatters
# => 2026-08-09 12:34:56,viewer_a,viewer_b
```

`channel` と `username` を除いた結果が 0 人になっても行末のカンマは省略しない（例: `2026-08-09 00:00:00,`）ため、この行は常にタイムスタンプとカンマ以降の 2 フィールドとして読める。

`--interval <SECS>` を付けると、起動直後に 1 行出したあと SECS 秒ごとに取得を繰り返す常駐モードになる。SECS は 1 以上の整数（秒）で、0 を渡すとエラーになる。

```powershell
cargo run -p tcyb -- show-chatters --interval 60 >> chatters.csv
```

停止は Ctrl+C。1 行ごとに出力が flush されるため、途中で停止してもそれまでの行はリダイレクト先に残る。

常駐は前面で動くプロセスであって、自身をバックグラウンドへ回したりサービス登録したりはしない。ログオン時の自動起動やバックグラウンド実行が必要なら、Windows のタスクスケジューラなど OS 側の仕組みから起動する。

トークンが失効した場合（401）は自動でリフレッシュして同じ取得をやり直し、常駐は継続する。ただしリフレッシュ直後にも 401 が返った場合は再リフレッシュせずそのままプロセスを終了する（トークンの新鮮さが原因でない 401 とみなす）。401 以外のエラー（回線断・5xx・スコープ不足の 403 など）でも同様にプロセスがそのまま終了する。**自動では再起動しない**ので、長時間の無人記録では起動元の側で再実行を設定する。

例外として、**応答が返らないまま接続が切れた場合だけは同じ tick 内で 1 回だけ取得をやり直す**。tick の間アイドルだった keep-alive 接続をサーバ側が先に捨てていると、次の取得がその接続を掴んで `os error 10054`（既存の接続はリモート ホストに強制的に切断されました）などで落ちるため。張り直しは副作用の無い GET に限り、2 回目も失敗すればこれまでどおりプロセスを終了する（[ADR-0022](../docs/adr/0022-retry-idempotent-gets-once-when-no-response-arrives.md)）。
