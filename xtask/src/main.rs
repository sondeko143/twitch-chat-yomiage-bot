//! 開発用タスクランナー。入口は justfile に集約: `just <recipe>`。
//!
//! コマンド:
//!   analyze-trace [path]   just profile-startup が出す Chrome trace を解析する
//!                          (既定 path: target/profile/trace.json)
//!   check-env-leak         追跡ファイルへの環境固有パス混入を検査する
//!                          (個人/マシン依存の絶対パスをゲート。leak.rs 参照)
//!
//! 注意: span の instances/active は async では wall-clock ではない
//! (.instrument() は await ごとに re-enter する)。回数はログで数え、
//! 待ち時間は "no-span gaps" で見ること。

mod leak;

use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;

const GAP_THRESHOLD_US: f64 = 15_000.0;
const DEFAULT_TRACE: &str = "target/profile/trace.json";

/// Chrome trace の 1 イベント。B/E 以外(メタ M など)は ts/name を持たないことがある。
#[derive(Debug, Deserialize)]
struct Event {
    ph: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tid: i64,
    #[serde(default)]
    ts: f64,
}

/// B/E をペアリングした 1 スパン (µs)。
#[derive(Debug, PartialEq)]
struct Span {
    name: String,
    start: f64,
    end: f64,
}

/// どの span も走っていない空白区間 (µs)。
#[derive(Debug, PartialEq)]
struct Gap {
    from: f64,
    to: f64,
}

/// B/E イベントを tid ごとのスタックでペアリングする。E の name は使わず B の name を採る。
fn pair_spans(events: &[Event]) -> Vec<Span> {
    let mut stacks: BTreeMap<i64, Vec<&Event>> = BTreeMap::new();
    let mut spans = Vec::new();
    for e in events {
        match e.ph.as_str() {
            "B" => stacks.entry(e.tid).or_default().push(e),
            "E" => {
                if let Some(b) = stacks.get_mut(&e.tid).and_then(Vec::pop) {
                    spans.push(Span {
                        name: b.name.clone(),
                        start: b.ts,
                        end: e.ts,
                    });
                }
            }
            _ => {}
        }
    }
    spans
}

fn by_start(a: &f64, b: &f64) -> Ordering {
    a.partial_cmp(b).unwrap_or(Ordering::Equal)
}

/// span を時系列にマージし、threshold(µs) を超える「無 span 空白」を返す。
fn compute_gaps(spans: &[Span], threshold: f64) -> Vec<Gap> {
    let mut sorted: Vec<&Span> = spans.iter().collect();
    sorted.sort_by(|a, b| by_start(&a.start, &b.start));

    let mut merged: Vec<(f64, f64)> = Vec::new();
    for s in sorted {
        match merged.last_mut() {
            Some(last) if s.start <= last.1 => last.1 = last.1.max(s.end),
            _ => merged.push((s.start, s.end)),
        }
    }

    merged
        .windows(2)
        .filter(|w| w[1].0 - w[0].1 > threshold)
        .map(|w| Gap {
            from: w[0].1,
            to: w[1].0,
        })
        .collect()
}

fn load_events(path: &str) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn print_timeline(spans: &[Span]) {
    let tmin = spans.iter().map(|s| s.start).fold(f64::INFINITY, f64::min);
    let tmax = spans
        .iter()
        .map(|s| s.end)
        .fold(f64::NEG_INFINITY, f64::max);
    println!("=== timeline ===");
    println!("first span start : {:.1} ms", tmin / 1000.0);
    println!(
        "last  span end   : {:.1} ms  (= wall-clock の概算)",
        tmax / 1000.0
    );
    println!();
}

