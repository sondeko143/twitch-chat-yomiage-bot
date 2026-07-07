# CLAUDE.md

Twitch チャット読み上げ bot。Rust ワークスペース（`tcyb` = 本体 / `vstc` = client lib / `vstc_cli` / `igdb`）。

## 品質ゲート — PR 作成・main マージ前に必須

**PR を作成する前、および main へマージする前に、必ず `just ci` を実行して全緑（exit code 0）を確認すること。** 赤のまま PR/マージしない。

```
just ci      # fmt-check + clippy + test + check-env-leak + gitleaks + deny + audit（フルゲート）
just check   # fmt-check + clippy + test + check-env-leak（外部ツール監査を省いた開発中の高速版）
```

対応方針:

- **fmt-check の赤** → `just fmt` で機械的に整形して解消してよい（`cargo fmt`、`git` で復元可）。
- **clippy / test / deny の赤** → 機械的に黙らせない。原因を調べて修正するか、判断が要るものは相談する。設定や lint スコープを緩めてゲートを通すのは禁止。
- **check-env-leak の赤** → plan/spec 等に個人/マシン依存の絶対パス（ホームディレクトリ・AppData・Claude 内部メモリパス等）が混入している。相対パスやプレースホルダへ置換して解消する（検出器は [xtask/src/leak.rs](xtask/src/leak.rs)、単体実行は `just check-env-leak`）。
- **gitleaks の赤** → コミット履歴に本物のシークレット（標準ルール群）か、カスタムルールの private IP（RFC1918）/ AmiVoice appkey が入った。**allowlist で黙らせない**。値をローテーション（無効化・再発行）し、追跡から外して履歴からも除去する。設定は [.gitleaks.toml](.gitleaks.toml)、単体実行は `just gitleaks`。走査対象はコミット済み内容のみで、gitignore 済みのローカル機密（`.env.*` / `db/**` / `target/**`）は対象外。要 `gitleaks`（`scoop install gitleaks`）。なお個人/マシン依存の絶対パス検出は [check-env-leak](xtask/src/leak.rs)（作業ツリー走査）が担当（履歴の既存パスを再フラグしないための役割分担）。
- `just` 未導入の環境なら `cargo install just`。

### pre-commit フック（leak ゲート）

- コミット前に leak 系ゲートを走らせる pre-commit フックを [.githooks/pre-commit](.githooks/pre-commit) に用意済み。中身は (1) `check-env-leak`（環境固有パス・作業ツリー）＋ (2) `gitleaks --staged`（シークレット/IP/appkey・ステージ済み差分）。
- **clone 後に一度 `just setup-hooks`**（= `git config core.hooksPath .githooks`）を実行して有効化する。緊急回避は `git commit --no-verify`（多用しない。最終ゲートは `just ci`）。
- フックは git 同梱 sh で動くため Windows でも追加導入不要。コミット時は実行ビット保持のため `git add --chmod=+x .githooks/pre-commit` を推奨。

### 既知の非ブロッキング警告

- cargo-audit が `yaml-rust` unmaintained（RUSTSEC-2024-0320）を警告するが、推移的依存のため allowed warning 扱いで `just ci` は通る。対応は任意。（advisory-db は随時更新されるため、この一覧は例示。現況は必ず `just audit` / `cargo deny check advisories` を再実行して確認する。）

### 保留中の脆弱性 advisory（ignore 登録済み）

- **quick-xml 0.39.4 の DoS 2件（RUSTSEC-2026-0194 / 0195・高7.5）を [deny.toml](deny.toml) と [.cargo/audit.toml](.cargo/audit.toml) の両方で ID 付き ignore に登録**して `just ci` を通している（両ゲートは別リストなので片方だけだと残り一方が赤）。`wayland-scanner`（build-time proc-macro・Linux/Wayland 専用）経由の推移的依存で、最新の wayland-scanner が依然 `quick-xml ^0.39` を要求（上流未対応）のため更新不可。Windows 標的では非コンパイル・信頼済みローカル XML のみ解析で到達不能と評価。**解除トリガ:** `eframe`/`wayland-scanner` の更新で `quick-xml >= 0.41` へ上げられるようになったら両ファイルから ID を削除し再 audit する。トリアージ手順は skill `triaging-cargo-audit` を参照。

## 環境メモ

- Windows 前提。`sh` が PATH に無いため、justfile は `set windows-shell := ["cmd.exe","/c"]` を指定済み（これが無いと just はレシピを実行できない）。
- 複雑度メトリクスは [clippy.toml](clippy.toml) の閾値 + 各クレートの `[workspace.lints]` 配線により、`cargo clippy`（= `just clippy`）実行時に常時チェックされる。
- 依存監査の設定は [deny.toml](deny.toml)（cargo-deny）と `.cargo/audit.toml`（cargo-audit）。
