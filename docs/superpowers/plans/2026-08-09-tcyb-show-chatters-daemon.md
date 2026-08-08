# tcyb show-chatters 常駐モード Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

ADR: [0019](../../adr/0019-foreground-only-daemon-for-show-chatters.md)（常駐は前面ループのみ・再起動は OS 側）, [0020](../../adr/0020-interval-option-on-show-chatters.md)（`--interval` オプションで表す）。どちらも `Proposed` で起票済み。実装完了時に `Accepted` へ昇格させる（Task 5）。

**Goal:** `tcyb show-chatters --interval <SECS>` で、Get Chatters を SECS 秒ごとに叩いて 1 行 CSV を stdout に出し続ける前面常駐モードを追加する。

**Architecture:** ループは `tcyb/src/chat.rs` の中に置く。`Store` の生成・`user_id` の取得・チャンネル ID の解決は起動時に 1 回だけ行い、以降の各サンプルでは Get Chatters だけを叩く。出力行の組み立ては純粋関数 `format_chatters_line` に切り出してユニットテストする。Ctrl+C は `main.rs` で `tokio::select!` して受ける（`read-chat` と同形）。

**Tech Stack:** Rust 2021 / tokio 1.28（`full` features、`tokio::time::interval` を使う）/ clap 4.2 derive / chrono 0.4 / anyhow。追加依存なし。

## Global Constraints

- 対象クレートは `tcyb` のみ。`vstc` / `vstc_cli` / `igdb` / `vstc_gui` / `xtask` は変更しない。
- 新しい依存クレートを追加しない（`tokio` の `full` features に `time` が含まれる）。
- 引数を足さずに実行した従来の `show-chatters` は挙動を変えない（出力書式・並び順・除外規則・終了コード）。
- 失敗時の自己回復を実装しない。401 のトークンリフレッシュ再試行のみ従来どおり残す（ADR-0019）。
- 出力先は stdout の 1 行 CSV のみ。`--output` 等のファイル出力オプションを足さない（ADR-0019）。
- 間隔は `config.toml` に置かない（ADR-0020）。
- clippy 閾値: 1 関数の引数は 7 個まで（`clippy.toml` の `too-many-arguments-threshold = 7`）。本計画で導入する関数は最大 7 引数に収まる。8 個目を足さないこと。
- 各タスクの最後は必ず `cargo fmt --all` 済み・`just check` 相当が緑の状態でコミットする。
- コミットメッセージは既存履歴に合わせて `feat(tcyb): ...` / `docs(tcyb): ...` の日本語本文とする。

---

## File Structure

| ファイル | 役割 | 変更 |
| --- | --- | --- |
| `tcyb/src/chat.rs` | chatters の取得・整形・常駐ループ | Modify。`format_chatters_line`（純粋関数）、`chatters_tick`、`resolve_channel_user_id` を切り出し、`chatters` に `interval` を追加。`#[cfg(test)] mod tests` を新設。 |
| `tcyb/src/main.rs` | CLI 定義と分岐 | Modify。`ShowChatters` に `--interval` を追加、常駐時のみ Ctrl+C を select。`#[cfg(test)] mod tests` を新設。 |
| `tcyb/README.md` | 利用者向けドキュメント | Modify。`show-chatters` の節を新設。 |
| `docs/adr/0019-*.md`, `docs/adr/0020-*.md`, `docs/adr/README.md` | ADR | Modify。Status を Accepted へ昇格。 |

---

### Task 1: 出力行の組み立てを純粋関数に切り出す

`chatters` の中に埋まっている「タイムスタンプ + ソート + 除外 + CSV 化」を、ネットワークに依存しない関数へ分離する。この時点では常駐機能は入れず、既存の一発実行の挙動を保ったままリファクタする。

**Files:**
- Modify: `tcyb/src/chat.rs:36-60`（`chatters` 内の出力組み立て部分）
- Test: `tcyb/src/chat.rs`（末尾に `#[cfg(test)] mod tests` を追加）

