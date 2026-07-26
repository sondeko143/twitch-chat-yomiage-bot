# vstc_cli プロファイル既定チェーン Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**ADR:** [ADR-0018](../../adr/0018-profile-default-chains-in-a-single-command.md)（Proposed。Task 7 で Accepted へ昇格する）

**Spec:** [2026-07-26-vstc-cli-default-chains-design.md](../specs/2026-07-26-vstc-cli-default-chains-design.md)

**Goal:** `vstc_cli` のプロファイルに複数の操作チェーンを保存し、`send` の位置引数を省略したときに単一 gRPC コマンドでまとめて送れるようにする。

**Architecture:** `vstc` 側に (1) route 文字列のスキーム省略形を正規化する公開パーサ `parse_route` と (2) 複数チェーンを単一 `Command` に載せる送信口 `process_chains_with_operand` を足す。`vstc_cli` 側は `Profile` に `chains: Option<Vec<Vec<String>>>` を持たせ、`profile chains add/del/show` サブコマンドで編集し、`send` は「位置引数があればそれ、無ければ保存済みチェーン」を純関数で選んで送る。

**Tech Stack:** Rust 2021 / clap 4（derive）/ serde + toml 1.1 / tonic 0.14 / url 2 / anyhow / mockall（テスト）

## Global Constraints

- `just ci` を全緑（exit code 0）にしてから PR 作成・main マージする。開発中の高速版は `just check`。
- clippy は `cargo clippy --workspace --all-targets -- -D warnings`。ワークスペースで `cognitive_complexity` / `too_many_lines` を warn 化済み。閾値は cognitive 25 / lines 120 / arguments 7（[clippy.toml](../../../clippy.toml)）。lint を緩めてゲートを通すのは禁止。
- `vstc` クレートは `#![warn(missing_docs)]` と `#![warn(clippy::pedantic)]`。**公開する関数には doc コメントと、`Result` を返すなら `## Errors` 節が必須**。
- ユーザー向けメッセージ・エラー文言・コメントは日本語（既存コードの慣習に合わせる）。doc コメントは既存に合わせ、`vstc` は英語、`vstc_cli` は英語の要約＋日本語の補足という現状の混在をそのまま踏襲する。
- Windows 前提。`sh` が無いため `just` 経由でコマンドを実行する。
- ADR-0018 の本文は書き換えない。Status 行の昇格のみ Task 7 で行う。
- コミットメッセージ末尾に `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>` を付ける。
- 新しい依存クレートは追加しない（スキーム判定は手書き。正規表現を入れない）。

## File Structure

| ファイル | 責務 | 変更 |
|---|---|---|
| `vstc/src/lib.rs` | route 文字列の正規化・パース、proto `Command` の組み立て、gRPC 送信 | Modify |
| `vstc/tests/test.rs` | モックサーバ相手の結合テスト | Modify |
| `vstc_cli/src/profile.rs` | プロファイルのデータモデルと、純粋なマージ / 解決 / 描画 | Modify |
| `vstc_cli/src/store.rs` | `profiles.toml` の読み書き（**ロジック変更なし。テストのリテラル修正のみ**） | Modify |
| `vstc_cli/src/main.rs` | CLI 定義、サブコマンドの配線、送信するチェーンの決定 | Modify |
| `vstc_cli/README.md` | 使い方ドキュメント | Modify |
| `docs/adr/0018-*.md` / `docs/adr/README.md` | ADR の Status 昇格 | Modify |

---

### Task 1: vstc — route 文字列の正規化と transcribe 対応

**Files:**
- Modify: `vstc/src/lib.rs`

**Interfaces:**
- Consumes: なし（最初のタスク）
- Produces: `pub fn parse_route(op_str: &str) -> Result<OperationRoute, VstcError>` — 1 本の route 文字列を proto の `OperationRoute` に変換する。`//host:port/op?q` / `op?q` / 絶対 URL（`o:/op` 含む）を受け付ける。既存の private `convert_to_operation` はこれに置き換わって消える。

- [ ] **Step 1: 失敗するテストを書く**

`vstc/src/lib.rs` の `mod tests` の末尾に追加する。

```rust
    #[test]
    fn parse_route_accepts_the_scheme_less_remote_form() {
        let route = parse_route("//localhost:8081/transc").expect("scheme-less remote form");
        assert_eq!(route.operation, Operation::Transcribe as i32);
        assert_eq!(route.remote, "//localhost:8081");
        assert!(route.queries.is_empty());
    }

    #[test]
    fn parse_route_accepts_the_scheme_less_bare_form() {
        let route = parse_route("transl?t=en").expect("scheme-less bare form");
        assert_eq!(route.operation, Operation::Translate as i32);
        assert!(route.remote.is_empty(), "no host means no remote");
        assert_eq!(route.queries["t"], "en");
    }

    #[test]
    fn parse_route_still_accepts_the_historical_scheme_form() {
        // ADR-0018 の正規化は受け付ける入力の拡張であって置き換えではない。
        let route = parse_route("o:/tts?spd=1.1").expect("o: form");
        assert_eq!(route.operation, Operation::Tts as i32);
        assert!(route.remote.is_empty());
        assert_eq!(route.queries["spd"], "1.1");

        let route = parse_route("o://localhost:8080/transl?t=en").expect("o:// form");
        assert_eq!(route.remote, "//localhost:8080");
        assert_eq!(route.queries["t"], "en");
    }

    #[test]
    fn parse_route_maps_both_transcribe_aliases() {
        for s in ["transc", "transcribe"] {
            let route = parse_route(s).unwrap_or_else(|e| panic!("{s} should parse: {e}"));
            assert_eq!(route.operation, Operation::Transcribe as i32, "for {s}");
        }
    }

    #[test]
    fn parse_route_does_not_mistake_a_query_value_for_a_scheme() {
        // 'transl?u=http://x' にはコロンがあるが、その手前は scheme として不正。
        // scheme 判定を「最初のコロンがあれば scheme」で済ませると、この文字列が
        // 絶対 URL 扱いのまま Url::parse に渡り、operation が空になって落ちる。
        let route = parse_route("transl?u=http://x").expect("query value containing a colon");
        assert_eq!(route.operation, Operation::Translate as i32);
        assert_eq!(route.queries["u"], "http://x");
    }

    #[test]
    fn parse_route_rejects_an_unknown_operation() {
        let err = parse_route("//localhost:8081/nope").expect_err("unknown operation");
        assert!(
            matches!(err, VstcError::OpConvertError { .. }),
            "should be an operation-conversion error, got {err:?}"
        );
    }
```

既存の 2 テストを新しい関数名に合わせて書き換える（同じ `mod tests` 内）。

