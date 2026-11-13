#!/bin/bash
# Runs all three benchmarks and compares them against the latest saved record
# (or the one given with --against <file>). Saves nothing; exits non-zero on regression.
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p target/perf
just bench-build
cargo build --profile perf --locked --no-default-features --features bench \
  --example frame_bench --example results

echo "== frame bench =="
target/perf/examples/frame_bench --json target/perf/frame-bench.json

echo "== frame replay =="
AS_GUI_DEMO=1 target/perf/as-gui --script scripts/bench.json 2>&1 |
  tee target/perf/bench-frames.log

echo "== startup =="
AS_GUI_DEMO=1 hyperfine --warmup 1 --runs 7 --export-json target/perf/bench-startup.json \
  "target/perf/as-gui --snap target/perf/bench-startup.png --scene conversation --size 1100x720"

target/perf/examples/results compare \
  --frames-bench target/perf/frame-bench.json \
  --frames-log target/perf/bench-frames.log \
  --startup target/perf/bench-startup.json \
  --commit "$(git rev-parse --short HEAD)" \
  --rune-rev "$(sed -n 's/.*rev=\([0-9a-f]\{7\}\).*/\1/p' Cargo.lock | head -1)" \
  --machine "$(sysctl -n hw.model)" \
  --os "$(sw_vers -productVersion)" \
  --date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --out-dir benchmarks/results \
  "$@"
