# vstc_gui — vstreamer クライアント GUI — spec

- 日付: 2026-06-26 ／ 状態: 実装済み
- 関連 ADR: [0009 process_routes 追加](../../adr/0009-add-process-routes-entrypoint-to-vstc.md), [0010 フラット param カタログ](../../adr/0010-flat-param-catalog-for-gui.md), [0011 無効ステップ→FORWARD](../../adr/0011-disabled-step-as-forward-route.md), [0012 eframe Storage 永続化](../../adr/0012-persist-gui-state-via-eframe-storage.md)

## 問題

`vstreamer-tool`（commander サーバー）へ gRPC で `Command` を送る GUI が無い。CLI（`vstc_cli`）はあるが、テキスト＋多段パイプライン＋パラメーターを対話的に組み立て、前回入力を復元できる GUI が欲しい。

## ゴール

`vstc` ライブラリを再利用する GUI クライアント `vstc_gui` を追加し、次を満たす:

1. 送信テキストを入力できる。
2. パイプラインを 1 ステップずつ（宛先 / コマンド / パラメーター）入力・追加・削除できる。
3. ボタンを押して送信できる。
4. OS ユーザーごとに前回入力が保存され、起動時に復元される。

## 非ゴール

- sound(WAV) / file_path / filters の送信（text のみ）。
- アプリ内の名前付きプロファイル切替（OS ユーザー単位の暗黙 1 プロファイルのみ、[0012](../../adr/0012-persist-gui-state-via-eframe-storage.md)）。
- パラメーターの custom key/value 行（7 キーカタログが完全なため不要）。

## 受入基準

- [ ] テキストとパイプライン（各ステップ 宛先/コマンド/パラメーター）を入力し、ボタンで送信できる。
- [ ] ステップの追加・削除ができる。
- [ ] パラメーターはコマンドごとに既定フィールドが表示され、「その他」から 7 キー全てを入力できる。入力済み（非空）のキーのみ送信される。
- [ ] パラメーター型エラー（int/float パース不可、`p` が `s`/`n` 以外）はインライン表示で送信をブロックし、該当ステップを示す。
- [ ] ステップを無効化しても送信でき、チェーンの route トポロジが保たれる（無効ステップは `FORWARD` に置換、[0011](../../adr/0011-disabled-step-as-forward-route.md)）。
- [ ] 再起動で前回の入力（接続先 / テキスト / ステップ）が復元される。`enabled` フィールドの無い旧保存状態も全ステップ有効として読める。
- [ ] 日本語が表示される（CJK フォント読込、未検出時は既定フォントへフォールバック）。
- [ ] 送信は UI をブロックせず、ステータス行に 送信中 / 成功 / エラー が反映される。
- [ ] `just ci` 全緑（egui/フォント系の新規依存が clippy/deny/audit を通る）。
