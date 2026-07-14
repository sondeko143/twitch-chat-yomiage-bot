# 0001. chat 翻訳の Twitch emote 除外を tcyb（チャットボット）側で行う

- Status: Accepted
- Date: 2026-06-24
- Related: [emote パススルー spec](../superpowers/specs/2026-06-24-chat-translation-emote-passthrough-design.md)

## Context

`tcyb` の `ReadChat` は視聴者チャットを外部翻訳コマンドへ渡し、結果を `@reply-parent-msg-id` 付きで返信する。本文に Twitch emote（例 `DinoDance`）が混ざると翻訳器がその単語まで訳そうとし結果が不自然になる。emote の正確な位置は IRC メッセージの `emotes` タグにしか無く、外部翻訳コマンドへはプレーンテキストしか渡らない（現状 `emotes` タグは `id` のみ抽出して破棄）。emote 除外をどの層で行うかを決める必要があった。

## Decision

emote の検出・除外・返信への再連結を `tcyb` 側（`tcyb/src/irc.rs`）で行う。`emotes` タグを位置範囲へパースして保持し、純粋関数で「翻訳用テキスト」と「emote リスト」に分割、翻訳結果に emote を末尾連結して返信する。外部翻訳コマンドのインターフェース（プレーンテキスト 1 引数）は変えない。

## Alternatives rejected

- **外部翻訳コマンド側で emote を処理する** — 外部側にはプレーンテキストしか渡らず `emotes` タグの位置情報が無い。文字列マッチによる推測になり誤検知・取りこぼしが不可避。「Twitch の emote とは何か」は Twitch 固有の関心事であり翻訳器の責務でない。
- **`emotes` タグを従来どおり破棄し続ける** — 位置情報を捨てると tcyb 側でも正確な除外ができない。

## Consequences

翻訳コマンドは汎用・差し替え可能なまま保てる。返信整形（`PRIVMSG` 組み立て）は元々 tcyb の管轄なので凝集が高い。対象は Twitch ネイティブ emote（サブスク/グローバル/bits）のみで、`emotes` タグに載らないサードパーティ emote（BTTV/FFZ/7TV）は扱えない。emote 位置はコードポイント単位で切るため、astral plane 文字が混在すると位置がズレ得る（通常の日本語＋ネイティブ emote では問題ない）。