```rust
    #[test]
    fn convert_without_host() {
        let result = parse_route("o:/transl?t=en&s=ja").unwrap();
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }

    #[test]
    fn convert_with_host() {
        let result = parse_route("o://localhost:8080/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:8080");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");

        let result = parse_route("https://localhost/transl?t=en&s=ja").unwrap();
        let remote = result.remote;
        assert_eq!(remote, "//localhost:443");
        let qs = result.queries;
        assert_eq!(qs["s"], "ja");
        assert_eq!(qs["t"], "en");
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。`cannot find function 'parse_route' in this scope` が 8 テスト分出る。

- [ ] **Step 3: 実装する**

`vstc/src/lib.rs` の先頭 `use` に `Cow` を足す。

```rust
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
```

既存の `fn convert_to_operation(op_str: &str) -> Result<OperationRoute, VstcError> { ... }` を丸ごと次で置き換える（`const CONNECT_TIMEOUT_SECS` などの定数群の直後に `ROUTE_SCHEME` を置く）。

```rust
/// Scheme prefix that turns a scheme-less route string into an absolute URL.
/// It carries no meaning of its own: `Url::parse` only accepts absolute URLs.
const ROUTE_SCHEME: &str = "o:";
```

```rust
/// True when `s` already starts with a URL scheme (`scheme:`).
///
/// RFC 3986 spells a scheme as an ASCII letter followed by letters, digits,
/// `+`, `-` or `.`, terminated by `:`. Checked by hand so this crate does not
/// grow a regex dependency. Guarding on the whole prefix (not just "there is a
/// colon") keeps a query value like `?u=http://x` from being mistaken for one.
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let mut chars = s[..colon].chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Make `op_str` absolute so `Url::parse` accepts it (ADR-0018).
///
/// * `//host:port/op?q` gains `o:`
/// * `scheme:...` is left alone, which covers the historical `o:/op` form
/// * anything else gains `o:/`, so `op?q` becomes `o:/op?q`
fn normalize_op_str(op_str: &str) -> Cow<'_, str> {
    if has_scheme(op_str) {
        Cow::Borrowed(op_str)
    } else if op_str.starts_with("//") {
        Cow::Owned(format!("{ROUTE_SCHEME}{op_str}"))
    } else {
        Cow::Owned(format!("{ROUTE_SCHEME}/{op_str}"))
    }
}

/// Parse one route string into a proto [`OperationRoute`].
///
/// Accepts `//host:port/op?query`, a bare `op?query`, and any absolute URL
/// (including the historical `o:/op` form). Public so a caller can validate a
/// route before storing it, without sending anything (ADR-0018).
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * The normalized string is not a parsable URL.
/// * The path names an operation this crate does not know.
pub fn parse_route(op_str: &str) -> Result<OperationRoute, VstcError> {
    let normalized = normalize_op_str(op_str);
    let parsed = Url::parse(&normalized)?;
    let hash_query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    let operation = match parsed.path().strip_prefix('/').unwrap_or_default() {
        "transc" | "transcribe" => Ok(Operation::Transcribe),
        "transl" | "translate" => Ok(Operation::Translate),
        "tts" => Ok(Operation::Tts),
        "play" | "playback" => Ok(Operation::Playback),
        "sub" | "subtitle" => Ok(Operation::Subtitle),
        "vc" => Ok(Operation::Vc),
        "reload" => Ok(Operation::Reload),
        "pause" => Ok(Operation::Pause),
        "resume" => Ok(Operation::Resume),
        "forward" | "fwd" => Ok(Operation::Forward),
        _ => Err(VstcError::OpConvertError {
            op_str: String::from(op_str),
        }),
    };
    let remote = match parsed.host_str() {
        Some(host) => format!(
            "//{}{}",
            host,
            match parsed.port_or_known_default() {
                Some(port) => format!(":{port}"),
                None => String::new(),
            }
        ),
        None => String::new(),
    };
    Ok(OperationRoute {
        operation: operation?.into(),
        remote,
        queries: hash_query,
    })
}
```

`process_command` 内の呼び出しを新しい名前に差し替える。

```rust
    let op_routes: Result<Vec<_>, _> = operations
        .iter()
        .map(String::as_ref)
        .map(parse_route)
        .collect();
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS（`vstc` の全テストが緑）

- [ ] **Step 5: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 6: コミット**

```bash
git add vstc/src/lib.rs
git commit -F - <<'EOF'
feat(vstc): route 文字列のスキーム省略形を受け付ける parse_route を公開

`//host:port/op?q` と `op?q` を `o:` / `o:/` の前置で正規化してから
Url::parse に渡す。既存の `o:` 付き文字列は無変更で通る。あわせて
proto の TRANSCRIBE を `transc` / `transcribe` で指定できるようにした。
保存前検証のため公開 API にしている（ADR-0018）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: vstc — 複数チェーンを単一 Command で送る口

**Files:**
- Modify: `vstc/src/lib.rs`
- Test: `vstc/tests/test.rs`

**Interfaces:**
- Consumes: Task 1 の `parse_route`
- Produces: `pub async fn process_chains_with_operand(uri: &str, chains: Vec<Vec<OperationRoute>>, operand: RouteOperand) -> Result<Response, VstcError>` — N 本のチェーンを 1 つの `Command` に載せて 1 回送る。private `build_command` のシグネチャが `(chains: Vec<Vec<OperationRoute>>, operand: RouteOperand)` に変わる。

- [ ] **Step 1: 失敗するユニットテストを書く**

`vstc/src/lib.rs` の `mod tests` 末尾に追加する。

```rust
    #[test]
    fn build_command_keeps_each_chain_separate() {
        let route = |op: Operation, remote: &str| OperationRoute {
            operation: op as i32,
            remote: remote.to_string(),
            queries: HashMap::new(),
        };
        let first = vec![route(Operation::Transcribe, "//h1:1")];
        let second = vec![
            route(Operation::Translate, ""),
            route(Operation::Subtitle, "//h2:2"),
        ];
        let cmd = build_command(
            vec![first.clone(), second.clone()],
            RouteOperand::default(),
        );
        assert_eq!(cmd.chains.len(), 2);
        assert_eq!(cmd.chains[0].operations, first, "chain order must be kept");
        assert_eq!(cmd.chains[1].operations, second);
    }

    #[test]
    fn build_command_shares_one_operand_across_chains() {
        // ADR-0018: N 本のチェーンは 1 つの入力を共有する。trace_id も 1 つ。
        let cmd = build_command(
            vec![Vec::new(), Vec::new(), Vec::new()],
            RouteOperand {
                text: "hi".to_string(),
                ..RouteOperand::default()
            },
        );
        assert_eq!(cmd.chains.len(), 3);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hi");
        assert!(!operand.trace_id.is_empty());
    }
```

既存の 2 テストを新しいシグネチャに合わせて書き換える（`routes` を `vec![routes]` に包む）。

```rust
    #[test]
    fn build_command_wraps_routes_in_single_chain() {
        let routes = vec![OperationRoute {
            operation: Operation::Tts as i32,
            remote: String::new(),
            queries: HashMap::new(),
        }];
        let cmd = build_command(
            vec![routes.clone()],
            RouteOperand {
                text: "hello".to_string(),
                ..RouteOperand::default()
            },
        );
        assert_eq!(cmd.chains.len(), 1);
        assert_eq!(cmd.chains[0].operations, routes);
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.text, "hello");
        assert!(operand.sound.is_none());
        assert!(!operand.trace_id.is_empty());
    }

    #[test]
    fn build_command_carries_optional_operand_fields() {
        let cmd = build_command(
            vec![Vec::new()],
            RouteOperand {
                file_path: "conf.yml".to_string(),
                filters: vec!["a".to_string()],
                ..RouteOperand::default()
            },
        );
        let operand = cmd.operand.expect("operand present");
        assert_eq!(operand.file_path, "conf.yml");
        assert_eq!(operand.filters, vec!["a".to_string()]);
        assert!(operand.text.is_empty());
        assert!(operand.origin_ts > 0.0);
    }
```

- [ ] **Step 2: 失敗する結合テストを書く**

`vstc/tests/test.rs` の末尾に追加する。

