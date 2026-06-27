# vstc_gui — vstreamer クライアント GUI 設計

- 日付: 2026-06-26
- ステータス: 承認済み（実装計画へ）
- 関連: `vstc`（client lib）, `vstc_cli`（CLI）, `vstreamer_protos`（proto, tag v0.1.2）

## 1. 目的・背景

`vstreamer-tool`（commander サーバー）へ gRPC で `Command` を送る GUI クライアントを
新しいワークスペースメンバ `vstc_gui` として追加する。既存の CLI(`vstc_cli`) と同じく
`vstc` ライブラリを再利用して送信処理を行う。

要件:

1. 送信するテキストを入力できる。
2. 送信先パイプラインを 1 ステップずつ入力できる（宛先 / コマンド / パラメーター）。追加・削除可能。
3. ボタンを押して送信する。
4. ユーザー（OS ユーザー）ごとに前回の入力値が保存され、起動時に復元される。

## 2. データモデル対応（proto）

proto の構造に各 UI 要素を対応させる。

- `Command { chains: Vec<OperationChain>, operand: Operand }`
- `OperationChain { operations: Vec<OperationRoute> }` ← 「パイプライン」= 1 本の chain
- `OperationRoute { operation, remote, queries: map<string,string> }` ← 「ステップ」
  - `operation` = コマンド
  - `remote` = 宛先
  - `queries` = パラメーター
- `Operand { text, sound, file_path, filters, trace_id, origin_ts }`
  - 本 GUI のスコープは **text のみ**（sound/file_path/filters は既定値）。

## 3. パラメーターカタログ（リファレンスなし入力の中核）

パラメーターのキー仕様はサーバー側 `vstreamer-tool/vspeech/shared_context.py` の `Params`
クラスが唯一の正。これは **全コマンド共通のフラットなスキーマ**（サーバーは任意の route で
任意のサブセットを受理する）であり、コマンド別ではない。完全な集合は次の 7 つ。

| key | label（日本語） | type | 許容値 / 備考 |
|-----|------|------|------|
| `t` | 翻訳先言語 | text | 言語コード（en, ja 等） |
| `s` | 翻訳元言語 | text | 言語コード |
| `p` | 位置 | enum | `s` / `n` |
| `i` | 話者ID | int | |
| `v` | 音量 | int | |
| `spd` | 速度 | float | |
| `pit` | ピッチ | float | |

送信時は **入力済み（非空）のキーのみ**を文字列化して `queries` に入れる
（`Params.to_pb()` の `{key: str(value) for ... if value is not None}` と同じ挙動）。

### 3.1 コマンド別 relevance マップ

「コマンド毎のラベル付きフィールド」を実現するため、コマンドごとに既定表示するキーを
小さなマップで持つ。relevance は README の用例とセマンティクスから導いた **ベストエフォート**。
relevance に無いキーも「その他のパラメーター」expander から常に入力可能（サーバーは任意キーを
受理するため、relevance の推定誤りが入力を制限することはない）。

| command | 既定表示フィールド |
|---------|------|
| TRANSLATE | t, s |
| TTS | i, spd, pit |
| PLAYBACK | v |
| SUBTITLE | p |
| VC | i |
| TRANSCRIBE / FORWARD / PAUSE / RESUME / RELOAD / SET_FILTERS / PING | （既定なし。7 キー全てが expander から利用可） |

### 3.2 カタログのデータ表現

```rust
enum ParamKind { Text, Int, Float, Enum(&'static [&'static str]) }
struct ParamSpec { key: &'static str, label: &'static str, kind: ParamKind }
```

7 件の静的テーブル（`ParamKind::Enum(&["s", "n"])` は `p` 用）＋ relevance マップ
（`Operation` → `&[&str]` キー集合）を `vstc_gui` 内の `catalog` モジュールに持つ。
将来サーバーがパラメーターを増やしたら、このテーブルに 1 件追加するだけで対応する。

## 4. UI 構成（単一ウィンドウ・egui/eframe）

- **接続先**: `host`（text）＋ `port`（number）。送信時に `http://{host}:{port}` を構築
  （CLI と同じ既定 `localhost:8080`）。
- **送信テキスト**: 複数行テキストボックス → `Operand.text`。
- **パイプライン**: 動的リスト。各行 = 1 ステップ:
  - **コマンド**: `Operation` enum のドロップダウン。
  - **宛先 (remote)**: 任意のテキスト（空 = 接続先と同じ）。
  - **パラメーター**: §3 のカタログ駆動ラベル付きフィールド（relevance 既定 ＋ 「その他」expander）。
    型に応じた入力（int/float はテキスト＋検証、`p` はドロップダウン）。
  - **削除** ボタン（行ごと）。
  - **＋ ステップ追加** ボタンで末尾に行を追加。
- **送信** ボタン ＋ **ステータス行**（idle / 送信中… / 成功 / エラーメッセージ）。

## 5. 状態と永続化（OS ユーザー単位・自動）

```rust
#[derive(Serialize, Deserialize, Default)]
struct AppState {
    host: String,
    port: u16,
    text: String,
    steps: Vec<PipelineStep>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PipelineStep {
    op: OpKind,                       // proto Operation を写したローカル enum
    remote: String,
    params: BTreeMap<String, String>, // key -> 現在の入力文字列（空/欠落 = 未設定）
}
```

- `OpKind` は `Operation`（`Serialize` を持たない）を写した `Copy` なローカル enum。
  `Display` ＋ proto 変換（`to_proto()` / `from_proto()`）を実装。
