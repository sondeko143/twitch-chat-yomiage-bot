# tcyb read-chat の視聴者一覧ログ Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development（推奨）
> または superpowers:executing-plans。実行の規律は implementer-led-execution に従う。

ADR: [0028](../../adr/0028-read-chat-appends-chatters-log-to-configured-file.md)（Proposed / 既定）— 視聴者一覧の記録は `read-chat` が設定のパスへ `show-chatters` 書式で直接追記する。Task 3 で実装と突き合わせ、裏づけが取れたら `Accepted` に昇格させる。

Spec: [2026-10-07-tcyb-read-chat-chatters-log-design.md](../specs/2026-10-07-tcyb-read-chat-chatters-log-design.md)

**Goal:** `monitor.enabled = true` の `read-chat` が、視聴者一覧の周期取得に成功するたびに、設定で指定したファイルへ `show-chatters` と同一の 1 行 CSV を追記する。

**Architecture:** `read-chat` 内で既に動いている視聴者一覧の周期取得ループ（ハブへ送っている箇所）に、成功時の追記先を 1 つ足す。追記先は起動時に設定から作り、開けなければ起動を中止する。行の書式は `show-chatters` と共通にし、二重保守を避ける。

**Tech Stack:** Rust 2021 / tokio / chrono / config 0.13（既存のまま。新規依存なし）。

## Global Constraints

- 対象クレートは `tcyb` のみ。他クレートは変更しない。新規依存は追加しない。
- 設定キー（逐語）: `[monitor]` テーブルの `chatters_log`（任意・文字列・ファイルパス）。省略時は記録しない。既存キー `enabled` / `port` / `history_size` / `chatters_interval_secs` の名前と既定値は変えない。
- `chatters_log` の相対パスは、`read-chat` プロセスのカレントディレクトリ基準で解決する（独自のベースディレクトリを導入しない）。
- 行の書式（逐語）: `YYYY-MM-DD HH:MM:SS` のローカル時刻、カンマ、設定の `channel` と `username` を除いた login 名を昇順にカンマ区切り。除外後 0 人でも行末のカンマは省略しない（例: `2026-08-09 00:00:00,`）。改行は `\n`。これは現行 `show-chatters` の出力と同一で、`show-chatters` 側の書式・挙動は変えない。
- 行の追記は取得成功時のみ。取得失敗の周期では何も書かない。
- 既存ファイルがあれば末尾に追記、無ければ作成する。既存の内容を切り詰めない。1 行ごとに flush する。
- `chatters_log` が設定されているのに `monitor.enabled = false` のとき、`read-chat` は矛盾を示すエラーで起動を中止する。この検査は `read-chat` の起動時に限り、共有の設定読み込みには入れない（`auth-code` 等の他サブコマンドは同じ設定で従来どおり動く）。
- 起動時に `chatters_log` を追記モードで開けないとき、`read-chat` は原因（OS のエラー）とパスを含むエラーで起動を中止する。
- 稼働中の書き込み失敗では `read-chat` を終了させず、読み上げ・配信・次周期の取得と記録を続ける。失敗は理由とともに `warn` ログに出す。
- 配信口のプロトコル（メッセージ型・`v`）と TUI の表示は変えない。
- clippy の閾値（`clippy.toml`）は緩めない。
- 各 task の終わりは `just check` が緑の状態でコミットする。PR 前に `just ci` を全緑にする。
- コミットメッセージは既存履歴に合わせ、`feat(tcyb): ...` / `docs(...): ...` の形で本文は日本語にする。

## Implementer Authority

この plan が拘束するのは 3 つだけ: **公開契約**（各 task の「契約」欄の名前・型・方向）、
**Global Constraints の逐語値**、**各 task の受入基準**。

それ以外 — 内部設計、関数・ファイルの分割、命名、アルゴリズム、テストの設計と粒度、
エラー処理の形、依存の使い方 — はすべて実装者が決める。plan に書かれていない実装を
選んだことは逸脱ではない。

この plan が参照する ADR のうち、実装者を拘束するのは **効力: 制約** のものだけ。**既定** と
**便宜** は「なぜ今こうなっているか」の記録であって、守る義務はない。より良い方法があれば
変えてよい。変えたら report の逸脱1行に書く。

plan の記述より良い方法を見つけたら、良い方を採る。plan は使い捨てなので書き換えない。
その選択が adr-writing の基準に当たるならトリガ2 で ADR を起票する。

停止して人間に確認するのは、上の 3 つのいずれかを**変える必要がある**と判断したときだけ。

---

### Task 1: 設定キー `chatters_log` と雛形

**目的:** `[monitor] chatters_log` を読めるようにし、初回起動で生成される雛形に載せる。

**範囲:** `tcyb/src/settings.rs`（`MonitorSettings`・`CONFIG_TEMPLATE`・テスト）。

**契約**
- Consumes: なし。
- Produces: `settings::MonitorSettings::chatters_log: Option<std::path::PathBuf>`。TOML キー `chatters_log`。省略時 `None`。

