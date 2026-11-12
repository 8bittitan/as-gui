//! The streaming turn: sending a prompt or retrying a reply, the request thread, and the
//! state machine its events drive.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use super::Chat;
use crate::agent::{ChatMessage, Role, StreamEvent, ToolCall, TurnTrace, spawn_stream};
use crate::loading::Load;

/// A turn event with the moment the request thread handed it over.
pub struct Timed {
    event: StreamEvent,
    at: Instant,
}

/// The request thread's end of a turn: it stamps each event as it is sent, so the timeline
/// isn't quantised to frames.
#[derive(Clone)]
pub struct TurnSender(Sender<Timed>);

impl TurnSender {
    pub fn send(&self, event: StreamEvent) -> Result<(), mpsc::SendError<StreamEvent>> {
        self.0
            .send(Timed {
                event,
                at: Instant::now(),
            })
            .map_err(|e| mpsc::SendError(e.0.event))
    }
}

impl Chat {
    /// Pushes the user message and the assistant placeholder, and returns the sender the
    /// request thread streams events into. The receiver drains through [`poll`].
    pub fn begin_turn(&mut self, text: impl Into<String>) -> TurnSender {
        let text: String = text.into();
        let user = ChatMessage::user(self.fresh_id(), text.clone());
        let mut assistant = ChatMessage::assistant(self.fresh_id());
        assistant.trace = Some(TurnTrace::new(self.now));
        let session = &mut self.sessions[self.current];
        if session.messages.is_empty() {
            session.title = text.chars().take(40).collect();
        }
        session.messages.push(user);
        session.messages.push(assistant);
        self.pending = true;
        self.error = None;
        self.open_turn_channel()
    }

    fn open_turn_channel(&mut self) -> TurnSender {
        let (sender, receiver) = mpsc::channel();
        self.events = Some(receiver);
        self.turn_origin = Some((Instant::now(), self.now));
        TurnSender(sender)
    }

    /// Drops the in-flight turn's event channel, so navigation can't stream deltas into a
    /// session that has no assistant placeholder (or into the wrong session). The request
    /// thread keeps running; its sends fail silently against the dropped receiver.
    pub(super) fn cancel_turn(&mut self) {
        self.events = None;
        self.turn_started = None;
        self.pending = false;
        if let Some(m) = self.sessions[self.current].messages.last_mut() {
            m.streaming = false;
        }
    }

    pub(super) fn streaming_message(&mut self) -> Option<&mut ChatMessage> {
        let session = &mut self.sessions[self.current];
        session.messages.last_mut()
    }

    fn record_thought(&mut self, now: f64) {
        if let Some(started) = self.turn_started.take()
            && let Some(m) = self.streaming_message()
        {
            m.thought = Some((now - started).max(0.0));
        }
    }

    fn merge_tool(&mut self, call: ToolCall) {
        let Some(m) = self.streaming_message() else {
            return;
        };
        match m.tools.iter_mut().find(|t| t.name == call.name) {
            Some(t) if call.input.is_some() => {
                if call.request.is_some() {
                    t.request = call.request;
                }
                t.input = call.input;
                // Calls merge by name: a result means the tool's latest attempt worked.
                t.error = None;
            }
            Some(t) if call.error.is_some() => t.error = call.error,
            Some(_) => {}
            None => m.tools.push(call),
        }
    }

    /// Everything the request should send: the whole exchange except the assistant
    /// placeholder this turn is streaming into.
    fn history_for_request(&self) -> Vec<ChatMessage> {
        let session = &self.sessions[self.current];
        session.messages[..session.messages.len() - 1].to_vec()
    }

    pub fn send(&mut self) {
        let body = self.composer.text.trim().to_string();
        if body.is_empty() || self.pending {
            return;
        }
        self.composer.clear();
        self.send_prompt(&body);
    }

    /// Sends `prompt` as the user's message — the composer's send and the reply's
    /// suggestion quick actions both land here.
    pub fn send_prompt(&mut self, prompt: &str) {
        if self.pending {
            return;
        }
        let body = prompt.trim().to_string();
        if body.is_empty() {
            return;
        }
        // Suggestions belong to the reply they followed; a new turn retires them.
        for m in &mut self.sessions[self.current].messages {
            m.suggestions = None;
        }
        let sender = self.begin_turn(body);
        let history = self.history_for_request();
        self.spawn_turn(sender, history);
    }

