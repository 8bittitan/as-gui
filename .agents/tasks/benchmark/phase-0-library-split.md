# Phase 0: Split as-gui into a library and a thin binary

**Status:** ready to implement
**Goal:** make the crate importable so later benchmark phases can drive `Chat` from `examples/` and `benches/`. This phase contains **no benchmark code** and must produce zero behaviour change.
**Commit:** land on its own branch as a single commit before any benchmark work starts.

## Background

The crate currently has only a binary target (`src/main.rs`, ~261 lines). Cargo cannot compile `benches/*.rs` or `examples/*.rs` that `use as_gui::...` without a library target. The split also keeps every module private, so the existing `pub(super)` rule from `AGENTS.md` and `ARCHITECTURE.md` ("Keep seams narrow") stays intact.

## What to do

### 1. Create `src/lib.rs`

Move out of `src/main.rs` into `src/lib.rs`:

- Every `mod` declaration (including `#[cfg(test)] mod tests;`)
- The `env`, `mock_parts`, `mock_chat`, `launch_size`, `available_screen` helpers
- The `SceneFn` type alias and the `SCENES` const
- All of the body of `fn main()` (everything from `dotenv::load_env_file()` through `rune::run_windows_with(...)`) as a new `pub fn run()`

Resulting shape:

```rust
//! as-gui: a chat front end for an Algolia Agent Studio agent, built on Rune and rune-kit.
//! (keep the existing crate-level doc, reworded for a lib)

mod agent;
mod brand;
// ... all modules exactly as in main.rs today, still private
#[cfg(test)]
mod tests;

pub fn run() {
    dotenv::load_env_file();
    logging::init();
    // ... the current body of main(), unchanged
}
```

Keep the module declaration order and all attributes exactly as they are today.

### 2. Rewrite `src/main.rs`

```rust
fn main() {
    as_gui::run()
}
```

Crate-level doc comments from `main.rs` move to `lib.rs`.

### 3. `Cargo.toml`

Add a `[lib]` section:

```toml
[lib]
doctest = false
```

Reason: the crate doc comments contain code-like inline text that rustdoc would try to compile as doctests. Each doctest is a fresh executable, and every rebuilt executable can raise the Jamf `familyControlsEnabled` permission dialog on the dev Mac (see `AGENTS.md`, "Headless UI checks"). `default-run = "as-gui"` already exists and stays.

Nothing else changes: no new dependencies, no feature changes.

### 4. Check for fallout

- `src/tests/mod.rs` uses `crate::SCENES`, `crate::satellite::FONTS`, `crate::mock_chat`, `crate::playground::playground()` — these all keep working unchanged because the modules moved with the same paths.
- Grep for `crate::` and `super::` across `src/` to catch anything that assumed it lived in the binary crate root.
- `menubar::launch_mode`, `paths::BUNDLE_ID` etc. are all reached via `crate::`, so they are unaffected.

### 5. Update docs

- `ARCHITECTURE.md` first line says "as-gui is one Rust crate (edition 2024)". Extend the source map intro with one sentence: the crate is a library with a thin binary; `run()` in `src/lib.rs` builds the two windows; benchmarks (later phases) drive the library.
- `CONTRIBUTING.md`: no change needed unless it references `src/main.rs`.

## Verification (one batched run)

Per `AGENTS.md`: batch the checks, one pass per chunk of work, because each rebuild can trigger a Jamf permission dialog.

```
just check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Also smoke the binary once (this is the one UI run for this phase):

```
AS_GUI_DEMO=1 cargo run -- --tree /tmp/phase0-tree.json
```

Confirm the tree JSON has the same shape as before the split (window titled `as-gui`, agent picker present).

## Definition of done

- [ ] `src/lib.rs` holds all modules, `pub fn run()`, `SCENES`, `mock_chat`
- [ ] `src/main.rs` is three lines
- [ ] `[lib] doctest = false` in `Cargo.toml`
- [ ] `just check` passes; CI's two clippy/test lines pass
- [ ] No module gained `pub` beyond `run()`
- [ ] `ARCHITECTURE.md` mentions the lib/bin split
- [ ] `--tree` smoke run unchanged
