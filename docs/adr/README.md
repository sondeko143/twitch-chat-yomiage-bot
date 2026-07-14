# Architecture Decision Records (ADR)

このプロジェクトの「決定と却下根拠」を隔離する不変ログ。

## 3-doc モデル

1 つの変更は 3 種のドキュメントに分かれる。ADR はその中央層。

- **spec（薄・不変寄り）** — 何を・なぜ・非ゴール・受入基準。実装で解決すると陳腐化しにくい記述に絞る。置き場: [`docs/superpowers/specs/`](../superpowers/specs/)。
- **ADR（不変）** — 決定と却下根拠。同期し続けず、覆すときは新 ADR で supersede する。置き場: このディレクトリ。
- **plan（使い捨て）** — 手順・影響ファイル・テスト。実装後すぐ陳腐化する前提で参照・同期しない。置き場: [`docs/superpowers/plans/`](../superpowers/plans/)。

恒久価値のある「決定＋却下根拠」を plan や spec に混ぜると、日付付きスナップショットの中で陳腐化し再調査コストを生む。ADR に隔離してそれを防ぐ。

## いつ ADR を起こすか

spec 承認後、plan の手前で毎回判定する。1 つでも該当したら決定層あり＝ADR を起こす。

- 検討して**却下した代替案**がある。
- **後戻りしにくい**選択をした（依存追加・データ形式・エンドポイント・永続化スキーマ）。
- **境界/契約**を変えた（公開 interface・モジュール依存方向・プロトコル）。
- 既存の決定を**覆す/変更**する（→ supersede）。
- 非自明なトレードオフを伴う既定値・閾値・タイムアウトを決めた。

該当が無ければ ADR は作らず、plan 冒頭に `ADR: none — 決定層なし` と 1 行残す。

## 起票手順

1. 採番は `docs/adr/` 直下の既存最大番号 +1（4 桁ゼロ埋め）。
2. ファイル名は `NNNN-kebab-title.md`（ascii kebab。H1 見出しは決定を一行で表す命令形、日本語可）。
3. 本文は [`template.md`](template.md) に従う。**「Alternatives rejected」節は必須**（却下案が無いなら「なし（唯一の現実解）」と書く）。
4. Status は起票時 `Accepted`（提案段階なら `Proposed`）。
5. 下の索引表に 1 行追記する。

## 覆すとき（supersede）

旧 ADR は書き換えず、Status を `Superseded by [ADR-NNNN](NNNN-title.md)` に変える 1 行だけ更新する。新 ADR を起こし `Related` に旧 ADR を書き、Context で「なぜ旧決定を覆すか」を述べる。

## 索引

| ADR | 決定 | Status | 日付 | 関連 spec |
|-----|------|--------|------|-----------|
| [0001](0001-emote-exclusion-on-tcyb-side.md) | chat 翻訳の Twitch emote 除外を tcyb 側で行う | Accepted | 2026-06-24 | [emote パススルー](../superpowers/specs/2026-06-24-chat-translation-emote-passthrough-design.md) |
| [0002](0002-extract-igdb-as-separate-crate.md) | artwork/IGDB を独立クレート `igdb` へ切り出す | Accepted | 2026-06-24 | [igdb 抽出](../superpowers/specs/2026-06-24-igdb-artwork-crate-extraction-design.md) |
| [0003](0003-duplicate-auth-headers.md) | 共有 `auth_headers` は複製する（共有クレートを作らない） | Accepted | 2026-06-24 | [igdb 抽出](../superpowers/specs/2026-06-24-igdb-artwork-crate-extraction-design.md) |
| [0004](0004-modernize-http-stack-to-hyper-1x.md) | HTTP/gRPC スタックを hyper 1.x へフル近代化する | Accepted | 2026-06-24 | [近代化](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md) |
| [0005](0005-keep-native-tls-backend.md) | TLS バックエンドは native-tls を据え置く | Accepted | 2026-06-24 | [近代化](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md) |
| [0006](0006-pin-protos-by-tag.md) | `vstreamer_protos` を semver タグ v0.1.2 で固定参照する | Accepted | 2026-06-24 | [近代化](../superpowers/specs/2026-06-24-reqwest-axum-hyper-tonic-modernization-design.md) |
| [0007](0007-wall-clock-tracing-for-startup-profiling.md) | 起動レイテンシは wall-clock tracing スパンで計測する | Accepted | 2026-06-25 | [起動計測](../superpowers/specs/2026-06-25-startup-latency-profiling-design.md) |
| [0008](0008-always-on-spans-feature-gated-subscriber.md) | 計装スパンは常時コンパイルし計測サブスクライバのみ feature 隔離する | Accepted | 2026-06-25 | [起動計測](../superpowers/specs/2026-06-25-startup-latency-profiling-design.md) |
| [0009](0009-add-process-routes-entrypoint-to-vstc.md) | GUI 用に `vstc` へ構造化エントリポイント `process_routes` を追加する | Accepted | 2026-06-26 | [vstc_gui](../superpowers/specs/2026-06-26-vstc-gui-client-design.md) |
| [0010](0010-flat-param-catalog-for-gui.md) | GUI パラメーターは全コマンド共通のフラット静的カタログで表す | Accepted | 2026-06-26 | [vstc_gui](../superpowers/specs/2026-06-26-vstc-gui-client-design.md) |
| [0011](0011-disabled-step-as-forward-route.md) | 無効パイプラインステップは FORWARD route に置換する | Accepted | 2026-06-26 | [vstc_gui](../superpowers/specs/2026-06-26-vstc-gui-client-design.md) |
| [0012](0012-persist-gui-state-via-eframe-storage.md) | GUI 状態は eframe 標準 Storage で OS ユーザー単位に永続化する | Accepted | 2026-06-26 | [vstc_gui](../superpowers/specs/2026-06-26-vstc-gui-client-design.md) |
| [0013](0013-config-secret-in-os-standard-user-dir.md) | 秘密・設定・トークンを OS 標準ユーザーディレクトリに平文で置く | Accepted | 2026-07-14 | [config OS 標準化](../superpowers/specs/2026-07-14-config-os-standard-location-design.md) |
