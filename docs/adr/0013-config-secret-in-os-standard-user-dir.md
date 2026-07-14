# 0013. 秘密・設定・トークンを OS 標準ユーザーディレクトリに平文で置く

- Status: Accepted
- Date: 2026-07-14
- Related: [config OS 標準化 spec](../superpowers/specs/2026-07-14-config-os-standard-location-design.md)

## Context

`client_secret` を含む秘密・設定は CWD の `.env` から平文で読まれ（`dotenvy::dotenv()` + `config` の `cb_` プレフィックス env）、トークン DB（`db/data.json`）も CWD 相対に置かれていた。これにより (1) 起動ディレクトリに挙動が縛られ、(2) 平文の秘密がリポジトリ作業ツリー内に同居して誤コミット・履歴混入の余地が残っていた。保存場所と保護水準を決める必要があった。改善動機として明示されたのは「CWD 依存の解消」と「git/履歴へのリーク防止」で、**at-rest 暗号化は要求されていない**。

## Decision

秘密・設定・トークン DB のデフォルト保存先を、リポジトリ作業ツリーの外にある **OS 標準ユーザーディレクトリ**（`directories` クレートで解決。Windows は Roaming AppData 配下）へ移す。ファイルは**平文のまま**とし、暗号化や資格情報ストアは導入しない。CWD の `.env` 自動読み込み（`dotenvy::dotenv()`）は廃止する。shell 環境変数（`cb_` プレフィックス）によるオーバーライドと `--config <path>` 明示指定は上書き層として維持する。設定ファイル不在の初回起動時はプレースホルダ入りテンプレを OS 標準パスに生成し、絶対パスと編集案内を表示して終了する。

## Alternatives rejected

- **OS 資格情報ストア（keyring / Windows Credential Manager）** — at-rest 暗号化が得られるが、それは今回の動機に含まれない。`keyring` 依存の追加、Linux/headless バックエンドの摩擦、格納/取り出しの一手間という複雑さが、要求に対して過剰。
- **最小移動（`.env` のまま読み先だけ固定 OS パスへ）** — 2 動機は満たすが、`.env` と env プレフィックスの間接指定という古いメンタルモデルを温存する。実ファイルの `config.toml` に寄せる方が構成として素直。
- **CWD 依存の据え置き（`.gitignore` のみで防御）** — リポジトリ作業ツリー内に平文秘密が残り、誤コミットが構造的には可能なまま。動機を満たさない。

## Consequences

秘密ファイルが作業ツリー外に出るため、`git add` / gitleaks / check-env-leak の対象範囲から構造的に外れ、CWD に依存せず起動できる。一方、平文のまま（暗号化しない）なので、同一マシンの他プロセス/他ユーザーからの読み取りは OS のファイル権限に委ねる。既存利用者は `.env` / `db/data.json` を新パスへ移すか再認証する移行が一度必要になる。将来 at-rest 暗号化が要件化したら、本 ADR を supersede して keyring 案を再評価する。