**Interfaces:**
- Consumes: `crate::api::Chatters` / `crate::api::Chatter`（`tcyb/src/api.rs:161-171`。`Chatters { data: Vec<Chatter> }`、`Chatter { user_id: String, user_login: String, user_name: String }`、いずれも `pub`）
- Produces: `fn format_chatters_line(now: chrono::NaiveDateTime, chatters: &api::Chatters, channel_name: &str, username: &str) -> String`（`chat.rs` 内の非公開関数。Task 2 の `chatters_tick` が呼ぶ）

- [ ] **Step 1: 失敗するテストを書く**

`tcyb/src/chat.rs` の末尾に追加する。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Chatter, Chatters};

    fn chatter(login: &str) -> Chatter {
        Chatter {
            user_id: format!("{login}-id"),
            user_login: login.to_string(),
            user_name: login.to_string(),
        }
    }

    fn at(h: u32, mi: u32, s: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 9)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    #[test]
    fn formats_timestamp_then_logins_sorted_ascending() {
        let chatters = Chatters {
            data: vec![chatter("zoe"), chatter("alice"), chatter("bob")],
        };

        let line = format_chatters_line(at(12, 34, 56), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 12:34:56,alice,bob,zoe");
    }

    #[test]
    fn excludes_channel_owner_and_bot_account() {
        let chatters = Chatters {
            data: vec![chatter("mychannel"), chatter("mybot"), chatter("viewer")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,viewer");
    }

    #[test]
    fn keeps_trailing_comma_when_every_chatter_is_excluded() {
        let chatters = Chatters {
            data: vec![chatter("mybot")],
        };

        let line = format_chatters_line(at(0, 0, 0), &chatters, "mychannel", "mybot");

        assert_eq!(line, "2026-08-09 00:00:00,");
    }
}
```

3 本目は現状の `format!("{},{}", t, users.join(","))` が空リストで末尾カンマを残す挙動を固定するためのもの。挙動を変えないことがこのタスクの目的なので、末尾カンマも仕様として固定する。

- [ ] **Step 2: テストが落ちることを確認**

Run: `cargo test -p tcyb chat::tests`
Expected: コンパイルエラー `cannot find function 'format_chatters_line' in this scope`

- [ ] **Step 3: 純粋関数を実装して `chatters` から呼ぶ**

`tcyb/src/chat.rs` の先頭 `use` はそのまま。`chatters` の直前に関数を足す。

```rust
fn format_chatters_line(
    now: chrono::NaiveDateTime,
    chatters: &api::Chatters,
    channel_name: &str,
    username: &str,
) -> String {
    let mut users: Vec<&str> = chatters
        .data
        .iter()
        .map(|c| c.user_login.as_str())
        .filter(|name| *name != channel_name && *name != username)
        .collect();
    users.sort_unstable();
    format!("{},{}", now.format("%Y-%m-%d %H:%M:%S"), users.join(","))
}
```

`chatters` 内の 2 つ目の `loop` の `Ok(res)` 腕（現 `tcyb/src/chat.rs:38-50`）を次に置き換える。

```rust
            Ok(res) => {
                let now = chrono::Local::now().naive_local();
                println!(
                    "{}",
                    format_chatters_line(now, &res, channel_name, username)
                );
                break;
            }
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb chat::tests`
Expected: 3 tests passed

- [ ] **Step 5: ゲートを通してコミット**

```bash
cargo fmt --all
just check
git add tcyb/src/chat.rs
git commit -m "refactor(tcyb): chatters の出力行組み立てを純粋関数へ切り出す"
```

---

### Task 2: 常駐ループを `chat::chatters` に実装する

チャンネル ID の解決と 1 サンプル分の取得をそれぞれ関数に切り出し、`chatters` に `interval` 引数を足してループを入れる。CLI からはまだ `None` しか渡らないので、この時点で挙動は変わらない。

**Files:**
- Modify: `tcyb/src/chat.rs:1-62`（`use` 追加、`chatters` の再構成）
- Modify: `tcyb/src/main.rs:106-116`（`chat::chatters` の呼び出しに `None` を追加。CLI フラグは Task 3）

**Interfaces:**
- Consumes: `format_chatters_line`（Task 1）、`crate::store::Store`（`Store::new(db_dir, db_name) -> anyhow::Result<Store>`、`store.user_id(username, client_id).await -> anyhow::Result<String>`、`store.access_token() -> &str`、`store.update_tokens(client_id, client_secret).await -> anyhow::Result<()>`）、`crate::api::{get_user, get_chatters}`
- Produces: `pub async fn chatters(db_dir: &Path, db_name: &str, channel_name: &str, username: &str, client_id: &str, client_secret: &str, interval: Option<std::time::Duration>) -> anyhow::Result<()>`（Task 3 の `main.rs` が呼ぶ）

- [ ] **Step 1: `chat.rs` の `use` を足す**

`tcyb/src/chat.rs:1-4` を次にする。

```rust
use crate::{api, store::Store};
use anyhow::bail;
use log::warn;
use std::{path::Path, time::Duration};
```

- [ ] **Step 2: チャンネル ID 解決を関数に切り出す**

`format_chatters_line` の下に足す。中身は現 `tcyb/src/chat.rs:16-35` の `loop` をそのまま移したもの。

```rust
async fn resolve_channel_user_id(
    store: &mut Store,
    channel_name: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<String> {
    loop {
        match api::get_user(channel_name, store.access_token(), client_id).await {
            Ok(channel_user) => {
                if channel_user.data.is_empty() {
                    bail!("channel not found");
                }
                return Ok(channel_user.data[0].id.clone());
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                } else {
                    bail!(err);
                }
            }
        }
    }
}
```

- [ ] **Step 3: 1 サンプル分の取得を関数に切り出す**

`resolve_channel_user_id` の下に足す。引数はちょうど 7 個で `too-many-arguments-threshold = 7` の上限。これ以上増やさないこと。

```rust
async fn chatters_tick(
    store: &mut Store,
    channel_user_id: &str,
    user_id: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
) -> anyhow::Result<()> {
    loop {
        match api::get_chatters(channel_user_id, user_id, store.access_token(), client_id).await {
            Ok(res) => {
                let now = chrono::Local::now().naive_local();
                println!(
                    "{}",
                    format_chatters_line(now, &res, channel_name, username)
                );
                return Ok(());
            }
            Err(err) => {
                if err.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                    warn!("refresh token: {}", err);
                    store.update_tokens(client_id, client_secret).await?;
                } else {
                    bail!(err);
                }
            }
        }
    }
}
```

- [ ] **Step 4: `chatters` を書き換える**

現 `tcyb/src/chat.rs:6-62` の `chatters` 全体を次で置き換える。

```rust
pub async fn chatters(
    db_dir: &Path,
    db_name: &str,
    channel_name: &str,
    username: &str,
    client_id: &str,
    client_secret: &str,
    interval: Option<Duration>,
) -> anyhow::Result<()> {
    let mut store = Store::new(db_dir, db_name)?;
    let user_id = store.user_id(username, client_id).await?;
    let channel_user_id =
        resolve_channel_user_id(&mut store, channel_name, client_id, client_secret).await?;

    let Some(period) = interval else {
        return chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await;
    };

    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        chatters_tick(
            &mut store,
            &channel_user_id,
            &user_id,
            channel_name,
            username,
            client_id,
            client_secret,
        )
        .await?;
    }
}
```

`tokio::time::interval` の最初の `tick()` は即座に返るので、起動直後に 1 行出る。`MissedTickBehavior::Delay` は取りこぼした tick を溜めないので、取得が `period` を超えても遅れを取り戻すまとめ発火は起きない（既定の `Burst` はこれをやるので必ず上書きすること）。

- [ ] **Step 5: 呼び出し側をコンパイルが通る形に直す**

`tcyb/src/main.rs:106-116` の `chat::chatters(...)` 呼び出しの最後の引数に `None` を足す。

```rust
        Some(Commands::ShowChatters {}) => {
            chat::chatters(
                &settings.db_dir,
                &settings.db_name,
                &settings.channel,
                &settings.username,
                &settings.client_id,
                &settings.client_secret,
                None,
            )
            .await?;
        }
