//! Client tool calls: the agent ends its turn on calls the app has to run. Each waits for the
//! user's Run or Decline, runs on a worker thread, and when every call of the step has a
//! result the turn continues in the same assistant message.

use std::collections::BTreeMap;

use rune::{TextEdit, Tx};
use serde_json::Value;

use super::Chat;
use crate::agent::{PendingCall, StreamEvent, non_empty_output, run_with_timeout};

/// How many rounds of client tools one user turn may run, so a loop can't run away.
const MAX_ROUNDS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CallState {
    Awaiting,
    Running,
    Resolved(Result<Value, String>),
    Declined,
}

#[derive(Clone, Debug)]
pub(crate) struct ClientCall {
    pub call_id: String,
    pub name: String,
    pub input: Value,
    pub state: CallState,
}

impl ClientCall {
    fn settled(&self) -> bool {
        matches!(self.state, CallState::Resolved(_) | CallState::Declined)
    }
}

/// The calls of the step the stream ended on, and the bookkeeping that bounds them.
#[derive(Default)]
pub(super) struct ClientRound {
    calls: Vec<ClientCall>,
    /// The result field of each call, for the calls nothing handles.
    replies: BTreeMap<String, TextEdit>,
    /// Whether keyboard focus should move to the first call still waiting.
    focus_wanted: bool,
    /// Rounds run since the user's last message.
    rounds: usize,
    /// Bumped whenever the turn is cancelled or replaced: a result tagged with an older
    /// number belongs to a turn that is gone.
    generation: u64,
}

/// A worker's result, tagged with the turn generation it started in.
pub(super) struct ToolResult {
    generation: u64,
    call_id: String,
    result: Result<Value, String>,
}

impl Chat {
    pub(crate) fn client_call(&self, call_id: &str) -> Option<&ClientCall> {
        self.client_round
            .calls
            .iter()
            .find(|call| !call_id.is_empty() && call.call_id == call_id)
    }

    /// Puts a call into the round in `state`, as a scene fixture would find it mid-turn.
    pub(crate) fn scene_call(&mut self, call: PendingCall, state: CallState) {
        self.note_state(&call.call_id, state.clone());
        self.client_round
            .replies
            .insert(call.call_id.clone(), TextEdit::new(""));
        self.client_round.calls.push(ClientCall {
            call_id: call.call_id,
            name: call.name,
            input: call.input,
            state,
        });
    }

    /// Where a client call stands, during its round and after it.
    pub(crate) fn client_state(&self, call_id: &str) -> Option<&CallState> {
        self.client_states.get(call_id)
    }

    fn note_state(&mut self, call_id: &str, state: CallState) {
        self.client_states.insert(call_id.into(), state);
    }

    /// The calls waiting for the user, in order.
    pub(crate) fn awaiting_calls(&self) -> impl Iterator<Item = &ClientCall> {
        self.client_round
            .calls
            .iter()
            .filter(|call| call.state == CallState::Awaiting)
    }

    pub(crate) fn client_reply(&self, call_id: &str) -> Option<&TextEdit> {
        self.client_round.replies.get(call_id)
    }

    pub(crate) fn manual_reply(&mut self, call_id: &str) -> &mut TextEdit {
        self.client_round
            .replies
            .entry(call_id.to_string())
            .or_insert_with(|| TextEdit::new(""))
    }

    pub(super) fn client_round_open(&self) -> bool {
        !self.client_round.calls.is_empty()
    }

    /// Forgets the round and everything its workers will still send.
    pub(super) fn reset_client_round(&mut self) {
        let generation = self.client_round.generation + 1;
        self.client_round = ClientRound {
            generation,
            ..ClientRound::default()
        };
        self.continuation_break = false;
    }

