//! Client tool calls: the agent ends its turn on calls the app has to run, each runs on a
//! worker thread, and when every call of the step has a result the turn continues in the same
//! assistant message.

use serde_json::Value;

use super::Chat;
use crate::agent::{
    CLIENT_TOOL_TIMEOUT, PendingCall, StreamEvent, non_empty_output, run_with_timeout,
};

/// How many rounds of client tools one user turn may run, so a loop can't run away.
const MAX_ROUNDS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum CallState {
    Awaiting,
    Running,
    Resolved(Result<Value, String>),
    Declined,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ClientCall {
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
        self.client_round.calls = calls
            .into_iter()
            .map(|call| ClientCall {
                call_id: call.call_id,
                name: call.name,
                input: call.input,
                state: CallState::Awaiting,
            })
            .collect();
        // Phase 2 asks the user here; until then every call runs.
        let ids: Vec<String> = self
            .client_round
            .calls
            .iter()
            .map(|call| call.call_id.clone())
            .collect();
        for id in ids {
            self.approve_client_call(&id);
        }
    }

    fn stop_turn(&mut self, error: String) {
        self.reset_client_round();
        if let Some(m) = self.streaming_message() {
            m.streaming = false;
        }
        self.pending = false;
        self.error = Some(error);
    }

    /// Runs the call on a worker thread.
    pub fn approve_client_call(&mut self, call_id: &str) {
        let Some(call) = self
            .client_round
            .calls
            .iter_mut()
            .find(|call| call.call_id == call_id && call.state == CallState::Awaiting)
        else {
            return;
        };
        call.state = CallState::Running;
        let tools = std::sync::Arc::clone(&self.client_tools);
        let (name, input) = (call.name.clone(), call.input.clone());
        let results = self.tool_tx.clone();
        let waker = self.waker.clone();
        let generation = self.client_round.generation;
        let call_id = call_id.to_string();
        std::thread::spawn(move || {
            let result = run_with_timeout(tools, name, input, CLIENT_TOOL_TIMEOUT);
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

    /// Answers the agent that the user said no.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn decline_client_call(&mut self, call_id: &str) {
        let Some(call) = self
            .client_round
            .calls
            .iter_mut()
            .find(|call| call.call_id == call_id && call.state == CallState::Awaiting)
        else {
            return;
        };
        call.state = CallState::Declined;
        let name = call.name.clone();
        self.apply_at(
            StreamEvent::ToolFailed {
                call_id: call_id.into(),
                name,
                error: "The user declined to run this tool.".into(),
            },
            self.now,
        );
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

    /// Applies the results the workers sent, and continues the turn once every call of the
    /// step has one. True when state changed.
    pub(super) fn drain_client_tools(&mut self) -> bool {
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
            self.continue_turn();
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ToolState, mock_client, mock_directory};

    #[test]
    fn a_declined_call_answers_the_agent_with_an_error_and_lets_the_turn_continue() {
        let mut chat = Chat::new(mock_client(), mock_directory());
        let _sender = chat.begin_turn("Party");
        chat.client_round.calls = vec![ClientCall {
            call_id: "c1".into(),
            name: "throw_confetti".into(),
            input: Value::Null,
            state: CallState::Awaiting,
        }];

        chat.decline_client_call("c1");

        assert_eq!(chat.client_round.calls[0].state, CallState::Declined);
        let reply = &chat.sessions[0].messages[1];
        let crate::agent::Part::Tool(part) = &reply.parts[0] else {
            panic!("the call has a part: {:?}", reply.parts);
        };
        assert_eq!(part.state, ToolState::OutputError);
        assert!(
            part.error
                .as_deref()
                .is_some_and(|e| e.contains("declined"))
        );
        assert!(chat.drain_client_tools(), "every call has its answer");
        assert!(chat.client_round.calls.is_empty());
    }

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
