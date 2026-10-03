# tcyb monitor（配信モニタ TUI）と通知読み上げ統一 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development（推奨）
> または superpowers:executing-plans。実行の規律は implementer-led-execution に従う。

ADR:
- [0023](../../adr/0023-separate-monitor-tui-via-local-feed-from-read-chat.md)（Proposed / 既定）— read-chat に配信口を設け、TUI を別プロセスにする
- [0024](../../adr/0024-websocket-json-for-monitor-feed.md)（Proposed / 既定）— 配信口は axum の WebSocket で JSON を送る
- [0025](../../adr/0025-unify-notification-speech-templates-by-type.md)（Proposed / 既定）— 通知読み上げを種別ごとのテンプレート配列にし、`greeting_template` を廃止する
- [0026](../../adr/0026-receive-sub-events-via-chat-notification.md)（Proposed / 制約）— サブスク等は `channel.chat.notification` で受け取る

いずれも Task 7 で実装と突き合わせ、裏づけが取れたものを `Accepted` に昇格させる。

Spec: [2026-10-03-tcyb-monitor-tui-design.md](../specs/2026-10-03-tcyb-monitor-tui-design.md)

**Goal:** `read-chat` がコメント・EventSub 通知・視聴者一覧をローカルの WebSocket で配信し、新サブコマンド `tcyb monitor` がそれを TUI で表示する。あわせて、通知の読み上げを種別ごとのテンプレート設定に統一する。

**Architecture:**
- `read-chat` の中にイベントハブを置き、IRC・EventSub・視聴者一覧の周期取得の各ループがここへ送る。ハブは broadcast と直近分のリングバッファを持つ。
- axum の WebSocket エンドポイントが、新しい接続にまずスナップショットを送り、以降は新着を 1 件ずつ送る。
- `tcyb monitor` は同じクレートに定義した型で受信し、ratatui で描画する。

**Tech Stack:** Rust 2021 / tokio / axum 0.8（`ws` feature）/ tokio-tungstenite / serde・serde_json / ratatui＋crossterm（新規）/ config 0.13。

## Global Constraints

- 対象クレートは `tcyb` のみ。`vstc` / `vstc_cli` / `vstc_gui` / `igdb` / `xtask` は変更しない。
- 新しく追加してよい依存は `ratatui` と `crossterm` と、既存依存の feature 追加（例: axum の `ws`）だけ。それ以外を足す場合は停止して人間に確認する。いずれも `cargo deny` を通ること。
- 設定キー（逐語）:
  - `[[notification_speech]]` の配列テーブル。各要素のキーは `type`（必須・文字列）、`notice_type`（任意・文字列）、`template`（必須・文字列）。
  - `[monitor]` テーブル。キーと既定値は次のとおり。
    - `enabled = false`
    - `port = 8765`
    - `history_size = 500`（コメントと通知それぞれの保持件数）
    - `chatters_interval_secs = 60`
- `greeting_template` は設定から削除する。値が残っていたら `read-chat` はエラーで起動を中止し、エラー文に `[[notification_speech]]` と `type = "channel.follow"` を使った書き換え例を含める。
- テンプレートの置換規則（逐語）:
  - `{name}` は event の同名フィールドに置き換える。
  - `{a.b}` はネストしたフィールドに置き換える。
  - `{{` は `{`、`}}` は `}` として出力する。
  - 存在しないフィールドは空文字にして、警告ログを出す。
  - 置き換える値が文字列でないとき（数値など）は、JSON の表記をそのまま文字列にして使う。
- 待受は `127.0.0.1:<monitor.port>` のみ。`0.0.0.0` や `localhost` の名前解決には bind しない。
- WebSocket のパスは `/feed`。プロトコル版は整数 `1` で、全メッセージに含める。
- EventSub の購読は次の 3 つで、`condition` の配信者 ID には設定の `channel` から解決した ID を使う。
  - `channel.follow` v2
  - `channel.raid` v1（`to_broadcaster_user_id` で購読する）
  - `channel.chat.notification` v1（`user_id` は bot の ID）
- `auth-code` が要求するスコープに `user:read:chat` を追加する。既存のスコープは削らない。
- 監視用の処理（ハブ、配信口、視聴者一覧の周期取得）で何が失敗しても、`read-chat` の読み上げを止めたり終了させたりしない。
- 既存の `show-chatters` の出力書式と挙動は変えない。ページング対応で 100 人を超えても全員が出るようになる変化だけは許容する。
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

### Task 1: 通知テンプレートの設定と置換

**目的:** `greeting_template` を廃止し、種別ごとのテンプレートを読み込み・選択・置換する仕組みを作る。この task ではネットワークに依存しない部分だけを扱う。

