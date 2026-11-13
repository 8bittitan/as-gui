# Contributing

Thanks for taking an interest. This page covers setting up, the checks a change has to pass, and how to record the demos and cut a release. [ARCHITECTURE.md](ARCHITECTURE.md) explains how the code is laid out.

## Set up

The easiest way to get the tools is [mise](https://mise.jdx.dev). Run `mise install` in the repository root and it installs the Rust toolchain, `just` and `git-cliff` at the versions `mise.toml` pins.

Without mise, install [rustup](https://rustup.rs) and [just](https://github.com/casey/just). `rust-toolchain.toml` pins the Rust version and rustup installs it on the first build.

The first build fetches Rune, the UI framework, from GitHub. It is slow once and cached after.

You do not need an Algolia account to work on the app. A debug build started with `AS_GUI_MOCK=1` talks to a built-in mock agent, and `AS_GUI_DEMO=1` swaps in a demo agent that searches and streams a reply.

## Commands

| Command | What it does |
| --- | --- |
| `just dev` | Run a debug build. It loads `.env` if one exists. |
| `just fmt` | Format the code. |
| `just lint` | Clippy with warnings as errors, in the shipped configuration. |
| `just test` | Run the tests in the shipped configuration. |
| `just check` | Format check, lint and tests. |
| `just build` | Release build without the tuning panel. |
| `just bundle` | Build `target/bundle/Agent Studio GUI.app`. |
| `just dmg` | Build the app and a disk image. |
| `just changelog` | Refresh the Unreleased section of `CHANGELOG.md` from the commits since the last tag. |
| `just release <version>` | Cut a release: checks, version bump, changelog, commit and tag. |

Run a single test with `cargo test <name>`.

The shipped configuration builds without the `knobs` feature (the live tuning panel, ⌘⇧K). CI also lints and tests the default features, so before you open a pull request run both:

```sh
just check
cargo clippy --all-targets -- -D warnings && cargo test
```

## Making a change

- Add a test for new behavior. Parsing and formatting logic is tested next to its code. Tests that drive state, view and keys together live in `src/tests/`, one file per feature.
- Follow the design rules in [ARCHITECTURE.md](ARCHITECTURE.md), mainly one feature per module and traits for anything that touches the network, the OS or the clock.
- Keep comments for the reason behind a non-obvious choice. Names and small functions should explain the rest.
- `Cargo.toml` must keep that exact spelling. Do not rename it.

### Updating the Rust version

The Rust version is pinned in both `rust-toolchain.toml` and `mise.toml`. Change them together, so mise users and rustup users build with the same compiler.

### Updating Rune

The four Rune crates (`rune`, `rune-kit`, `rune-knobs`, `rune-notify`) are git dependencies pinned to one revision. Change `rev` in `Cargo.toml` on all four together. The theme is not a Rune crate. It is Satellite, built in `src/satellite.rs`. The `wgpu` version must also match the one Rune uses.

## Checking the UI without a window

Rune apps run headless, so you can check the interface from a terminal.

```sh
cargo run -- --script scripts/chat.json --tree tree.json   # replay input, dump the element tree
cargo run -- --snap out.png --scene conversation           # screenshot of a named scene
```

`--scene` starts from a fixture in `src/scenes.rs`. `--at`, `--size` and `--theme` set the time, viewport and appearance. Saved state overrides a scene's theme and accent, so set `AS_GUI_DEMO=1` to render a scene as written. `just screenshots` rebuilds the images in `docs/`.

Two things to know when writing UI tests:

- Modal controls are not clickable until their entrance animation settles. Call the `settle` helper before tapping.
- A popup that opens from inside a dialog must be added to the layer list after the dialog is built. Rune only resolves an anchor against earlier nodes.

## Recording the demos

The demos need `ffmpeg`. `just demo <name>` writes `demos/<name>.mp4`, and the videos are not committed. `just demo` records the tour, `just demo all` records every demo in turn, and `scripts/demo.sh <name>` does the same without `just`.

- `just demo` (or `just demo tour`) plays `scripts/demo.json` against the demo agent and records `demos/tour.mp4`. A tap on a key that no longer exists fails the run.
- `just demo quick-ask` records the Quick Ask panel. The panel is a second window, so the recording uses `scripts/backdrop.png` as the desktop.
- `just demo menu-bar` records the menu bar icon and its menu. Headless recordings cannot draw a native status item, so the script paints the menu bar and the menu over the video with `ffmpeg`.
- `just demo ask-selection` records Ask about selection. The script paints a browser window and the selected post. The post's words must match `selection::DEMO_SELECTION`.
- `just demo notify` records a background notification. The script paints the browser and the macOS banner. The banner text must match the first line of the demo reply, as `notify::first_line` trims it.

The menu bar and selection scripts share the painted menu bar in `scripts/menu-bar.sh`.

## Releasing

1. Run `just release 0.2.0` with the version you want. The script refuses a dirty working tree, runs `just check`, raises `version` in `Cargo.toml` and `Cargo.lock`, writes the version's section into `CHANGELOG.md`, and commits and tags `v0.2.0`.
2. `git push && git push --tags`.

`CHANGELOG.md` is generated from the commit messages by [git-cliff](https://git-cliff.org), so write conventional commits: `feat` for a feature, `fix` for a bug fix, and `feat!` or a `BREAKING CHANGE` footer for something breaking. `just changelog` refreshes the Unreleased section at any point, so the changelog is current between releases.

`.github/workflows/release.yml` runs the checks, builds the disk image, writes a SHA-256 checksum and takes the release notes from the tag's section in `CHANGELOG.md`, in a job that can only read the repository. A second job, which has write access and runs no repository code, publishes both files to a GitHub release.

To build the same files locally, run `just dmg`. They land in `target/bundle/`. The app is signed ad hoc. Notarizing it needs a paid Apple Developer ID and is not set up.

## Where state and logs go

Debug builds keep state in `state/` and logs in `logs/` inside the project. Release builds use `~/Library/Application Support/com.8bittitan.as-gui/state.json` and `~/Library/Logs/com.8bittitan.as-gui/`. `LOG_LEVEL` (`off`, `error`, `warn`, `info`, `debug`, `trace`) sets the level, and `warn` is the default. To read today's log in a debug build:

```sh
tail -n 200 logs/as-gui.log.$(date +%F)
```
