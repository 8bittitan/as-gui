set dotenv-load
set shell := ["bash", "-euo", "pipefail", "-c"]

set default-list := true

build:
  cargo build --release --locked --no-default-features

fmt:
  cargo fmt

lint:
  cargo clippy --all-targets --locked --no-default-features -- -D warnings

test:
  cargo test --locked --no-default-features

check:
  cargo fmt --check
  just lint
  just test

dev:
  cargo run

# Records a demo video into demos/: the main tour by default, or quick-ask, menu-bar, ask-selection, notify or all.
demo name="tour":
  scripts/demo.sh "{{name}}"

bundle:
  packaging/bundle.sh

dmg:
  packaging/bundle.sh dmg

screenshots:
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/conversation.png --scene conversation --size 1100x720 --theme light
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/search-panel.png --scene search --size 1100x720 --theme light
  AS_GUI_DEMO=1 cargo run --release --locked --no-default-features -- --snap docs/command-bar.png --scene command-bar --size 1100x720 --theme light

# Builds the benchmark binary (release codegen, non-incremental).
bench-build:
  cargo build --profile perf --locked --no-default-features

# Replays scripts/bench.json headlessly and prints Rune's per-frame CPU stats.
bench-frames: bench-build
  AS_GUI_DEMO=1 target/perf/as-gui --script scripts/bench.json

# Times process startup: launch to settled first frame of the conversation scene.
bench-startup: bench-build
  AS_GUI_DEMO=1 hyperfine --warmup 1 --runs 7 \
    "target/perf/as-gui --snap target/perf/bench-startup.png --scene conversation --size 1100x720"

# Runs the instrumented frame benchmarks (per-scenario median/p95 of CPU, phases, allocations).
bench *frame_bench_args="":
  cargo run --profile perf --locked --no-default-features --features bench --example frame_bench -- {{frame_bench_args}}

changelog:
  git cliff -u -o CHANGELOG.md

release version:
  scripts/release.sh "{{version}}"
