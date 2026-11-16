# Phase 1: Protocol and loop

**Status:** todo
**Goal:** a turn can end on pending client tool calls, wait for their results, and continue under the same assistant message. Nothing runs a real tool yet: results come from a fake `ClientTools` and the mock agent. No new UI beyond what the existing tool card already shows.

## Where the code stands

- `request_body` (`src/agent/http.rs`) sends every message as one `text` part, so tool parts never go back.
- `merge_tool` (`src/chat/turn.rs`) merges calls **by name**, so two calls of one tool collapse into one.
- `ToolCall` (`src/agent/model.rs`) has no call id, raw input, state or provider metadata.
- `Decoder` (`src/agent/sse.rs`) ignores `start-step` and drops `providerMetadata`.
- A turn always ends at `Finished`. Nothing reopens the same assistant message.

## What to do

### 1. Model (`src/agent/model.rs`)

- Add `ChatMessage::parts: Vec<Part>`, where `Part` is `StepStart`, `Text(String)` or `Tool(ToolPart)`.
- `ToolPart { call_id, name, input: serde_json::Value, state: ToolState, output: Option<Value>, error: Option<String>, provider_metadata: Option<Value> }`.
- `ToolState` is `InputAvailable`, `OutputAvailable`, `OutputError`.
- Keep `text` and `tools`: the views read them. `parts` is what the request replays.
- `ToolCall` gets a `call_id`. `merge_tool` matches on it and falls back to the name when the id is empty (history loaded from the API, older tests).
- History loaded through `parse_conversation` builds `parts` too, so a loaded conversation that ends on a resolved tool replays correctly.

### 2. Decoder (`src/agent/sse.rs`)

- `start-step` becomes `StepStart`. `ToolUsed` carries `call_id` and `provider_metadata`.
- Track which calls received an output. When the stream ends, `Decoder::finish` emits `StreamEvent::ClientToolsPending(Vec<PendingCall>)` for every call that has complete input and no output, and did not fail (`tool-input-error`).
- Do not use the agent's config to tell client calls from server calls: a server call always gets its output in the same stream.
- Mixed steps: server calls keep their outputs, which go back with the client calls.
- Flush the last event at EOF (`[DONE]` has no trailing blank line). Check how the line reader handles this today.

### 3. Seam: `ClientTools` (new `src/agent/client_tools.rs`)

```rust
pub trait ClientTools: Send + Sync {
    fn run(&self, name: &str, input: &serde_json::Value) -> Result<serde_json::Value, String>;
}
```

- `Chat` holds an `Arc<dyn ClientTools>`. Tests and the mock use a fake.
- A worker thread calls `run` with a 20 s timeout. Phase 2 puts the real tools behind it.
- An output that is empty or falsy is wrapped as `{"ok": true}` before it is stored.

### 4. Request body (`src/agent/http.rs`)

- The trailing assistant message goes out with all its `parts` in order. A tool part is `tool-<name>`, with `toolCallId`, `state`, `input`, `output` or `errorText`, and `callProviderMetadata` when there is one.
- Earlier messages stay text-only.
- Drop `data-*` parts. Never send an empty `output`.

### 5. Chat feature (new `src/chat/client_tools.rs`)

- State per call: `Awaiting → Running → Resolved(Ok | Err)`, and `Declined` out of `Awaiting`. In this phase a fake approves automatically; Phase 2 adds the card.
- `apply_client_tools` handles `ClientToolsPending`: it records the calls, keeps `pending` true so the composer stays locked, and starts the workers.
- A `ClientToolResolved { call_id, result }` event arrives on the same channel and gets one arm in `apply_at`.
- `drain_client_tools` is a new method in the `poll` list. When every call of the last step has a result, it calls `continue_turn`.
- `continue_turn` (in `turn.rs`) reuses the same assistant message, sends the history up to and including it, and starts the next text part on a new paragraph.
- Stop after 8 rounds per user turn and show an error row, so a loop can't run away.
- `cancel_turn` and switching application drop results that arrive late (use the generation number).

### 6. Mock (`src/agent/mock.rs`)

- A script for `MockAgent`: the first request ends on a pending `throw_confetti` call. A request whose history ends with that call's result gets a text reply.

## Tests (write first)

Decoder, in `src/agent/sse.rs`:
- A call with input and no output produces `ClientToolsPending` at the end of the stream.
- A `tool-input-error` call is not pending.
- A mixed step reports only the call without an output.
- `providerMetadata` is kept.

Request body, in `src/agent/http.rs`:
- The trailing message replays `step-start`, text and tool parts in order.
- A mixed step sends both calls with their results.
- An empty output is sent as `{"ok": true}`.
- A failure is sent as `output-error` with `errorText`.
- `data-*` parts never go out.

Whole app, in `src/tests/client_tools.rs`:
- The continuation keeps the same message id and the new text starts a new paragraph.
- The composer stays locked while calls are pending.
- The loop stops after 8 rounds.
- A result that arrives after a cancel or an application switch is dropped.
- Two calls of the same tool in one step stay two cards.

## Done when

- [ ] `parts` recorded in stream order, and loaded history builds them
- [ ] `merge_tool` matches by `call_id`
- [ ] `ClientToolsPending` emitted by the decoder
- [ ] Request body replays the trailing message
- [ ] `ClientTools` trait with a fake; worker with a 20 s timeout
- [ ] `src/chat/client_tools.rs`, one `apply_at` arm, one `poll` call
- [ ] Mock script and `src/tests/client_tools.rs`
- [ ] `ARCHITECTURE.md` updated
- [ ] `just check`, default-feature clippy and tests pass
