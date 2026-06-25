# chat 翻訳 Twitch emote パススルー Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** チャット翻訳時に Twitch ネイティブ emote を翻訳クエリから除外し、翻訳結果の末尾に emote を連結して返信する。

**Architecture:** ロジックは `tcyb/src/irc.rs` に閉じる。IRC `emotes` タグを `parse_message` でパースして位置範囲を保持し、純粋関数 `split_message_emotes` で本文を「翻訳用テキスト」と「emote リスト」に分割する。`process_message` は分割結果を使って翻訳コマンド呼び出しと返信本文の組み立てを行う。外部翻訳コマンドのインターフェースは変更しない。

**Tech Stack:** Rust (edition 2021), tokio, tokio-tungstenite, regex, log。テストは標準 `#[cfg(test)]` ユニットテスト。

## Global Constraints

- パッケージ名は `tcyb`。テストは `cargo test -p tcyb` で実行する（作業ディレクトリはリポジトリルート）。
- 既存モジュールのスタイルに従う（`if let` ベース、`lazy_static` 正規表現、`info!`/`warn!` ロギング）。
- `emotes` タグの位置は **コードポイントオフセット**。バイトインデックスでは切らない（`chars()` ベース）。
- 対象は Twitch ネイティブ emote のみ。TTS（`send_chat_message_to_speak`）と外部翻訳コマンドのインターフェースは変更しない。
- 最終的に `cargo clippy -p tcyb` が警告なしで通ること（直近コミットで clippy 警告を解消済みの方針を踏襲）。
- プラットフォームは Windows / PowerShell。

## File Structure

- Modify: `tcyb/src/irc.rs`
  - 新規純粋関数 `parse_emote_ranges`, `split_message_emotes`, `translated_reply_body`
  - 新規 async ヘルパー `send_reply`
  - `IrcMessage` 構造体に `emote_ranges: Vec<(usize, usize)>` フィールド追加
  - `parse_message` で `emotes` タグを抽出
  - `process_message` の `Chat` 分岐を上記関数を使う形に書き換え
  - `#[cfg(test)] mod tests` にユニットテスト追加

全変更が 1 ファイルに収まる。既存の関心の分離（パース / 分割 / 整形 / 送信）に沿って関数を切る。

---

### Task 1: `parse_emote_ranges` — emotes タグ文字列を位置範囲リストへパース

