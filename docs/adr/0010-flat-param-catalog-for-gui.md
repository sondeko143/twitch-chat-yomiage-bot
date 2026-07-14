# 0010. GUI パラメーターは全コマンド共通のフラット静的カタログで表す

- Status: Accepted
- Date: 2026-06-26
- Related: [vstc_gui spec](../superpowers/specs/2026-06-26-vstc-gui-client-design.md)

## Context

GUI はリファレンス無しでパラメーターを入力させる必要がある。パラメーターのキー仕様はサーバー側 `vstreamer-tool/vspeech/shared_context.py` の `Params` クラスが唯一の正で、これは**全コマンド共通のフラットなスキーマ**（サーバーは任意 route で任意サブセットを受理する）であり、コマンド別ではない。完全な集合は 7 キー（`t`/`s`/`p`/`i`/`v`/`spd`/`pit`）。

## Decision

7 キーの静的テーブル（`ParamSpec { key, label, kind }`）を `vstc_gui` の `catalog` モジュールに持つ。加えてコマンドごとに既定表示するキーの **ベストエフォート relevance マップ**を持つ。relevance に無いキーも「その他のパラメーター」expander から常に入力可能にする（サーバーが任意キーを受理するため relevance の推定誤りが入力を制限しない）。送信時は入力済み（非空）のキーのみ文字列化して `queries` に入れる（サーバー `Params.to_pb()` と同じ挙動）。

## Alternatives rejected

- **コマンド別の厳密なパラメータースキーマ** — サーバーは全コマンド共通のフラットスキーマで任意サブセットを受理するため、コマンド別に絞ると誤って入力を制限しうる。
- **relevance を厳密な仕様として扱う** — relevance は README の用例から導いた推定でありベストエフォート。誤りが入力を塞がないよう expander で 7 キーを常時開放する。

## Consequences

サーバーがパラメーターを増やしても静的テーブルに 1 件足すだけで対応できる。relevance マップは README 用例からの推定なので、正確さはサーバー実装に対して保証されない（既定表示の利便性のみ）。
