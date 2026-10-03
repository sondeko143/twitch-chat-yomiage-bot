# 0025. 通知読み上げを種別ごとのテンプレート配列に統一し、greeting_template を廃止する

- Status: Accepted
- 効力: 既定
- Date: 2026-10-03
- Related: [monitor TUI spec](../superpowers/specs/2026-10-03-tcyb-monitor-tui-design.md), [ADR-0026](0026-receive-sub-events-via-chat-notification.md)

## Context

通知の読み上げは、フォロー専用の設定キー `greeting_template` 1 本でハードコードされている。テンプレート中の `user_name` という文字列を素朴に置換するだけで、他の通知種別へは広げられない。

レイドや、`channel.chat.notification`（サブスク・ギフト・アナウンス等。種類は `notice_type` で区別される）を受け取るようになるので、種別ごとに文面を変えて読み上げたい。

設定は `config` クレート経由で TOML から読む。`config` クレートは、キー中のドットを階層の区切りとして解釈する。

利用者は、現行のフォロー読み上げを廃止してよいと明言している。

## Decision

- 設定に `[[notification_speech]]` の配列テーブルを設ける。各要素は `type`、任意の `notice_type`、`template` を持つ。
- 読み上げに使う要素は次の順で選ぶ。`type` が一致し、`notice_type` も一致する要素を優先する。無ければ `notice_type` 未指定の要素を使う。どれにも一致しなければ読み上げない。
- テンプレートの `{field}` は、EventSub の `event` JSON のフィールドで置換する。`{a.b}` はネストしたフィールドをたどる。`{{` / `}}` はそれぞれ波括弧そのものを表す。存在しないフィールドは空文字にし、警告ログを出す。
- 読み上げの送り先は、現行のフォロー読み上げと同じ経路を使う。
- `greeting_template` は廃止する。設定に残っていたら、新形式への書き換え方を示すエラーで起動を中止する。

## Alternatives rejected

- **`type` をキーにしたテーブル（`[notification_speech]` の下に `"channel.follow" = "..."`）** — `config` クレートがキー中のドットを階層として解釈し、意図しない入れ子になる恐れがある。`notice_type` まで表すとキー設計がさらに歪む。
- **`greeting_template` を `channel.follow` の別名として残す** — 同じことを書く方法が 2 つになり、両方書かれたときの優先規則が要る。利用者が廃止を許容している。
- **旧キーを黙って無視する** — 移行を忘れると、配信中にフォロー読み上げだけが黙って鳴らなくなる。気付く機会が配信本番になってしまう。
- **テンプレートエンジン（handlebars / tera 等）を入れる** — 必要なのはフィールド置換だけで、条件分岐やループは要らない。依存と構文の学習コストに見合わない。
- **旧来の素の `user_name` 置換を踏襲する** — 本文中に偶然現れる語まで置換される。また、どこが差し込み位置なのかが読み手に分からない。

## Consequences

- 既存利用者は設定の書き換えが必須になる。起動エラーと README がその手順を案内する。
- 環境変数（`cb_` 接頭辞）からは、この配列テーブルを上書きできない。
- 同じ出来事が複数の種別で届く場合（レイドは `channel.raid` と `channel.chat.notification` の両方で届く）、両方にテンプレートを書くと二重に読み上げる。これは設定側の責任とし、README で注意する。