```rust
#[tokio::test]
async fn process_chains_with_operand_sends_every_chain_in_one_command() {
    use std::collections::HashMap;
    use std::sync::mpsc::channel;
    use std::time::Duration;
    use vstreamer_protos::{Operation, OperationRoute};

    const ADDR_STR: &str = "127.0.0.1:9004";
    let (tx, rx) = channel();
    tokio::spawn(async move {
        let mut mock = MockCommanderService::new();
        mock.expect_process_command().returning(move |req| {
            let inner = req.into_inner();
            let operand = inner.operand.expect("operand should be present");
            let shapes: Vec<usize> = inner.chains.iter().map(|c| c.operations.len()).collect();
            tx.send((shapes, operand.trace_id, operand.text))
                .expect("test channel should accept the command");
            Ok(tonic::Response::new(Response { result: true }))
        });
        let addr = ADDR_STR.parse().unwrap();
        build(mock).serve(addr).await.unwrap();
    });

    let route = |op: Operation| OperationRoute {
        operation: op as i32,
        remote: String::new(),
        queries: HashMap::new(),
    };
    process_chains_with_operand(
        format!("http://{ADDR_STR}").as_str(),
        vec![
            vec![route(Operation::Transcribe)],
            vec![route(Operation::Transcribe), route(Operation::Translate)],
            vec![
                route(Operation::Transcribe),
                route(Operation::Translate),
                route(Operation::Subtitle),
            ],
        ],
        RouteOperand {
            text: String::from("one input"),
            ..RouteOperand::default()
        },
    )
    .await
    .unwrap();

    let (shapes, trace_id, text) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server should have received a command");
    assert_eq!(
        shapes,
        vec![1, 2, 3],
        "each chain must arrive intact and in order"
    );
    assert!(!trace_id.is_empty());
    assert_eq!(text, "one input");
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "chains must not be split across multiple commands"
    );
}
```

- [ ] **Step 3: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。ユニットテストは `build_command` の引数型不一致、結合テストは `cannot find function 'process_chains_with_operand'`。

- [ ] **Step 4: 実装する**

`vstc/src/lib.rs` の `build_command` を置き換える。

```rust
/// Wrap the given chains into a `Command` carrying a single shared `operand`.
///
/// Every chain sees the same input and the same trace id (ADR-0018).
fn build_command(chains: Vec<Vec<OperationRoute>>, operand: RouteOperand) -> Command {
    Command {
        chains: chains
            .into_iter()
            .map(|operations| OperationChain { operations })
            .collect(),
        operand: Some(build_operand(operand)),
    }
}
```

`process_routes_with_operand` を新しい口への委譲にする。

```rust
pub async fn process_routes_with_operand(
    uri: &str,
    routes: Vec<OperationRoute>,
    operand: RouteOperand,
) -> Result<Response, VstcError> {
    process_chains_with_operand(uri, vec![routes], operand).await
}
```

その直後に新しい送信口を足す。

```rust
/// Send several operation chains together, sharing one operand.
///
/// [`process_routes_with_operand`] covers the single-chain case. Use this when
/// one input fans out to several destinations: the chains travel in a single
/// `Command`, so the server sees one request and every chain shares the same
/// `trace_id` (ADR-0018).
///
/// ## Errors
///
/// This function fails under the following circumstances:
///
/// * Any error occurring during connecting or sending to the target uri.
pub async fn process_chains_with_operand(
    uri: &str,
    chains: Vec<Vec<OperationRoute>>,
    operand: RouteOperand,
) -> Result<Response, VstcError> {
    let mut channel = connect(uri).await?;
    let c = tonic::Request::new(build_command(chains, operand));
    let result = channel.process_command(c).await?;
    Ok(result.into_inner())
}
```

`process_command` を新しい口への委譲に書き換える。route のパースが接続より先になるので、壊れた操作文字列は接続を試みる前に落ちるようになる。

```rust
pub async fn process_command(
    uri: &str,
    operations: &[String],
    text: String,
    sound: Option<Sound>,
    file_path: Option<String>,
    filters: Option<Vec<String>>,
) -> Result<Response, VstcError> {
    let routes: Result<Vec<_>, _> = operations
        .iter()
        .map(String::as_ref)
        .map(parse_route)
        .collect();
    process_chains_with_operand(
        uri,
        vec![routes?],
        RouteOperand {
            text,
            sound,
            file_path: file_path.unwrap_or_default(),
            filters: filters.unwrap_or_default(),
        },
    )
    .await
}
```

この書き換えで `process_command` 内の `Operand` 直接組み立てが不要になる。`use vstreamer_protos::{...}` から未使用になった `Operand` は消さない（`build_operand` が使っている）。`OperationChain` も `build_command` が使うので残す。

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS（`vstc` のユニット + 結合テストが全て緑）

- [ ] **Step 6: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 7: コミット**

```bash
git add vstc/src/lib.rs vstc/tests/test.rs
git commit -F - <<'EOF'
feat(vstc): 複数チェーンを単一 Command で送る process_chains_with_operand

proto の repeated chains を使い、N 本のチェーンを 1 回の RPC で送る。
operand は 1 つを共有するので trace_id も 1 つになる。既存の
process_routes_with_operand / process_command は新しい口への委譲にした
（ADR-0018）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: vstc_cli — Profile.chains のデータモデルと追加・削除

**Files:**
- Modify: `vstc_cli/src/profile.rs`
- Modify: `vstc_cli/src/store.rs`（テスト内の `Profile` リテラルのみ）
- Modify: `vstc_cli/src/main.rs`（`ProfileCmd::Set` の `Profile` リテラルとテストのみ）

**Interfaces:**
- Consumes: なし
- Produces:
  - `Profile` に `pub chains: Option<Vec<Vec<String>>>` フィールド
  - `ProfileStore::add_chain(&mut self, name: &str, chain: Vec<String>) -> anyhow::Result<()>`
  - `ProfileStore::del_chain(&mut self, name: &str, index: usize) -> anyhow::Result<()>`（`index` は 1 始まり）
  - `ProfileStore::chains_of(&self, name: &str) -> anyhow::Result<&[Vec<String>]>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/profile.rs` の `mod tests` 末尾に追加する。

```rust
    #[test]
    fn add_chain_appends_to_an_existing_profile() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain(
                "main",
                vec!["//h:1/transc".to_string(), "sub".to_string()],
            )
            .expect("add");
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        let got = store.profiles["main"].chains.clone().expect("chains saved");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], vec!["//h:1/transc".to_string(), "sub".to_string()]);
        assert_eq!(got[1], vec!["tts".to_string()]);
    }

    #[test]
    fn add_chain_to_an_unknown_profile_errors_and_creates_nothing() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        let err = store
            .add_chain("nope", vec!["tts".to_string()])
            .expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(msg.contains("nope"), "should name the profile: {msg}");
        assert!(msg.contains("main"), "should list known names: {msg}");
        assert_eq!(
            store.profiles.len(),
            1,
            "a typo must not silently create a profile"
        );
    }

    #[test]
    fn del_chain_removes_by_one_based_index() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        for op in ["a-tts", "b-tts", "c-tts"] {
            store
                .add_chain("main", vec![op.to_string()])
                .expect("add");
        }
        store.del_chain("main", 2).expect("del the middle one");
        let got = store.profiles["main"].chains.clone().expect("chains saved");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], vec!["a-tts".to_string()]);
        assert_eq!(got[1], vec!["c-tts".to_string()]);
    }

    #[test]
    fn del_chain_of_the_last_chain_clears_the_field() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store.del_chain("main", 1).expect("del");
        assert_eq!(
            store.profiles["main"].chains, None,
            "the profile must end up identical to one that never saved a chain"
        );
    }

    #[test]
    fn del_chain_rejects_out_of_range_indexes_without_changing_anything() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        for bad in [0, 2, 99] {
            let err = store
                .del_chain("main", bad)
                .expect_err("out-of-range index should error");
            let msg = err.to_string();
            assert!(
                msg.contains("1 本"),
                "should state how many are saved (for {bad}): {msg}"
            );
        }
        assert_eq!(
            store.profiles["main"].chains.as_ref().map(Vec::len),
            Some(1),
            "a rejected delete must leave the chains untouched"
        );
    }

    #[test]
    fn chains_of_returns_empty_for_a_profile_without_chains() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        assert!(store.chains_of("main").expect("known profile").is_empty());
    }

    #[test]
    fn chains_of_an_unknown_profile_errors() {
        let store = ProfileStore::default();
        assert!(store.chains_of("nope").is_err());
    }

    #[test]
    fn merge_never_touches_saved_chains() {
        // `profile set` は host/port/config_path 専用。チェーンは
        // `profile chains` が持つので、set が消してはいけない（ADR-0018）。
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store.merge(
            "main",
            &Profile {
                port: Some(2),
                ..Profile::default()
            },
        );
        assert_eq!(
            store.profiles["main"].chains.as_ref().map(Vec::len),
            Some(1)
        );
        assert_eq!(store.profiles["main"].port, Some(2));
    }

    #[test]
    fn chains_round_trip_through_toml() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain(
                "main",
                vec![
                    "//localhost:8081/transc".to_string(),
                    "//windesk:8080/sub".to_string(),
                ],
            )
            .expect("add");
        store
            .add_chain(
                "main",
                vec![
                    "//localhost:8081/transc".to_string(),
                    "transl?t=en".to_string(),
                ],
            )
            .expect("add");
        let text = toml::to_string(&store).expect("serialize");
        let back: ProfileStore = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, store);
    }

    #[test]
    fn a_profile_without_chains_writes_no_chains_key() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        let text = toml::to_string(&store).expect("serialize");
        assert!(
            !text.contains("chains"),
            "unset chains must stay out of the file: {text}"
        );
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。`no method named 'add_chain'` などのコンパイルエラー。

