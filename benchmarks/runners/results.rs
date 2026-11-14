//! Benchmark results: `save` merges the three benchmarks' outputs plus metadata into one
//! JSON record under `benchmarks/results/`, and `compare` diffs a fresh run against a saved
//! one, exiting non-zero on regression. History is append-only; numbers only compare within
//! one machine and one Rune revision, both of which every record carries.
//!
//! The Justfile and `scripts/bench-*.sh` gather the metadata and the three benchmarks'
//! outputs; this tool stays free of process and OS calls so its logic tests with fixed inputs.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const DETERMINISTIC: f64 = 0.005;
const CPU: f64 = 0.10;
const STARTUP_SIGMAS: f64 = 2.0;

#[derive(Serialize, Deserialize, Clone)]
struct Record {
    commit: String,
    rune_rev: String,
    date: String,
    machine: String,
    os: String,
    frame_bench: FrameBench,
    frames: FramesSummary,
    startup: Startup,
}

#[derive(Serialize, Deserialize, Clone)]
struct FrameBench {
    samples: usize,
    frames: usize,
    scenarios: BTreeMap<String, BTreeMap<String, Stat>>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Stat {
    median: f64,
    p95: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct FramesSummary {
    frames: u64,
    first_frame_ms: f64,
    cpu_mean_ms: f64,
    cpu_p50_ms: f64,
    cpu_p95_ms: f64,
    cpu_max_ms: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Startup {
    runs: usize,
    mean_ms: f64,
    stddev_ms: f64,
    median_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Deterministic,
    Cpu,
    Startup,
}

#[derive(Clone)]
struct Row {
    path: String,
    kind: Kind,
    old: f64,
    new: f64,
    sigma: f64,
}

fn flagged(row: &Row) -> bool {
    match row.kind {
        Kind::Deterministic => (row.new - row.old).abs() > DETERMINISTIC * row.old.abs(),
        Kind::Cpu => (row.new - row.old).abs() > CPU * row.old.abs(),
        Kind::Startup => (row.new - row.old).abs() > STARTUP_SIGMAS * row.sigma,
    }
}

/// The `frames` section, out of the run's two `rune:` summary lines.
fn parse_frames_log(log: &str) -> Result<FramesSummary, String> {
    let plain = strip_ansi(log);
    let line = plain
        .lines()
        .find(|l| l.starts_with("rune: ") && l.contains("frames at"))
        .ok_or_else(|| {
            "no `rune: N frames at …` summary line in the bench-frames log".to_string()
        })?;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let number = |key: &str| -> Result<f64, String> {
        let i = tokens
            .iter()
            .position(|t| *t == key)
            .ok_or_else(|| format!("the summary line has no `{key}`"))?;
        let raw = tokens
            .get(i + 1)
            .ok_or_else(|| format!("the summary line has nothing after `{key}`"))?;
        raw.trim_end_matches(',')
            .parse()
            .map_err(|_| format!("`{key}` is not a number: {raw}"))
    };
    let frames = tokens
        .get(1)
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| "the summary line does not start with a frame count".to_string())?;
    Ok(FramesSummary {
        frames,
        first_frame_ms: number("frame")?,
        cpu_mean_ms: number("mean")?,
        cpu_p50_ms: number("p50")?,
        cpu_p95_ms: number("p95")?,
        cpu_max_ms: number("max")?,
    })
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.next() == Some('[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The `startup` section, out of hyperfine's `--export-json` file (seconds there, ms here).
fn parse_hyperfine(json: &str) -> Result<Startup, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("hyperfine json: {e}"))?;
    let run = value
        .get("results")
        .and_then(|r| r.get(0))
        .ok_or_else(|| "the hyperfine export has no results".to_string())?;
    let seconds = |key: &str| -> Result<f64, String> {
        run.get(key)
            .and_then(|v| v.as_f64())
            .map(|s| s * 1000.0)
            .ok_or_else(|| format!("the hyperfine export has no `{key}`"))
    };
    Ok(Startup {
        runs: run
            .get("times")
            .and_then(|t| t.as_array())
            .map(|t| t.len())
            .unwrap_or(0),
        mean_ms: seconds("mean")?,
        stddev_ms: seconds("stddev")?,
        median_ms: seconds("median")?,
        min_ms: seconds("min")?,
        max_ms: seconds("max")?,
    })
}

fn frame_bench_json(json: &str) -> Result<FrameBench, String> {
    serde_json::from_str(json).map_err(|e| format!("frame-bench json: {e}"))
}

fn record_name(stamp: &str, commit: &str) -> String {
    format!("{stamp}-{commit}.json")
}

struct Meta<'a> {
    commit: &'a str,
    rune_rev: &'a str,
    machine: &'a str,
    os: &'a str,
    date: &'a str,
}

fn build_record(
    frames_bench: &FrameBench,
    frames: &FramesSummary,
    startup: &Startup,
    meta: Meta<'_>,
) -> Record {
    Record {
        commit: meta.commit.into(),
        rune_rev: meta.rune_rev.into(),
        date: meta.date.into(),
        machine: meta.machine.into(),
        os: meta.os.into(),
        frame_bench: frames_bench.clone(),
        frames: *frames,
        startup: *startup,
    }
}

/// The newest record file in `dir` (filenames start with the UTC stamp, so they sort in time
/// order), or an error when the directory holds none.
fn newest_record(dir: &Path) -> Result<PathBuf, String> {
    let mut best: Option<(String, PathBuf)> = None;
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".json") && best.as_ref().is_none_or(|(b, _)| name > b.as_str()) {
            best = Some((name.to_string(), path));
        }
    }
    best.map(|(_, path)| path)
        .ok_or_else(|| format!("no records under {}", dir.display()))
}

