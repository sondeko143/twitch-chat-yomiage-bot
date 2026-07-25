# 0015. 接続プロファイルを vstc_cli 専用の単一 profiles.toml に保存する

- Status: Proposed
- Date: 2026-07-26
- Related: [vstc_cli プロファイル spec](../superpowers/specs/2026-07-26-vstc-cli-profiles-design.md), [ADR-0013](0013-config-secret-in-os-standard-user-dir.md), [ADR-0012](0012-persist-gui-state-via-eframe-storage.md)

## Context

宛先（host / port）とリロード設定パスを名前付きで永続化する必要が出た。保存するのは接続先の座標だけで、**秘密は含まない**（ADR-0013 が扱った `client_secret` やトークンとは性質が違う）。

このワークスペースには既に 2 つの永続化先がある。`tcyb` は OS 標準ユーザー設定ディレクトリの `config.toml`（ADR-0013）、`vstc_gui` は eframe Storage（ADR-0012）。新しい保存先を足すか、どちらかに相乗りするかを決める必要があった。

書き込みは `profile set` 実行のたびに発生し、1 ファイルに全プロファイルが入る以上、書き込み中の中断が全件消失につながりうる点も考慮に入れた。

## Decision

`directories::ProjectDirs::from("", "", "vstc")` が返す OS 標準ユーザー設定ディレクトリ配下の**単一ファイル `profiles.toml`** に、全プロファイルを保存する。ADR-0013 と同じく `tcyb` の `TCYB_CONFIG_DIR` に倣い、環境変数 `VSTC_CONFIG_DIR` で保存先ディレクトリを上書きできるようにする。

`tcyb` とはアプリケーション名を分け（`tcyb` ではなく `vstc`）、`tcyb` の `config.toml` には相乗りしない。書き込みは一時ファイルへ書いてから rename する形で原子的に行う。プロファイル本体は全フィールドが省略可能な形で表現し、未設定のフィールドはファイルに書き出さない。

## Alternatives rejected

- **プロファイルごとに 1 ファイル（`profiles/<name>.toml`）** — 個別の削除・コピー・原子的更新は素直になるが、`list` がディレクトリ走査になり、ファイル名とプロファイル名の対応（大文字小文字・パス不正文字・拡張子違いのゴミファイル）を管理し続ける責務が増える。プロファイル数はせいぜい数個で、全件を 1 度に読む方が実態に合う。
- **`tcyb` の `config.toml` へ相乗り** — 新しい保存先を増やさずに済むが、`vstc_cli` は `tcyb` に依存しない独立クレートであり、`tcyb` の設定ファイル（秘密を含む・不在なら起動を止める）を CLI クライアントが読み書きするのは依存方向として逆行する。`tcyb` 未使用の環境で `vstc_cli` だけ使う場合も破綻する。
- **`vstc_gui` と同じ eframe Storage** — GUI との共有は魅力的だが、eframe は GUI フレームワークであり CLI に GUI 依存を持ち込むことになる。保存形式も JSON blob で手編集に向かず、「設定ファイルを直接開いて直せる」という CLI の期待に反する。
- **カレントディレクトリの `.vstc.toml`** — 手編集は最も容易だが、ADR-0013 で CWD 依存を明示的に捨てた方針に逆行し、起動ディレクトリで挙動が変わる問題が再発する。

## Consequences

`profile list` が 1 度のファイル読み込みで完結し、全プロファイルの一括バックアップ・手編集・別マシンへのコピーがファイル 1 つで済む。`VSTC_CONFIG_DIR` があるためテストは実ユーザー環境を汚さずに一時ディレクトリで完結できる。

一方、単一ファイルなので破損は全件に及ぶ。これは temp→rename の原子的書き込みで中断耐性を確保するが、TOML として壊れた手編集からは回復できない（その場合はファイル削除＝全プロファイル再作成になる）。また保存先が `tcyb` と別ディレクトリになるため、両方を使う利用者は設定ディレクトリを 2 箇所把握することになる。将来 `vstc_gui` もプロファイルを持つなら、GUI 側の eframe Storage と本ファイルのどちらを正とするかを改めて決める必要がある。
