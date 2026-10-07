# 0028. 視聴者一覧の記録は read-chat が設定のパスへ show-chatters 書式で直接追記する

- Status: Proposed
- 効力: 既定 — 却下理由はいずれも技術的な比較（記録の連続性・重複・起動方法への依存）で、時間都合ではない
- Date: 2026-10-07
- Related: [read-chat 視聴者ログ spec](../superpowers/specs/2026-10-07-tcyb-read-chat-chatters-log-design.md), [ADR-0019](0019-foreground-only-daemon-for-show-chatters.md), [ADR-0020](0020-interval-option-on-show-chatters.md), [ADR-0023](0023-separate-monitor-tui-via-local-feed-from-read-chat.md)

## Context

配信中の視聴者の推移をファイルに残す手段は `show-chatters --interval N >> chatters.csv` だけで、ADR-0019 は「出力は stdout の 1 行 CSV に限り、ファイルへの保存はシェルのリダイレクトに委ねる」と決め、`--output` を却下した。bot 側にファイルハンドル・追記・flush・ローテーションの責任を持たせないためである。

その後 ADR-0023 で、`read-chat` が `monitor.enabled = true` のとき視聴者一覧を周期取得して配信口へ流すようになった。記録を残したい利用者は、同じ Get Chatters を叩く `show-chatters` を並走させることになり、API 呼び出しが二重になるうえ、同じトークンストアを 2 プロセスがリフレッシュしうる（ADR-0023 が TUI の自前接続を却下した理由と同じ競合）。

`read-chat` は常駐が本務のプロセスで、stderr にログを出す。stdout は使っていない。TUI（`tcyb monitor`）は閲覧専用の別プロセスで、再接続のたびにスナップショットを受け取り直す。

## Decision

- 視聴者一覧の記録は **`read-chat` が行う**。周期取得が成功するたびに、設定で指定したファイルへ 1 行追記する。
- 出力先は `[monitor]` の設定キーにパスで書く。省略時は記録しない。相対パスは `read-chat` のカレントディレクトリ基準。
- 行の書式は `show-chatters` と同一にし、既存の `chatters.csv` にそのまま続けて追記できることを保つ。
- 出力先が設定されているのに `monitor.enabled = false`、または起動時に出力先を追記モードで開けない場合は、設定不備として起動を中止する。稼働中の書き込み失敗は警告ログに出して `read-chat` を止めない。
- ADR-0019 は `show-chatters` についての決定であり、覆さない。`show-chatters` は従来どおり stdout への出力のみを続ける。ファイル書き込みの責任を bot 側が負うのは `read-chat` の記録に限る。

## Alternatives rejected

- **`tcyb monitor`（TUI）が受信した視聴者一覧を書く** — TUI を開いている間しか記録されず、「配信中の推移を残す」目的に対して穴が開く。再接続のたびにスナップショットで同じ一覧が届くため、重複行を避ける判定（最終取得時刻の比較）を TUI が持つことになる。TUI は閲覧専用（ADR-0023）という位置づけにも反する。
- **`read-chat` の stdout へ 1 行ずつ出し、シェルの `>>` に委ねる（ADR-0019 の流儀）** — 記録の有無と出力先が起動方法（リダイレクトの有無）に依存し、設定ファイルからは分からない。`read-chat` は常駐の本務プロセスで、将来 stdout を別用途に使えなくなる。`show-chatters` のように「出力がその 1 行だけ」のコマンドではないので、ADR-0019 の前提が成り立たない。
- **TUI なしの記録モードを `tcyb monitor` に足し、配信口から受けて stdout へ出す** — 記録のためだけに常駐プロセスを 1 つ余計に起動・停止する運用は現状と変わらず、配信口の再接続・スナップショット重複の問題も TUI 案と同じ。
- **スイッチだけ置いてファイル名を `db_dir` 下に固定する** — トークン DB と同じディレクトリに記録が混じり、別の場所（配信ごとのフォルダ等）に置きたいときに動かせない。パスを 1 つ書く手間で済むなら、パスを書かせる方が単純。
- **`monitor.enabled = false` でも出力先があれば周期取得だけ動かす** — 設定の 1 キーが「配信口なしで取得だけ動く」という第 3 の動作モードを生む。取得は配信口の機能として `enabled` に束ねてあるので（ADR-0023）、矛盾は起動時エラーで知らせる方が分かりやすい。

## Consequences

- `monitor.enabled = true` の運用では `show-chatters --interval` の並走が不要になり、Get Chatters の呼び出しとトークンリフレッシュが 1 プロセスに集約される。
- `read-chat` がファイルの追記・flush の責任を持つ。ローテーションは持たない。長期運用でファイルが大きくなる場合は、利用者がファイルを動かす（次の周期で新しいファイルが作られる）か外側で切る。
- `show-chatters` の行の書式を変えるときは、この記録と共通の書式として両方に効く。
- 記録は取得の成功時だけなので、取得失敗が続いた期間はファイル上では空白になる。失敗の事実は配信口の接続状態と警告ログにのみ残る。
