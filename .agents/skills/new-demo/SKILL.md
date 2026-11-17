---
name: new-demo
description: Add a recorded demo video to as-gui (`just demo <name>`). Use when asked to record, script or add a demo or walkthrough video. Not for `--snap` screenshots or `--scene` fixtures.
---

# New demo

Adds one video to `just demo`, written to `demos/<name>.mp4`.

## Before you start

Read `scripts/demo.sh` and "Recording the demos" in `CONTRIBUTING.md`, then the existing demo closest to yours:

- `scripts/demo.json` (tour): a plain script against the main window
- `scripts/demo-quick-ask.json`: a second window, so it records with `--desktop scripts/backdrop.png` and a `displays` block
- `scripts/demo-notify.sh`, `demo-ask-selection.sh`, `demo-menu-bar.sh`: paint what headless runs can't draw (browser, banner, menu bar) over the video with `ffmpeg`, sharing `scripts/menu-bar.sh`

Pick a kebab-case name. It is the `just demo` argument, the video name and the `scripts/demo-<name>.json` / `.sh` file names.

The script format belongs to Rune: `crates/rune/src/tooling/script.rs` in https://github.com/anomalyco/rune, at the `rev` pinned in `Cargo.toml`.

## Rules for a demo

- Record with `AS_GUI_DEMO=1` so it runs the demo agent and ignores saved state.
- End with `{"type": "mark", "name": "end"}`.
- Tap only keys that exist; a missing key fails the run. Dump keys with `cargo run -- --script <file> --tree tree.json`.
- Give the demo agent's reply time to stream before acting on it.
- Painted overlays must match the app: times in the `.sh` match the `t` values in the `.json`, and text matches its source (`selection::DEMO_SELECTION`, `notify::first_line`).
- App changes the demo needs go behind `agent::demo_mode()` or an `AS_GUI_DEMO_*` variable, like `menu_bar_demo` in `src/menubar.rs`, with a test.

## Steps

1. Write `scripts/demo-<name>.json`. If it needs overlays, add `scripts/demo-<name>.sh` modelled on `demo-notify.sh` (`set -euo pipefail`, temp work dir, raw recording, then the `ffmpeg` filter) and `chmod +x` it.
2. Register it in each place that lists the demos:
   - `scripts/demo.sh`: the `DEMOS` array, a `case` arm, the header comment
   - `Justfile`: the comment above `demo`
   - `CONTRIBUTING.md`: a bullet under "Recording the demos"
3. Verify in one batch (each rebuilt binary can raise the Jamf dialog, see `AGENTS.md`): the `--tree` run to confirm keys, then `just demo <name>`. Run `just check` only if Rust code changed.

## Results

Do not watch or inspect the video. Tell the user where `demos/<name>.mp4` is so they can check it. Never commit `demos/*.mp4`.