- [ ] **Step 3: Profile にフィールドを足す**

`vstc_cli/src/profile.rs` の `Profile` を置き換える。

```rust
/// One saved connection profile.
///
/// Every field is optional so `profile set` can update a single field without
/// clearing the rest (ADR-0016), and so unset fields stay out of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Destination host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Destination port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Config file path used by `reload`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// Default operation chains sent by `send` when no operation is given on
    /// the command line (ADR-0018). Each inner vector is one chain, written as
    /// route strings. Edited through `profile chains`, never through
    /// `profile set`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chains: Option<Vec<Vec<String>>>,
}
```

`merge` の doc コメントに 1 行足す（本体は変更しない）。

```rust
    /// Merge `patch` into the profile named `name`, creating it when absent.
    /// Fields left `None` in `patch` keep their existing value (ADR-0016).
    ///
    /// `chains` is deliberately not merged: it is owned by `profile chains`
    /// (ADR-0018), so a `profile set` can never clear it.
    pub fn merge(&mut self, name: &str, patch: &Profile) {
```

- [ ] **Step 4: 追加・削除・参照を実装する**

`impl ProfileStore` の `get` と `unknown` の間に足す。

```rust
    /// Append `chain` to the named profile's default chains.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists. A typo must not silently
    /// create a profile — `profile set` is the only command that creates one.
    pub fn add_chain(&mut self, name: &str, chain: Vec<String>) -> anyhow::Result<()> {
        if !self.profiles.contains_key(name) {
            return Err(self.unknown(name));
        }
        let entry = self
            .profiles
            .get_mut(name)
            .expect("contains_key checked immediately above");
        entry.chains.get_or_insert_with(Vec::new).push(chain);
        Ok(())
    }

    /// Remove the `index`-th (1-based, as shown by `profile chains show`)
    /// chain from the named profile.
    ///
    /// Removing the last chain drops the field entirely, so the profile ends up
    /// indistinguishable from one that never saved a chain.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists, or when `index` is outside
    /// the saved range. In both cases nothing is modified.
    pub fn del_chain(&mut self, name: &str, index: usize) -> anyhow::Result<()> {
        if !self.profiles.contains_key(name) {
            return Err(self.unknown(name));
        }
        let entry = self
            .profiles
            .get_mut(name)
            .expect("contains_key checked immediately above");
        let saved = entry.chains.as_ref().map_or(0, Vec::len);
        if index == 0 || index > saved {
            return Err(anyhow!(
                "チェーン番号 {index} は範囲外です（プロファイル '{name}' に保存されているチェーンは {saved} 本）\n\
                 確認: vstc_cli profile chains show {name}"
            ));
        }
        if let Some(chains) = entry.chains.as_mut() {
            chains.remove(index - 1);
            if chains.is_empty() {
                entry.chains = None;
            }
        }
        Ok(())
    }

    /// The named profile's saved chains, empty when it has none.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn chains_of(&self, name: &str) -> anyhow::Result<&[Vec<String>]> {
        Ok(self.get(name)?.chains.as_deref().unwrap_or(&[]))
    }
```

- [ ] **Step 5: 既存の `Profile` リテラルにフィールドを足す**

フィールド追加でコンパイルが壊れる箇所を直す。`..Profile::default()` を使っている箇所は修正不要。

`vstc_cli/src/profile.rs`（テスト内）:

```rust
    fn profile(host: &str, port: u16) -> Profile {
        Profile {
            host: Some(host.to_string()),
            port: Some(port),
            config_path: None,
            chains: None,
        }
    }
```

同ファイル内の残り 4 箇所（`merge_keeps_fields_the_patch_leaves_unset` の 1 つ目のリテラル、`resolve_prefers_explicit_flags_over_profile`、`resolve_overrides_each_field_independently`、`store_round_trips_through_toml`）の `Profile { host: ..., port: ..., config_path: ... }` に `chains: None,` を足す。

`vstc_cli/src/store.rs`（テスト内）: `save_then_load_round_trips` の `Profile` リテラルに `chains: None,` を足す。

`vstc_cli/src/main.rs`: `run_profile` の `ProfileCmd::Set` 分岐。

```rust
            saved.merge(
                &name,
                &Profile {
                    host,
                    port,
                    config_path,
                    chains: None,
                },
            );
```

同ファイルのテスト `profile_list_output_renders_the_table_when_store_is_non_empty` の `Profile` リテラルに `chains: None,` を足す。

- [ ] **Step 6: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS

- [ ] **Step 7: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 8: コミット**

```bash
git add vstc_cli/src/profile.rs vstc_cli/src/store.rs vstc_cli/src/main.rs
git commit -F - <<'EOF'
feat(vstc_cli): Profile に既定チェーンを持たせ追加・削除を実装

chains: Option<Vec<Vec<String>>> を追加。add_chain / del_chain /
chains_of を足し、未登録プロファイル名では作成せずエラーにする。
最後の 1 本を削除したらフィールドごと落として、保存前と同じ状態に戻す。
profile set はチェーンに触れない（ADR-0018）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: vstc_cli — チェーンの解決と描画

**Files:**
- Modify: `vstc_cli/src/profile.rs`

**Interfaces:**
- Consumes: Task 3 の `Profile.chains` / `add_chain`
- Produces:
  - `Resolved` に `pub chains: Vec<Vec<String>>`（空 = 未設定）
  - `pub fn render_chains(chains: &[Vec<String>]) -> String` — `[1] a -> b` 形式。0 本なら空文字列
  - `render_list` が 5 列目 `CHAINS` を出す

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/profile.rs` の `mod tests` 末尾に追加する。

