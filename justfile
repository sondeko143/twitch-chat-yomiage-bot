# justfile — プロジェクトのタスクランナー (https://just.systems)
# 実行: just <recipe>        例) just check / just ci
#       just --list          レシピ一覧
# 導入: cargo install just   (or winget install Casey.Just / scoop install just)
#
# 注意: Windows には sh が無いため、レシピは cmd 経由で実行する。
set windows-shell := ["cmd.exe", "/c"]

# 既定: レシピ一覧を表示
default:
    @just --list

# フォーマット適用（コードを書き換える）
fmt:
    cargo fmt --all

# フォーマット検査（書き換えず、差分があれば失敗。CI/ゲート用）
fmt-check:
    cargo fmt --all -- --check

# clippy 静的解析（警告をエラー扱い。clippy.toml の複雑度閾値も適用）
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# テスト実行
test:
    cargo test --workspace

# 依存監査: 脆弱性・ライセンス・重複・取得元 (cargo-deny / deny.toml)
deny:
    cargo deny check

# 依存監査: 脆弱性のみ (cargo-audit / RustSec, .cargo/audit.toml)
audit:
    cargo audit

# 一括チェック（整形検査 + clippy + テスト）— 開発時用
check: fmt-check clippy test

# フルゲート（check + 依存監査）— コミット前 / CI 用
ci: fmt-check clippy test deny audit
