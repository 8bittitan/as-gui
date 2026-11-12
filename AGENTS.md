# AGENTS.md

A single-crate Rust GUI for Algolia Agent Studio agents, built on the Rune UI framework. Read `ARCHITECTURE.md` for the module map and the design rules before you change structure, and `CONTRIBUTING.md` for the workflow.

## Commands

- `just` is the task runner: `just dev`, `just build` (release, without the `knobs` feature), `just fmt`, `just lint`, `just test`.
- `just check` is the gate: `cargo fmt --check`, then `just lint` (clippy with `-D warnings`), then `just test`. Both of those run without the `knobs` feature. CI also runs `cargo clippy --all-targets -- -D warnings` and `cargo test` with the default features, so run those too before you call work done.
- Single test: `cargo test <test_name>`. Parser and formatting tests are inline `#[cfg(test)]` modules. Tests that drive the whole app are in `src/tests/`.
- The toolchain is pinned in both `rust-toolchain.toml` and `mise.toml` (which also pins `just`). Change them together. The manifest must stay `Cargo.toml`, because a lowercase `cargo.toml` broke tooling that looks it up by exact name.
- The five Rune crates in `Cargo.toml` share one `rev`. Change all five together.

## Design rules in short

The full text is in `ARCHITECTURE.md`.

- One feature gets one `impl Chat` block in its own module under `src/chat/`. Do not add feature logic to `chat.rs`.
- A new `StreamEvent` variant is handled in the owning module and gets one arm in the `apply_at` router. The router has no wildcard. A new per-frame job is a method that `poll` calls.
- Code that reaches the network, the OS or the clock goes behind a trait, so tests and the mock can replace it.
- Keep `Chat` fields private where you can. A helper another module needs is `pub(super)`, not `pub`.
- Say "seam", not "boundary".

## Environment

- Credentials: on first launch a dialog asks for the Application ID and a search API key with the `search`, `settings` and `logs` ACLs. They are saved in the state file. `AS_GUI_MOCK=1` runs the mock agent without asking (debug builds and headless runs). `ALGOLIA_AGENT_ID` binds an agent directly and skips the picker.
- `AS_GUI_DEMO=1` swaps in the demo agent and ignores the saved state, so scenes and recordings look the same on every machine. It applies only to debug builds and headless runs. `AS_GUI_DEMO_MENU_BAR=1` is for `just demo menu-bar`.
- Debug builds keep state in `state/` and logs in `logs/` inside the project. Release builds use `~/Library/Application Support/com.8bittitan.as-gui/` and `~/Library/Logs/com.8bittitan.as-gui/`. See `src/paths.rs`.
- Logging goes to stderr and to a file per day, with the last seven kept. `LOG_LEVEL` sets the level and `warn` is the default. The filter applies to this crate only. Read today's log with `tail -n 200 logs/as-gui.log.$(date +%F)`.
- `.env` loads in debug builds only. Real environment variables win over the file. The Justfile sets `dotenv-load`, so `just dev` sees it.
- Demos, packaging and releases are in `CONTRIBUTING.md`.

## Headless UI checks

Rune apps run without a window. `scripts/chat.json` is an example of a timestamped drive script, with `tap` and `type` events that name element keys.

- `cargo run -- --script scripts/chat.json --tree tree.json` replays the input and dumps the element tree.
- `cargo run -- --snap out.png --scene <name>` takes an offscreen screenshot. `--at`, `--size` and `--theme` set the time, the viewport and the appearance. Saved state overrides a scene's theme and accent, so set `AS_GUI_DEMO=1`.

Two gotchas. A popup whose trigger sits inside a dialog must be added to the layer list after the dialog builds, because Rune resolves an anchor only against earlier nodes. `view` does this for the settings dialog's chat-width select. Modal controls also cannot be clicked until their entrance animation settles, so tests call the `settle` helper before tapping.