**Files:**
- Modify: `tcyb/src/irc.rs`（`parse_message` の上、`#[derive(Default)] enum IrcMessageKind` の前あたりに関数追加）
- Test: `tcyb/src/irc.rs` の `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: なし
- Produces: `fn parse_emote_ranges(tag_value: &str) -> Vec<(usize, usize)>`
  - 入力は `emotes` タグの値（`=` の右側、例 `25:0-4,12-16/1902:6-10`）。
  - 全 emote の全出現範囲を `(start, end)`（end 包含）で出現テキスト順に近い形で返す。空文字列なら空 `Vec`。

- [ ] **Step 1: 失敗するテストを書く**

`tcyb/src/irc.rs` の `mod tests` 内（`use super::*;` の下）に追加:

```rust
    #[test]
    fn parse_emote_ranges_empty() {
        assert_eq!(parse_emote_ranges(""), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn parse_emote_ranges_single() {
        assert_eq!(parse_emote_ranges("25:0-4"), vec![(0, 4)]);
    }

    #[test]
    fn parse_emote_ranges_same_emote_multiple() {
        assert_eq!(parse_emote_ranges("25:0-4,12-16"), vec![(0, 4), (12, 16)]);
    }

    #[test]
    fn parse_emote_ranges_multiple_emotes() {
        assert_eq!(parse_emote_ranges("25:0-4/1902:6-10"), vec![(0, 4), (6, 10)]);
    }
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

Run: `cargo test -p tcyb parse_emote_ranges`
Expected: コンパイルエラー `cannot find function `parse_emote_ranges` in this scope`

- [ ] **Step 3: 最小実装を書く**

`tcyb/src/irc.rs` の `fn parse_message` の直前に追加:

```rust
fn parse_emote_ranges(tag_value: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if tag_value.is_empty() {
        return ranges;
    }
    for emote in tag_value.split('/') {
        if let Some((_id, positions)) = emote.split_once(':') {
            for pos in positions.split(',') {
                if let Some((start, end)) = pos.split_once('-') {
                    if let (Ok(s), Ok(e)) = (start.parse::<usize>(), end.parse::<usize>()) {
                        ranges.push((s, e));
                    }
                }
            }
        }
    }
    ranges
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb parse_emote_ranges`
Expected: PASS（4 件すべて）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(irc): emotes タグを位置範囲へパースする parse_emote_ranges を追加"
```

---

### Task 2: `split_message_emotes` — 本文を翻訳用テキストと emote に分割

**Files:**
- Modify: `tcyb/src/irc.rs`（`parse_emote_ranges` の下に関数追加）
- Test: `tcyb/src/irc.rs` の `mod tests`

**Interfaces:**
- Consumes: `parse_emote_ranges` が返す `Vec<(usize, usize)>`
- Produces: `fn split_message_emotes(chat_msg: &str, ranges: &[(usize, usize)]) -> (String, Vec<String>)`
  - 戻り値 `.0` = emote を除き余分な空白を詰めた翻訳用テキスト（前後トリム、連続空白は 1 個）。
  - 戻り値 `.1` = 出現順の emote 文字列リスト。**同一 emote 文字列は最初の 1 つにまとめる**。
  - 切り出しはコードポイント単位。範囲が範囲外/不正なものは無視。

- [ ] **Step 1: 失敗するテストを書く**

`mod tests` 内に追加:

```rust
    #[test]
    fn split_message_emotes_no_ranges() {
        let (cleaned, emotes) = split_message_emotes("hello world", &[]);
        assert_eq!(cleaned, "hello world");
        assert!(emotes.is_empty());
    }

    #[test]
    fn split_message_emotes_trailing_emote() {
        let (cleaned, emotes) = split_message_emotes("Hello DinoDance", &[(6, 14)]);
        assert_eq!(cleaned, "Hello");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_middle_emote_collapses_space() {
        let (cleaned, emotes) = split_message_emotes("a Kappa b", &[(2, 6)]);
        assert_eq!(cleaned, "a b");
        assert_eq!(emotes, vec!["Kappa".to_string()]);
    }

    #[test]
    fn split_message_emotes_japanese_codepoint() {
        let (cleaned, emotes) = split_message_emotes("こんにちは DinoDance", &[(6, 14)]);
        assert_eq!(cleaned, "こんにちは");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_emote_only() {
        let (cleaned, emotes) = split_message_emotes("DinoDance", &[(0, 8)]);
        assert_eq!(cleaned, "");
        assert_eq!(emotes, vec!["DinoDance".to_string()]);
    }

    #[test]
    fn split_message_emotes_dedup_same_emote() {
        let (cleaned, emotes) = split_message_emotes("Kappa hi Kappa", &[(0, 4), (9, 13)]);
        assert_eq!(cleaned, "hi");
        assert_eq!(emotes, vec!["Kappa".to_string()]);
    }

    #[test]
    fn split_message_emotes_keeps_distinct_emotes() {
        let (cleaned, emotes) = split_message_emotes("hi Kappa PogChamp", &[(3, 7), (9, 16)]);
        assert_eq!(cleaned, "hi");
        assert_eq!(emotes, vec!["Kappa".to_string(), "PogChamp".to_string()]);
    }
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

Run: `cargo test -p tcyb split_message_emotes`
Expected: コンパイルエラー `cannot find function `split_message_emotes` in this scope`

- [ ] **Step 3: 最小実装を書く**

`parse_emote_ranges` の下に追加:

```rust
fn split_message_emotes(chat_msg: &str, ranges: &[(usize, usize)]) -> (String, Vec<String>) {
    if ranges.is_empty() {
        return (chat_msg.to_string(), Vec::new());
    }
    let chars: Vec<char> = chat_msg.chars().collect();
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort_by_key(|&(start, _)| start);

    let mut emotes: Vec<String> = Vec::new();
    let mut covered = vec![false; chars.len()];
    for &(start, end) in &sorted {
        if start > end || end >= chars.len() {
            continue;
        }
        let emote: String = chars[start..=end].iter().collect();
        if !emotes.contains(&emote) {
            emotes.push(emote);
        }
        for slot in &mut covered[start..=end] {
            *slot = true;
        }
    }

    let cleaned_raw: String = chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if covered[i] { None } else { Some(*c) })
        .collect();
    let cleaned = cleaned_raw.split_whitespace().collect::<Vec<_>>().join(" ");

    (cleaned, emotes)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb split_message_emotes`
Expected: PASS（7 件すべて）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(irc): 本文を翻訳用テキストと emote に分割する split_message_emotes を追加"
```

---

### Task 3: `translated_reply_body` — 翻訳結果と emote から返信本文を組み立て

**Files:**
- Modify: `tcyb/src/irc.rs`（`split_message_emotes` の下に関数追加）
- Test: `tcyb/src/irc.rs` の `mod tests`

**Interfaces:**
- Consumes: 翻訳コマンドの stdout 文字列、`split_message_emotes` の emote を `join(" ")` した suffix 文字列
- Produces: `fn translated_reply_body(translated_stdout: &str, emote_suffix: &str) -> Option<String>`
  - stdout を trim し、空なら `None`（無返信）。
  - emote suffix が空なら trim 済み翻訳のみ、空でなければ `"{translated} {emote_suffix}"`（末尾連結）。

- [ ] **Step 1: 失敗するテストを書く**

`mod tests` 内に追加:

```rust
    #[test]
    fn translated_reply_body_empty_stdout_is_none() {
        assert_eq!(translated_reply_body("   \n", "DinoDance"), None);
    }

    #[test]
    fn translated_reply_body_no_emotes() {
        assert_eq!(
            translated_reply_body("hello\n", ""),
            Some("hello".to_string())
        );
    }

    #[test]
    fn translated_reply_body_appends_emotes() {
        assert_eq!(
            translated_reply_body("hello\n", "DinoDance"),
            Some("hello DinoDance".to_string())
        );
    }
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

Run: `cargo test -p tcyb translated_reply_body`
Expected: コンパイルエラー `cannot find function `translated_reply_body` in this scope`

- [ ] **Step 3: 最小実装を書く**

`split_message_emotes` の下に追加:

```rust
fn translated_reply_body(translated_stdout: &str, emote_suffix: &str) -> Option<String> {
    let translated = translated_stdout.trim();
    if translated.is_empty() {
        return None;
    }
    if emote_suffix.is_empty() {
        Some(translated.to_string())
    } else {
        Some(format!("{translated} {emote_suffix}"))
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb translated_reply_body`
Expected: PASS（3 件すべて）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(irc): 翻訳結果と emote を連結する translated_reply_body を追加"
```

---

### Task 4: `IrcMessage.emote_ranges` フィールド追加と `parse_message` での抽出

**Files:**
- Modify: `tcyb/src/irc.rs`（`struct IrcMessage` 定義と `fn parse_message` の `Chat` 分岐）
- Test: `tcyb/src/irc.rs` の `mod tests`

**Interfaces:**
- Consumes: `parse_emote_ranges`（Task 1）
- Produces: `IrcMessage` に `pub(self) emote_ranges: Vec<(usize, usize)>` フィールド（モジュール内可視）。`Chat` の場合に `emotes` タグから抽出した範囲が入り、それ以外は空 `Vec`。

- [ ] **Step 1: 失敗するテストを書く**

`mod tests` 内に追加:

```rust
    #[test]
    fn parse_message_extracts_emote_ranges() {
        let message = parse_message(
            "@badge-info=;badges=;emotes=25:0-4,6-10;id=abc :u!u@u.tmi.twitch.tv PRIVMSG #chan :Kappa Kappa",
        );
        assert_eq!(message.emote_ranges, vec![(0, 4), (6, 10)]);
    }

    #[test]
    fn parse_message_empty_emotes_yields_no_ranges() {
        let message = parse_message(
            "@badge-info=;badges=;emotes=;id=abc :u!u@u.tmi.twitch.tv PRIVMSG #chan :hello",
        );
        assert!(message.emote_ranges.is_empty());
    }
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

Run: `cargo test -p tcyb parse_message`
Expected: コンパイルエラー `no field `emote_ranges` on type `IrcMessage``

- [ ] **Step 3: フィールドを追加し `parse_message` で抽出する**

`struct IrcMessage` にフィールドを追加（既存の `channel` の下）:

```rust
#[derive(Default)]
struct IrcMessage {
    kind: IrcMessageKind,
    msg_id: Option<String>,
    chat_msg: Option<String>,
    user: Option<String>,
    channel: Option<String>,
    emote_ranges: Vec<(usize, usize)>,
}
```

`fn parse_message` の `Chat` 分岐（`let id_tag = ...; let msg_id = ...;` の直後、`return IrcMessage { ... }` の前）に追加:

```rust
            let emotes_tag = tags
                .split(';')
                .find(|tag| {
                    let name_value: Vec<_> = tag.split('=').collect();
                    name_value[0] == "emotes"
                })
                .unwrap_or_default();
            let emote_ranges = parse_emote_ranges(emotes_tag.get(7..).unwrap_or_default());
```

そして `return IrcMessage { ... }` に `emote_ranges` を追加:

```rust
            return IrcMessage {
                kind: IrcMessageKind::Chat,
                msg_id: Some(msg_id.into()),
                chat_msg: Some(caps["chat_msg"].into()),
                channel: Some(caps["channel"].into()),
                user: Some(caps["user"].into()),
                emote_ranges,
            };
```

（注: `emotes=` は 7 文字なので `get(7..)` で値部分を取り出す。タグが無い場合は `unwrap_or_default()` で空文字列となり `parse_emote_ranges` が空 `Vec` を返す。）

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p tcyb parse_message`
Expected: PASS（`parse_message_extracts_emote_ranges`, `parse_message_empty_emotes_yields_no_ranges`, 既存の `parse_smile_emoji_message` すべて）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(irc): IrcMessage に emote_ranges を追加し parse_message で抽出"
```

---

### Task 5: `process_message` を分割結果ベースに書き換え（emote パススルー有効化）

**Files:**
- Modify: `tcyb/src/irc.rs`（`fn process_message` の `Chat` 分岐、および新規 `send_reply` ヘルパー）

**Interfaces:**
- Consumes: `split_message_emotes`（Task 2）, `translated_reply_body`（Task 3）, `IrcMessage.emote_ranges`（Task 4）
- Produces: `async fn send_reply(ws_stream, msg_id, channel, body)` — `@reply-parent-msg-id` 付き返信を送る共通ヘルパー。
- 振る舞い: 翻訳コマンドへは emote 除去後の `cleaned` を渡す。emote のみ（`cleaned` 空）なら翻訳をスキップして emote だけ返信。翻訳結果末尾に emote を連結。

このタスクはネットワーク結合部のため新規ユニットテストは追加せず、ロジックは Task 2/3 のユニットテストでカバー済み。検証は全テスト通過 + clippy + ビルドで行う（既存コードも `process_message` 自体のユニットテストは持たない方針に揃える）。

- [ ] **Step 1: `send_reply` ヘルパーを追加**

`fn send_chat_message_to_speak` の直後に追加:

```rust
async fn send_reply(
    ws_stream: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    msg_id: &str,
    channel: &str,
    body: &str,
) {
    let reply = format!("@reply-parent-msg-id={msg_id} PRIVMSG #{channel} :{body}");
    match ws_stream
        .send(Message::Text(String::from(reply.as_str())))
        .await
    {
        Ok(_) => info!("{reply}"),
        Err(err) => warn!("{err}"),
    }
}
```

- [ ] **Step 2: `process_message` の `Chat` 分岐を書き換える**

`tcyb/src/irc.rs` の `IrcMessageKind::Chat => { ... }` 分岐（現状 `let chat_msg = ...` から内側 `Ok(())` / `else { Ok(()) }` まで）を、以下で置き換える:

```rust
            IrcMessageKind::Chat => {
                let chat_msg = irc_message.chat_msg.unwrap_or_default();
                let user = irc_message.user.unwrap_or_default();
                if user != username {
                    info!(
                        "{:?} says {:?} in #{:?}",
                        user.as_str(),
                        chat_msg.as_str(),
                        irc_message.channel.unwrap_or_default().as_str(),
                    );
                    send_chat_message_to_speak(chat_msg.as_str(), address, operations).await?;
                    let msg_id = irc_message.msg_id.unwrap_or_default();
                    let (cleaned, emotes) =
                        split_message_emotes(&chat_msg, &irc_message.emote_ranges);
                    let emote_suffix = emotes.join(" ");

                    if cleaned.is_empty() {
                        // emote のみのメッセージ: 翻訳をスキップし emote だけ返信する。
                        if !emote_suffix.is_empty() {
                            send_reply(ws_stream, &msg_id, channel, &emote_suffix).await;
                        }
                        return Ok(());
                    }

                    let translate_fut = Command::new(translate_command)
                        .args([cleaned.as_str()])
                        .kill_on_drop(true)
                        .output();
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(TRANSLATE_TIMEOUT_SECS),
                        translate_fut,
                    )
                    .await
                    {
                        Ok(Ok(output)) => {
                            let stdout = match std::str::from_utf8(&output.stdout) {
                                Ok(val) => val,
                                Err(err) => {
                                    warn!("{err}");
                                    ""
                                }
                            };
                            info!("{stdout}");
                            if let Some(body) = translated_reply_body(stdout, &emote_suffix) {
                                send_reply(ws_stream, &msg_id, channel, &body).await;
                            }
                        }
                        Ok(Err(err)) => {
                            warn!("{err}");
                        }
                        Err(_) => {
                            warn!(
                                "translate command timed out after {}s, killed child",
                                TRANSLATE_TIMEOUT_SECS
                            );
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }
```

- [ ] **Step 3: 全テストが通ることを確認**

Run: `cargo test -p tcyb`
Expected: PASS（Task 1〜4 で追加した全テスト + 既存 `parse_smile_emoji_message` + `translate_command_timeout_kills_long_running_child`）

- [ ] **Step 4: clippy とビルドが警告なしで通ることを確認**

Run: `cargo clippy -p tcyb --all-targets` と `cargo build -p tcyb`
Expected: どちらもエラー・警告なし（特に未使用関数の `dead_code` 警告が出ないこと = 全関数が結線済み）

- [ ] **Step 5: コミット**

```bash
git add tcyb/src/irc.rs
git commit -m "feat(irc): 翻訳クエリから emote を除外し結果末尾に連結する"
```

---

## Self-Review

**1. Spec coverage:**
- 実装場所＝tcyb 側 → Task 4/5 で `irc.rs` に実装。✓
- emotes タグのパース（位置範囲）→ Task 1 + Task 4。✓
- 翻訳クエリから emote 除外 → Task 2 + Task 5（`cleaned` を翻訳に渡す）。✓
- コードポイント単位の切り出し → Task 2（`chars()`）+ `split_message_emotes_japanese_codepoint` テスト。✓
- 余分な空白の詰め → Task 2（`split_whitespace().join(" ")`）+ `split_message_emotes_middle_emote_collapses_space` テスト。✓
- 同一 emote をまとめる → Task 2（`!emotes.contains`）+ `split_message_emotes_dedup_same_emote` テスト。✓
- 末尾に連結 → Task 3 + Task 5。✓
- emote のみ → emote だけ返信 → Task 5（`cleaned.is_empty()` 分岐）。✓
- stdout 空 → 無返信（emote も付けない）→ Task 3（`None`）。✓
- 後方互換（emote 無し＝従来通り）→ Task 2（ranges 空で本文そのまま）+ `parse_message_empty_emotes_yields_no_ranges`。✓
- TTS・外部翻訳コマンド不変 → Task 5（`send_chat_message_to_speak` は `chat_msg` 全文のまま、コマンド引数のみ `cleaned` に）。✓

**2. Placeholder scan:** TBD/TODO/「適切に」等なし。全ステップに実コードと実コマンドあり。✓

**3. Type consistency:**
- `parse_emote_ranges(&str) -> Vec<(usize, usize)>` — Task 1 定義、Task 4 で使用。✓
- `split_message_emotes(&str, &[(usize, usize)]) -> (String, Vec<String>)` — Task 2 定義、Task 5 で使用。✓
- `translated_reply_body(&str, &str) -> Option<String>` — Task 3 定義、Task 5 で使用。✓
- `send_reply(&mut WebSocketStream<...>, &str, &str, &str)` — Task 5 定義・使用。✓
- `IrcMessage.emote_ranges: Vec<(usize, usize)>` — Task 4 定義、Task 5 で `&irc_message.emote_ranges` として使用。型一致。✓
