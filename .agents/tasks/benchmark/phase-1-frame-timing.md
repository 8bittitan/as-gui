# Phase 1: Frame-timing and startup benchmarks from the command line

**Status:** ready to implement (requires Phase 0 landed first)
**Goal:** measure per-frame CPU cost of a realistic session and process startup time, using Rune's built-in headless tooling and hyperfine. Almost no Rust code in this phase — a profile, a drive script, three `just` recipes and docs.
**Commit:** one commit after Phase 0.

## Background

Rune's README (at our pin, `04b38ae0a73bda03f010e97d2fff540ab91f67b0`) promises: "**Timing**: every headless run prints per-frame CPU time (update, build and encode/submit, without readback)." Rune's own headline number ("Frame CPU: over 180 frames of `examples/settings/scripts/bench.json` … mean 0.13 ms, p95 0.29 ms") is produced exactly this way: replay a drive script over N frames and read the printed stats.

Rune also defines this profile in its workspace, and Cargo ignores profiles from dependencies, so we copy it:

```toml
# Benchmarks and perf numbers. Same codegen as release, without incremental, which can move
# frame timings.
[profile.perf]
inherits = "release"
opt-level = 3
codegen-units = 16
lto = false
debug = false
incremental = false
```

## What to do

### 1. `Cargo.toml`: add `[profile.perf]`

Use the block above. Our crate also sets `strip = true` on `[profile.release]` — leave the perf profile **unstripped** so a `perf`/Instruments attach works later; add one line `strip = false` with a brief comment, matching the comment style of the existing `[profile.dev.package."*"]` note.

### 2. `scripts/bench.json`: a realistic session drive script

Model it on the existing `scripts/chat.json` (timestamped `tap` / `type` events naming element keys) and the demo scripts (`scripts/demo.json`). Cover the app's hot paths:

1. Pick the mock shopping agent (`tap` on the agent row key — copy the key used in `scripts/chat.json`: `agent-mock-shopping`)
2. `tap` the `composer`, `type` a question, `tap` `send`
3. Wait for the mock reply to stream (the reply must finish; give a generous `t`)
4. `tap` a tool card header (e.g. `tool-schedule_task_tool` from `scripts/chat.json`)
5. `scroll` the conversation (Rune scripts take `"scroll": [width, height, dy]`)
6. Open the command bar (⌘K — use the `"key"` event with the chord the app binds, check `src/keymap.rs` for the default; or `tap` the command bar entry point)
7. `type` a short query, then Escape
8. Open settings, switch the theme, close settings

Constraints:

- The mock agent streams on wall-clock sleeps (`src/agent/mock.rs` sleeps 150 ms before the stream and 20 ms per chunk). Keep the script's timestamps comfortably larger than the mock's total stream time so every run replays the same way.
- Keep total script length moderate (~10–15 s of script time); the per-frame print covers the whole run.
- Verify every key resolves: run once with `--tree tree.json` and grep for each key used; Rune fails a `tap` on a missing key, so a successful run is itself the check.

### 3. `Justfile` recipes

```just
# Builds the benchmark binary (release codegen, non-incremental).
bench-build:
  cargo build --profile perf --locked --no-default-features

# Replays scripts/bench.json headlessly and prints Rune's per-frame CPU stats.
bench-frames: bench-build
  AS_GUI_DEMO=1 target/perf/as-gui --script scripts/bench.json
```

Keep the existing Justfile conventions (`set dotenv-load` is already on; recipes use lowercase kebab names; comments above each recipe in the style of `demo`).

Notes:

- `--no-default-features` matches `just build`/`just lint`/`just test` (shipped configuration, no knobs panel).
- `AS_GUI_DEMO=1` forces the demo agent and ignores saved state so numbers are reproducible on any machine (see `AGENTS.md`, Environment). It applies to debug builds and headless runs — headless is what we are, but set it explicitly anyway.
- Do **not** add a `bench` recipe name that shadows `cargo bench` semantics; the names above are distinct on purpose.

### 4. Startup benchmark with hyperfine

- Add `hyperfine` to `mise.toml` so its version is pinned alongside `just` and the Rust toolchain. Both `rust-toolchain.toml` and `mise.toml` are pinned in this repo — change them together when relevant; here only `mise.toml` gains a tool.
- Recipe:

```just
# Times process startup: launch to settled first frame of the conversation scene.
bench-startup: bench-build
  AS_GUI_DEMO=1 hyperfine --warmup 1 --runs 7 \
    "target/perf/as-gui --snap target/perf/bench-startup.png --scene conversation --size 1100x720"
```

- `--snap` runs fully offscreen and exits, which is what makes wall-clock timing meaningful. The PNG is a throwaway; write it under `target/perf/` so it never lands in the repo.
- Keep `--runs 7` to mirror Rune's own "one warmup, seven runs" startup methodology (`scripts/bench-startup.py` in Rune uses the same counts).
- If `--scene conversation` shows the welcome/credentials dialog instead, first check how the `screenshots` recipe handles it (it doesn't need to — `AS_GUI_DEMO=1` supplies the demo agent); adjust the scene choice if the tree shows a dialog.

### 5. Docs: a "Benchmarks" section in `CONTRIBUTING.md`

Short section covering:

- What each recipe measures and what the printed per-frame numbers mean (update, build, encode/submit CPU, without readback — quote Rune's wording)
- That numbers are only comparable on the same machine and Rune revision
- That `target/perf/` is unstripped on purpose
- That Phase 2 (`cargo run --example frame_bench`) adds scenario-level benchmarks; link the task file `agents/tasks/benchmark/phase-2-frame-bench.md`

## Verification (one batched run)

```
just check
just bench-build
just bench-frames        # read the printed mean/p95/first-frame lines
just bench-startup       # read hyperfine's table
```

Record a baseline: paste the `bench-frames` per-frame stats and hyperfine's mean ± σ into the PR description. Later phases compare against these numbers.

## Definition of done

- [ ] `[profile.perf]` in `Cargo.toml` matches Rune's, plus `strip = false`
- [ ] `scripts/bench.json` replays cleanly (no missing-key failures) and covers send, tool card, scroll, ⌘K, settings/theme
- [ ] `bench-build`, `bench-frames`, `bench-startup` recipes work from a clean shell
- [ ] `hyperfine` pinned in `mise.toml`
- [ ] `CONTRIBUTING.md` has the Benchmarks section
- [ ] `just check` still passes (the profile change must not affect the gate)
- [ ] Baseline numbers recorded in the PR