/// One comparison row per measured number, in a stable order: the frame bench's scenarios,
/// then the frames summary, then startup.
fn rows(old: &Record, new: &Record) -> Result<Vec<Row>, String> {
    let mut out = Vec::new();
    let deterministic = |m: &str| m == "commits" || m == "allocations" || m == "live_only_events";
    for (scenario, metrics) in &old.frame_bench.scenarios {
        let Some(new_metrics) = new.frame_bench.scenarios.get(scenario) else {
            return Err(format!("scenario `{scenario}` is missing from the new run"));
        };
        for (metric, stat) in metrics {
            let Some(new_stat) = new_metrics.get(metric) else {
                return Err(format!("metric `{metric}` is missing from the new run"));
            };
            let kind = if deterministic(metric) {
                Kind::Deterministic
            } else {
                Kind::Cpu
            };
            out.push(Row {
                path: format!("frame_bench/{scenario}/{metric}/median"),
                kind,
                old: stat.median,
                new: new_stat.median,
                sigma: 0.0,
            });
            out.push(Row {
                path: format!("frame_bench/{scenario}/{metric}/p95"),
                kind,
                old: stat.p95,
                new: new_stat.p95,
                sigma: 0.0,
            });
        }
    }
    for (metric, kind, old_value, new_value) in [
        (
            "frames/frames",
            Kind::Deterministic,
            old.frames.frames as f64,
            new.frames.frames as f64,
        ),
        (
            "frames/first_frame_ms",
            Kind::Cpu,
            old.frames.first_frame_ms,
            new.frames.first_frame_ms,
        ),
        (
            "frames/cpu_mean_ms",
            Kind::Cpu,
            old.frames.cpu_mean_ms,
            new.frames.cpu_mean_ms,
        ),
        (
            "frames/cpu_p50_ms",
            Kind::Cpu,
            old.frames.cpu_p50_ms,
            new.frames.cpu_p50_ms,
        ),
        (
            "frames/cpu_p95_ms",
            Kind::Cpu,
            old.frames.cpu_p95_ms,
            new.frames.cpu_p95_ms,
        ),
        (
            "frames/cpu_max_ms",
            Kind::Cpu,
            old.frames.cpu_max_ms,
            new.frames.cpu_max_ms,
        ),
    ] {
        out.push(Row {
            path: metric.into(),
            kind,
            old: old_value,
            new: new_value,
            sigma: 0.0,
        });
    }
    out.push(Row {
        path: "startup/mean_ms".into(),
        kind: Kind::Startup,
        old: old.startup.mean_ms,
        new: new.startup.mean_ms,
        sigma: (old.startup.stddev_ms * old.startup.stddev_ms
            + new.startup.stddev_ms * new.startup.stddev_ms)
            .sqrt(),
    });
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("save") => save(&args[1..]),
        Some("compare") => compare(&args[1..]),
        _ => die("usage: results save|compare …"),
    }
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn die(message: &str) -> ! {
    eprintln!("results: {message}");
    std::process::exit(1);
}

