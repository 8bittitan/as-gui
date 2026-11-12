use std::sync::Arc;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    /// The id Agent Studio persists the message under, when it has one: feedback needs it.
    pub server_id: Option<String>,
    pub role: Role,
    pub text: String,
    pub tools: Vec<ToolCall>,
    pub tools_open: bool,
    /// Seconds the turn spent "thinking", recorded when the stream settles.
    pub thought: Option<f64>,
    pub streaming: bool,
    /// This message's 👍/👎, once the vote was sent.
    pub vote: Option<i8>,
    /// Prompt suggestions the agent offered with this reply, as quick actions.
    pub suggestions: Option<Vec<String>>,
    /// Where this live turn's time went; history loaded from the API has none.
    pub trace: Option<TurnTrace>,
}

/// When a live turn's events arrived, in seconds on the app's frame clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnTrace {
    pub sent: f64,
    pub first_token: Option<f64>,
    pub last_token: Option<f64>,
    /// Characters streamed in text deltas, the basis of the token-rate estimate.
    pub chars: usize,
    pub finished: Option<f64>,
    pub spans: Vec<TraceSpan>,
    pub open: bool,
}

impl TurnTrace {
    pub fn new(sent: f64) -> Self {
        TurnTrace {
            sent,
            ..TurnTrace::default()
        }
    }

    /// The running span of call `call_id` (else the latest running one of tool `name`, else a
    /// new one), ended at `at`.
    fn closing(&mut self, call_id: &str, name: &str, at: f64) -> &mut TraceSpan {
        let running = |s: &TraceSpan| s.end.is_none();
        let index = self
            .spans
            .iter()
            .rposition(|s| running(s) && !call_id.is_empty() && s.call_id == call_id)
            .or_else(|| {
                self.spans
                    .iter()
                    .rposition(|s| running(s) && s.name == name)
            })
            .unwrap_or_else(|| {
                self.spans.push(TraceSpan::new(call_id, name, at, None));
                self.spans.len() - 1
            });
        let span = &mut self.spans[index];
        span.end = Some(at);
        span
    }

    /// Notes what `event`, arriving at `at`, adds to the turn's timeline.
    pub fn record(&mut self, event: &StreamEvent, at: f64) {
        match event {
            StreamEvent::TextDelta(text) => {
                self.first_token.get_or_insert(at);
                self.last_token = Some(at);
                self.chars += text.chars().count();
            }
            StreamEvent::ToolStarted { call_id, name } => {
                self.spans.push(TraceSpan::new(call_id, name, at, None));
            }
            StreamEvent::ToolUsed(call) => {
                match self.spans.iter_mut().rev().find(|s| s.name == call.name) {
                    Some(span) => {
                        if call.input.is_some() {
                            span.input = call.input.clone();
                        }
                    }
                    None => {
                        let mut span = TraceSpan::new("", &call.name, at, Some(at));
                        span.input = call.input.clone();
                        self.spans.push(span);
                    }
                }
            }
            StreamEvent::ToolFinished {
                call_id,
                name,
                output,
            } => {
                let span = self.closing(call_id, name, at);
                span.output = output.clone();
            }
            StreamEvent::ToolFailed {
                call_id,
                name,
                error,
            } => {
                let span = self.closing(call_id, name, at);
                span.error = Some(error.clone());
            }
            StreamEvent::Finished | StreamEvent::Failed(_) => {
                self.finished.get_or_insert(at);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraceSpan {
    pub call_id: String,
    pub name: String,
    pub start: f64,
    pub end: Option<f64>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub open: bool,
}

impl TraceSpan {
    fn new(call_id: &str, name: &str, start: f64, end: Option<f64>) -> Self {
        TraceSpan {
            call_id: call_id.into(),
            name: name.into(),
            start,
            end,
            input: None,
            output: None,
            error: None,
            open: false,
        }
    }
}

impl ChatMessage {
    pub fn user(id: impl Into<String>, text: impl Into<String>) -> Self {
        ChatMessage {
            id: id.into(),
            server_id: None,
            role: Role::User,
            text: text.into(),
            tools: Vec::new(),
            tools_open: false,
            thought: None,
            streaming: false,
            vote: None,
            suggestions: None,
            trace: None,
        }
    }
    pub fn assistant(id: impl Into<String>) -> Self {
        ChatMessage {
            id: id.into(),
            server_id: None,
            role: Role::Assistant,
            text: String::new(),
            tools: Vec::new(),
            tools_open: false,
            thought: None,
            streaming: true,
            vote: None,
            suggestions: None,
            trace: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    /// Pretty-printed JSON of the call's input (or output), when the stream carried one.
    pub input: Option<String>,
    /// The call's own arguments, kept when its result replaces `input`.
    pub request: Option<String>,
    /// Why the call failed, when the stream said so (`tool-output-error`).
    pub error: Option<String>,
    pub open: bool,
}

impl ToolCall {
    /// The search this call made, when its arguments describe one.
    pub fn search_request(&self) -> Option<QueryParams> {
        query_params(self.request.as_deref().or(self.input.as_deref())?)
    }
}

/// One event of the AI SDK v5 UI message stream.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Start {
        message_id: String,
    },
    TextDelta(String),
    ToolUsed(ToolCall),
    ToolStarted {
        call_id: String,
        name: String,
    },
    ToolFinished {
        call_id: String,
        name: String,
        output: Option<String>,
    },
    /// A tool call that errored (`tool-output-error`). The turn goes on: the agent may try
    /// something else.
    ToolFailed {
        call_id: String,
        name: String,
        error: String,
    },
    Finished,
    Failed(String),
    AgentsLoaded(Vec<AgentInfo>),
    AgentsFailed(String),
    SessionsLoaded(Vec<SessionInfo>),
    SessionsFailed(String),
    ConversationLoaded {
        session_id: String,
        messages: Vec<ChatMessage>,
    },
    ConversationFailed {
        session_id: String,
        error: String,
    },
    /// The id the server persists this turn under, from the response's `X-Conversation-Id`
    /// header: a new conversation's id, or the one the request continued.
    ConversationStarted(String),
    /// Prompt suggestions the agent offered with its reply (`data-suggestions`).
    Suggestions(Vec<String>),
    /// A "search it yourself" run's page, tagged with the run that asked for it.
    SearchLoaded {
        run: u64,
        page: SearchPage,
    },
    SearchFailed {
        run: u64,
        error: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Published,
    Draft,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentInfo {
    pub agent_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: AgentStatus,
    pub model: Option<String>,
}

/// One conversation of an agent's history (`ConversationBaseResponse`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub title: Option<String>,
}

/// The agent a Quick Ask panel talks to: a snapshot of the main window's current agent and
/// its client, published on every change.
#[derive(Clone)]
pub struct AgentSnapshot {
    pub agent_id: String,
    pub agent_name: String,
    pub client: Arc<dyn AgentClient>,
    /// The main window's appearance, so the panel dresses the same.
    pub theme: crate::chat::ThemePref,
    pub accent: crate::chat::Accent,
    /// The application in use, as its name and colour, for the panel's header chip.
    pub application: Option<(String, crate::palette::AppColour)>,
}

impl Default for AgentSnapshot {
    fn default() -> Self {
        AgentSnapshot {
            agent_id: String::new(),
            agent_name: String::new(),
            client: Arc::new(MockAgent {
                response: MOCK_REPLY,
                chunk_chars: 24,
                tools: Vec::new(),
            }),
            theme: Default::default(),
            accent: Default::default(),
            application: None,
        }
    }
}
