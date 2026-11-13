# Benchmark introduction tasks

Introduces benchmarks to as-gui following the three layers Rune itself uses (headless frame timing, instrumented frame benchmarks with `frame-profile`/`procwatch`, and later micro-benchmarks).

Execute **in order** — each phase depends on the previous one:

1. [Phase 0: library split](phase-0-library-split.md) — no benchmark code, zero behaviour change, its own commit
2. [Phase 1: frame timing & startup](phase-1-frame-timing.md) — `[profile.perf]`, `scripts/bench.json`, `just bench-frames` / `bench-startup` (hyperfine)
3. [Phase 2: instrumented frame benchmark](phase-2-frame-bench.md) — `bench` feature, `procwatch`, `src/bench.rs` scenarios, `just bench`, CI lint line
4. [Phase 3: Criterion micro-benchmarks](phase-3-micro-benchmarks.md) — **deferred**; design only, implement when profiling shows which pure functions matter

Shared context every task assumes:

- Rune pinned at rev `04b38ae0a73bda03f010e97d2fff540ab91f67b0` across all Rune crates; the reference for the benchmark pattern is `crates/rune/examples/pinch_bench.rs` in that repo.
- The gate is `just check`; benchmarks stay out of it (CI compiles the bench feature with clippy but never runs it).
- Batch test/UI runs on the dev Mac — every rebuilt executable can raise a Jamf permission dialog (see `AGENTS.md`, "Headless UI checks").
- Determinism rule: the mock agent sleeps on a real thread (`src/agent/mock.rs`), so measured loops never stream through the mock; they feed `StreamEvent`s through the `TurnSender` from `Chat::begin_turn`.