**範囲:** `tcyb/src/settings.rs` と、新しく作るテンプレート用モジュール（`tcyb/src/` 配下）。

**契約**
- Consumes: なし。
- Produces:
  - `Settings` に `notification_speech: Vec<NotificationSpeech>` を追加する。`NotificationSpeech` は `{ type_: String, notice_type: Option<String>, template: String }` で、TOML のキー名は Global Constraints のとおり。
  - `Settings` から `greeting_template` を削除する。
  - 次の関数を作る（名前と引数の形は実装者に任せるが、意味はこのとおり）。
    - 通知の種別 `type: &str` と `event: &serde_json::Value` を受け取り、読み上げる文字列を `Option<String>` で返す。一致するテンプレートが無ければ `None`。
    - `notice_type` は `event["notice_type"]` から取る。

**受入基準**
- [ ] `[[notification_speech]]` を複数書いた TOML から `Settings` を読み込める。`notice_type` は省略できる。
- [ ] `greeting_template` が設定ファイルか環境変数 `cb_greeting_template` に残っていると、設定の読み込みが Global Constraints の書き換え例を含むエラーになる。
- [ ] `type` と `notice_type` の両方が一致する要素は、`notice_type` 未指定の要素より優先される。
- [ ] `notice_type` 未指定の要素は、その `type` のどの `notice_type` にも一致する。
- [ ] どの要素にも一致しなければ `None` になる。
- [ ] 置換規則（同名フィールド・ネスト・波括弧のエスケープ・欠落時は空文字と警告・文字列以外の値）がテストで検証されている。
- [ ] 設定ファイルの雛形（`auth-code` などが初回に生成するもの）に、`greeting_template` が無く、`channel.follow` の `[[notification_speech]]` 例がある。

**検証**
`cargo test -p tcyb` が緑。`just check` が緑。

**コミット単位:** 設定スキーマとテンプレートの選択・置換を 1 コミットにする。

---

### Task 2: EventSub の購読拡張と、統一テンプレートによる読み上げ

**目的:** follow・raid・chat.notification を購読する。受けた通知はすべて内部で `type` と `event` の組として扱い、Task 1 の仕組みで読み上げる。

**範囲:** `tcyb/src/eventsub.rs`、`tcyb/src/api.rs`、`tcyb/src/auth.rs`、`tcyb/src/yomiage.rs`。

**契約**
- Consumes: Task 1 のテンプレート選択・置換と `Settings.notification_speech`。
- Produces: EventSub ループが、受け取った通知ごとに「通知を受けた」ことを外へ渡す口を持つ。この口が受け取るのは `(subscription_type: String, event: serde_json::Value, received_at)` 相当の値。Task 5 がこの口をハブにつなぐ。この task の時点では、口に何もつながっていなくてもよい。

**受入基準**
- [ ] セッション開始時に、Global Constraints の 3 種を購読する。そのうちどれかの購読が失敗しても、他の購読と読み上げは続き、失敗した種別がログに出る。
- [ ] `auth-code` の認可 URL のスコープに `user:read:chat` が含まれる。
- [ ] `channel.follow` の通知は、`notification_speech` に一致する設定があるときだけ読み上げられる。旧来のハードコードされたあいさつは残っていない。
- [ ] `channel.raid` と `channel.chat.notification` の通知も、一致するテンプレートがあれば同じ経路（現行の follow と同じ送り先と operations）で読み上げられる。
- [ ] 購読していない種別を含め、受け取った通知は `event` の JSON を捨てずに外へ渡す口へ届く。
- [ ] 通知の解析と読み上げ文の決定が、ネットワークに依存しないテストで検証されている。代表的な follow / raid / chat.notification（sub）の各ペイロードを使う。

**検証**
`cargo test -p tcyb` と `just check` が緑。

**コミット単位:** 購読拡張とスコープ追加で 1 コミット、読み上げのテンプレート化で 1 コミット（1 つにまとめてもよい）。

---

### Task 3: 配信プロトコルの型とイベントハブ

**目的:** `read-chat` と `monitor` が共有するメッセージ型と、broadcast・リングバッファ・スナップショットを持つハブを作る。

**範囲:** `tcyb/src/` 配下の新しいモジュール。

