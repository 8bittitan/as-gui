# Phase 4: Polish

**Status:** todo (needs Phase 3)
**Goal:** the feature reads clearly, Quick Ask behaves sensibly, and scenes cover every new state.

## What to do

### 1. Card labels

- Tool cards say "Waiting for you", "Running on this Mac", "Done" or "Failed", and "Declined" for a declined call. Colour is never the only signal.
- A declined or failed call shows its error text under the header, like other tool errors.
- Tool cards that ran on this Mac are marked so they differ from server tools.

### 2. Timeline (`src/timeline.rs`)

- A client tool's span starts when **Run** is pressed and ends when the result is stored, so the waterfall shows how long it ran. Time spent waiting for approval is a separate, lighter segment.
- `TurnTrace::record` handles the new events. `bars`, `summary` and `token_rate` stay correct across a continued turn (one trace for the turn, with the continuation appended).

### 3. Quick Ask (`src/quick_ask.rs`)

- Quick Ask calls `spawn_stream` directly and doesn't run the loop.
- When a turn ends on pending client tools, show the call as a card with "Continue in the main window". It uses the existing promote path, which brings the exchange into the main window where the approval card lives.
- Add a test that a Quick Ask turn with a pending call never runs the tool.

### 4. Scenes (`src/scenes.rs`)

Add scenes, each in light and dark:
- `client-tool-approval`: a card waiting for Run or Decline
- `client-tool-manual`: the fallback card with its text field
- `client-tool-running`: a running card
- `agent-panel`: the sheet with one declared tool and one stale binding

Take all the snapshots in **one** shell command (they share one binary). Use `AS_GUI_DEMO=1`.

### 5. Docs

- `ARCHITECTURE.md`: the source map entries and "How a turn flows" (a turn can end on pending calls and continue).
- `CONTRIBUTING.md`: how to try a client tool with the mock agent, and how to bind a shell command.
- Check that `AGENTS.md` still describes the commands correctly.

## Done when

- [ ] Card labels and the timeline segments
- [ ] Quick Ask shows the call and offers to continue in the main window
- [ ] Four scenes, snapshots reviewed in both themes
- [ ] Docs updated
- [ ] Review with `better-interface`
- [ ] `just check`, default-feature clippy and tests pass
