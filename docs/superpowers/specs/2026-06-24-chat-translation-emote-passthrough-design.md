# chat 翻訳での Twitch emote パススルー — 設計

- 日付: 2026-06-24
- 対象コンポーネント: `tcyb` の IRC チャット翻訳（`tcyb/src/irc.rs`）

## 背景・目的

`tcyb` の `ReadChat` コマンドは、視聴者のチャットを外部翻訳コマンドに渡し、その結果を
`@reply-parent-msg-id=...` 付きでチャットへ返信する（`tcyb/src/irc.rs` の `process_message`）。

現状はメッセージ本文をそのまま翻訳コマンドへ渡している。本文に Twitch の emote
（例: `DinoDance`）が含まれると、翻訳器がその単語まで翻訳しようとし、結果が不自然になる。

目的: **翻訳クエリから Twitch ネイティブ emote を除外し、翻訳結果に emote を連結して返信する。**

## 実装場所の判断

この chatbot（`tcyb`）側で実装する。理由:

1. **判定情報の所在**: emote の正確な位置は IRC メッセージの `emotes` タグにしか存在しない。
   外部翻訳コマンドへはプレーンテキスト（`tcyb/src/irc.rs` の `chat_msg`）しか渡っておらず、
   `emotes` タグは現状 `parse_message` 内で `id` のみ抽出して破棄している。外部側で実装すると
   文字列マッチによる推測になり誤検知・取りこぼしが避けられない。
2. **関心の分離**: 「Twitch の emote とは何か」は Twitch プラットフォーム固有の関心事であり、
   翻訳器の責務ではない。`tcyb` 側に閉じ込めれば翻訳コマンドは汎用・差し替え可能なまま保てる。
3. **整形の所在**: 返信メッセージ（`PRIVMSG`）を組み立てているのは `tcyb` 側であり、emote を
   どこに連結するかという整形ももともと `tcyb` の管轄。

## スコープ

### 対象
- `ReadChat` → IRC 経由のチャット翻訳（`tcyb/src/irc.rs` の `process_message`）のみ。
- 対象とする emote は **Twitch ネイティブ emote のみ**（サブスク / グローバル / bits emote）。
  これらは IRC の `emotes` タグに位置情報が必ず入る。`DinoDance` はグローバル emote なので含まれる。

### 対象外
- TTS（`send_chat_message_to_speak`）は従来通り元メッセージ全文を読み上げる。
- 外部翻訳コマンドのインターフェースは変更しない（プレーンテキスト 1 引数のまま）。
- サードパーティ emote（BTTV / FFZ / 7TV）。これらは `emotes` タグに入らないため対象外。

## データフロー

1. **タグの保持**: `parse_message` で IRC タグの `emotes` を読み取り、`(start, end)` の
   コードポイント位置範囲リストへパースして `IrcMessage.emote_ranges: Vec<(usize, usize)>` に保持する。
   `emotes` タグが空（`emotes=`）または無い場合は空 `Vec`。

2. **分割**: 純粋関数 `split_message_emotes(chat_msg, &ranges) -> (cleaned: String, emotes: Vec<String>)`
   で、本文を「emote を除いた翻訳用テキスト（`cleaned`）」と「出現順の emote 文字列リスト（`emotes`）」に分割する。
   - 切り出しは **コードポイント単位**（`chars()` ベース）で行う。`emotes` タグの位置はバイトではなく
     コードポイントオフセットのため。
   - emote を除去して生じた余分な空白は詰める（連続空白を 1 個に、前後をトリム）。

3. **返信本文の決定**（`process_message` 内）:
   - `cleaned` が空（emote のみのメッセージ）→ 翻訳コマンドを呼ばず、返信本文 = emote を空白区切りで連結したもの。
   - それ以外 → `cleaned` を翻訳コマンドへ渡す。
     - stdout が非空 → 返信本文 = `"{translated} {emotes...}"`（emote を **末尾に**空白区切りで追加。
       emote が無ければ `translated` のみ）。
     - stdout が空 → 従来通り返信しない（emote も付けない）。

返信の送信処理（`@reply-parent-msg-id=...` の組み立てと `ws_stream.send`）は既存のものを再利用する。

## 関数分割

テスト容易性とネットワーク非依存性のため、ロジックを純粋関数に切り出す:

- `parse_emote_ranges(tag_value: &str) -> Vec<(usize, usize)>`
  - `emotes` タグの値（例: `25:0-4,12-16/1902:6-10`）のみを受け取り、全 emote の全出現範囲を
    `(start, end)` のリストで返す。`end` は包含（inclusive）。
  - 空文字列 → `vec![]`。
- `split_message_emotes(chat_msg: &str, ranges: &[(usize, usize)]) -> (String, Vec<String>)`
  - ネットワーク非依存。コードポイント単位で `cleaned` と `emotes` を構築。
- `process_message` は上記を呼び出して返信本文を組み立てる薄い層に保つ。

`parse_message` は `emotes` タグを `parse_emote_ranges` でパースして `IrcMessage.emote_ranges` に格納する。

## 後方互換性

- emote を含まない（`emotes=`）メッセージでは `cleaned` == 本文全文、`emotes` 空となり、
  返信は従来と完全に同一（翻訳全文をそのまま返信）。

## 既知の制限

- Twitch の emote 位置は BMP（基本多言語面）の文字では正しく、絵文字など astral plane の文字が
  混在する場合に位置がズレ得る。ただし Twitch ネイティブ emote + 日本語の通常ケースでは問題ない。
- 本文に文章がありつつ翻訳結果（stdout）が空になった場合は emote も付けず無返信（既存挙動の踏襲）。

## テスト

`tcyb/src/irc.rs` の `#[cfg(test)] mod tests` に追加する。

### `parse_emote_ranges`
- 空文字列 → `[]`
- 単一: `25:0-4` → `[(0, 4)]`
- 同一 emote 複数出現: `25:0-4,12-16` → `[(0, 4), (12, 16)]`
- 複数 emote: `25:0-4/1902:6-10` → `[(0, 4), (6, 10)]`

### `split_message_emotes`
- 末尾 emote: `"Hello DinoDance"` / `[(6, 14)]` → (`"Hello"`, `["DinoDance"]`)
- 文中 emote（空白詰め）: `"a Kappa b"` / `[(2, 6)]` → (`"a b"`, `["Kappa"]`)
- 日本語 + emote（コードポイント検証）: `"こんにちは DinoDance"` / `[(6, 14)]`
  （`こんにちは ` で 6 コードポイント、`DinoDance` が 6-14）→ (`"こんにちは"`, `["DinoDance"]`)。
  バイトオフセットで切ると壊れることの検証。
- emote のみ: `"DinoDance"` / `[(0, 8)]` → (`""`, `["DinoDance"]`)
- 同一 emote 複数: `"Kappa hi Kappa"` → (`"hi"`, `["Kappa", "Kappa"]`)

### 既存テスト
- `parse_smile_emoji_message`（`emotes=` 空ケース）が引き続き通ること = 後方互換性の確認。