```rust
    #[test]
    fn resolve_carries_the_profiles_chains() {
        let mut p = profile("h", 1);
        p.chains = Some(vec![vec!["tts".to_string()]]);
        let got = resolve(Some(&p), &Overrides::default());
        assert_eq!(got.chains, vec![vec!["tts".to_string()]]);
    }

    #[test]
    fn resolve_without_a_profile_has_no_chains() {
        assert!(resolve(None, &Overrides::default()).chains.is_empty());
    }

    #[test]
    fn render_chains_numbers_from_one_and_shows_hop_order() {
        let chains = vec![
            vec![
                "//localhost:8081/transc".to_string(),
                "//windesk:8080/sub".to_string(),
            ],
            vec![
                "//localhost:8081/transc".to_string(),
                "transl?t=en".to_string(),
            ],
        ];
        let rendered = render_chains(&chains);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "[1] //localhost:8081/transc -> //windesk:8080/sub");
        assert_eq!(lines[1], "[2] //localhost:8081/transc -> transl?t=en");
    }

    #[test]
    fn render_chains_is_empty_when_there_are_none() {
        assert_eq!(render_chains(&[]), "");
    }

    #[test]
    fn render_list_shows_the_saved_chain_count() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.merge("sub", &profile("h", 2));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store
            .add_chain("main", vec!["transl?t=en".to_string()])
            .expect("add");
        let rendered = render_list(&store);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(
            lines[0].contains("CHAINS"),
            "header should name the column: {}",
            lines[0]
        );
        assert!(
            lines[1].ends_with('2'),
            "main has 2 saved chains: {}",
            lines[1]
        );
        assert!(
            lines[2].ends_with('-'),
            "sub has none, shown as unset: {}",
            lines[2]
        );
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。`no field 'chains' on type 'Resolved'` と `cannot find function 'render_chains'`。

- [ ] **Step 3: Resolved と resolve を拡張する**

`vstc_cli/src/profile.rs` の `Resolved` に 1 フィールド足す。

```rust
/// The effective values for one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Destination host.
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// Config file path, if any source supplied one.
    pub config_path: Option<String>,
    /// Default chains from the profile, empty when none are saved. There is no
    /// command-line override: explicit operations replace these wholesale
    /// rather than merging field by field (ADR-0018).
    pub chains: Vec<Vec<String>>,
}
```

`resolve` の戻り値に 1 行足す。

```rust
        config_path: overrides
            .config_path
            .clone()
            .or_else(|| profile.and_then(|p| p.config_path.clone())),
        chains: profile
            .and_then(|p| p.chains.clone())
            .unwrap_or_default(),
    }
}
```

- [ ] **Step 4: 描画を実装する**

`render_list` を 5 列に広げる。ヘッダ行・行生成・幅計算の 3 箇所を直す。

```rust
/// Render the aligned table shown by `profile list`.
/// Returns an empty string when there is nothing to show.
pub fn render_list(store: &ProfileStore) -> String {
    if store.profiles.is_empty() {
        return String::new();
    }
    let mut rows: Vec<[String; 5]> = vec![[
        "NAME".to_string(),
        "HOST".to_string(),
        "PORT".to_string(),
        "CONFIG_PATH".to_string(),
        "CHAINS".to_string(),
    ]];
    for (name, p) in &store.profiles {
        rows.push([
            name.clone(),
            p.host.clone().unwrap_or_else(|| UNSET.to_string()),
            p.port
                .map_or_else(|| UNSET.to_string(), |port| port.to_string()),
            p.config_path.clone().unwrap_or_else(|| UNSET.to_string()),
            p.chains
                .as_ref()
                .map_or_else(|| UNSET.to_string(), |c| c.len().to_string()),
        ]);
    }
    let widths: Vec<usize> = (0..5)
        .map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}", width = *width))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

`render_list` の直後に足す。

```rust
/// Render the numbered chain list shown by `profile chains show`.
/// Returns an empty string when the profile has no saved chains.
pub fn render_chains(chains: &[Vec<String>]) -> String {
    chains
        .iter()
        .enumerate()
        .map(|(i, chain)| format!("[{}] {}", i + 1, chain.join(" -> ")))
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS。既存の `render_list_sorts_by_name_and_marks_unset_fields` も 5 列化後に通る（`lines[2].contains('-')` は CHAINS 列の `-` でも満たされる）。

- [ ] **Step 6: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 7: コミット**

```bash
git add vstc_cli/src/profile.rs
git commit -F - <<'EOF'
feat(vstc_cli): 既定チェーンの解決と描画を足す

Resolved に chains を持たせ、profile list に CHAINS 列（本数、未設定は
ハイフン）を追加。chains show 用に [N] a -> b 形式の render_chains を
新設した。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: vstc_cli — profile chains サブコマンド

**Files:**
- Modify: `vstc_cli/src/main.rs`

**Interfaces:**
- Consumes: Task 1 の `vstc::parse_route`、Task 3 の `add_chain` / `del_chain` / `chains_of`、Task 4 の `render_chains`
- Produces:
  - `enum ChainsCmd { Add { name: String, routes: Vec<String> }, Del { name: String, index: usize }, Show { name: String } }`
  - `ProfileCmd::Chains(ChainsCmd)` バリアント
  - `fn validate_routes(routes: &[String]) -> Result<()>`
  - `fn chains_show_output(name: &str, chains: &[Vec<String>]) -> String`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/main.rs` の `mod tests` 末尾に追加する。

```rust
    #[test]
    fn profile_chains_add_parses_the_name_and_every_route() {
        let cli = Cli::parse_from([
            "vstc_cli",
            "profile",
            "chains",
            "add",
            "main",
            "//localhost:8081/transc",
            "transl?t=en",
            "//windesk:8080/sub?p=s",
        ]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Add { name, routes })) = cli.command
        else {
            panic!("expected profile chains add");
        };
        assert_eq!(name, "main");
        assert_eq!(
            routes,
            vec![
                "//localhost:8081/transc".to_string(),
                "transl?t=en".to_string(),
                "//windesk:8080/sub?p=s".to_string(),
            ]
        );
    }

    #[test]
    fn profile_chains_add_requires_at_least_one_route() {
        assert!(Cli::try_parse_from(["vstc_cli", "profile", "chains", "add", "main"]).is_err());
    }

    #[test]
    fn profile_chains_del_and_show_parse() {
        let cli = Cli::parse_from(["vstc_cli", "profile", "chains", "del", "main", "2"]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Del { name, index })) = cli.command
        else {
            panic!("expected profile chains del");
        };
        assert_eq!(name, "main");
        assert_eq!(index, 2);

        let cli = Cli::parse_from(["vstc_cli", "profile", "chains", "show", "main"]);
        let Commands::Profile(ProfileCmd::Chains(ChainsCmd::Show { name })) = cli.command else {
            panic!("expected profile chains show");
        };
        assert_eq!(name, "main");
    }

    #[test]
    fn validate_routes_accepts_every_documented_form() {
        validate_routes(&[
            "//localhost:8081/transc".to_string(),
            "transl?t=en".to_string(),
            "o:/tts".to_string(),
        ])
        .expect("all three documented forms are valid");
    }

    #[test]
    fn validate_routes_rejects_an_unparsable_route_naming_it() {
        let err = validate_routes(&[
            "//localhost:8081/transc".to_string(),
            "nope".to_string(),
        ])
        .expect_err("an unknown operation must fail before saving");
        let msg = format!("{err:#}");
        assert!(msg.contains("nope"), "should name the offending route: {msg}");
    }

    #[test]
    fn chains_show_output_guides_when_the_profile_has_none() {
        let out = chains_show_output("main", &[]);
        assert!(
            out.contains("保存済みチェーンはありません"),
            "should say there are none: {out}"
        );
        assert!(
            out.contains("profile chains add main"),
            "should suggest how to add one: {out}"
        );
    }

    #[test]
    fn chains_show_output_renders_the_saved_chains() {
        let chains = vec![vec!["o:/tts".to_string()]];
        let out = chains_show_output("main", &chains);
        assert!(out.contains("[1] o:/tts"), "should number the chain: {out}");
        assert!(
            !out.contains("ありません"),
            "a non-empty profile must not show the empty guidance: {out}"
        );
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。`cannot find type 'ChainsCmd'` / `cannot find function 'validate_routes'` / `cannot find function 'chains_show_output'`。

- [ ] **Step 3: サブコマンドを定義する**

`vstc_cli/src/main.rs` の `ProfileCmd` に 1 バリアント足す。

```rust
#[derive(Subcommand)]
enum ProfileCmd {
    /// プロファイルを作成・更新する（渡したフィールドのみ更新）
    Set {
        /// プロファイル名
        name: String,
        /// 送信先ホスト
        #[arg(short = 'H', long)]
        host: Option<String>,
        /// 送信先ポート
        #[arg(short, long)]
        port: Option<u16>,
        /// reload が読む設定ファイルのパス
        #[arg(long)]
        config_path: Option<String>,
    },
    /// 保存済みプロファイルを一覧表示する
    List,
    /// プロファイルを削除する
    Remove {
        /// プロファイル名
        name: String,
    },
    /// send の既定チェーンを管理する
    #[command(subcommand)]
    Chains(ChainsCmd),
}

/// `send` が操作を省略したときに送るチェーンの編集操作（ADR-0018）。
#[derive(Subcommand)]
enum ChainsCmd {
    /// route 列を 1 本のチェーンとして末尾に追加する
    Add {
        /// プロファイル名
        name: String,
        /// route ex: `//localhost:8081/transc` `transl?t=en`
        #[arg(required = true)]
        routes: Vec<String>,
    },
    /// 番号を指定してチェーンを削除する
    Del {
        /// プロファイル名
        name: String,
        /// `chains show` が表示する 1 始まりの番号
        index: usize,
    },
    /// 保存済みチェーンを表示する
    Show {
        /// プロファイル名
        name: String,
    },
}
```

- [ ] **Step 4: 実行部を実装する**

`run_profile` の `match` に 1 分岐足す。

```rust
        ProfileCmd::Chains(cmd) => run_profile_chains(&path, cmd),
