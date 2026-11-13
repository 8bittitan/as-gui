# Architecture

as-gui is one Rust crate (edition 2024). It is a chat front end for Algolia Agent Studio agents, built on the Rune UI framework: `rune`, `rune-kit`, `rune-knobs` and `rune-notify`, all pinned to one revision of the `anomalyco/rune` fork. The app has two windows, the main chat window and the Quick Ask launcher panel, started with `rune::run_windows`. Its look is Algolia's Satellite design system, built in the app (`src/satellite.rs`).

## How a turn flows

1. The composer calls `Chat::begin_turn`, which adds the user message and an empty assistant message and opens a channel.
2. `agent::spawn_stream` runs the request on a thread. The `AgentClient` posts the conversation to the completions endpoint and feeds each line of the server-sent event stream to a `Decoder`.
3. The decoder turns lines into `StreamEvent`s. The thread stamps each one with a time and sends it down the channel, followed by `Finished` or `Failed`.
4. Each frame, `chat::poll` drains the channel. `apply_at` records the event in the message's trace, then passes it to the module that owns that kind of event.
5. The views read `Chat` and draw. Nothing in a view starts work.

## Design rules

The rules come from treating a seam as the place where behavior can change without editing that place.

- **One feature, one `impl Chat` block, in its own module.** Feature behavior lives in a child module of `chat` (`src/chat/*.rs`) or in the feature's own module. `chat.rs` holds the struct, a few shared helpers, the `apply_at` router and the `poll` list, and no feature logic.
- **Events are routed, not handled in one place.** A new `StreamEvent` variant gets handled in the owning module's `apply_*` function and one arm in `apply_at`. The router has no wildcard arm, so the compiler flags a variant with no owner. A new per-frame job is a method on its module that `poll` calls, and `poll` stays a list of calls.
- **Traits are the seams to the outside world.** `AgentClient`, `AgentDirectory`, `IndexSearch`, `ShortcutRegistrar`, `Notifier`, `SelectionSource`, `AppShell`, `StatusItem` and `Connector` let tests swap in fakes. Code that reaches the network, the OS or the clock goes behind a trait.
- **Keep seams narrow.** Child modules can see `Chat`'s private fields, so most fields stay private. A helper that another module needs is `pub(super)`, not `pub`.
- **Find the seam before splitting a file.** Locate each feature's methods, events and share of `poll`, then move whole blocks and change only visibility and dispatch.

## Source map

### Agent client (`src/agent/`)

- `model.rs`: messages, tool calls, turn traces, `StreamEvent`, agent and session info.
- `sse.rs`: the stream `Decoder` and the line parsers. It speaks the AI SDK v5 data-stream protocol. Only an `error` part ends a turn. A tool output carries no tool name, so the decoder names it from the call's start event and drops outputs for calls that never started. `tool-output-error` and `tool-input-error` fail one call and let the turn go on. Guardrail blocks show their fallback text as the reply. Reasoning parts are ignored.
- `history.rs`: parsers for the agent list, the conversation list and a stored conversation.
- `search.rs`: search hits, the `IndexSearch` trait with its real and mock versions, and search page parsing.
- `http.rs`: the `AgentClient` and `AgentDirectory` traits, `AlgoliaClient`, `spawn_stream`, pagination, status errors and path-segment encoding.
- `mock.rs`: the mock agent and the demo agent that tests, scenes and demos use.

### Chat state (`src/chat.rs` and `src/chat/`)

`Chat` is the application state. Its modules each own one feature: `turn` (send, retry, apply events), `directory` (agent, conversation and message loads), `applications`, `sessions`, `search`, `notifications`, `message_actions`, `keybinds`, `appearance`, `persistence` and `snapshot`. `poll` calls `advance_loads`, `drain_loads`, `track_views` and `drain_turn` in that order.

