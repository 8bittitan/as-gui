# Phase 3: Agent sheet

**Status:** todo (needs Phase 2)
**Goal:** per-agent settings, starting with which handler each client tool uses. The settings dialog is app-wide, so the agent gets its own panel: a modal sheet pinned to the right edge of the window.

## Why a separate panel

Built-in tools behave the same for every agent, so they are app-wide. A shell command is bound to a tool **name**, and names come from each agent's config: two agents, or one agent id in two applications, can use `export_report` for different things. A global map would run the wrong command. Because every call asks first, nothing about consent is saved per agent.

## What to do

### 1. Agent config from the API

- Add `fn agent(&self, agent_id: &str) -> Result<AgentConfig, String>` to `AgentDirectory`, calling `GET /1/agents/{id}`.
- `AgentConfig` holds the name, description, status, model, the declared client tools (`name`, `description`, `inputSchema`, `isTerminal`) and the names of the other tools (read-only).
- Parse it in `src/agent/history.rs` next to the other parsers, with tests on a recorded response. A tool of an unknown type reads as "other", never an error.
- The mock and the demo agent return a config that declares one client tool.

### 2. State (`src/store.rs`)

- Add `agents: BTreeMap<String, SavedAgent>` to `SavedApplication`. `SavedAgent { tools: BTreeMap<String, ToolBinding> }`.
- `ToolBinding` is a `Command { program, args, timeout_secs }` or `Ask`.
- Putting it under the application means the agent id doesn't need the app id, and removing an application removes its agents' settings.
- `serde(default)` and `skip_serializing_if` keep older state files loading and unchanged files unchanged. Add a round-trip test.

### 3. Chat state (new `src/chat/agent_panel.rs`)

- `agent_panel: Option<AgentPanel { focus: Option<String>, editing: Option<String> }>`.
- The agent's config is a `Load`, fetched when the panel opens or the agent changes. It carries a generation number, so a result for an agent you've left is dropped.
- `set_binding`, `remove_binding` and `open_agent_panel(focus)` are the only ways to change it. They persist.
- The registry from Phase 2 reads the current agent's bindings.

### 4. The sheet (new `src/agent_panel.rs`)

Built from `modal_panel(..)`, `.modal(true, close)`, `.modal_click_outside(true)` and `push_modal_layer`, like `settings_dialog` in `src/shell.rs`. Pinned to the right edge, full height, about 400 pt wide, a z400 surface from `satellite::elevation`, rounded only on the left corners. It slides in from the right on `k.motion.overlay`; with Reduce Motion it fades in place.

Sections:
1. **Header:** the agent's name, its status, its model, and the application's colour dot and name so the scope is clear. The same close `IconButton` as settings.
2. **About:** the description, read-only.
3. **Client tools:** one row per declared tool with the name, the description, a scrollable JSON preview of the input schema, and a handler select (Built-in when one matches, Command…, Ask me). **Command…** expands the row in place to edit the program, the arguments and the timeout; it never opens a second dialog. A tool with no binding says "Asks you each time".
4. **No longer declared:** bindings for tools the agent dropped, each with **Remove**.
5. **Server tools:** a short read-only list, so the tool cards in the chat make sense.

Details:
- With no agent open, or while loading or failed, the sheet shows the usual skeleton or error row from `Load::phase`.
- The handler selects go in the layer list **after** the sheet (AGENTS.md: a popup whose trigger sits inside a dialog is resolved only against earlier nodes). `view` already does this for the settings width select.
- Tests call `settle` before tapping: modal controls can't be clicked until the entrance animation ends.
- Accessibility: Escape closes the sheet and focus returns to whatever opened it. The sheet is a labelled region. Selects are named "Handler for <tool>". Long previews scroll in their own box so the layout doesn't move when text is resized.

### 5. Entry points

- A button in the chat header, beside the agent's name.
- A ⌘K entry "Agent settings" (`command_bar::entries`, in the actions group).
- A new rebindable shortcut in `keymap.rs` (it joins the existing six; check for conflicts).
- The **Set up a handler…** link on a card waiting for a manual result: it opens the sheet with that tool's row focused. When the sheet closes, focus returns to the card.

## Tests (write first)

- The config parser on a recorded response, an unknown tool type, and a missing field.
- State: a binding round-trips through the state file under the right application and agent; an old file loads; removing an application removes its agents' bindings.
- Whole app, after `settle`: each entry point opens the sheet; Escape closes it and returns focus; a config result for an agent you've left is dropped; saving a command binding reaches the next Run; a binding for a dropped tool is listed under "No longer declared" and **Remove** deletes it.
- `command_bar::entries` includes "Agent settings".

## Done when

- [ ] `AgentDirectory::agent` with the mock and demo versions
- [ ] `SavedAgent` and `ToolBinding` in the state file, with round-trip tests
- [ ] `src/chat/agent_panel.rs` and `src/agent_panel.rs`
- [ ] Four entry points
- [ ] Focus and keyboard behavior checked against `better-accessibility`
- [ ] `ARCHITECTURE.md` updated
- [ ] `just check`, default-feature clippy and tests pass
