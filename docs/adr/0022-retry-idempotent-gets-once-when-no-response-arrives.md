# 0022. 応答が返らなかった Helix の GET は共有クライアント側で 1 回だけ張り直す

- Status: Accepted
- Date: 2026-08-30
- Related: [ADR-0019](0019-foreground-only-daemon-for-show-chatters.md), [ADR-0021](0021-bound-401-retry-to-a-single-refresh.md)

## Context

`show-chatters --interval` の常駐が、一定時間動いたあと次のエラーで必ず落ちる。

```
error sending request for url (https://api.twitch.tv/helix/chat/chatters?...)
  0: client error (SendRequest)
  1: connection error
  2: 既存の接続はリモート ホストに強制的に切断されました。 (os error 10054)
```

`tcyb/src/api.rs` の HTTP クライアントはプロセス全体で共有され、コネクションプールと keep-alive を再利用する。常駐は tick 間（既定の運用例では 60 秒）その接続を寝かせるが、reqwest 0.13 の `pool_idle_timeout` 既定値は 90 秒なので、接続はアイドルのままプールに残り続ける。サーバ/経路側がそれより先に接続を捨てていると、次の tick はその死んだ接続を掴んで書き込み、RST（Windows で os error 10054）か応答前の EOF を受け取る。

この経路は下層では自動回復しない。hyper がリクエストを取り戻して張り直せるのは、接続が「リクエスト送出前」に落ちた場合（`Canceled`）だけで、送出済みの場合は `SendRequest` としてそのまま返す。reqwest の既定リトライは HTTP/2 の GOAWAY / REFUSED_STREAM 専用であり、`http2` feature を切っている本ビルドでは一切働かない。

上位も回復しない。`with_token_refresh`（`tcyb/src/chat.rs`）は 401 だけを再試行対象にしており、それ以外はそのまま `bail!` する。結果として、接続を 1 本掴み損ねただけで無人記録が終了する。

ADR-0019 は「一過性エラーを握りつぶして次の tick へ進む」を、恒久的な失敗が沈黙故障になることを理由に却下している。この決定はその方針を覆すものではない。

## Decision

共有 HTTP クライアント（`api.rs` の `HTTP_CLIENT`）に reqwest のリトライポリシーを設定し、**応答が返らなかった GET を 1 回だけ張り直す**。

- 対象ホストは Helix API（`api.twitch.tv`）に限る（`reqwest::retry::for_host`）。
- 対象は `GET` のみ。POST（ban、EventSub 購読、トークン交換）は張り直さない。
- 対象は「応答が返らなかった」場合のみ（`status()` が `None`）。ステータスを伴う失敗（401 / 403 / 5xx）は従来どおり呼び出し元の判断に委ねる。
- 上限は 1 回（`max_retries_per_request(1)`）。reqwest 既定のリトライ予算も併用する。

根拠: GET は副作用が無いので 2 度届いても害が無く、応答が返っていない以上、呼び出し元は 1 回目から何も得ていない。死んだ接続を掴んだかどうかはこの層からは判別できないが、判別する必要も無い — どちらであれ再送は安全で、恒久的な障害なら 2 回目も同じ失敗を返す。

## Alternatives rejected

- **常駐ループ側で一過性エラーを握りつぶし次の tick へ進む** — ADR-0019 が却下済み。恒久的な失敗でもプロセスが生き続け、出力だけが止まる沈黙故障になる。本決定は同一 tick 内で同じサンプルを取り直すもので、失敗が続けば従来どおりプロセスは終了する。
- **`pool_idle_timeout` をサーバ側のアイドル切断より短く設定する** — 死んだ接続を掴む確率は下がるが、無くならない。切断はアイドル時間だけでなくロードバランサの入れ替えや経路の都合でも起き、tick 間隔は利用者が決める（`--interval 1` もありうる）。加えて「何秒なら安全か」は Twitch 側の非公開な設定に依存する非自明な閾値になる。張り直しがあれば掴んだ後でも回復できるので、確率を下げるだけの設定は今回は入れない。
- **`with_token_refresh` に接続エラーの再試行を足す** — 直せるのは `chat.rs` を通る経路だけで、同じ共有クライアントを使う `store`・`channel`・`eventsub` は取り残される。欠陥は共有プールを持つクライアント側にあるので、そこで塞ぐ方が漏れが無い。
- **エラー種別（`io::ErrorKind::ConnectionReset` など）で判定する** — 同じ原因が RST（`Io`）にも応答前 EOF（`IncompleteMessage`）にもなり、hyper / std のどの層に何が現れるかは実装詳細で動く。「応答が返ったか」だけで判定する方が安定し、GET に限る限り安全性も変わらない。
- **リトライ上限を 2 回以上にする** — 1 回目の張り直しは必ず新しい接続で行われるため、死んだ接続が原因ならそこで解決する。解決しないなら原因は別で、回数を増やしても新しい情報は得られない（ADR-0021 と同じ理由）。

## Consequences

- `show-chatters --interval` は、接続を掴み損ねた tick でサンプルを落とさず継続する。回線が本当に切れている場合は 2 回目も失敗し、従来どおり非 0 で終了する（ADR-0019 の「監督は外側」の方針は維持される）。
- 効果は Helix API を叩く全コマンド（`read-chat` の EventSub 購読・ユーザー解決を含む）に及ぶ。POST は対象外なので、ban や購読が二重に適用されることはない。
- リクエスト単位の `timeout()` を張った GET は、タイムアウト時に 1 回分待ち時間が伸びる。現状 `timeout()` を使っているのは `get_tokens_by_refresh`（POST）だけなので影響は無いが、GET に timeout を足すときはこの相互作用に注意する。
- 張り直しはログに出ない。1 回で回復した接続断は外から見えないので、頻度を知りたくなったら計装を足す必要がある。