    /// Re-runs the turn that produced `message_id`: drops the assistant message and streams
    /// a fresh one against the same history and conversation.
    pub fn retry(&mut self, message_id: &str) {
        if self.pending {
            return;
        }
        let Some(idx) = self.sessions[self.current]
            .messages
            .iter()
            .position(|m| m.id == message_id)
        else {
            return;
        };
        let messages = &self.sessions[self.current].messages;
        if idx == 0 || messages[idx - 1].role != Role::User {
            return;
        }
        let assistant_id = self.fresh_id();
        let session = &mut self.sessions[self.current];
        session.messages.truncate(idx);
        let mut assistant = ChatMessage::assistant(assistant_id);
        assistant.trace = Some(TurnTrace::new(self.now));
        session.messages.push(assistant);
        self.pending = true;
        self.error = None;
        tracing::info!("retrying the turn for {}", message_id);
        let sender = self.open_turn_channel();
        let history = self.history_for_request();
        self.spawn_turn(sender, history);
    }

    /// Spawns the request thread for a turn whose placeholder, channel and history exist.
    fn spawn_turn(&mut self, sender: TurnSender, history: Vec<ChatMessage>) {
        let client = Arc::clone(&self.client);
        let waker = self.waker.clone();
        let conversation = self.sessions[self.current].id.clone();
        tracing::info!(
            "turn starts via the {} client with {} history messages{}",
            client.name(),
            history.len(),
            conversation
                .as_deref()
                .map(|id| format!(" (conversation {id})"))
                .unwrap_or_default()
        );
        spawn_stream(
            client,
            history,
            conversation,
            move |event| {
                let _ = sender.send(event);
            },
            waker,
        );
    }

    /// Applies a turn event to the streaming reply.
    pub(super) fn apply_turn(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::Start { message_id } => {
                let session = &mut self.sessions[self.current];
                session.agent_message_id = Some(message_id.clone());
                // The streamed reply persists under this id: feedback needs it later.
                if let Some(m) = session.messages.last_mut() {
                    m.server_id = Some(message_id);
                }
            }
            StreamEvent::ConversationStarted(id) => {
                // The server persists the turn under this id: a fresh local session binds
                // to it, so later turns continue the same conversation. Its messages are
                // already here, so it counts as loaded: fetching them back would blank the
                // pane and replace the live exchange with whatever the server has so far.
                if let Some(session) = self.sessions.get_mut(self.current)
                    && session.id.is_none()
                {
                    session.id = Some(id);
                    session.history = Load::Idle.finished(self.now);
                    self.persist();
                }
            }
            StreamEvent::Suggestions(suggestions) => {
                if let Some(m) = self.streaming_message() {
                    m.suggestions = Some(suggestions);
                }
            }
            StreamEvent::TextDelta(t) => {
                if let Some(m) = self.streaming_message() {
                    m.text.push_str(&t);
                }
            }
            StreamEvent::ToolUsed(mut call) => {
                call.request = call.input.clone();
                self.merge_tool(call);
            }
            StreamEvent::ToolStarted { name, .. } => self.merge_tool(ToolCall {
                name,
                input: None,
                open: false,
                request: None,
                error: None,
            }),
            StreamEvent::ToolFailed { name, error, .. } => self.merge_tool(ToolCall {
                name,
                input: None,
                request: None,
                error: Some(error),
                open: false,
            }),
            StreamEvent::ToolFinished { name, output, .. } => self.merge_tool(ToolCall {
                name,
                input: output,
                open: false,
                request: None,
                error: None,
            }),
            StreamEvent::Finished => {
                if let Some(m) = self.streaming_message() {
                    m.streaming = false;
                }
                self.pending = false;
                self.maybe_notify();
            }
            StreamEvent::Failed(e) => {
                if let Some(m) = self.streaming_message() {
                    m.streaming = false;
                }
                self.pending = false;
                self.error = Some(e);
                self.maybe_notify();
            }
            _ => {}
        }
    }

    /// Drains the request thread's events. Frame time `t` also records how long the turn
    /// thought, shown on the assistant message once it settles. True when state changed.
    pub(super) fn drain_turn(&mut self, t: f64) -> bool {
        let mut changed = false;
        if self.pending && self.turn_started.is_none() {
            self.turn_started = Some(t);
            changed = true;
        }
        let mut dead = false;
        while let Some(receiver) = self.events.as_ref() {
            match receiver.try_recv() {
                Ok(Timed { event, at }) => {
                    if matches!(event, StreamEvent::Finished | StreamEvent::Failed(_)) {
                        self.record_thought(t);
                    }
                    let received = self.turn_origin.map_or(t, |(origin, base)| {
                        base + at.saturating_duration_since(origin).as_secs_f64()
                    });
                    self.apply_at(event, received);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    dead = true;
                    break;
                }
            }
        }
        if dead {
            self.events = None;
            if self.pending {
                self.record_thought(t);
                self.pending = false;
                if let Some(m) = self.sessions[self.current].messages.last_mut() {
                    m.streaming = false;
                }
                self.error = Some("the request thread ended early".into());
                changed = true;
            }
        }
        changed
    }
}
