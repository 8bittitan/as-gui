# Client-side tools tasks

Adds Agent Studio client-side tools to as-gui: the agent calls a tool, the app runs it on this Mac after the user approves, and the result goes back to the agent so it can finish its reply.

Execute **in order**; each phase depends on the previous one:

1. [Phase 1: protocol and loop](phase-1-protocol-and-loop.md): ordered message parts, `ClientToolsPending`, the request body, the call state machine, `continue_turn`, the mock script
2. [Phase 2: tool handlers](phase-2-tool-handlers.md): the `ClientTools` trait, built-in tools, shell commands, ask-the-user, the approval card
3. [Phase 3: agent sheet](phase-3-agent-sheet.md): `AgentDirectory::agent`, per-agent bindings in the state file, the right-hand modal sheet
4. [Phase 4: polish](phase-4-polish.md): card labels, timeline, Quick Ask, scenes

## How Agent Studio does it (source: `algolia/conversational-ai`, branch `development`)

- A client tool is configured **on the agent**, never in the request: `{ "type": "client_side", "name", "description", "inputSchema": { "type": "object", "properties", "required" }, "isTerminal"? }`. The name is 3-32 characters matching `^[A-Za-z][\w-]+$`. The completion request has no `tools` field.
- When the model calls one, the server checks the arguments against the schema, then ends the turn. The stream is `start`, `start-step`, optional text, `tool-input-start`, `tool-input-delta`*, `tool-input-available` (may carry `providerMetadata`), `finish-step`, `finish`, `[DONE]`. There is no "paused" event: a call with input and no `tool-output-available` at the end of the stream is the signal. `[DONE]` has no trailing blank line.
- A call whose arguments fail the schema streams `tool-input-error` then `tool-output-error` instead. The client must not run it.
- The client runs the tool and POSTs the **full history** again to `.../completions?stream=true&compatibilityMode=ai-sdk-5`, with the same conversation `id`. The history **ends with the same assistant message** (same `id`, the `messageId` from `start`), now holding `step-start`, `text` and `tool-<name>` parts. A tool part is `{ type: "tool-<name>", toolCallId, state: "output-available", input, output }`, or `state: "output-error"` with `errorText`.
- The server does not load history to call the model, so the client resends everything every time. The new stream reuses the same `messageId` and sends only the new parts, which the client appends to the same message.
- In a step that mixes a server tool and a client tool, both parts go back: the server part with the output the client already received, the client part with its own result.
- **A falsy `output` (`{}`, `""`, `[]`, `0`, `false`) counts as no result** and fails with 422 "Trailing tool calls must all be resolved before continuing". Always send something non-empty.
- Keep `providerMetadata` from `tool-input-available` and send it back as `callProviderMetadata` on the tool part. Gemini thinking models need it.
- Drop `data-*` parts before sending. Only `text` parts are accepted on user messages.
- The server stops offering a client tool after 8 resolved calls of it since the last user text message (`maxClientToolRoundtrips`, default 8). It is not an error.
- Client tools have no approval flag. `data-tool-approval` and `toolApprovals` exist only for MCP tools and are out of scope here.
- Reference client: the InstantSearch chat widget (`algolia/instantsearch`, `connectChat.ts`). It runs each tool with a 20 s timeout, answers an unknown tool with the output `No tool implemented for "<name>".`, and posts again when every tool part of the last step has a result.

## Decisions

- First tools: a small native set (`open_url`, `copy_to_clipboard`, `notify`, `read_selection`), user-defined shell commands, and a generic "ask the user" fallback.
- Consent: **always ask**. Every call waits for Run or Decline. There is no allow list, so nothing about consent is saved per agent.
- Replay: ordered parts, recorded in stream order on the message.
- Quick Ask: main window only. Quick Ask shows the pending call and offers to continue in the main window.
- Agent settings: a modal sheet pinned to the right edge, built from `modal_panel` and `push_modal_layer`. Rune-kit has no drawer component.

## Shared context every task assumes

- The gate is `just check`; also run `cargo clippy --all-targets -- -D warnings` and `cargo test` with the default features (see `AGENTS.md`).
- Write the tests first (the `test-driven-development` skill).
- Batch test and snapshot runs on the dev Mac: every rebuilt executable can raise a Jamf permission dialog (see `AGENTS.md`, "Headless UI checks"). One test pass and one round of snapshots per phase.
- One feature gets one `impl Chat` block in its own module under `src/chat/`. A new `StreamEvent` variant is handled in the owning module and gets one arm in `apply_at`, which has no wildcard. A new per-frame job is a method that `poll` calls.
- Code that reaches the network, the OS or the clock goes behind a trait.
- Buttons are `satellite::button(..)` with `.sized(k, Size::Small | Size::Medium)`.
- Use `better-accessibility` for every view change, and the Satellite design system in `src/satellite.rs`.
- Update `ARCHITECTURE.md` (source map and "How a turn flows") when a phase adds a module.

## Progress

- [x] Phase 1
- [x] Phase 2
- [x] Phase 3
- [x] Phase 4