```

`run_profile` の直後に足す。`path` を引数で受けるのは、`run_profile` が既に解決済みのパスを再利用するため。

```rust
/// `profile chains` の 3 操作を実行する。
///
/// `add` は保存の前に全 route を検証する。壊れた route をファイルに残すと、
/// 次に送信するまで気づけないため（ADR-0018）。
fn run_profile_chains(path: &Path, cmd: ChainsCmd) -> Result<()> {
    match cmd {
        ChainsCmd::Add { name, routes } => {
            validate_routes(&routes)?;
            let mut saved = store::load(path)?;
            saved.add_chain(&name, routes)?;
            store::save(path, &saved)?;
            println!("プロファイル '{name}' にチェーンを追加しました");
            Ok(())
        }
        ChainsCmd::Del { name, index } => {
            let mut saved = store::load(path)?;
            saved.del_chain(&name, index)?;
            store::save(path, &saved)?;
            println!("プロファイル '{name}' のチェーン {index} を削除しました");
            Ok(())
        }
        ChainsCmd::Show { name } => {
            let saved = store::load(path)?;
            println!("{}", chains_show_output(&name, saved.chains_of(&name)?));
            Ok(())
        }
    }
}

/// 保存前に全 route を検証する。1 つでも解釈できなければ保存しない。
fn validate_routes(routes: &[String]) -> Result<()> {
    for route in routes {
        vstc::parse_route(route)
            .with_context(|| format!("route '{route}' を解釈できませんでした"))?;
    }
    Ok(())
}

/// `chains show` の表示内容を組み立てる。0 本は異常ではないので、
/// 追加方法を案内する。
fn chains_show_output(name: &str, chains: &[Vec<String>]) -> String {
    let rendered = profile::render_chains(chains);
    if rendered.is_empty() {
        return format!(
            "プロファイル '{name}' に保存済みチェーンはありません\n\
             追加: vstc_cli profile chains add {name} <ROUTES>..."
        );
    }
    rendered
}
```

`run_profile` は既存の 3 分岐でも `path` を使っているので、そのままで通る。`Path` は既に `use std::path::{Path, PathBuf};` で導入済み。

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS

- [ ] **Step 6: 手で動作確認する**

```bash
cargo run -p vstc_cli -- profile --help
```
Expected: `set` / `list` / `remove` / `chains` の 4 サブコマンドが出る。

```bash
cargo run -p vstc_cli -- profile chains add nosuch o:/tts
```
Expected: 「プロファイル 'nosuch' は登録されていません」で終了（新規作成されない）。

- [ ] **Step 7: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 8: コミット**

```bash
git add vstc_cli/src/main.rs
git commit -F - <<'EOF'
feat(vstc_cli): profile chains add/del/show サブコマンドを足す

add は保存前に全 route を vstc::parse_route で検証し、1 つでも壊れて
いればファイルに書かない。show は 0 本のとき追加方法を案内して正常
終了する（ADR-0018）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 6: vstc_cli — send への既定チェーン適用

**Files:**
- Modify: `vstc_cli/src/main.rs`

**Interfaces:**
- Consumes: Task 1 の `vstc::parse_route`、Task 2 の `vstc::process_chains_with_operand`、Task 4 の `Resolved.chains`
- Produces:
  - `fn chains_for_send(operations: &[String], saved: &[Vec<String>]) -> Result<Vec<Vec<String>>>`
  - `fn chains_origin(operations: &[String], profile: Option<&str>) -> String`
  - `fn parse_chains(chains: &[Vec<String>], origin: &str) -> Result<Vec<Vec<OperationRoute>>>`

- [ ] **Step 1: 失敗するテストを書く**

`vstc_cli/src/main.rs` の `mod tests` 末尾に追加する。

```rust
    #[test]
    fn chains_for_send_prefers_command_line_operations() {
        // ADR-0018: 明示した操作が勝ち、保存済みチェーンは同乗しない。
        let operations = vec!["o:/tts".to_string()];
        let saved = vec![vec!["transl?t=en".to_string()]];
        let got = chains_for_send(&operations, &saved).expect("operations win");
        assert_eq!(got, vec![vec!["o:/tts".to_string()]]);
    }

    #[test]
    fn chains_for_send_falls_back_to_the_saved_chains() {
        let saved = vec![
            vec![
                "//localhost:8081/transc".to_string(),
                "//windesk:8080/sub".to_string(),
            ],
            vec![
                "//localhost:8081/transc".to_string(),
                "transl?t=en".to_string(),
            ],
        ];
        let got = chains_for_send(&[], &saved).expect("saved chains are used");
        assert_eq!(got, saved);
    }

    #[test]
    fn chains_for_send_without_any_source_errors_with_guidance() {
        let err = chains_for_send(&[], &[]).expect_err("nothing to send");
        let msg = err.to_string();
        assert!(msg.contains("send"), "should show the positional form: {msg}");
        assert!(
            msg.contains("profile chains add"),
            "should show the saved form too: {msg}"
        );
    }

    #[test]
    fn chains_origin_distinguishes_the_two_sources() {
        assert_eq!(
            chains_origin(&["o:/tts".to_string()], Some("main")),
            "コマンドライン引数"
        );
        assert_eq!(chains_origin(&[], Some("main")), "プロファイル 'main'");
        assert_eq!(chains_origin(&[], None), "保存済みチェーン");
    }

    #[test]
    fn parse_chains_converts_every_chain() {
        let chains = vec![
            vec!["//localhost:8081/transc".to_string()],
            vec![
                "transl?t=en".to_string(),
                "//windesk:8080/sub".to_string(),
            ],
        ];
        let got = parse_chains(&chains, "コマンドライン引数").expect("all routes are valid");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len(), 1);
        assert_eq!(got[1].len(), 2);
        assert_eq!(got[0][0].operation, Operation::Transcribe as i32);
        assert_eq!(got[1][1].remote, "//windesk:8080");
    }

    #[test]
    fn parse_chains_names_the_origin_chain_number_and_route() {
        let chains = vec![
            vec!["o:/tts".to_string()],
            vec![
                "//localhost:8081/transc".to_string(),
                "nope".to_string(),
            ],
        ];
        let err = parse_chains(&chains, "プロファイル 'main'")
            .expect_err("the second chain has a bad route");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("プロファイル 'main'"),
            "should name the origin: {msg}"
        );
        assert!(msg.contains('2'), "should name the chain number: {msg}");
        assert!(msg.contains("nope"), "should name the route: {msg}");
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `just test`
Expected: FAIL。`cannot find function 'chains_for_send'` など 3 件。

- [ ] **Step 3: 実装する**

`vstc_cli/src/main.rs` の `use` に `anyhow` を足す。

```rust
use anyhow::{anyhow, Context, Result};
```

`run_send` を次で置き換え、その直後に 3 つの純関数を足す。

```rust
async fn run_send(args: SendArgs) -> Result<()> {
    let resolved = resolve_conn(&args.conn, None)?;
    let uri = resolved.uri();
    let sound = load_sound(args.wav.as_deref())?;
    let chains = chains_for_send(&args.operations, &resolved.chains)?;
    let origin = chains_origin(&args.operations, args.conn.profile.as_deref());
    let routes = parse_chains(&chains, &origin)?;
    vstc::process_chains_with_operand(
        &uri,
        routes,
        RouteOperand {
            text: args.text.unwrap_or_default(),
            sound,
            file_path: args.file_path.unwrap_or_default(),
            filters: args.filters.unwrap_or_default(),
        },
    )
    .await
    .with_context(|| format!("{uri} への送信に失敗しました"))?;
    Ok(())
}