```

- [ ] **Step 6: ビルドとテストを確認**

Run: `cargo test -p tcyb`
Expected: PASS（Task 1 の 3 テストが引き続き通る）

Run: `just clippy`
Expected: exit 0。`too_many_arguments` / `cognitive_complexity` の警告が出ないこと。

- [ ] **Step 7: ゲートを通してコミット**

```bash
cargo fmt --all
just check
git add tcyb/src/chat.rs tcyb/src/main.rs
git commit -m "feat(tcyb): chatters に周期取得ループを実装する"
```

---

### Task 3: `--interval` フラグと Ctrl+C 停止を CLI に足す

**Files:**
- Modify: `tcyb/src/main.rs:25-34`（`Commands::ShowChatters` の定義）
- Modify: `tcyb/src/main.rs:106-116`（分岐）
- Test: `tcyb/src/main.rs`（末尾に `#[cfg(test)] mod tests` を追加）

**Interfaces:**
- Consumes: `chat::chatters(..., interval: Option<Duration>)`（Task 2）
- Produces: `Commands::ShowChatters { interval: Option<u64> }`（秒数。`None` = 一発実行）

- [ ] **Step 1: 失敗するテストを書く**

`tcyb/src/main.rs` の末尾に追加する。バイナリクレートでも `cargo test` はテストターゲットをビルドするので、そのまま動く。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parse_interval(args: &[&str]) -> Option<u64> {
        match Cli::try_parse_from(args).unwrap().command {
            Some(Commands::ShowChatters { interval }) => interval,
            _ => panic!("expected the show-chatters subcommand"),
        }
    }

    #[test]
    fn show_chatters_defaults_to_one_shot() {
        assert_eq!(parse_interval(&["tcyb", "show-chatters"]), None);
    }

    #[test]
    fn show_chatters_takes_interval_in_seconds() {
        assert_eq!(
            parse_interval(&["tcyb", "show-chatters", "--interval", "60"]),
            Some(60)
        );
    }

    #[test]
    fn show_chatters_rejects_zero_interval() {
        assert!(Cli::try_parse_from(["tcyb", "show-chatters", "--interval", "0"]).is_err());
    }

    #[test]
    fn show_chatters_rejects_non_numeric_interval() {
        assert!(Cli::try_parse_from(["tcyb", "show-chatters", "--interval", "1m"]).is_err());
    }

    #[test]
    fn cli_definition_is_internally_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
