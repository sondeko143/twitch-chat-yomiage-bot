//! 環境固有値の混入ゲート。
//!
//! git 追跡ファイルの内容を走査し、個人/マシン依存の絶対パス
//! （ホームディレクトリ・AppData・Claude の内部メモリパス等）が
//! 紛れ込んでいないか検査する。plan/spec などのドキュメントに
//! 可搬性の無いローカルパスが混入する事故を CI で止めるのが目的。
//!
//! 入口: `just check-env-leak`（= `cargo run -p xtask -- check-env-leak`）。
//! このファイル自身は検査パターンをリテラルとして含むため走査対象から外す。

use std::error::Error;
use std::path::Path;
use std::process::Command;

type Res<T> = Result<T, Box<dyn Error>>;

/// 小文字化した行に対して照合する禁止部分文字列と、その説明。
/// `/users/` のように区切り付きにすることで `/helix/users` や
/// `followed_users` 等の正当な識別子・URL を弾かない。
const FORBIDDEN: &[(&str, &str)] = &[
    (":\\users\\", "Windows ホームの絶対パス"),
    ("/users/", "macOS ホーム / git-bash 形式の絶対パス"),
    ("/home/", "Linux ホームの絶対パス"),
    ("\\appdata\\", "Windows AppData 配下の絶対パス"),
    ("/appdata/", "Windows AppData 配下の絶対パス"),
    (".claude/projects", "Claude Code の内部プロジェクトパス"),
    (".claude\\projects", "Claude Code の内部プロジェクトパス"),
];

/// パターン定義をリテラルで含む自分自身は走査から除外する。
const SELF_PATH: &str = "xtask/src/leak.rs";

/// 1 件の違反。
struct Hit {
    file: String,
    line: usize,
    why: &'static str,
    text: String,
}

/// `git <args>` を実行して stdout を文字列で得る。
fn git_stdout(args: &[&str]) -> Res<String> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        return Err(format!("git {} に失敗しました", args.join(" ")).into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// 1 ファイルを走査して違反を返す。バイナリ・読めないファイルは空。
fn scan_file(root: &Path, rel: &str) -> Vec<Hit> {
    let bytes = match std::fs::read(root.join(rel)) {
        Ok(b) if !b.contains(&0) => b,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut hits = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        for (needle, why) in FORBIDDEN {
            if lower.contains(*needle) {
                hits.push(Hit {
                    file: rel.to_string(),
                    line: i + 1,
                    why,
                    text: line.chars().take(100).collect(),
                });
                break; // 1 行 1 件報告で十分
            }
        }
    }
    hits
}

/// 検出結果を stderr に整形出力する。
fn report(hits: &[Hit]) {
    eprintln!("check-env-leak: 環境固有値の混入を {} 件検出:", hits.len());
    for h in hits {
        eprintln!("  {}:{}  [{}]", h.file, h.line, h.why);
        eprintln!("    {}", h.text.trim());
    }
    eprintln!();
    eprintln!("個人/マシン依存の絶対パスが含まれています。");
    eprintln!("相対パス(例: ../sibling-repo)やプレースホルダ(<repo-root> 等)へ置換してください。");
}

/// 追跡ファイル全体を走査する。混入が 1 件でもあれば Err を返す。
pub fn run() -> Res<()> {
    let root_str = git_stdout(&["rev-parse", "--show-toplevel"])?;
    let root = root_str.trim();
    let files = git_stdout(&["-C", root, "ls-files"])?;
    let root_path = Path::new(root);
    let mut hits = Vec::new();
    for rel in files.lines().filter(|f| *f != SELF_PATH) {
        hits.extend(scan_file(root_path, rel));
    }
    if hits.is_empty() {
        println!("check-env-leak: OK（環境固有パスの混入なし）");
        return Ok(());
    }
    report(&hits);
    Err(format!("{} 件の環境固有値混入", hits.len()).into())
}
