# Phase 2: Instrumented frame benchmark in the `pinch_bench` style

**Status:** ready to implement (requires Phase 0 and Phase 1 landed)
**Goal:** per-scenario frame benchmarks (CPU, view/layout phases, allocations, commits) reported as median/p95, using the exact instrumentation Rune itself uses: the `frame-profile` feature, `procwatch`'s counting allocator, and Rune's phase clocks.
**Commit:** one commit (plus its tests written first, per the repo's TDD rule).

## Background — the pattern we are copying

Rune's `crates/rune/examples/pinch_bench.rs` (rev `04b38ae0a73bda03f010e97d2fff540ab91f67b0`) is the reference. Its headline table ("2,000 cards … CPU per frame … allocations per frame") comes from an example that:

1. Installs a counting global allocator:
   ```rust
   #[global_allocator]
   static ALLOC: procwatch::alloc::Counting = procwatch::alloc::Counting::system();
   ```
2. Builds a `Runtime` headlessly and drives it frame by frame.
3. Samples N runs; per sample, per frame, records:
   - `procwatch::alloc::thread_allocations()` deltas → allocations
   - `rune::thread_cpu_ms()` deltas → CPU time (frame, and the draw's encode separately)
   - `rune::take_frame_phases()` → view and layout time per commit
   - `rt.stats.commits` and `rt.live_only_events()` → commits
4. Prints `METRIC mode=.. metric=.. median=.. p95=..` over samples, each sample the mean of the 120 frames.

Build line: `cargo build --profile perf --features frame-profile` (in our case `--features bench`, which turns it on transitively). The metrics list in `pinch_bench.rs`: `["commits", "cpu_ms", "frame_ms", "encode_ms", "view_ms", "layout_ms", "allocations", "live_only_events"]`.

## Known constraint (design around it)

**The mock agent sleeps on a real thread** (`src/agent/mock.rs`: 150 ms before the stream, 20 ms between chunks). Benchmarks must be deterministic, so scenarios never stream through the mock. Instead they feed events directly through the sender from `Chat::begin_turn(&str) -> TurnSender` (defined in `src/chat/turn.rs`), the same way `src/tests/mod.rs` feeds `begin_agent_load` with `sender.send(StreamEvent::...)` and then `drop(sender)`.

## What to do

### 1. Dependency and feature

`Cargo.toml`:

- `[dependencies]`: add `procwatch` as optional, from the same Rune git repo and the **same rev** as the other four Rune crates:
  ```toml
  procwatch = { optional = true, git = "https://github.com/anomalyco/rune.git", rev = "04b38ae0a73bda03f010e97d2fff540ab91f67b0" }
  ```
- `[features]`:
  ```toml
  bench = ["rune/frame-profile", "dep:procwatch"]
  ```
- **Update `AGENTS.md`**: "The four Rune crates in `Cargo.toml` share one `rev`" becomes five. Change all together in future.

### 2. `src/bench.rs` — the scenario engine (feature-gated)

`#[cfg(feature = "bench")] pub mod bench;` from `src/lib.rs`. Everything in it is driven through one small public surface so `Chat` stays private:

```rust
pub struct Scenario { /* name, size, run closure */ }

impl Scenario {
    pub fn new(name: &str, build: impl FnOnce() -> Runtime<Chat>) -> Self;
    pub fn name(&self) -> &str;
    /// Warm-up + sampling loop. Returns per-sample means for each metric.
    pub fn sample(&self, samples: usize, frames: usize) -> Vec<Sample>;
}
```

Details:

- Runtime setup copies the test harness (`src/tests/mod.rs::test_chat_sized`): `Env { system_dark: true, scale: 2.0 }`, size **1100×720** (the `screenshots` recipe's size), `rt.poll = Some(chat::poll)`, an application registered and `mock_forced = true` so `poll` never touches the network.
- The draw step copies `pinch_bench.rs`'s `Draw` struct: `Gpu`, `Offscreen` with `OFFSCREEN_FORMAT`, `Renderer::prepare` → `create_command_encoder` → `draw` → `submit`, then `device.poll(wait_indefinitely)`. The glow compositor (`glow::Glow`) is **not** in this path; document that its shader cost is not measured.
- Each frame: `rt.step_fn` (scenario-specific input) then `rt.frame(1.0 / 60.0)`, bracketed by the allocation and CPU probes exactly as `pinch_bench.rs` does it.
- A `median`/`p95` helper over the per-sample means — **unit-test this helper** (even count, odd count, two samples).

### 3. Scenarios

All deterministic; every one starts settled (drive `settle`-style frames first) and then measures `frames = 120` frames:

| Scenario | Per-frame action |
| --- | --- |
| `idle` | nothing — a settled screen must cost ~nothing; this is the floor |
| `stream` | one `StreamEvent::TextDelta` chunk (a few chars) through the `begin_turn` sender, so markdown re-renders and the glow state advances each frame; pre-fill the conversation so the reply wraps |
| `tool-card` | alternate opening/closing a tool card with hits every 30 frames (send the tool-call events up front via the sender, then toggle with `rt` taps on the card header key, mirroring `scripts/chat.json`) |
| `scroll` | `rt` wheel/scroll step through a 200-message conversation pre-loaded via the directory sender |
| `command-bar` | type one character of a query per frame (open on frame 1), then close; covers `command_bar::entries` fuzzy matching over a large entry set |

Fill large fixtures (200 messages, many agents/conversations) through `MockDirectory`/`MockAgent` constructors in `src/agent/mock.rs`, not loops that call the network traits.

**Tests first (TDD):** before writing the scenarios, add `#[test]`s that build each scenario and run **one frame**, asserting the frame succeeded and (where cheap) that the expected element key exists (`rt.find("composer")` etc.). These tests run under the default feature set only if `bench` is on — gate them `#[cfg(feature = "bench")]` and add a CI lint line that compiles them (step 5). This catches renamed element keys, the most likely breakage.

### 4. `examples/frame_bench.rs`

```toml
[[example]]
name = "frame_bench"
required-features = ["bench"]
```

The example:

- Declares the counting allocator (as above)
- Parses `--samples N` (default 7, mirroring Rune) and `--scenario NAME` (default: all)
- Runs each scenario and prints, in Rune's format:
  `METRIC scenario=<name> metric=<m> median=<..> p95=<..>`
- Metrics: `commits`, `cpu_ms`, `frame_ms`, `encode_ms`, `view_ms`, `layout_ms`, `allocations` (skip `live_only_events` unless a scenario uses live values)

### 5. Justfile and CI

```just
# Runs the instrumented frame benchmarks (per-scenario median/p95 of CPU, phases, allocations).
bench frame_bench_args="":
  cargo run --profile perf --locked --no-default-features --features bench --example frame_bench -- {{frame_bench_args}}
```

(Phase 1 named its recipes `bench-*` precisely so this plain `bench` stays free.)

CI (`.github/workflows/ci.yml`, `check` job): the existing `cargo clippy --all-targets --locked -- -D warnings` **does not compile** the example without the feature. Add one line to the "default features" step:

```
cargo clippy --all-targets --locked --features bench -- -D warnings
```

This compiles the benchmark code (including its tests) but never runs it — timing numbers from shared runners are noise. Note: clippy with `--features bench` may hit the `knobs` + `bench` combination; if the feature resolver complains, run it exactly as written (bench implies no default knob change) rather than adding `--no-default-features`, and verify both clippy invocations pass locally before pushing.

### 6. Docs

- `CONTRIBUTING.md` Benchmarks section (from Phase 1): add the `just bench` usage, the scenario list, the glow-compositor caveat, and how to read the `METRIC` lines.
- `AGENTS.md`: the Rune-crate `rev` count.

## Verification (one batched run)

```
just check
cargo clippy --all-targets --locked --features bench -- -D warnings
just bench -- --samples 3            # quick sanity pass
just bench                            # the recorded run
```

Record the `just bench` output in the PR as the first baseline, next to Phase 1's `bench-frames` numbers. Compare `idle` (floor) vs `stream` (heaviest) to sanity-check that the numbers move in the expected direction.

## Definition of done

- [ ] `procwatch` added at the shared rev; `AGENTS.md` says five Rune crates
- [ ] `bench` feature wired to `rune/frame-profile`; example has `required-features`
- [ ] `src/bench.rs` with median/p95 unit tests and one-frame scenario tests (written first)
- [ ] Five scenarios implemented; all deterministic (no mock sleeps in the measured loop)
- [ ] `just bench` prints `METRIC scenario=.. metric=.. median=.. p95=..` lines
- [ ] CI compiles the bench feature via clippy
- [ ] `CONTRIBUTING.md` updated; glow caveat documented
- [ ] `just check` still passes; baseline recorded