**契約**
- Consumes: なし。
- Produces:
  - **配信メッセージ型。** 1 つの enum で、JSON にすると `{"v":1,"kind":"...", ...}` の形になる。`kind` の値は逐語で `snapshot` / `chat` / `notification` / `chatters` / `status` の 5 つ。各 kind が運ぶ内容は次のとおり。
    - `chat`: 受信時刻、`user_login`、`display_name`、`text`、任意の `color`。
    - `notification`: 受信時刻、`subscription_type`、表示用の 1 行要約 `summary`、元の `event`（JSON）。
    - `chatters`: 取得時刻、login と表示名の一覧（全量）。
    - `status`: IRC の接続状態、EventSub の接続状態、視聴者一覧の最終成功時刻と、直近の失敗理由（任意）。
    - `snapshot`: 保持中の `chat` 列と `notification` 列（古い順）、最新の `chatters`（任意）、最新の `status`。
  - **要約の生成。** 通知の種別と event から `summary` を作る関数。follow・raid・chat.notification（`notice_type` ごと）は人が読める 1 行にし、それ以外は種別名を含む 1 行にする。
  - **ハブ。** `Clone` できるハンドルで、次の操作を持つ。
    - 送信者向け: chat・notification・chatters・status を送る。
    - 購読者向け: 購読の開始。現在のスナップショットと、それ以降の新着を受け取る受信口の組を得る。
    - 受信口が取りこぼしたことを購読者が判定でき、そのときにスナップショットを取り直せる。

**受入基準**
- [ ] 全メッセージが serde の JSON 往復で同値に戻り、`v` が `1` である。
- [ ] chat と notification は、それぞれ `history_size` 件を超えると古い順に捨てられる。
- [ ] 購読開始時に受け取るスナップショットに、それまでに送られた chat・notification（上限内）、最新の chatters、最新の status が含まれる。
- [ ] 送信操作は購読者がいなくても、購読者の受信が止まっていても、待たされずに戻る。
- [ ] 購読者が取りこぼした場合に、それを検出してスナップショットを取り直せることがテストで検証されている。
- [ ] 要約の生成が、既知の種別と未知の種別の両方でテストされている。

**検証**
`cargo test -p tcyb` と `just check` が緑。

**コミット単位:** 型・ハブ・要約を 1 コミットにする。

---

### Task 4: 視聴者一覧のページングと周期取得

**目的:** Get Chatters の全ページ取得に対応し、`read-chat` の中で一定間隔で視聴者一覧を取得してハブへ送るループを作る。

**範囲:** `tcyb/src/api.rs`、`tcyb/src/chat.rs`。

**契約**
- Consumes: Task 3 のハブ（chatters と status の送信）。
- Produces: 「ハブのハンドル、`chatters_interval_secs`、認証に必要な情報を受け取り、終わらずに回り続ける非同期の処理」。Task 5 がこれを `read-chat` から起動する。

**受入基準**
- [ ] Get Chatters が `pagination.cursor` を返す限り、`after` を付けて次のページを取得し、全員を集める。モックサーバによるテストで、2 ページ以上の取得が検証されている。
- [ ] `show-chatters` の出力が、ページングで全員を含むこと以外は変わらない。既存のテストは緑のまま。
- [ ] 周期取得は、成功すると chatters を送り、status の最終成功時刻を更新する。
- [ ] 失敗すると status に失敗理由を載せ、次の周期で再試行する。401 は既存のリフレッシュ処理を 1 回だけ通す。それでも失敗したときも、ループも `read-chat` も終了しない。
- [ ] 取得が間隔より長くかかっても、遅れを取り戻すためにまとめて発火しない。

**検証**
`cargo test -p tcyb` と `just check` が緑。

**コミット単位:** ページングで 1 コミット、周期取得で 1 コミット。

---

### Task 5: read-chat へのハブの接続と WebSocket 配信口

**目的:** `read-chat` の起動時にハブを作り、IRC・EventSub・視聴者一覧の周期取得をつなぐ。`monitor.enabled` のときに `/feed` を配信する。

**範囲:** `tcyb/src/yomiage.rs`、`tcyb/src/irc.rs`、`tcyb/src/eventsub.rs`、`tcyb/src/settings.rs`、`tcyb/Cargo.toml`、配信口用の新しいモジュール。

**契約**
- Consumes:
  - Task 2 の通知を外へ渡す口。
  - Task 3 のハブと配信メッセージ型。
  - Task 4 の周期取得。
  - `Settings` に追加する `monitor`（Global Constraints のキーと既定値）。
- Produces: `ws://127.0.0.1:<port>/feed` のエンドポイント。接続直後に `snapshot` を 1 件送り、以降は `chat` / `notification` / `chatters` / `status` を送る。

**受入基準**
- [ ] `monitor.enabled = false`（既定）のとき、ポートを開かず、視聴者一覧の周期取得も起動しない。
- [ ] `enabled = true` のとき、`127.0.0.1:<port>` だけで待ち受ける。
- [ ] bind に失敗したら警告ログを出し、読み上げは続く。
- [ ] IRC で受けたチャットが `chat` として、EventSub で受けた全通知が `notification` として配信される。
- [ ] IRC と EventSub の接続・切断・再接続が `status` に反映される。
- [ ] 購読者が取りこぼしたら、その接続に `snapshot` が送り直される。接続は切らない。
- [ ] `read-chat` が IRC / EventSub を再接続しても、ハブとその保持内容、配信口は維持される。
- [ ] テストで次のことが検証されている。ローカルにサーバを立ててクライアントを接続し、最初に `snapshot`、その後に送った `chat` が届く。
- [ ] テストで次のことが検証されている。購読者がいない、または受信が止まっている状態でも、チャットの処理（読み上げを呼ぶまで）が待たされない。