fn save(args: &[String]) {
    let record = current_record(args);
    let Some(stamp) = value(args, "--stamp") else {
        die("save: needs --stamp <utc-stamp>")
    };
    let Some(out_dir) = value(args, "--out-dir") else {
        die("save: needs --out-dir <dir>")
    };
    let path = Path::new(&out_dir).join(record_name(&stamp, &record.commit));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| die(&format!("{}: {e}", parent.display())));
    }
    let json = serde_json::to_string_pretty(&record).expect("the record serialises");
    if let Err(e) = fs::write(&path, json + "\n") {
        die(&format!("{}: {e}", path.display()));
    }
    println!("results: saved {}", path.display());
}

fn compare(args: &[String]) {
    let current = current_record(args);
    let old_path = match value(args, "--against") {
        Some(path) => PathBuf::from(path),
        None => {
            let Some(dir) = value(args, "--out-dir") else {
                die("compare: needs --out-dir <dir> or --against <file>")
            };
            newest_record(Path::new(&dir)).unwrap_or_else(|e| die(&e))
        }
    };
    let json = fs::read_to_string(&old_path)
        .unwrap_or_else(|e| die(&format!("{}: {e}", old_path.display())));
    let old: Record = serde_json::from_str(&json)
        .unwrap_or_else(|e| die(&format!("{}: {e}", old_path.display())));
    if old.rune_rev != current.rune_rev {
        eprintln!(
            "results: warning: the old record ran against Rune {}, this one against {}",
            old.rune_rev, current.rune_rev
        );
    }
    if old.machine != current.machine {
        eprintln!(
            "results: warning: the old record ran on {}, this one on {}",
            old.machine, current.machine
        );
    }
    let rows = rows(&old, &current).unwrap_or_else(|e| die(&e));
    println!(
        "{:<46} {:>10} {:>10} {:>8}",
        "metric", "old", "new", "change"
    );
    let mut regressions = 0;
    for row in &rows {
        let change = if row.old == 0.0 {
            "—".to_string()
        } else {
            format!("{:+.1}%", (row.new - row.old) / row.old * 100.0)
        };
        let mark = if flagged(row) {
            regressions += 1;
            "FLAG"
        } else {
            ""
        };
        println!(
            "{:<46} {:>10.4} {:>10.4} {:>8} {}",
            row.path, row.old, row.new, change, mark
        );
    }
    if regressions > 0 {
        die(&format!(
            "{regressions} metric(s) regressed against {}",
            old_path.display()
        ));
    }
    println!("results: no regressions against {}", old_path.display());
}

/// Reads the three benchmark outputs and builds the run's record; dies when an input is
/// missing or unparseable.
fn current_record(args: &[String]) -> Record {
    let frame_bench = match frame_bench_json(&read(args, "--frames-bench")) {
        Ok(f) => f,
        Err(e) => die(&e),
    };
    let frames = match parse_frames_log(&read(args, "--frames-log")) {
        Ok(f) => f,
        Err(e) => die(&e),
    };
    let startup = match parse_hyperfine(&read(args, "--startup")) {
        Ok(s) => s,
        Err(e) => die(&e),
    };
    build_record(
        &frame_bench,
        &frames,
        &startup,
        Meta {
            commit: &needed(args, "--commit"),
            rune_rev: &needed(args, "--rune-rev"),
            machine: &needed(args, "--machine"),
            os: &needed(args, "--os"),
            date: &needed(args, "--date"),
        },
    )
}