fn print_per_span(spans: &[Span]) {
    let mut groups: BTreeMap<&str, Vec<&Span>> = BTreeMap::new();
    for s in spans {
        groups.entry(&s.name).or_default().push(s);
    }
    let mut rows: Vec<(&str, usize, f64, f64)> = groups
        .iter()
        .map(|(name, g)| {
            let lo = g.iter().map(|s| s.start).fold(f64::INFINITY, f64::min);
            let hi = g.iter().map(|s| s.end).fold(f64::NEG_INFINITY, f64::max);
            let active: f64 = g.iter().map(|s| s.end - s.start).sum();
            (*name, g.len(), (hi - lo) / 1000.0, active / 1000.0)
        })
        .collect();
    rows.sort_by(|a, b| by_start(&b.2, &a.2));

    println!("=== span ごと (instances は POLL 回数であり操作回数ではない) ===");
    println!(
        "{:<16} {:>9} {:>10} {:>10}",
        "name", "instances", "window_ms", "active_ms"
    );
    for (name, n, window, active) in rows {
        println!("{name:<16} {n:>9} {window:>10.1} {active:>10.2}");
    }
    println!();
}

fn print_gaps(spans: &[Span], threshold: f64) {
    let thr_ms = threshold / 1000.0;
    println!(
        "=== no-span gaps (>{thr_ms:.0}ms): 待ち時間。サーバRTT / 未計装 / backoff のいずれか ==="
    );
    for g in compute_gaps(spans, threshold) {
        println!(
            "  gap {:>7.1}ms  (from {:>7.1} to {:>7.1}ms)",
            (g.to - g.from) / 1000.0,
            g.from / 1000.0,
            g.to / 1000.0
        );
    }
}

fn analyze_trace(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let events = load_events(path)?;
    let spans = pair_spans(&events);
    if spans.is_empty() {
        return Err(format!("no B/E spans found in {path}").into());
    }
    print_timeline(&spans);
    print_per_span(&spans);
    print_gaps(&spans, GAP_THRESHOLD_US);
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("analyze-trace") => {
            let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_TRACE);
            analyze_trace(path)
        }
        Some("check-env-leak") => leak::run(),
        _ => {
            eprintln!("usage: cargo run -p xtask -- <command>");
            eprintln!("  analyze-trace [path]   Chrome trace を解析（既定 path: {DEFAULT_TRACE}）");
            eprintln!("  check-env-leak         追跡ファイルへの環境固有パス混入を検査");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ph: &str, name: &str, tid: i64, ts: f64) -> Event {
        Event {
            ph: ph.to_string(),
            name: name.to_string(),
            tid,
            ts,
        }
    }

    #[test]
    fn pairs_nested_begin_end_using_begin_name() {
        let events = vec![
            ev("M", "thread_name", 0, 0.0), // メタは無視
            ev("B", "outer", 1, 10.0),
            ev("B", "inner", 1, 12.0),
            ev("E", "", 1, 15.0), // E は name 無し → B の name を採る
            ev("E", "", 1, 20.0),
        ];
        assert_eq!(
            pair_spans(&events),
            vec![
                Span {
                    name: "inner".to_string(),
                    start: 12.0,
                    end: 15.0
                },
                Span {
                    name: "outer".to_string(),
                    start: 10.0,
                    end: 20.0
                },
            ]
        );
    }

    #[test]
    fn pairs_are_isolated_per_tid() {
        let events = vec![
            ev("B", "a", 1, 10.0),
            ev("B", "b", 2, 11.0), // 別 tid。a を閉じてはいけない
            ev("E", "", 1, 15.0),  // tid1 の a を閉じる
            ev("E", "", 2, 20.0),  // tid2 の b を閉じる
        ];
        assert_eq!(
            pair_spans(&events),
            vec![
                Span {
                    name: "a".to_string(),
                    start: 10.0,
                    end: 15.0
                },
                Span {
                    name: "b".to_string(),
                    start: 11.0,
                    end: 20.0
                },
            ]
        );
    }

    #[test]
    fn gaps_merge_overlaps_and_filter_by_threshold() {
        let spans = vec![
            Span {
                name: "a".to_string(),
                start: 0.0,
                end: 100.0,
            },
            Span {
                name: "b".to_string(),
                start: 50.0,
                end: 120.0,
            }, // a と重複 → [0,120]
            Span {
                name: "c".to_string(),
                start: 200.0,
                end: 210.0,
            }, // gap 120..200 = 80
            Span {
                name: "d".to_string(),
                start: 215.0,
                end: 220.0,
            }, // gap 210..215 = 5 (閾値未満)
        ];
        assert_eq!(
            compute_gaps(&spans, 15.0),
            vec![Gap {
                from: 120.0,
                to: 200.0
            }]
        );
    }
}
