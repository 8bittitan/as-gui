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
    /// The reply as the stream laid it out, which a continued turn replays to the server.
    pub parts: Vec<Part>,
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

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    StepStart,
    Text(String),
    Tool(ToolPart),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolPart {
    pub call_id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub state: ToolState,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    /// Sent back as `callProviderMetadata`: some providers need it to accept the result.
    pub provider_metadata: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    InputAvailable,
    OutputAvailable,
    OutputError,
}

/// A tool call the stream ended on without an output: the client has to run it.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingCall {
    pub call_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

fn json_of(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.into()))
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
                let same_call = |s: &TraceSpan| {
                    if call.call_id.is_empty() || s.call_id.is_empty() {
                        s.name == call.name
                    } else {
                        s.call_id == call.call_id
                    }
                };
                match self.spans.iter_mut().rev().find(|s| same_call(s)) {
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
            parts: Vec::new(),
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
            parts: Vec::new(),
            thought: None,
            streaming: true,
            vote: None,
            suggestions: None,
            trace: None,
        }
    }
}

impl ChatMessage {
    /// Appends streamed text to the last text part, or opens one: text after a tool or a
    /// step starts a part of its own, and the paragraph break that marked it is dropped.
    pub fn push_text(&mut self, delta: &str) {
        self.text.push_str(delta);
        if let Some(Part::Text(text)) = self.parts.last_mut() {
            text.push_str(delta);
        } else {
            self.parts
                .push(Part::Text(delta.trim_start_matches('\n').to_string()));
        }
    }

    fn tool_part_mut(&mut self, call_id: &str, name: &str) -> Option<&mut ToolPart> {
        let mut tools = self.parts.iter_mut().filter_map(|part| match part {
            Part::Tool(tool) => Some(tool),
            _ => None,
        });
        if call_id.is_empty() {
            tools.rfind(|tool| tool.name == name)
        } else {
            tools.find(|tool| tool.call_id == call_id)
        }
    }

    fn tool_part_or_new(&mut self, call_id: &str, name: &str) -> &mut ToolPart {
        if self.tool_part_mut(call_id, name).is_none() {
            self.parts.push(Part::Tool(ToolPart {
                call_id: call_id.into(),
                name: name.into(),
                input: serde_json::Value::Null,
                state: ToolState::InputAvailable,
                output: None,
                error: None,
                provider_metadata: None,
            }));
        }
        self.tool_part_mut(call_id, name)
            .expect("the part was just pushed")
    }

    /// Records a call whose input arrived complete.
    pub fn note_tool_input(&mut self, call: &ToolCall) {
        let part = self.tool_part_or_new(&call.call_id, &call.name);
        part.input = call
            .input
            .as_deref()
            .map_or(serde_json::Value::Null, json_of);
        if call.provider_metadata.is_some() {
            part.provider_metadata = call.provider_metadata.clone();
        }
    }

    pub fn note_tool_output(&mut self, call_id: &str, name: &str, output: Option<&str>) {
        let part = self.tool_part_or_new(call_id, name);
        part.state = ToolState::OutputAvailable;
        part.output = output.map(json_of);
        part.error = None;
    }

    pub fn note_tool_error(&mut self, call_id: &str, name: &str, error: &str) {
        let part = self.tool_part_or_new(call_id, name);
        part.state = ToolState::OutputError;
        part.output = None;
        part.error = Some(error.into());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    /// The stream's id for the call; empty for history that predates ids.
    pub call_id: String,
    pub name: String,
    /// `providerMetadata` from the call's `tool-input-available`.
    pub provider_metadata: Option<serde_json::Value>,
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
    AgentConfigLoaded {
        agent_id: String,
        config: AgentConfig,
    },
    AgentConfigFailed {
        agent_id: String,
        error: String,
    },
    /// The id the server persists this turn under, from the response's `X-Conversation-Id`
    /// header: a new conversation's id, or the one the request continued.
    ConversationStarted(String),
    /// A step began (`start-step`): the next text and tools belong to it.
    StepStart,
    /// The stream ended with these calls holding complete input and no output: the client
    /// runs them and continues the turn.
    ClientToolsPending(Vec<PendingCall>),
    /// A client tool call's result, from its worker thread.
    ClientToolResolved {
        call_id: String,
        result: Result<serde_json::Value, String>,
    },
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

/// A tool the app runs for the agent, as the agent declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientToolDecl {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub is_terminal: bool,
}

/// One agent's configuration (`GET /agents/{id}`).
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    pub name: String,
    pub description: Option<String>,
    pub status: AgentStatus,
    pub model: Option<String>,
    pub client_tools: Vec<ClientToolDecl>,
    /// The names of the tools the server runs.
    pub other_tools: Vec<String>,
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
                script: None,
            }),
            theme: Default::default(),
            accent: Default::default(),
            application: None,
        }
    }
}
