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

# profiling 経路のテスト（既定 test は feature 無効のため別途）
test-profiling:
    cargo test -p tcyb --features profiling

# profiling 経路の clippy（既定 clippy は feature 無効のため別途）
clippy-profiling:
    cargo clippy -p tcyb --features profiling --all-targets -- -D warnings

# 起動レイテンシ計測: profiling ビルドで read-chat を実行し、両接続確立で自動終了。
# 出力: target/profile/trace.json (→ ui.perfetto.dev) と target/profile/flame.svg。
# 事前に: cargo install inferno / 有効な認証(.env, `tcyb auth-code` 済み)。
profile-startup:
    cargo run -p tcyb --release --features profiling -- read-chat
    inferno-flamegraph target/profile/tracing.folded > target/profile/flame.svg

# profile-startup が出した Chrome trace を解析（span 別実時間・タイムライン・no-span gap）
analyze-trace path="target/profile/trace.json":
    cargo run -p xtask -- analyze-trace {{path}}

# 依存監査: 脆弱性・ライセンス・重複・取得元 (cargo-deny / deny.toml)
deny:
    cargo deny check

# 依存監査: 脆弱性のみ (cargo-audit / RustSec, .cargo/audit.toml)
audit:
    cargo audit

# 一括チェック（整形検査 + clippy + テスト）— 開発時用
check: fmt-check clippy clippy-profiling test test-profiling

# フルゲート（check + 依存監査）— コミット前 / CI 用
ci: fmt-check clippy clippy-profiling test test-profiling deny audit
