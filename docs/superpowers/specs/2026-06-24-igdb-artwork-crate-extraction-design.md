# artwork/IGDB を `igdb` クレートへ切り出す — spec

- 日付: 2026-06-24 ／ 状態: 実装済み
- 関連 ADR: [0002 igdb を独立クレート化](../../adr/0002-extract-igdb-as-separate-crate.md), [0003 auth_headers を複製](../../adr/0003-duplicate-auth-headers.md)

## 問題

`tcyb`（TTS ボット）に、IGDB API を叩く artwork 機能（ゲームサムネイル生成）が同居し、重い `image` 依存とその依存ツリーを本体バイナリに引き込んでいる。artwork はチャット読み上げと無関係。

## ゴール

artwork/IGDB を独立クレート `igdb` に切り出し、ボット本体（`tcyb`）の依存（特に `image`）を軽くする。認証は同じ Twitch アプリを共有し続ける。

## 非ゴール

- artwork の機能・出力（`thumbnails.jpg`）の挙動変更。移設のみ。
- Twitch アプリの再登録（同じ `client_id`/`client_secret` を共有）。
- `tcyb` の Twitch 機能（読み上げ・認証・モデレーション等）のロジック変更。
- 共有 auth 用の独立クレート新設（複製方針、[0003](../../adr/0003-duplicate-auth-headers.md)）。

## 受入基準

- [ ] `cargo build -p igdb` が通る。
- [ ] `cargo build`（ワークスペース全体）が通る。
- [ ] `cargo tree -p tcyb` に `image` が現れない（本体バイナリから image 依存ツリーが消えた）。
- [ ] `.env` に `client_id`/`client_secret` がある状態で `igdb get-artwork <ゲーム名>` を実行すると、移設前の `tcyb get-artwork` と同じ挙動でカレントディレクトリに `thumbnails.jpg` が生成される。
- [ ] `tcyb` から `GetArtwork` サブコマンドが無くなっている。