/// `send` が実際に送るチェーンを決める。位置引数があればそれだけを 1 本の
/// チェーンとして送り、無ければプロファイルの保存済みチェーンを送る
/// （ADR-0018）。送信も I/O も行わない純粋な変換なので、この優先順位を
/// テストで固定できる。
///
/// ## Errors
///
/// どちらの経路からもチェーンを得られなかったとき。無言の no-op を送るより、
/// 両方の指定方法を案内して止める。
fn chains_for_send(operations: &[String], saved: &[Vec<String>]) -> Result<Vec<Vec<String>>> {
    if !operations.is_empty() {
        return Ok(vec![operations.to_vec()]);
    }
    if !saved.is_empty() {
        return Ok(saved.to_vec());
    }
    Err(anyhow!(
        "送信する操作がありません\n\
         操作を直接渡すか: vstc_cli send 'o:/tts' -t \"hello\"\n\
         プロファイルに保存してください: vstc_cli profile chains add <NAME> <ROUTES>..."
    ))
}

/// エラーメッセージでチェーンの出所を示すラベル。`chains_for_send` と同じ
/// 分岐を辿るので、どちらが選ばれたかが文言に反映される。
fn chains_origin(operations: &[String], profile: Option<&str>) -> String {
    if !operations.is_empty() {
        return "コマンドライン引数".to_string();
    }
    profile.map_or_else(
        || "保存済みチェーン".to_string(),
        |name| format!("プロファイル '{name}'"),
    )
}