**受入基準**
- [ ] `[monitor]` に `chatters_log = "chatters.csv"` と書いた設定を読むと `chatters_log` が `Some("chatters.csv")` になる。
- [ ] `chatters_log` を省略した設定（`[monitor]` 自体の省略を含む）では `None` になり、他のキーの既定値は従来と同じ。
- [ ] 雛形（`CONFIG_TEMPLATE`）の `[monitor]` 節に `chatters_log` がコメントアウト行として、用途が分かる 1 行コメント付きで載っている。
- [ ] 雛形はこれまでどおり実行時ローダで読めて、`chatters_log` は `None` になる。

**検証**
`cargo test -p tcyb settings` が PASS。`just check` が exit 0。

**コミット単位:** 設定キーの追加と雛形更新で 1 コミット。

---

### Task 2: 周期取得の成功時に追記する

**目的:** `read-chat` の視聴者一覧の周期取得ループに追記先を足し、起動時の検査と稼働中の失敗の扱いを実装する。

**範囲:** `tcyb/src/chat.rs`（周期取得ループと既存の行書式関数 `format_chatters_line` の周辺）、`tcyb/src/yomiage.rs`（`read-chat` の起動と `Monitor::start` の周辺）。`show-chatters` の経路（`chatters` / `chatters_tick`）の出力は変えない。

**契約**
- Consumes: `settings::MonitorSettings::chatters_log: Option<PathBuf>`（Task 1）。
- Produces: なし（後続 task はドキュメントのみ）。

**受入基準**
- [ ] `chatters_log` 設定時、周期取得が成功するたびにそのファイルへ 1 行追記される。行は Global Constraints の書式どおりで、同じ視聴者一覧を `show-chatters` が出した行とタイムスタンプを除いて一致する。
- [ ] 周期取得が失敗した周期では行が増えない。配信口へ流れる視聴者一覧・接続状態はこれまでと同じ。
- [ ] `chatters_log` 未設定なら、周期取得の挙動・配信・ログ出力は従来と変わらない。
- [ ] 既存ファイルの末尾に追記され、既存行は失われない。ファイルが無ければ作られる。追記のたびに flush され、プロセスを kill してもその行までは残る。
- [ ] 相対パスは `read-chat` を起動したカレントディレクトリに対して解決され、絶対パスはそのまま使われる。
- [ ] `chatters_log` が設定されていて `monitor.enabled = false` のとき、`read-chat` は矛盾を示すエラーで起動を中止する（両キーの名前がエラー文に含まれる）。`auth-code` など他のサブコマンドは同じ設定で従来どおり動く。
- [ ] `chatters_log` の親ディレクトリが存在しない等で開けないとき、`read-chat` は OS のエラーとパスを含むエラーで起動を中止する。
- [ ] 稼働中の追記に失敗しても周期取得ループは止まらず、失敗は理由付きの `warn` ログに出て、次の周期では再び追記を試みる（追記先が復旧すれば書ける）。
- [ ] 以上のうち書式・成功時のみ追記・失敗継続・起動時検査が、ネットワークに依存しないテストで検証されている。

**検証**
`cargo test -p tcyb` が PASS。`just check` が exit 0。手元確認: `monitor.enabled = true` と `chatters_log` を設定して `cargo run -p tcyb -- read-chat` を起動し、`chatters_interval_secs` 経過後に指定ファイルへ行が増え、`show-chatters` の出力と同じ形であること。

**コミット単位:** 追記処理と起動時検査（テスト含む）で 1 コミット。

---

### Task 3: README と ADR の突合

**目的:** 利用者向けの記述を足し、ADR-0028 を実装と突き合わせる。

**範囲:** `tcyb/README.md`（`[monitor]` の設定例と「視聴者一覧の記録」節）、`docs/adr/0028-*.md` と `docs/adr/README.md`（Status 行と索引の Status 列のみ）。

**契約**
- Consumes: Task 1 のキー名、Task 2 の挙動。
- Produces: なし。

**受入基準**
- [ ] README の `[monitor]` 設定例に `chatters_log` が既定（未設定＝記録しない）の説明付きで載っている。
- [ ] README に、記録の書式が `show-chatters` と同一で既存の `chatters.csv` へ続けて追記できること、`monitor.enabled = true` なら `show-chatters --interval` の並走が不要になること、`enabled = false` との組み合わせと開けないパスは起動時エラーになること、稼働中の書き込み失敗では `read-chat` が止まらず警告ログに出ることが書かれている。
- [ ] ADR-0028 の Decision と実装の間に乖離が無いことを確認し、Status を `Accepted` に変える（Status 行と索引の列だけ。本文は触らない）。乖離があれば Status は変えず、adr-writing の「覆すとき」に従う。
- [ ] `just ci` が exit 0。

**検証**
`just ci` が exit 0。`just check-env-leak` が OK（README・plan に個人/マシン依存の絶対パスが無い）。

**コミット単位:** README で 1 コミット、ADR の Status 昇格で 1 コミット。