```

- [ ] **Step 2: テストが落ちることを確認**

Run: `cargo test -p tcyb --bin tcyb`
Expected: コンパイルエラー（`ShowChatters` に `interval` フィールドが無い: `struct variant 'Commands::ShowChatters' has no field named 'interval'`）

- [ ] **Step 3: サブコマンド定義にフラグを足す**

`tcyb/src/main.rs:31` の `ShowChatters {},` を置き換える。

```rust
    ShowChatters {
        /// 指定した秒数ごとに取得を繰り返す（省略時は 1 回取得して終了）
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..))]
        interval: Option<u64>,
    },
```

- [ ] **Step 4: 分岐を書き換える**

`tcyb/src/main.rs:106-116` を置き換える。常駐時のみ Ctrl+C を待ち受ける。一発実行では `select!` を挟まず、従来どおりの経路を通す。

```rust
        Some(Commands::ShowChatters { interval }) => {
            let interval = interval.map(std::time::Duration::from_secs);
            let run = chat::chatters(
                &settings.db_dir,
                &settings.db_name,
                &settings.channel,
                &settings.username,
                &settings.client_id,
                &settings.client_secret,
                interval,
            );
            if interval.is_none() {
                run.await?;
            } else {
                tokio::select! {
                    res = run => res?,
                    sig = tokio::signal::ctrl_c() => {
                        sig?;
                        log::warn!("Ctrl+C received, shutting down");
                    }
                }
            }
        }
```

分岐は `match &args.command` なので、パターンで受け取る `interval` の型は `&Option<u64>` になる。`Option<u64>` が `Copy` なので `interval.map(..)` はそのまま通る。束縛し直した後の `interval` は `Option<Duration>`（これも `Copy`）なので、`chatters` へ値渡ししたあとに `is_none()` で読んでよい。

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p tcyb --bin tcyb`
Expected: 5 tests passed

Run: `cargo test -p tcyb`
Expected: PASS（`chat::tests` と `settings::tests` も含めて全緑）

