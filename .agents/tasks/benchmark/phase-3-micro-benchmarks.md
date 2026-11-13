# Phase 3 (planned for later): Criterion micro-benchmarks for pure code

**Status:** deliberately deferred — design captured now so Phase 0's split doesn't get undone later. Do **not** implement until Phases 0–2 are landed and the frame benchmarks have been used long enough to know which pure functions actually matter.
**Goal:** statistically sound micro-benchmarks for the CPU-bound pure functions, using Criterion — the most established Rust benchmark harness: stable toolchain, macOS-friendly, saved baselines, statistical outlier handling.

## Why not the alternatives

- **`#[bench]` / libtest bench**: needs a nightly compiler; the toolchain here is pinned in `rust-toolchain.toml` and `mise.toml`. Out.
- **gungraun** (formerly iai-callgrind): what Rune itself uses for instruction counts (`cargo bench -p rune-ref --bench instructions`, gated by `scripts/check-reference-instructions.sh`, which refuses non-Linux). Deterministic and CI-friendly, but needs **Linux + Valgrind**, and this project's dev Mac and macOS CI runners can't run it. Optional follow-up: a separate Linux CI job running gungraun against a saved baseline, mirroring Rune's `save`/`check` script.
- **Criterion**: chosen. Runs on stable, works on the dev Mac, gives median/mean/σ with change detection against stored baselines.

## Prerequisite check (from Phase 0)

The lib split is in place. The remaining seam work: the target functions are private (`pub(super)` or module-private). Micro-benches need a public path. Two options, in order of preference:

1. **A feature-gated public re-export** in `src/lib.rs`, mirroring how Phase 2 gates `bench`:
   ```rust
   #[cfg(feature = "bench")]
   pub mod bench; // already exists from Phase 2
   ```
   and inside `src/bench.rs`, re-export the pure functions under test (e.g. `pub use crate::agent::sse::Decoder;`). The `bench` feature is never on in shipped builds, so the public surface costs nothing in release.
2. Making the functions `pub` directly — **avoid**: violates the `pub(super)`, not `pub` rule in `AGENTS.md`/`ARCHITECTURE.md` for anything the app itself doesn't need public.

## Targets (measured by actual cost/risk, revisit before implementing)

| Function | Module | Why it's on the list |
| --- | --- | --- |
| `Decoder` over a recorded SSE stream | `src/agent/sse.rs` | runs per line of every streamed reply; parser changes are the most likely regression |
| Agent list / conversation list / stored-conversation parsers | `src/agent/history.rs` | runs on every directory load; allocation-heavy |
| `entries(&Chat, query)` | `src/command_bar.rs` | documented as "a pure function over four groups with fuzzy matching" — fuzzy matching scales with the corpus |
| `bars`, `summary`, `token_rate` | `src/timeline.rs` | "do the layout math" per turn trace; runs while a turn is open |

Not on the list: markdown rendering and Satellite palette math (downstream of rune-kit; covered by the Phase 2 frame benchmarks instead).

## Implementation sketch

- `Cargo.toml`:
  ```toml
  [dev-dependencies]
  criterion = { package = "criterion", version = "0.7", features = [] } # pin at implementation time

  [[bench]]
  name = "parsing"
  harness = false

  [[bench]]
  name = "command_bar"
  harness = false
  ```
- `benches/parsing.rs`: feed `Decoder` a **recorded** SSE fixture (a real agent stream captured to `benches/fixtures/stream.ndjson` — no network, no mock sleeps), decode it fully per iteration. For `history.rs`, benchmark parsing of a 500-agent and a 2,000-conversation fixture.
- `benches/command_bar.rs`: build the `Chat` fixtures through the same `MockDirectory` helpers the Phase 2 scenarios use; iterate queries of 0, 1 and 8 characters against a corpus of 500 agents + 2,000 conversations (the size where a quadratic match would show).
- Fixtures live as files under `benches/fixtures/` (checked in) rather than string literals in code, so they can be regenerated from real traffic and diffed.
- `just bench-units`: `cargo bench --locked --no-default-features --features bench` — Criterion needs the feature for the re-exports.
- CI: keep **out** of the gate initially. If the team wants a regression gate later, the Rust-native path is gungraun on a Linux job (deterministic instruction counts), not Criterion timings on shared runners.

## Ties back to earlier decisions

- Phase 0 exists so `benches/*.rs` can `use as_gui::...`.
- Phase 2's `bench` feature is the vehicle for the gated re-exports — no new feature needed.
- If, while using Phase 2's `allocations` metric, a pure function shows up as an allocation hog, promote it here instead of guessing the list now.