**検証**
`cargo test -p tcyb` と `just check` が緑。手動確認として、`read-chat` を起動して `websocat ws://127.0.0.1:8765/feed` で JSON が流れること（人間が実施）。

**コミット単位:** ハブの接続で 1 コミット、配信口で 1 コミット。

---

### Task 6: `tcyb monitor` TUI

**目的:** 配信口に接続して表示する、閲覧専用の TUI サブコマンドを作る。

**範囲:** `tcyb/src/main.rs`、`tcyb/Cargo.toml`、TUI 用の新しいモジュール。

**契約**
- Consumes:
  - Task 3 の配信メッセージ型。
  - Task 5 の `/feed` エンドポイント。
  - `Settings.monitor.port`。
- Produces: サブコマンド `monitor`（`tcyb monitor`）。引数は無い。接続先は `127.0.0.1:<monitor.port>/feed`。

**受入基準**
- [ ] 画面にコメント欄・通知欄・視聴者欄（人数付き）・接続状態行がある。
- [ ] 接続状態行に次が出る。
  - `read-chat` に接続しているかどうか。
  - IRC と EventSub の接続状態。
  - 視聴者一覧の最終更新時刻。取得に失敗しているときはその旨。
- [ ] `monitor` を起動した後に一覧へ初めて現れた視聴者が、他と区別して表示される。起動直後の最初の一覧に入っていた人は区別しない。
- [ ] `snapshot` を受けたら表示内容を置き換え、その後の `chat` / `notification` を追記する。
- [ ] `v` が `1` 以外のメッセージを受けたら、非互換である旨を表示し、内容は反映しない。
- [ ] `read-chat` が動いていなくても終了せず、未接続と表示して再接続を繰り返す。再接続の間隔は伸ばしていき、上限を設ける。`read-chat` の起動・再起動には自動で追従する。
- [ ] キー操作が効く。
  - `q`: 終了
  - `Tab`: 欄の切り替え
  - `↑` `↓` `PgUp` `PgDn`: スクロール
  - `End`: 最新への追従に戻る
  - スクロールしている間は、新着が来ても表示位置が動かない。
- [ ] 通常終了時も panic 時も、端末が元の状態（raw モード解除・代替画面の終了）に戻る。
- [ ] 表示状態の更新（スナップショットの適用、新規視聴者の判定、スクロールの追従）と描画が、端末に依存しないテスト（ratatui の `TestBackend` など）で検証されている。

**検証**
`cargo test -p tcyb` と `just check` が緑。手動確認として、`read-chat`（`monitor.enabled = true`）と `tcyb monitor` を別ターミナルで起動し、表示・TUI を閉じても読み上げが続くこと・`read-chat` を再起動したときの追従を確認する（人間が実施）。

**コミット単位:** 状態モデルで 1 コミット、描画と入力で 1 コミット。

---

### Task 7: ドキュメント、ADR の突合、最終ゲート

**目的:** 利用者向けドキュメントを新しい仕様に合わせ、ADR と実装を突き合わせて、`just ci` を全緑にする。

**範囲:** `tcyb/README.md`、`docs/adr/0023`〜`0026` と `docs/adr/README.md`。

**契約**
- Consumes: Task 1〜6 の成果。
- Produces: なし。

**受入基準**
- [ ] README の設定表と設定例から `greeting_template`（`cb_greeting_template` を含む）が消えている。
- [ ] README に `[[notification_speech]]` の例がある（follow・raid・chat.notification の sub）。
- [ ] README に次の 2 点の注意がある。
  - レイドを `channel.raid` と `channel.chat.notification` の両方に書くと二重に読み上げられる。
  - 移行時に `auth-code` で再認可が必要。
- [ ] README に `[monitor]` 設定と `tcyb monitor` の使い方（起動方法、キー操作、TUI を閉じても読み上げは止まらないこと）がある。
- [ ] ADR-0023〜0026 を実装と突き合わせる。裏づけが取れたものは Status を `Accepted` にし、索引表も同じにする。食い違いがあれば adr-writing の手順で処理し、その結果を報告する。
- [ ] `just ci` が exit code 0 で終わる。

**検証**
`just ci` が exit 0。

**コミット単位:** README で 1 コミット、ADR の昇格で 1 コミット。