- **Loading.** Every background load is a `Load` (`Idle`, `Loading`, `Ready`, `Failed`) read through `Load::phase(now)`: nothing for 150 ms, then a skeleton that stays at least 300 ms. `Chat::screen`, `sidebar_view` and `pane_view` are the only places that decide what to show, so the sidebar and the chat pane never flash a half-loaded state. Loads carry a generation number, so a result for an agent the user already left is dropped.
- **New conversations.** A conversation that starts in this run, or a promoted Quick Ask, is marked loaded at once. Fetching it back would blank the pane and replace the live exchange with the server's older copy.
- **Applications.** `Chat::applications` and `active` hold the saved Algolia applications. `add_application`, `switch_application`, `remove_application` and `switch_to_mock` are the only ways to change them. An app ID must be ten letters or digits because it becomes part of a hostname. Switching cancels any reply, bumps the generation and reopens the application's last agent. A `Connector` builds the client and directory for each application, which is how tests run offline.
- **Application colors.** `palette.rs` gives each application an `AppColour`, chosen as the least used one, saved by id, and shown as a dot, a swatch picker, the Quick Ask chip and an optional top bar. The colors are Satellite scale steps and brand planets, each at least 3:1 against the page in both appearances and never an accent tone. The name always appears beside the dot, so color is never the only signal.
- **Appearance.** `Chat::appearance` holds width, theme, accent, window tint and open-at-launch. `Chat::add_form` holds the add-application dialog, which the first run reuses.

### Theme (`src/satellite.rs` and `src/satellite/`)

`satellite::kit(dark, accent)` builds the rune-kit `Kit` every view reads. `install` sets Satellite's focus ring and tooltip, and each window's view calls it on every run.

- `scale.rs`: Satellite's color scales (`Hue`) and the brand's planet colors (`Planet`).
- `roles.rs`: the palette. Light follows Satellite's roles. Satellite has no dark mode, so dark is derived from its navy greys.
- `accent.rs`: the settings' accents mapped to Satellite scales, as a fill and an ink.
- `elevation.rs`: the z100 to z500 shadows.
- `typography.rs`: the type scale, in SF Pro and the bundled Fira Code (`assets/fonts/`).
- `markdown.rs`: the look of rendered markdown and code syntax.
- `icons.rs`: the Lucide icons the app and the kit draw.

### Views

- `conversation.rs`: message bubbles and their actions, tool calls as one card per call (a header with what it did and found, and its results, JSON or error beneath; hits as plain-excerpt rows, the first few until Show all), the hit reader, the empty state and the composer.
- `shell.rs`: the window chrome, the agent picker, the settings dialog, global key handling and the error toast.
- `command_bar.rs`: the ⌘K command bar. `entries(&Chat, query)` is a pure function over four groups (agents, applications, conversations, actions) with fuzzy matching.
- `search_panel.rs`: "Search it yourself". It re-runs a tool call's query through `IndexSearch`, tags each run so a late result is dropped, marks the hits the agent saw, validates the parameters as you type, and builds a curl command with the key masked and every value shell-quoted.
- `timeline.rs`: the per-turn waterfall. `Chat::apply_at` records tool starts and ends into a `TurnTrace`, and `bars`, `summary` and `token_rate` do the layout math.
- `glow.rs`: the composer's glow while a reply streams. The color math and timing are Rust and tested. A small WGSL shader draws them.
- `knobs.rs`: the live tuning panel, behind the `knobs` feature. Release builds turn it off and run on its default values.
- `playground.rs`: a storybook of the scenes, opened with ⌘⇧D in debug builds.
- `scenes.rs`: fixed states for `--scene` and the playground.

### Quick Ask and selection

- `quick_ask.rs`: the launcher panel, a second non-activating window. It reads the agent, theme and accent from a snapshot that `Chat` publishes on every change, streams one question, shows the agent's tool calls as cards and the settled reply's copy and feedback actions, and can promote the exchange into the main window.
- `selection.rs`: what Ask about selection quotes. `copy_and_restore` is a pure algorithm over a `Desktop` trait. Without Accessibility access it reads the clipboard. With it, it saves the pasteboard, sends ⌘C, waits for the change count to move, reads the text and restores the pasteboard. A clipboard marked secret is never quoted. `selection/mac.rs` is the AppKit side.

### Platform and support

- `menubar.rs` and `menubar/mac.rs`: the menu bar icon and its menu. `launch_mode` decides at startup whether the window opens. The Dock icon shows only while the main window is open.
- `notify.rs`: reply notifications through the `Notifier` trait. `first_line` makes the body from a reply's first line of prose.
- `keymap.rs`: the six rebindable shortcuts, conflict checks, and the trait that registers the two system-wide ones.
- `store.rs`: the saved state as JSON. A value the app no longer knows reads as unset. The file is created readable by its owner only.
- `paths.rs`: where state and logs live. `logging.rs`: the log setup. `brand.rs`: the Algolia logo icon.
- `src/tests/`: tests that drive state, view and keys together, one file per feature.