- 永続化は **eframe 標準の `Storage`**（`persistence` feature）を使う。
  `App::save` で `eframe::set_value(storage, eframe::APP_KEY, &state)`、起動時
  （`App::new`）に `eframe::get_value` で復元。保存先は OS ユーザーの app-data ディレクトリ
  （プラットフォーム規定）で、明示のファイル操作は不要。暗黙の 1 プロファイル / OS ユーザー。

## 6. 非同期送信（UI を止めない）

- アプリは `tokio` のマルチスレッド `Runtime` を保持する。
- **送信** 押下時: ステップ群から `Vec<OperationRoute>` を構築（`OpKind`→`Operation`、
  パラメーター → `queries`）、検証し、`runtime.spawn` で `vstc::process_routes(...)` を実行。
- 結果はチャネル（`std::sync::mpsc` または `oneshot`）で受け取り、毎フレーム
  ポーリングして `ctx.request_repaint()` で UI を更新。ステータス行に
  送信中 / 成功 / エラーを反映する。

## 7. `vstc` ライブラリ追加

GUI が URL 文字列を経由せず、また接続/送信ロジックを重複させないため、構造化された
エントリポイントを追加する。

```rust
pub async fn process_routes(
    uri: &str,
    routes: Vec<OperationRoute>,
    text: String,
) -> Result<Response, VstcError>;
```

- `routes` を 1 本の `OperationChain` に包み、text のみの `Operand` を構築
  （sound/file_path/filters は既定、`trace_id`=UUID、`origin_ts`=現在時刻は内部生成）。
- 既存の文字列ベース `process_command` はそのまま（CLI が利用）。`Operand` 構築は
  private な `build_operand(text)` に切り出して両者で共有する。

## 8. バリデーション・エラー処理

- パラメーター型エラー（int/float パース不可、`p` が `s`/`n` 以外）→ インライン表示し送信を
  ブロック、該当ステップを示す。pydantic（`Params`）と同じ受理基準。
- 送信失敗（`VstcError`: transport / status / parse）→ ステータス行に表示。
- 日本語フォント未検出 → 警告ログ ＋ 既定フォントへフォールバック（致命的でない）。

## 9. フォント（日本語表示）

egui の既定フォントは日本語グリフを含まないため、UI ラベル（宛先/コマンド等）と送信テキストの
日本語表示には CJK フォントが必須。本プロジェクトは Windows 前提のため、起動時に Windows 同梱の
日本語フォント（Yu Gothic UI / Meiryo / MS Gothic のうち最初に見つかったもの）を
`egui::FontDefinitions` に最優先ファミリとして読み込む。いずれも無ければ警告し既定フォントへ
フォールバックする。

## 10. テスト方針

- **TDD のユニットテスト**（表示に依存しない純粋ロジック）:
  - パラメーター値の検証・文字列化（型ごと、非空のみ `queries` 化）。
  - `OpKind` ↔ `Operation` 変換 ＋ `Display`。
  - route / `Command` 構築（`vstc` 側の純粋な `build_operand` を network 無しで検証）。
  - `AppState` の serde ラウンドトリップ（永続化スキーマの退行防止）。
- **手動確認**（`cargo run -p vstc_gui`）: 日本語が表示される / ステップ追加・削除 /
  再起動で前回状態が復元される / ローカルサーバーへ送信（無ければ接続エラーが綺麗に出る）。

## 11. 品質ゲート・リスク

- 新規依存は `just ci`（clippy / deny / audit）を通すこと。
- 主リスク: egui/フォント系クレートのライセンスが `deny.toml` の allowlist 外の可能性。
  対応は CLAUDE.md の方針に従い、原因を調査の上、**正当に必要なライセンスのみ**を
  allowlist に追記する（deny.toml 自身が想定する手順）。確認の上で行い、lint スコープを
  緩めて黙らせることはしない。
- egui は winit/glow を含む比較的大きな依存ツリーを足すが、デスクトップツールとして許容。
- 仕様/ドキュメントに個人・マシン依存の絶対パスを書かない（`check-env-leak` ゲート）。

## 12. スコープ外（YAGNI）

- sound（WAV）入力、file_path、filters の送信。
- アプリ内の名前付きプロファイル切替（OS ユーザー単位の暗黙 1 プロファイルのみ）。
- パラメーターの custom key/value 行（7 キーカタログが完全なため不要）。

## 13. 追補: ステップの有効/無効トグル（2026-06-26 追加）

各パイプラインステップに `enabled: bool`（既定 true）を追加し、ヘッダ行のチェックボックス
「有効」で個別に ON/OFF する。用途: 普段は日本語→tts のみ、英語入力時だけ翻訳ステップを
一時的に有効化する、等（削除/再追加せず切替）。

- **送信挙動**: 無効ステップは**スキップせず `FORWARD`（転送）route に置換**する（`remote` は保持、
  `queries` は空）。チェーンは「route」であり各 hop は `remote` で実行され次 hop へ転送される。
  単純スキップだとチェーンが詰まって route（remote の並び・先頭 hop）が変わってしまうため、
  転送に置換してトポロジを保ったままその step の処理だけバイパスする。`FORWARD` は params を
  使わないので無効ステップは検証しない（未完成でも送信をブロックしない）。エラー時のステップ
  番号は表示順に一致。
- **ガード**: パイプラインにステップが 1 件も無いときのみ「パイプラインにステップがありません」で
  送信をブロック。全ステップ無効（= 全 FORWARD）は有効な route なので送信を許可する。
- **永続化の後方互換**: `enabled` は `#[serde(default = "default_enabled")]`（true を返す）。
  `enabled` フィールドが無い旧 persisted state も全ステップ有効として読み込まれ、既存入力は失われない。
- UI: 無効ステップもフィールドは編集可能（事前設定して OFF にしておける）。