- [ ] **Step 6: ヘルプの表示を目視確認**

Run: `cargo run -p tcyb -- show-chatters --help`
Expected: `--interval <SECS>  指定した秒数ごとに取得を繰り返す（省略時は 1 回取得して終了）` が出る

Run: `cargo run -p tcyb -- show-chatters --interval 0`
Expected: 非 0 終了。`0 is not in 1..` の趣旨のエラーが出て、Twitch API は呼ばれない

- [ ] **Step 7: ゲートを通してコミット**

```bash
cargo fmt --all
just check
git add tcyb/src/main.rs
git commit -m "feat(tcyb): show-chatters に --interval と Ctrl+C 停止を足す"
```

---

### Task 4: 実機で常駐を確認して README に書く

ここまでネットワークに依存する経路はテストしていない。実際の Twitch 認証で常駐を動かして確認し、その手順を README に残す。

**Files:**
- Modify: `tcyb/README.md`（`### 起動` の節の後ろ、ファイル末尾に `### 視聴者一覧の記録` を新設）

**Interfaces:**
- Consumes: Task 3 で確定した CLI（`tcyb show-chatters [--interval <SECS>]`）
- Produces: なし（ドキュメントのみ）

- [ ] **Step 1: 一発実行が従来どおりであることを確認**

Run: `cargo run -p tcyb -- show-chatters`
Expected: `2026-08-09 12:34:56,viewer_a,viewer_b` 形式の 1 行が出て、即座にプロセスが終了する（終了コード 0）

前提として `tcyb auth-code` 済みで、チャンネルのモデレーター権限を持つ bot アカウントで認可されていること（README の「Access Token を取る方法」節を参照）。

- [ ] **Step 2: 常駐が周期的に出力することを確認**

Run: `cargo run -p tcyb -- show-chatters --interval 5`
Expected: 起動直後に 1 行出て、以降およそ 5 秒間隔で 1 行ずつ増える。タイムスタンプの差がおよそ 5 秒であること

- [ ] **Step 3: Ctrl+C で正常終了することを確認**

Step 2 のプロセスで Ctrl+C を押す。
Expected: `Ctrl+C received, shutting down` の warn ログが出てプロセスが終了する。PowerShell なら直後に `$LASTEXITCODE` が `0` であること

- [ ] **Step 4: リダイレクトで行がファイルに残ることを確認**

```powershell
cargo run -p tcyb -- show-chatters --interval 5 >> chatters.csv
```

数行出たところで Ctrl+C し、`chatters.csv` に各行が残っていることを確認する。確認後 `chatters.csv` は削除する（コミットしない）。
Expected: Ctrl+C までに出た行がすべてファイルに入っている

- [ ] **Step 5: README に節を足す**

`tcyb/README.md` の末尾（`### 起動` の節の後ろ）に追記する。

````markdown
### 視聴者一覧の記録

`show-chatters` は、現在チャンネルに滞在している視聴者の login 名を 1 行の CSV で標準出力に書く。行頭はローカル時刻のタイムスタンプで、設定の `channel` と `username`（bot 自身）は除外し、残りを昇順に並べる。

```sh
cargo run -p tcyb -- show-chatters
# => 2026-08-09 12:34:56,viewer_a,viewer_b
```

`--interval <SECS>` を付けると、起動直後に 1 行出したあと SECS 秒ごとに取得を繰り返す常駐モードになる。SECS は 1 以上の整数（秒）で、0 を渡すとエラーになる。

```powershell
cargo run -p tcyb -- show-chatters --interval 60 >> chatters.csv
```

停止は Ctrl+C。1 行ごとに出力が flush されるため、途中で停止してもそれまでの行はリダイレクト先に残る。

常駐は前面で動くプロセスであって、自身をバックグラウンドへ回したりサービス登録したりはしない。ログオン時の自動起動やバックグラウンド実行が必要なら、Windows のタスクスケジューラなど OS 側の仕組みから起動する。

トークンが失効した場合（401）は自動でリフレッシュして同じ取得をやり直し、常駐は継続する。それ以外のエラー（回線断・5xx・スコープ不足の 403 など）ではプロセスがそのまま終了する。**自動では再起動しない**ので、長時間の無人記録では起動元の側で再実行を設定する。
````

