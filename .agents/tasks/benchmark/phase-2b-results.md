# Phase 2b: Persist benchmark results in the repo

**Status:** ready to implement (requires Phase 1 and Phase 2 landed)
**Goal:** every benchmark run can be saved as a machine-readable record under `benchmarks/results/`, and any run can be compared against a previous one, failing loudly on regressions. No benchmark code changes behaviour; `just bench` output stays byte-for-byte the same.
**Commit:** one commit.

## Decisions already made

- **All three benchmarks are saved** in one record per run: `just bench` (instrumented scenarios), `bench-frames` (script replay) and `bench-startup` (hyperfine). They measure different things — per-scenario cost, whole-session drift, launch time — and each is cheap to keep.
- **Full history, no separate baseline file.** One JSON file per run under `benchmarks/results/`; `compare` defaults to the newest file and `--against <file>` picks any older one.
- **Single machine.** No per-machine folders; the machine model and OS go into each record so a later comparison can warn when the hardware changed under the numbers.
- **`bench-compare` ships now**, not later.

## Record shape

```json
{
  "commit": "1f7b0a9",
  "rune_rev": "04b38ae",
  "date": "2026-11-13T21:40:00Z",
  "machine": "MacBookPro18,3",
  "os": "26.1",
  "frame_bench": {
    "samples": 7, "frames": 120,
    "scenarios": {
      "idle": { "cpu_ms": { "median": 0.1315, "p95": 0.1461 }, "allocations": { "median": 33.0, "p95": 33.0 } }
    }
  },
  "frames": { "frames": 901, "cpu_mean_ms": 0.262, "p50": 0.188, "p95": 0.388, "max": 16.03, "first_frame_ms": 1.75 },
  "startup": { "mean_ms": 117.9, "stddev_ms": 2.3, "min_ms": 115.6, "max_ms": 122.6, "runs": 7 }
}
```

Filename: `<UTC-timestamp>-<short-sha>.json`, e.g. `2026-11-13T2140Z-1f7b0a9.json`.

## What to do

### 1. `frame_bench` writes JSON

- Add `--json <path>` to `benchmarks/runners/frame_bench.rs`: writes the per-scenario median/p95 table as JSON and keeps printing the `METRIC` lines.
- `serde_json` is already a dependency; no new deps.

### 2. `bench-startup` exports JSON

- Add `--export-json target/perf/startup.json` to hyperfine in the `bench-startup` recipe. `target/perf/` never lands in the repo; the results tool consumes it from there.

### 3. `bench-frames` summary goes to a file

- Tee the run's stderr into `target/perf/bench-frames.log`. The tool parses the two `rune:` lines (`N frames at 60 fps, gpu init X ms, first frame Y ms ... frame cpu mean A ms p50 B p95 C max D`; `animating frames ...`) into the `frames` section.

### 4. New tool: `benchmarks/runners/results.rs`

Another `[[example]]` with `required-features = ["bench"]` and **`test = true`**, so `cargo test` and CI compile and run its logic. Two subcommands:

- `results save --frames-bench <json> --frames-log <log> --startup <hyperfine.json> --commit <sha> --rune-rev <rev> --machine <model> --os <ver> --out-dir benchmarks/results/`
- `results compare [--against <file>] --frames-bench <json> --frames-log <log> --startup <hyperfine.json> --commit <sha> ...`

Rust rather than shell+jq: the comparison has real logic (thresholds, parsing Rune's summary line, picking the newest file), and the repo's TDD rule says that logic gets tested. No new tool in `mise.toml`.

The Justfile gathers metadata (`git rev-parse`, `git status --porcelain`, `sysctl -n hw.model`, `sw_vers -productVersion`, the Rune rev from `Cargo.lock`) and passes it as arguments — the Rust code stays free of process and OS calls, per the "traits are the seams" rule, and tests feed it fixed inputs.

### 5. Comparison thresholds

Constants in the tool, metric-dependent:

| Metric | Flag when | Why |
| --- | --- | --- |
| `allocations`, `commits` | > 0.5% | Deterministic; any change means behaviour changed |
| Frame CPU metrics (`cpu_ms`, `frame_ms`, `encode_ms`, `view_ms`, `layout_ms`, `frames` section) | > 10% | Stable but not exact |
| `startup` | outside ~2σ of the two runs | Wall-clock; noisy |

`compare` prints a table of old → new for every metric, marks regressions, and **exits non-zero when anything is flagged**, so scripts can gate on it. It warns (without failing) when the compared file's `rune_rev` or `machine` differs from the current run.

### 6. Justfile recipes

```just
# Runs all three benchmarks and saves one JSON record under benchmarks/results/. Refuses a dirty tree.
bench-save:
  scripts/bench-save.sh

# Runs all three benchmarks and compares against the latest saved record (or --against <file>). Writes nothing.
bench-compare *args="":
  scripts/bench-compare.sh {{args}}
```

The two `scripts/bench-*.sh` wrappers do the gather-outputs-and-invoke plumbing (run the three benchmarks, collect `target/perf/` artifacts, pass metadata to `results save`/`compare`), matching the existing `scripts/` convention (`demo.sh`, `release.sh`). Both build with `bench-build` first.

### 7. Tests first (TDD)

`#[cfg(test)]` tests in `benchmarks/runners/results.rs` (runs under the normal `cargo test` via `test = true`):

- Parsing the `rune:` summary lines (exact match on the Phase 1 format, plus a line with missing fields fails cleanly).
- Parsing hyperfine's JSON export.
- Merging the three sections plus metadata into one record; the filename derives from date and sha.
- Threshold logic: above, at and below each threshold; deterministic vs noisy metrics.
- Newest-file selection over a fixture directory (and `--against` overriding it).

CI's `--features bench` clippy line already compiles the runner; add nothing there. CI never runs benchmarks.

### 8. Docs

- `CONTRIBUTING.md` Benchmarks section: `bench-save` / `bench-compare`, what a record contains, that history is append-only and per-machine numbers aren't comparable.
- This task file's notes section gets the first saved run's numbers.

## Verification (one batched run)

```
just check
cargo clippy --all-targets --locked --features bench -- -D warnings
cargo test --locked
just bench-save                 # writes the first history record
just bench-compare              # compares against itself-ish (the newest) — expect "no regressions"
just bench-compare --against <oldest>   # sanity-check the diff table
```

The first `bench-save` run on a clean tree is the seed record; do not backfill the Phase 1/2 numbers (they came from earlier commits and weren't captured through this tool).

## Definition of done

- [ ] `frame_bench --json` writes the record's `frame_bench` section; `METRIC` lines unchanged
- [ ] `bench-startup` exports hyperfine JSON; `bench-frames` log lands in `target/perf/`
- [ ] `benchmarks/runners/results.rs` with `save`/`compare`, `test = true`, tests written first
- [ ] `just bench-save` and `just bench-compare` work from a clean shell; `bench-save` refuses a dirty tree
- [ ] Thresholds: 0.5% deterministic, 10% CPU, 2σ startup; non-zero exit on regression
- [ ] First history record committed under `benchmarks/results/`
- [ ] `just check` still passes; `CONTRIBUTING.md` updated