    pub(super) fn apply_client_tools(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::ClientToolsPending(calls) => self.start_round(calls),
            StreamEvent::ClientToolResolved { call_id, result } => {
                self.resolve_call(&call_id, result)
            }
            _ => {}
        }
    }

    fn start_round(&mut self, calls: Vec<PendingCall>) {
        if self.client_round.rounds >= MAX_ROUNDS {
            self.stop_turn(format!(
                "The agent kept calling tools: stopped after {MAX_ROUNDS} rounds."
            ));
            return;
        }
        self.client_round.rounds += 1;
        self.client_round.focus_wanted = true;
        self.client_round.replies = calls
            .iter()
            .map(|call| (call.call_id.clone(), TextEdit::new("")))
            .collect();
        for call in &calls {
            self.note_state(&call.call_id, CallState::Awaiting);
        }
        self.client_round.calls = calls
            .into_iter()
            .map(|call| ClientCall {
                call_id: call.call_id,
                name: call.name,
                input: call.input,
                state: CallState::Awaiting,
            })
            .collect();
    }

    fn stop_turn(&mut self, error: String) {
        self.reset_client_round();
        if let Some(m) = self.streaming_message() {
            m.streaming = false;
        }
        self.pending = false;
        self.error = Some(error);
    }

    fn awaiting_mut(&mut self, call_id: &str) -> Option<&mut ClientCall> {
        self.client_round
            .calls
            .iter_mut()
            .find(|call| call.call_id == call_id && call.state == CallState::Awaiting)
    }

    /// Runs the call on a worker thread. Whatever handles the tool is looked up now, not when
    /// the call arrived, and its timeout starts here.
    pub fn approve_client_call(&mut self, call_id: &str) {
        let tools = std::sync::Arc::clone(&self.client_tools);
        let Some(call) = self.awaiting_mut(call_id) else {
            return;
        };
        call.state = CallState::Running;
        let (name, input) = (call.name.clone(), call.input.clone());
        self.note_running(call_id);
        self.note_state(call_id, CallState::Running);
        let timeout = tools.timeout(&name);
        let results = self.tool_tx.clone();
        let waker = self.waker.clone();
        let generation = self.client_round.generation;
        let call_id = call_id.to_string();
        self.client_round.focus_wanted = true;
        std::thread::spawn(move || {
            let result = run_with_timeout(tools, name, input, timeout);
            let _ = results.send(ToolResult {
                generation,
                call_id,
                result,
            });
            if let Some(wake) = waker {
                wake();
            }
        });
    }

    fn note_running(&mut self, call_id: &str) {
        self.apply_at(
            StreamEvent::ClientToolRunning {
                call_id: call_id.into(),
            },
            self.now,
        );
    }

    /// Answers the agent that the user said no.
    pub fn decline_client_call(&mut self, call_id: &str) {
        let Some(call) = self.awaiting_mut(call_id) else {
            return;
        };
        call.state = CallState::Declined;
        let name = call.name.clone();
        self.note_state(call_id, CallState::Declined);
        self.client_round.focus_wanted = true;
        self.apply_at(
            StreamEvent::ToolFailed {
                call_id: call_id.into(),
                name,
                error: "The user declined to run this tool.".into(),
            },
            self.now,
        );
    }

    /// Sends what the user typed as the call's result: JSON when it parses, otherwise text.
    pub fn send_manual_result(&mut self, call_id: &str) {
        let typed = self
            .client_round
            .replies
            .get(call_id)
            .map(|field| field.text.trim().to_string())
            .unwrap_or_default();
        let Some(call) = self.awaiting_mut(call_id) else {
            return;
        };
        call.state = CallState::Running;
        let result = if typed.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&typed).unwrap_or(Value::String(typed))
        };
        self.client_round.focus_wanted = true;
        self.note_running(call_id);
        self.note_state(call_id, CallState::Running);
        self.resolve_call(call_id, Ok(result));
    }

    fn resolve_call(&mut self, call_id: &str, result: Result<Value, String>) {
        let Some(call) = self
            .client_round
            .calls
            .iter_mut()
            .find(|call| call.call_id == call_id && call.state == CallState::Running)
        else {
            return;
        };
        let result = result.map(non_empty_output);
        call.state = CallState::Resolved(result.clone());
        let name = call.name.clone();
        self.note_state(call_id, CallState::Resolved(result.clone()));
        let ev = match result {
            Ok(output) => StreamEvent::ToolFinished {
                call_id: call_id.into(),
                name,
                output: serde_json::to_string_pretty(&output).ok(),
            },
            Err(error) => StreamEvent::ToolFailed {
                call_id: call_id.into(),
                name,
                error,
            },
        };
        self.apply_at(ev, self.now);
    }

    /// Applies the results the workers sent, moves focus to the call that waits next, and
    /// continues the turn once every call of the step has a result. True when state changed.
    pub(super) fn drain_client_tools(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        while let Ok(done) = self.tool_results.try_recv() {
            if done.generation != self.client_round.generation {
                continue;
            }
            self.apply_at(
                StreamEvent::ClientToolResolved {
                    call_id: done.call_id,
                    result: done.result,
                },
                self.now,
            );
            changed = true;
        }
        let ready =
            self.client_round_open() && self.client_round.calls.iter().all(ClientCall::settled);
        if ready {
            self.client_round.calls.clear();
            self.client_round.replies.clear();
            self.continue_turn();
            changed = true;
        }
        if std::mem::take(&mut self.client_round.focus_wanted)
            && let Some(call) = self.awaiting_calls().next()
        {
            tx.focus(format!("approval-{}", call.call_id), true);
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{mock_client, mock_directory};

    #[test]
    fn a_result_for_a_call_nobody_is_running_is_ignored() {
        let mut chat = Chat::new(mock_client(), mock_directory());
        let _sender = chat.begin_turn("Party");
        chat.apply_client_tools(StreamEvent::ClientToolResolved {
            call_id: "ghost".into(),
            result: Ok(Value::Bool(true)),
        });
        assert!(chat.sessions[0].messages[1].parts.is_empty());
    }
}
