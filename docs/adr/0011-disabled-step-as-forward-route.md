# 0011. 無効パイプラインステップは FORWARD route に置換する

- Status: Accepted
- Date: 2026-06-26
- Related: [vstc_gui spec](../superpowers/specs/2026-06-26-vstc-gui-client-design.md)

## Context

GUI の各パイプラインステップに有効/無効トグル（`enabled: bool`、既定 true）を追加した。用途は「普段は日本語→tts のみ、英語入力時だけ翻訳ステップを一時有効化」等、削除/再追加せずに切り替えること。チェーンは「route」であり各 hop は `remote` で実行され次 hop へ転送される。無効ステップを送信時にどう扱うか決める必要があった。

## Decision

無効ステップは送信時にスキップせず `FORWARD`（転送）route に置換する（`remote` は保持、`queries` は空）。`FORWARD` は params を使わないので無効ステップは検証しない（未完成でも送信をブロックしない）。ステップが 1 件も無いときのみ送信をブロックし、全ステップ無効（全 `FORWARD`）は有効な route として送信を許可する。エラー時のステップ番号は表示順に一致させる。

## Alternatives rejected

- **無効ステップを単純にスキップする** — チェーンが詰まって route（`remote` の並び・先頭 hop）のトポロジが変わってしまう。転送に置換すればトポロジを保ったままその step の処理だけバイパスできる。

## Consequences

無効化してもチェーンの route トポロジ（各 hop の `remote`）が保たれる。無効ステップは検証されないので事前設定して OFF にしておける。永続化は `enabled` を `#[serde(default = "default_enabled")]`（true を返す）にして後方互換とし、`enabled` フィールドが無い旧 persisted state も全ステップ有効として読み込める。