- [ ] **Step 6: ゲートを通してコミット**

```bash
just check-env-leak
git add tcyb/README.md
git commit -m "docs(tcyb): show-chatters の常駐モードを README に追記する"
```

`check-env-leak` は README に個人環境の絶対パスが混ざっていないことを見る。上の追記は相対パスと `chatters.csv` しか使っていないので通るはず。落ちた場合は該当箇所を相対パスかプレースホルダに直す。

---

### Task 5: ADR を Accepted へ昇格させてフルゲートを通す

**Files:**
- Modify: `docs/adr/0019-foreground-only-daemon-for-show-chatters.md:3`
- Modify: `docs/adr/0020-interval-option-on-show-chatters.md:3`
- Modify: `docs/adr/README.md`（索引の 0019 / 0020 の行）

**Interfaces:**
- Consumes: Task 1〜4 の実装
- Produces: なし

- [ ] **Step 1: ADR と実装の突合**

2 本の ADR を読み直し、実装が決定どおりかを確認する。

- ADR-0019: 前面ループのみか（デタッチ・PID・サービス登録を入れていないか）／Ctrl+C で 0 終了か／401 以外で即終了するか／出力は stdout の 1 行 CSV のみか
- ADR-0020: `--interval <SECS>` か（新サブコマンドを作っていないか）／省略時に一発実行へ戻るか／0 を弾くか／`config.toml` に間隔キーを足していないか

乖離があれば、実装を ADR に合わせるか、実装が正しいなら新 ADR で supersede する（Accepted 本文は書き換えない）。

- [ ] **Step 2: Status を昇格**

両ファイルの `- Status: Proposed` を `- Status: Accepted` に変える。

- [ ] **Step 3: 索引を更新**

`docs/adr/README.md` の索引表で、0019 と 0020 の行の `Proposed` を `Accepted` に変える。

- [ ] **Step 4: フルゲート**

Run: `just ci`
Expected: exit 0（fmt-check / clippy / clippy-profiling / test / test-profiling / check-env-leak / gitleaks / deny / audit がすべて緑）

赤が出た場合の対応は `CLAUDE.md` の「品質ゲート」節に従う。fmt-check だけは `just fmt` で機械的に直してよい。

- [ ] **Step 5: コミット**

```bash
git add docs/adr/0019-foreground-only-daemon-for-show-chatters.md docs/adr/0020-interval-option-on-show-chatters.md docs/adr/README.md
git commit -m "docs(adr): show-chatters 常駐の ADR 0019/0020 を Accepted へ"
```

---

## 受入基準との対応

| spec の受入基準 | 実装するタスク |
| --- | --- |
| 間隔なしで 1 行出して終了 / 書式・並び順・除外が従来どおり | Task 1（テストで固定）, Task 3 Step 4（分岐）, Task 4 Step 1（実機） |
| N 秒指定で起動直後に 1 行、以降 N 秒ごと | Task 2 Step 4, Task 4 Step 2 |
| 常駐中の行の書式が一発実行と同じ | Task 2 Step 3（同じ `format_chatters_line` を通す） |
| 0 を弾き API を呼ばない | Task 3 Step 3（clap の `range(1..)`）, Task 3 Step 6 |
| 取りこぼした tick を溜めない | Task 2 Step 4（`MissedTickBehavior::Delay`） |
| チャンネル ID 解決とトークン DB 読み込みは起動時 1 回 | Task 2 Step 4（ループの外で解決） |
| Ctrl+C でログを残し終了コード 0 | Task 3 Step 4, Task 4 Step 3 |
| 401 でリフレッシュして再試行・常駐継続 | Task 2 Step 3（`chatters_tick` 内の `loop`） |
| 401 以外で非 0 終了 | Task 2 Step 3（`bail!`）, Task 2 Step 4（`?` で伝播） |
| 出力行の組み立てをネットワーク非依存でテスト | Task 1 Step 1 |
| 間隔指定の解釈をネットワーク非依存でテスト | Task 3 Step 1 |
| README に起動方法・追記運用・停止方法・自動再起動しないこと | Task 4 Step 5 |
