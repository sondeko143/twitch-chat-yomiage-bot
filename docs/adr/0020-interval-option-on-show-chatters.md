# 0020. 周期取得は show-chatters の `--interval` で表し、新サブコマンドも設定キーも作らない

- Status: Accepted
- Date: 2026-08-09
- Related: [show-chatters 常駐 spec](../superpowers/specs/2026-08-09-tcyb-show-chatters-daemon-design.md), [ADR-0013](0013-config-secret-in-os-standard-user-dir.md), [ADR-0019](0019-foreground-only-daemon-for-show-chatters.md)

## Context

`show-chatters` の周期取得を CLI でどう表すかには 3 通りある。既存サブコマンドへのオプション追加、`watch-chatters` のような新サブコマンド、`config.toml` の設定キーである。

tcyb の CLI はサブコマンド 7 個から成り、いずれも「1 つの操作」を表す。周期取得は `show-chatters` と同じ操作を繰り返すだけで、認証・チャンネル解決・出力形式のすべてが一致する。

ADR-0013 で、秘密と設定は OS 標準ユーザーディレクトリの `config.toml` に集約している。ここに置かれるのは「マシンごとに一度決める値」である。

## Decision

`show-chatters` に `--interval <SECS>` を追加する。

- 省略時は従来どおり 1 回取得して終了する（後方互換）。
- `<SECS>` は 1 以上の整数のみ受け付け、0 は clap の値検証で弾く。
- 間隔は `config.toml` には置かない。

## Alternatives rejected

- **`watch-chatters` 新サブコマンドを立てる** — 出力形式・認証・チャンネル解決が `show-chatters` と完全に同一で、違いは繰り返すかどうかだけ。サブコマンドを分けると同じ説明を README とヘルプの二箇所に書くことになり、片方だけが古くなる。
- **`config.toml` に間隔キーを置く** — 間隔は「今回はどの粒度で記録したいか」という実行ごとの判断であって、マシンごとに一度決める値ではない。一発実行に戻すたびに設定ファイルを編集することになり、ADR-0013 が定めた config の役割とも合わない。
- **単位付き文字列（`30s` / `1m` など）で受ける** — パーサと単位解釈が増える割に、既存 CLI に単位付き引数の前例が無い。秒固定で足りる。
- **0 を「間隔を空けずに連続ポーリング」と解釈する** — タイプミス一つで Twitch のレート制限を踏み抜く操作が起きる。値検証で弾く方が安全。

## Consequences

- `show-chatters` のヘルプに 1 行増えるだけで、既存の呼び出しは影響を受けない。
- 一発実行と常駐が同じコードパスを通るため、出力形式を変えたときに両者へ自動的に効く。
- 他のサブコマンド（`read-chat` など）に周期実行が要るようになっても、この決定はそこへは及ばない。必要になった時点で別途判断する。

