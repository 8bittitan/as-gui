# Phase 2: Tool handlers

**Status:** todo (needs Phase 1)
**Goal:** a call waits for the user, then runs a real handler on this Mac. The handlers are a small set of built-in tools, user-defined shell commands, and an "ask the user" fallback.

## What to do

### 1. Registry (`src/agent/client_tools.rs`)

The real `ClientTools` resolves a tool name in this order:
1. A user-defined command bound to the name for the current agent (Phase 3 adds the setting; until then the lookup finds nothing).
2. A built-in tool.
3. Ask the user.

A user-defined command cannot use the name of a built-in tool.

### 2. Built-in tools

Each tool validates its input against its own schema and returns a clear error on a mismatch.

| Tool | Does | Seam |
|---|---|---|
| `open_url` | opens an `http` or `https` URL | `AppShell` |
| `copy_to_clipboard` | puts `text` on the clipboard | `AppShell` |
| `notify` | posts a notification with `title` and `body` | `Notifier` |
| `read_selection` | returns the selected text | `SelectionSource` |

- Other URL schemes (`file:`, `javascript:`, custom schemes) are refused.
- `read_selection` follows the existing rules in `selection.rs`: a clipboard marked secret is never read.
- Each tool is covered by a fake of its trait. No test touches the OS.

### 3. User-defined shell commands

- A binding is `{ program, args: Vec<String>, timeout_secs }`. It runs **without a shell**, so arguments can't inject commands.
- The input JSON goes in on stdin. stdout is the output: JSON if it parses, otherwise text.
- A non-zero exit is an error with stderr trimmed to 200 characters.
- A minimal environment (`PATH`, `HOME`) and a timeout (default 20 s). Kill the process on timeout.
- Output size is capped (1 MB). Anything over is an error.
- Put process spawning behind a trait (`CommandRunner`) so tests use a fake.

### 4. Ask the user

- For any name no handler matches, the card gets a text field for the result, with **Send result** and **Decline**.
- It also works as a way to test an agent's tools before writing the handler.

### 5. The approval card (`src/conversation.rs`)

Every call asks first. The card for a call in `Awaiting` shows:
- the tool name and its arguments as JSON
- for a shell command, the program and arguments it will run
- **Run** and **Decline** (`satellite::button`, `Size::Small`)
- for the fallback: the text field, **Send result** and **Decline**

Behavior:
- **Decline** sends `output-error` with "The user declined to run this tool."
- The 20 s timeout starts when **Run** is pressed. There is no limit on deciding.
- The handler is looked up when **Run** is pressed, not when the call arrived. A handler set up while the card waits still applies.
- A timeout sends "The tool call timed out before a result was received. The operation may have completed."
- Accessibility: focus moves to the first card in `Awaiting` when it appears. The card announces "Waiting for your approval". Run and Decline have names that include the tool name.

## Tests (write first)

- Each built-in tool: a valid input, an invalid input, and its refusal rule (a `file:` URL, a secret clipboard).
- Shell commands with a fake `CommandRunner`: arguments reach the program as separate strings, stdin carries the input, a non-zero exit gives an error, a timeout kills the process, a name that clashes with a built-in is refused.
- Whole app: a call waits until **Run**; **Decline** reaches the next request as `output-error`; a manual result reaches the next request; a handler bound while a card waits is used on Run.

## Done when

- [ ] Registry resolves command, built-in, ask-the-user in that order
- [ ] Four built-in tools behind their traits, with tests
- [ ] `CommandRunner` and the shell command handler, with tests
- [ ] Approval card with Run, Decline and the manual-result field
- [ ] Focus and announcement for a new approval card
- [ ] `ARCHITECTURE.md` updated
- [ ] `just check`, default-feature clippy and tests pass
