//! Instrumented frame benchmarks over `as_gui::bench`'s scenarios, in Rune's `pinch_bench`
//! style: per scenario, per frame, it records commit, CPU (frame, then the draw's encode),
//! view and layout phase time and allocations, and prints median/p95 over samples.
//!
//! cargo run --profile perf --locked --no-default-features --features bench --example frame_bench
//!
//! `--samples N` (default 7) sets how many measured passes each scenario gets; `--scenario
//! NAME` runs one scenario instead of all five; `--json <path>` writes the median/p95 table
//! as the `frame_bench` section of a results record. The glow compositor is not in the draw
//! path, so its shader cost is not measured.
use std::collections::BTreeMap;

use as_gui::bench::{self, Bench, METRICS};

#[global_allocator]
static ALLOC: procwatch::alloc::Counting = procwatch::alloc::Counting::system();

const FRAMES: usize = 120;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let samples: usize = value("--samples").map_or(7, |s| s.parse().expect("--samples N"));
    let chosen: Vec<_> = match value("--scenario") {
        Some(name) => {
            vec![bench::scenario(&name).unwrap_or_else(|| panic!("no scenario named {name}"))]
        }
        None => bench::scenarios(),
    };
    if cfg!(debug_assertions) {
        eprintln!("frame_bench: debug build; time a --profile perf build");
    }
    let mut bench = Bench::new();
    let mut scenarios = BTreeMap::new();
    for scenario in chosen {
        let rows = scenario.sample(&mut bench, samples, FRAMES);
        let mut metrics = BTreeMap::new();
        for (m, name) in METRICS.iter().enumerate() {
            let mut v: Vec<f64> = rows.iter().map(|r| r[m]).collect();
            let (median, p95) = (bench::median(&mut v), bench::p95(&mut v));
            println!(
                "METRIC scenario={} metric={name} median={median:.4} p95={p95:.4} n={}",
                scenario.name(),
                v.len()
            );
            metrics.insert(
                name.to_string(),
                serde_json::json!({ "median": median, "p95": p95 }),
            );
        }
        scenarios.insert(scenario.name().to_string(), metrics);
    }
    if let Some(path) = value("--json") {
        let doc =
            serde_json::json!({ "samples": samples, "frames": FRAMES, "scenarios": scenarios });
        let json = serde_json::to_string_pretty(&doc).expect("the frame bench section serialises");
        std::fs::write(&path, json + "\n").unwrap_or_else(|e| panic!("{path}: {e}"));
    }
}
