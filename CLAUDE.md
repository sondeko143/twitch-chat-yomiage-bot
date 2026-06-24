# CLAUDE.md

Twitch チャット読み上げ bot。Rust ワークスペース（`tcyb` = 本体 / `vstc` = client lib / `vstc_cli` / `igdb`）。

## 品質ゲート — PR 作成・main マージ前に必須

**PR を作成する前、および main へマージする前に、必ず `just ci` を実行して全緑（exit code 0）を確認すること。** 赤のまま PR/マージしない。

```
just ci      # fmt-check + clippy + test + deny + audit（フルゲート）
just check   # fmt-check + clippy + test（依存監査を省いた開発中の高速版）
```

対応方針:

- **fmt-check の赤** → `just fmt` で機械的に整形して解消してよい（`cargo fmt`、`git` で復元可）。
- **clippy / test / deny の赤** → 機械的に黙らせない。原因を調べて修正するか、判断が要るものは相談する。設定や lint スコープを緩めてゲートを通すのは禁止。
- `just` 未導入の環境なら `cargo install just`。

### 既知の非ブロッキング警告

- cargo-audit が `yaml-rust` unmaintained（RUSTSEC-2024-0320）を警告するが、推移的依存のため allowed warning 扱いで `just ci` は通る。対応は任意。

## 環境メモ

- Windows 前提。`sh` が PATH に無いため、justfile は `set windows-shell := ["cmd.exe","/c"]` を指定済み（これが無いと just はレシピを実行できない）。
- 複雑度メトリクスは [clippy.toml](clippy.toml) の閾値 + 各クレートの `[workspace.lints]` 配線により、`cargo clippy`（= `just clippy`）実行時に常時チェックされる。
- 依存監査の設定は [deny.toml](deny.toml)（cargo-deny）と `.cargo/audit.toml`（cargo-audit）。