fn read(args: &[String], name: &str) -> String {
    let path = needed(args, name);
    fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("{path}: {e}")))
}

fn needed(args: &[String], name: &str) -> String {
    match value(args, name) {
        Some(v) => v,
        None => die(&format!("needs {name} <value>")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const RUNE_LINES: &str = "rune: 901 frames at 60 fps, gpu init 8.3 ms, first frame 1.75 ms, \
frame cpu mean 0.262 ms p50 0.188 p95 0.388 max 16.029, 699 instances\n\
rune: animating frames 495 (cpu p50 0.188 p95 0.473 ms), at-rest frames 405 (the window draws \
none; forced cpu p50 0.189 p95 0.315 ms), layout nodes 131, glyphs 658\n";

    const HYPERFINE: &str = r#"{"results":[{"command":"target/perf/as-gui","mean":0.1179,
"stddev":0.0023,"median":0.1178,"user":0.0373,"system":0.0231,"min":0.1156,"max":0.1226,
"times":[0.1156,0.1179,0.1219,0.1184,0.1163,0.1226,0.1203]}]}"#;

    fn frame_bench_fixture() -> FrameBench {
        frame_bench_json(
            r#"{"samples": 7, "frames": 120, "scenarios": {
                "idle": {"cpu_ms": {"median": 0.1315, "p95": 0.1461},
                         "allocations": {"median": 33.0, "p95": 33.0}}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn the_rune_summary_lines_parse_into_the_frames_section() {
        let frames = parse_frames_log(RUNE_LINES).unwrap();
        assert_eq!(frames.frames, 901);
        assert_eq!(frames.first_frame_ms, 1.75);
        assert_eq!(frames.cpu_mean_ms, 0.262);
        assert_eq!(frames.cpu_p50_ms, 0.188);
        assert_eq!(frames.cpu_p95_ms, 0.388);
        assert_eq!(frames.cpu_max_ms, 16.029);
    }

    #[test]
    fn a_rune_log_missing_fields_fails_cleanly() {
        let short = "rune: 901 frames at 60 fps, gpu init 8.3 ms, first frame 1.75 ms\n";
        assert!(parse_frames_log(short).is_err());
        assert!(parse_frames_log("nothing useful here\n").is_err());
    }

    #[test]
    fn the_hyperfine_export_parses_into_the_startup_section() {
        let startup = parse_hyperfine(HYPERFINE).unwrap();
        assert_eq!(startup.runs, 7);
        assert!((startup.mean_ms - 117.9).abs() < 1e-9);
        assert!((startup.stddev_ms - 2.3).abs() < 1e-9);
        assert!((startup.min_ms - 115.6).abs() < 1e-9);
        assert!((startup.max_ms - 122.6).abs() < 1e-9);
    }

    #[test]
    fn a_hyperfine_export_without_results_fails_cleanly() {
        assert!(parse_hyperfine(r#"{"results": []}"#).is_err());
    }

    #[test]
    fn a_record_merges_the_three_sections_and_names_its_file() {
        let record = build_record(
            &frame_bench_fixture(),
            &parse_frames_log(RUNE_LINES).unwrap(),
            &parse_hyperfine(HYPERFINE).unwrap(),
            Meta {
                commit: "1f7b0a9",
                rune_rev: "04b38ae",
                machine: "MacBookPro18,3",
                os: "26.1",
                date: "2026-11-13T21:40:00Z",
            },
        );
        assert_eq!(record.commit, "1f7b0a9");
        assert_eq!(
            record.frame_bench.scenarios["idle"]["cpu_ms"].median,
            0.1315
        );
        assert_eq!(record.frames.cpu_p95_ms, 0.388);
        assert_eq!(record.startup.runs, 7);
        assert_eq!(
            record_name("2026-11-13T2140Z", "1f7b0a9"),
            "2026-11-13T2140Z-1f7b0a9.json"
        );
    }

    #[test]
    fn deterministic_metrics_flag_the_smallest_change() {
        let row = |old: f64, new: f64| Row {
            path: "x".into(),
            kind: Kind::Deterministic,
            old,
            new,
            sigma: 0.0,
        };
        assert!(flagged(&row(1.0, 1.01)));
        assert!(!flagged(&row(100.0, 100.5)), "at the threshold passes");
        assert!(!flagged(&row(100.0, 100.4)));
        assert!(flagged(&row(0.0, 0.03)), "zero to anything is a change");
        assert!(!flagged(&row(0.0, 0.0)));
    }

    #[test]
    fn cpu_metrics_allow_ten_percent() {
        let row = |old: f64, new: f64| Row {
            path: "x".into(),
            kind: Kind::Cpu,
            old,
            new,
            sigma: 0.0,
        };
        assert!(!flagged(&row(100.0, 110.0)), "at the threshold passes");
        assert!(flagged(&row(100.0, 110.1)));
    }

    #[test]
    fn startup_flags_outside_two_sigma_of_the_two_runs() {
        let row = |old: f64, new: f64, old_sigma: f64, new_sigma: f64| Row {
            path: "startup/mean_ms".into(),
            kind: Kind::Startup,
            old,
            new,
            sigma: (old_sigma * old_sigma + new_sigma * new_sigma).sqrt(),
        };
        assert!(!flagged(&row(117.9, 121.0, 2.3, 2.3)));
        assert!(flagged(&row(117.9, 126.0, 2.3, 2.3)));
    }

    #[test]
    fn the_newest_record_wins_over_the_directory_order() {
        let dir = std::env::temp_dir().join("as-gui-results-test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-11-13T2140Z-aaaaaaa.json"), "{}").unwrap();
        fs::write(dir.join("2026-11-20T1015Z-bbbbbbb.json"), "{}").unwrap();
        fs::write(dir.join("notes.txt"), "not a record").unwrap();
        assert_eq!(
            newest_record(&dir).unwrap(),
            dir.join("2026-11-20T1015Z-bbbbbbb.json")
        );
        assert!(newest_record(Path::new("/nonexistent-as-gui-dir")).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rows_cover_the_three_sections_in_order() {
        let old = build_record(
            &frame_bench_fixture(),
            &parse_frames_log(RUNE_LINES).unwrap(),
            &parse_hyperfine(HYPERFINE).unwrap(),
            Meta {
                commit: "aaaaaaa",
                rune_rev: "04b38ae",
                machine: "MacBookPro18,3",
                os: "26.1",
                date: "2026-11-13T21:40:00Z",
            },
        );
        let mut new = old.clone();
        let idle = new.frame_bench.scenarios.get_mut("idle").unwrap();
        idle.get_mut("cpu_ms").unwrap().median = 0.2;
        new.frames.cpu_mean_ms = 0.3;
        new.startup.mean_ms = 120.0;
        let mut rows = rows(&old, &new).unwrap();
        assert_eq!(rows[0].path, "frame_bench/idle/allocations/median");
        assert_eq!(rows[0].kind, Kind::Deterministic);
        assert_eq!(rows[1].path, "frame_bench/idle/allocations/p95");
        assert_eq!(rows[2].path, "frame_bench/idle/cpu_ms/median");
        assert_eq!(rows[2].kind, Kind::Cpu);
        assert!((rows[2].new - 0.2).abs() < 1e-9);
        assert!(
            rows.iter()
                .any(|r| r.path == "frames/cpu_mean_ms" && r.kind == Kind::Cpu)
        );
        assert!(
            rows.iter()
                .any(|r| r.path == "frames/frames" && r.kind == Kind::Deterministic)
        );
        let startup = rows.pop().unwrap();
        assert_eq!(startup.path, "startup/mean_ms");
        assert_eq!(startup.kind, Kind::Startup);
        assert!(!flagged(&startup.clone()));
        let regressed = Row {
            new: 126.0,
            ..startup
        };
        assert!(flagged(&regressed), "126.0 is outside 2σ of 117.9 ± 2.3");
    }
}
