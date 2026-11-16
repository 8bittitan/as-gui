---
name: new-benchmark
description: Add a frame-bench scenario to as-gui under benchmarks/cases/. Use when asked to benchmark, profile or measure a UI interaction or screen. Not for Criterion micro-benchmarks, which are not built yet.
---

# New benchmark

Adds one scenario to the instrumented frame benchmark (`just bench`).

## Before you start

Read `src/bench.rs`, then the existing case closest to yours in `benchmarks/cases/`:

- `idle.rs`: a static screen
- `stream.rs`: events sent every frame
- `tool_card.rs`: taps
- `scroll.rs`: wheel input
- `command_bar.rs`: keyboard input

Pick a kebab-case scenario name (it is the `--scenario` argument and the JSON key) and a snake_case file name.

## Rules for a case

- Stay deterministic. Never stream through the mock agent, because it sleeps on a real thread. Send `StreamEvent`s through the sender from `rt.state.begin_turn(..)`, kept in a `RefCell` as `stream.rs` does.
- Build the runtime with `bench::runtime(|s| ..)` and set fixtures inside the closure. Use `fill_conversation` and `published`. A new shared helper goes in `src/bench.rs` as `pub(crate)`.
- Frame 0 (`i == 0`) does the setup. Later frames repeat one action, and it must stay valid for all 120 frames (`scroll` reverses at frame 60).
- Find elements with `rt.find("<key>")`. Taps use the pointer move, down, up sequence from `tool_card.rs`.
- Start the file with a `//!` comment saying what the case exercises and why. Add no other comments.

## Steps

1. Write the case's `#[cfg(test)]` test first, using `one_frame("<name>")`. Assert the precondition: the element is on screen, or the turn is pending.
2. Create `benchmarks/cases/<file>.rs` with `pub(super) fn scenario() -> Scenario` until the test passes.
3. Wire it in. These places hard-code the list or the count:
   - `benchmarks/cases/mod.rs`: the `mod` line and the `all()` entry. `all()` order is the report order.
   - `src/bench.rs`: the `the_cases_cover_five_scenarios` test (rename it, update the array) and the "All five cases" doc line.
   - `benchmarks/runners/frame_bench.rs`: "instead of all five" in the header comment.
   - `CONTRIBUTING.md`, Benchmarks section: the count and the name list.
4. Verify in one batch, since each rebuilt binary can raise a Jamf permission dialog (see `AGENTS.md`):
   - `cargo test --locked --no-default-features --features bench <test_name>`. Case tests only exist with the `bench` feature, so `just test` skips them.
   - `cargo clippy --all-targets --locked --features bench -- -D warnings` (the CI line)
   - `just check`
   - `just bench -- --scenario <name> --samples 3`. Check that `commits` is not 0 when the case should commit frames.

## Results

Do not run `just bench-save`. It refuses a dirty tree. Tell the user to commit, then run `just bench-save`.

`just bench-compare` only diffs scenarios that are in the latest saved record, so the new case is not compared until a record that includes it exists.