/// 文字列のチェーンを proto の route へ変換する。どのチェーンのどの route が
/// 壊れているかを示せるよう、出所と 1 始まりの番号を添えてエラーにする。
///
/// ## Errors
///
/// いずれかの route を解釈できなかったとき。
fn parse_chains(chains: &[Vec<String>], origin: &str) -> Result<Vec<Vec<OperationRoute>>> {
    chains
        .iter()
        .enumerate()
        .map(|(i, chain)| {
            chain
                .iter()
                .map(|route| {
                    vstc::parse_route(route).with_context(|| {
                        format!(
                            "{origin}: {} 本目のチェーンの route '{route}' を解釈できませんでした",
                            i + 1
                        )
                    })
                })
                .collect()
        })
        .collect()
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `just test`
Expected: PASS

- [ ] **Step 5: 手で動作確認する**

```bash
cargo run -p vstc_cli -- send -t "hi"
```
Expected: 「送信する操作がありません」で終了（接続を試みない）。

```bash
cargo run -p vstc_cli -- send nope -t "hi"
```
Expected: 「コマンドライン引数: 1 本目のチェーンの route 'nope' を解釈できませんでした」で終了。

- [ ] **Step 6: clippy と整形を確認する**

Run: `just fmt` then `just clippy`
Expected: clippy が exit 0

- [ ] **Step 7: コミット**

```bash
git add vstc_cli/src/main.rs
git commit -F - <<'EOF'
feat(vstc_cli): send が operations 省略時に既定チェーンを送る

位置引数があればそれを 1 本のチェーンとして送り、無ければプロファイルの
保存済みチェーンを単一 Command でまとめて送る。どちらも無い実行は、
両方の指定方法を案内して送信前に止める（従来の空チェーン送信を廃止）。
route の解釈失敗は出所・何本目・該当文字列を示す（ADR-0018）。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: ドキュメント更新・ADR 昇格・フルゲート

**Files:**
- Modify: `vstc_cli/README.md`
- Modify: `docs/adr/0018-profile-default-chains-in-a-single-command.md`（Status 行のみ）
- Modify: `docs/adr/README.md`（索引の Status 列のみ）

**Interfaces:**
- Consumes: Task 1〜6 の全て
- Produces: なし（最終タスク）

- [ ] **Step 1: README に「既定チェーン」節を足す**

`vstc_cli/README.md` の「### 保存場所」節の**手前**に挿入する。

````markdown
### 既定チェーン

`send` に操作を書かなかったときに送るチェーンを、プロファイルに保存できる。チェーンは複数本持てて、1 回の送信でまとめて送られる。

```sh
# 追加（1 コマンド = 1 本のチェーン）
./vstc_cli.exe profile chains add main '//localhost:8081/transc' '//windesk:8080/sub'
./vstc_cli.exe profile chains add main '//localhost:8081/transc' 'transl?t=en' '//windesk:8080/sub?p=s'
./vstc_cli.exe profile chains add main '//localhost:8081/transc' 'transl?t=ru' '//windesk:8082/sub'

# 確認
./vstc_cli.exe profile chains show main
[1] //localhost:8081/transc -> //windesk:8080/sub
[2] //localhost:8081/transc -> transl?t=en -> //windesk:8080/sub?p=s
[3] //localhost:8081/transc -> transl?t=ru -> //windesk:8082/sub

# 削除（show が表示する番号を指定する）
./vstc_cli.exe profile chains del main 2
```

保存したら、操作を書かずに `send` するだけで全チェーンが送られる。

```sh
./vstc_cli.exe send --profile main -t "hello"
```

**操作を書いた `send` は保存済みチェーンを送らない。** 位置引数を渡した実行ではそれだけが 1 本のチェーンとして送られ、保存済みチェーンは無視される。どちらも無い場合はエラーになり、送信は行われない。

複数チェーンは 1 つの gRPC コマンドとしてまとめて送られるので、入力（テキスト / 音声 / file_path / filters）と trace_id は全チェーンで共有される。

`profile chains add` は保存する前に全 route を検証する。解釈できない route が 1 つでもあれば、何も保存されない。

#### route の書き方

route は 3 つの形で書ける。

| 形 | 意味 | 例 |
|---|---|---|
| `//<HOST>:<PORT>/<OP>?<QUERY>` | 宛先つきの 1 ホップ | `//windesk:8080/sub?p=s` |
| `<OP>?<QUERY>` | 宛先を指定しない 1 ホップ | `transl?t=en` |
| `o:/<OP>` / `o://<HOST>:<PORT>/<OP>` | 従来形式（引き続き有効） | `o:/tts?spd=1.1` |

`<OP>` に書けるのは `transc`（`transcribe`）/ `transl`（`translate`）/ `tts` / `play`（`playback`）/ `sub`（`subtitle`）/ `vc` / `reload` / `pause` / `resume` / `forward`（`fwd`）。

#### 保存された TOML

```toml
[profiles.main]
host = "localhost"
port = 19829
chains = [
    ["//localhost:8081/transc", "//windesk:8080/sub"],
    ["//localhost:8081/transc", "transl?t=en", "//windesk:8080/sub?p=s"],
    ["//localhost:8081/transc", "transl?t=ru", "//windesk:8082/sub"],
]
```

`profile set` はチェーンに触れないので、host / port を更新しても保存済みチェーンは消えない。
````

- [ ] **Step 2: README の既存記述を更新する**

「## プロファイル」節の `profile list` の例の下（63 行目付近の「値は「組み込み既定 → プロファイル → 明示フラグ」…」の段落の直後）に 1 行足す。

```markdown
`profile list` は各プロファイルの保存済みチェーン本数を `CHAINS` 列に表示する（未保存は `-`）。
```

「### 保存場所」節の TOML 例に `chains` を含む形を追記する。既存の例の直後に足す。

```markdown
既定チェーンを保存すると `chains` が加わる（[既定チェーン](#既定チェーン)を参照）。
```

- [ ] **Step 3: README の変更を目視確認する**

Run: `cargo run -p vstc_cli -- profile chains --help`
Expected: `add` / `del` / `show` の 3 サブコマンドが出て、README の記述と一致する。

- [ ] **Step 4: ADR-0018 を Accepted へ昇格する**

`docs/adr/0018-profile-default-chains-in-a-single-command.md` の Status 行**だけ**を書き換える。本文には触れない。

```markdown
- Status: Accepted
```

`docs/adr/README.md` の索引表の 0018 行の Status 列を `Proposed` → `Accepted` にする。

- [ ] **Step 5: ADR と実装を突合する**

ADR-0018 の Decision 3 項を実装と照合し、乖離が無いことを確認する。

1. 既定チェーンは operations 省略時のみ適用 → `chains_for_send`（Task 6）
2. 複数チェーンは単一 `Command` の複数 `chains` → `process_chains_with_operand` と結合テスト（Task 2）
3. スキーム省略形の正規化と `parse_route` 公開 → `normalize_op_str` / `parse_route`（Task 1）、保存前検証は `validate_routes`（Task 5）

乖離があれば、実装が正しいなら新 ADR で supersede、ADR が正しいなら実装を直す。**Accepted 本文は書き換えない。**

- [ ] **Step 6: フルゲートを実行する**

Run: `just ci`
Expected: exit code 0。全項目（fmt-check / clippy / clippy-profiling / test / test-profiling / check-env-leak / gitleaks / deny / audit）が緑。

赤が出た場合の対応は [CLAUDE.md](../../../CLAUDE.md) の「品質ゲート」節に従う。fmt-check だけは `just fmt` で機械的に解消してよい。clippy / test / deny を lint 緩和で黙らせるのは禁止。

- [ ] **Step 7: コミット**

```bash
git add vstc_cli/README.md docs/adr/0018-profile-default-chains-in-a-single-command.md docs/adr/README.md
git commit -F - <<'EOF'
docs(vstc_cli): 既定チェーンの使い方を README に追記し ADR-0018 を Accepted へ

profile chains add/del/show の使用例、route の 3 形式、保存される TOML、
send がどちらを送るかの規則を記載。実装が ADR-0018 の決定を裏づけたので
Status を昇格した。

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Self-Review

**1. Spec coverage** — spec の受入基準を全て走査した結果:

| spec の受入基準 | 実装タスク |
|---|---|
| `profile chains add` がチェーンを末尾に追加 | Task 3（`add_chain`）/ Task 5（配線） |
| `profile chains show` が 1 始まりの連番で表示 | Task 4（`render_chains`）/ Task 5 |
| 0 本のとき `show` が案内して正常終了 | Task 5（`chains_show_output`） |
| `profile chains del <INDEX>` が削除 | Task 3（`del_chain`）/ Task 5 |
| 範囲外の連番はエラーで保存内容不変 | Task 3 |
| `add` の route に不正があれば保存しない | Task 5（`validate_routes`） |
| 未登録名で新規作成しない | Task 3 |
| 最後の 1 本削除後は未保存と同じ状態 | Task 3 |
| 同じ TOML ファイルの同じプロファイル名の下 | Task 3（`Profile.chains`） |
| `profile list` が本数を表示、未保存は未設定表示 | Task 4（`render_list`） |
| 未保存プロファイルは TOML に項目を持たない | Task 3（`skip_serializing_if`） |
| 位置引数ありなら保存済みチェーンを送らない | Task 6（`chains_for_send`） |
| 位置引数なし + 保存済みなら全チェーン送信 | Task 6 |
| N 本が 1 コマンドで 1 回だけ届く | Task 2（結合テスト） |
| 同一入力・同一 trace_id を共有 | Task 2 |
| どちらも無ければ案内エラーで送信しない | Task 6 |
| 壊れた route はプロファイル名・本数目・文字列を含むエラー | Task 6（`parse_chains` + `chains_origin`） |
| 接続先の解決順序は従来どおり | Task 4（`resolve` は host/port に無変更） |
| `//HOST:PORT/OP?QUERY` の解釈 | Task 1 |
| `OP?QUERY` の解釈 | Task 1 |
| 従来の `o:` 形式の継続 | Task 1 |
| `add` と `send` で同じ解釈 | Task 1（両者が `parse_route` を通る） |
| 文字起こしを指定できる | Task 1（`transc` / `transcribe`） |
| README にチェーン操作例と TOML 例 | Task 7 |
| README に send の選択規則 | Task 7 |

未カバーの受入基準は無い。

**2. Placeholder scan** — 「TBD」「後で実装」「適切なエラー処理を追加」「Task N と同様」の類は無い。全コードステップに実際のコードを載せた。

**3. Type consistency** — タスク間で参照する名前を照合した。

- `parse_route`（Task 1 定義 → Task 2 / 5 / 6 で使用）: 一致
- `process_chains_with_operand`（Task 2 定義 → Task 6 で使用）: 一致
- `build_command(chains: Vec<Vec<OperationRoute>>, operand: RouteOperand)`（Task 2 で変更 → 既存テスト 2 件も同ステップで修正）: 一致
- `Profile.chains: Option<Vec<Vec<String>>>`（Task 3 定義 → Task 4 の `resolve` / `render_list` で使用）: 一致
- `add_chain` / `del_chain` / `chains_of`（Task 3 定義 → Task 4 テスト / Task 5 で使用）: 一致
- `Resolved.chains: Vec<Vec<String>>`（Task 4 定義 → Task 6 の `run_send` で使用）: 一致
- `render_chains`（Task 4 定義 → Task 5 の `chains_show_output` で使用）: 一致
- `ChainsCmd`（Task 5 定義 → 同タスク内のみ）: 一致
- `chains_for_send` / `chains_origin` / `parse_chains`（Task 6 で定義・使用）: 一致

`Profile` へのフィールド追加でコンパイルが壊れる既存リテラルは、Task 3 Step 5 で `profile.rs` 5 箇所・`store.rs` 1 箇所・`main.rs` 2 箇所として全て列挙済み。
