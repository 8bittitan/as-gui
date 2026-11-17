use std::sync::Arc;

use serde_json::json;

use super::*;

/// Canned directory for tests, snapshots and `--mock`.
pub struct MockDirectory {
    pub agents: Vec<AgentInfo>,
    pub sessions: Vec<SessionInfo>,
    pub conversation: Vec<ChatMessage>,
}

impl AgentDirectory for MockDirectory {
    fn list(&self) -> Result<Vec<AgentInfo>, String> {
        Ok(self.agents.clone())
    }

    fn sessions(&self, _agent_id: &str) -> Result<Vec<SessionInfo>, String> {
        Ok(self.sessions.clone())
    }

    fn agent(&self, agent_id: &str) -> Result<AgentConfig, String> {
        let info = self
            .agents
            .iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| format!("no agent {agent_id}"))?;
        Ok(AgentConfig {
            name: info.name.clone(),
            description: info.description.clone(),
            status: info.status.clone(),
            model: info.model.clone(),
            client_tools: vec![ClientToolDecl {
                name: "throw_confetti".into(),
                description: Some("Throws confetti over the window".into()),
                input_schema: Some(json!({
                    "type": "object",
                    "properties": { "count": { "type": "integer" } }
                })),
                is_terminal: false,
            }],
            other_tools: vec!["load_skill_tool".into(), "schedule_task_tool".into()],
        })
    }

    fn conversation(
        &self,
        _agent_id: &str,
        _conversation_id: &str,
    ) -> Result<Vec<ChatMessage>, String> {
        Ok(self.conversation.clone())
    }
}

/// Canned reply for the mock agent, and the picker's mock agents.
pub const MOCK_REPLY: &str = "I am the mock agent — press ⌘, to connect your Algolia credentials, or restart with `AS_GUI_MOCK=1` removed to pick an Agent Studio agent.\n\nMeanwhile, markdown works:\n\n```rust\nlet k = satellite::kit(cx.system_dark(), Accent::Blue);\nlet button = Button::new(\"Send\").build(k);\n```\n\n- Responses stream in as they arrive\n- The composer submits on Enter";

/// The demo tool calls the mock streams, so the tool-call UI has an example to show.
pub fn mock_tools() -> Vec<ToolCall> {
    let schedule = "{\n  \"name\": \"inbox-check\",\n  \"cron\": \"0 9 * * 1-5\"\n}".to_string();
    let skill = serde_json::to_string_pretty(&json!({ "skill": "email-triage" })).unwrap();
    vec![
        ToolCall {
            call_id: String::new(),
            provider_metadata: None,
            name: "load_skill_tool".into(),
            input: Some(skill),
            open: false,
            request: None,
            error: None,
        },
        ToolCall {
            call_id: String::new(),
            provider_metadata: None,
            name: "schedule_task_tool".into(),
            input: Some(schedule),
            open: false,
            request: None,
            error: None,
        },
    ]
}

/// A scripted exchange, for the agents whose turns depend on the history they are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockScript {
    /// Ends the first request on a pending `throw_confetti` client call, and answers the
    /// request that carries its result with text.
    Confetti,
}

const CONFETTI_CALL_ID: &str = "mock_confetti_1";

/// Emits a canned response in small chunks, for tests, snapshots and `--mock`.
pub struct MockAgent {
    pub response: &'static str,
    pub chunk_chars: usize,
    pub tools: Vec<ToolCall>,
    pub script: Option<MockScript>,
}

impl AgentClient for MockAgent {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn send_feedback(&self, _agent_id: &str, _message_id: &str, _vote: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        history: &[ChatMessage],
        _conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        if self.script == Some(MockScript::Confetti) {
            confetti_script(history, emit);
            return Ok(());
        }
        for tool in &self.tools {
            emit(StreamEvent::ToolUsed(tool.clone()));
        }
        let chars: Vec<char> = self.response.chars().collect();
        for chunk in chars.chunks(self.chunk_chars.max(1)) {
            emit(StreamEvent::TextDelta(chunk.iter().collect()));
        }
        Ok(())
    }
}

fn call_answered(history: &[ChatMessage], call_id: &str) -> bool {
    history.last().is_some_and(|m| {
        m.parts.iter().any(|part| {
            matches!(part, Part::Tool(tool)
                if tool.call_id == call_id && tool.state != ToolState::InputAvailable)
        })
    })
}

fn confetti_script(history: &[ChatMessage], emit: &mut dyn FnMut(StreamEvent)) {
    emit(StreamEvent::StepStart);
    if call_answered(history, CONFETTI_CALL_ID) {
        emit(StreamEvent::TextDelta("Confetti is flying.".into()));
        return;
    }
    let input = json!({ "count": 50 });
    emit(StreamEvent::TextDelta("Let's celebrate.".into()));
    emit(StreamEvent::ToolStarted {
        call_id: CONFETTI_CALL_ID.into(),
        name: "throw_confetti".into(),
    });
    emit(StreamEvent::ToolUsed(ToolCall {
        call_id: CONFETTI_CALL_ID.into(),
        provider_metadata: None,
        name: "throw_confetti".into(),
        input: serde_json::to_string_pretty(&input).ok(),
        request: None,
        error: None,
        open: false,
    }));
    emit(StreamEvent::ClientToolsPending(vec![PendingCall {
        call_id: CONFETTI_CALL_ID.into(),
        name: "throw_confetti".into(),
        input,
    }]));
}

const DEMO_REPLY: &str = "Wide feet work best with a **Cushioned Series** shoe — the search above pulled the current guides.\n\nA few things worth knowing:\n\n- Pair them with insoles that add *arch support*\n- The width matters more than the brand\n- The tier list below changes with every release\n\nWant me to compare two shoes side by side?";

/// The mock the recorded tour (`just demo`, `AS_GUI_DEMO=1`) talks to: a search tool with hit
/// cards, then a reply that streams slowly enough to watch.
pub struct DemoAgent;

impl AgentClient for DemoAgent {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn send_feedback(&self, _agent_id: &str, _message_id: &str, _vote: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        _history: &[ChatMessage],
        _conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        let hits = json!({ "hits": [
            { "title": "Fit guide: wide feet", "path": "/guides/wide-fit",
              "content": "###### *Updated for the 2026 season*\n\nThe current best **shoe and insole** pairings for wide feet.\n\n- W = Wide\n- XW = Extra wide\n\nAll sizes assume a half size up." },
            { "title": "Shoe tier list", "path": "/guides/shoes",
              "content": "Where every shoe lands **after the latest release** — including the new arrivals." },
            { "title": "Insole pairings", "path": "/guides/insoles",
              "content": "Pairings that cover **arch support** needs." },
        ] });
        emit(StreamEvent::ToolStarted {
            call_id: "demo_search".into(),
            name: "search_tool".into(),
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        emit(StreamEvent::ToolFinished {
            call_id: "demo_search".into(),
            name: "search_tool".into(),
            output: serde_json::to_string_pretty(&hits).ok(),
        });
        let chars: Vec<char> = DEMO_REPLY.chars().collect();
        for chunk in chars.chunks(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
            emit(StreamEvent::TextDelta(chunk.iter().collect()));
        }
        Ok(())
    }
}

const DEMO_CLIENT_CALL_ID: &str = "demo_open_guide";
const DEMO_GUIDE_URL: &str = "https://shop.example.com/guides/wide-fit";
const DEMO_CLIENT_INTRO: &str = "I'll open the wide-fit guide for you.";
const DEMO_CLIENT_REPLY: &str = "The wide-fit guide is open in your browser.";

/// The mock `just demo client-tool` (`AS_GUI_DEMO_CLIENT_TOOL=1`) talks to: it ends its first
/// request on an `open_url` call, and answers the request that carries the result.
pub struct DemoClientToolAgent;

impl AgentClient for DemoClientToolAgent {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn send_feedback(&self, _agent_id: &str, _message_id: &str, _vote: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        history: &[ChatMessage],
        _conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        emit(StreamEvent::StepStart);
        let answered = call_answered(history, DEMO_CLIENT_CALL_ID);
        let text = if answered {
            DEMO_CLIENT_REPLY
        } else {
            DEMO_CLIENT_INTRO
        };
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
            emit(StreamEvent::TextDelta(chunk.iter().collect()));
        }
        if answered {
            return Ok(());
        }
        let input = json!({ "url": DEMO_GUIDE_URL });
        emit(StreamEvent::ToolStarted {
            call_id: DEMO_CLIENT_CALL_ID.into(),
            name: "open_url".into(),
        });
        emit(StreamEvent::ToolUsed(ToolCall {
            call_id: DEMO_CLIENT_CALL_ID.into(),
            provider_metadata: None,
            name: "open_url".into(),
            input: serde_json::to_string_pretty(&input).ok(),
            request: None,
            error: None,
            open: false,
        }));
        emit(StreamEvent::ClientToolsPending(vec![PendingCall {
            call_id: DEMO_CLIENT_CALL_ID.into(),
            name: "open_url".into(),
            input,
        }]));
        Ok(())
    }
}

pub fn demo_mode() -> bool {
    (cfg!(debug_assertions) || rune::is_headless())
        && std::env::var("AS_GUI_DEMO").is_ok_and(|v| v == "1")
}

/// The mock directory: two agents, one a draft, and no conversations.
pub fn mock_directory() -> Arc<dyn AgentDirectory> {
    Arc::new(MockDirectory {
        agents: vec![
            AgentInfo {
                agent_id: "mock-shopping".into(),
                name: "Mock shopping assistant".into(),
                description: Some("Answers questions about a demo catalog".into()),
                status: AgentStatus::Published,
                model: Some("mock-2".into()),
            },
            AgentInfo {
                agent_id: "mock-draft".into(),
                name: "Mock draft".into(),
                description: None,
                status: AgentStatus::Draft,
                model: None,
            },
        ],
        sessions: vec![],
        conversation: vec![],
    })
}

/// The mock client: the demo's by `AS_GUI_DEMO=1` (the client-tool demo's with
/// `AS_GUI_DEMO_CLIENT_TOOL=1` on top), the client-tool script's by `AS_GUI_MOCK_CLIENT_TOOL=1`,
/// otherwise the plain canned one.
pub fn mock_client() -> Arc<dyn AgentClient> {
    if demo_mode() {
        if std::env::var_os("AS_GUI_DEMO_CLIENT_TOOL").is_some_and(|v| v == "1") {
            return Arc::new(DemoClientToolAgent);
        }
        return Arc::new(DemoAgent);
    }
    if std::env::var_os("AS_GUI_MOCK_CLIENT_TOOL").is_some_and(|v| v == "1") {
        return Arc::new(MockAgent {
            response: "",
            chunk_chars: 4,
            tools: Vec::new(),
            script: Some(MockScript::Confetti),
        });
    }
    Arc::new(MockAgent {
        response: MOCK_REPLY,
        chunk_chars: 24,
        tools: mock_tools(),
        script: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_agent_searches_then_streams_its_reply() {
        let mut events = Vec::new();
        DemoAgent
            .stream(&[], None, &mut |e| events.push(e))
            .unwrap();
        assert!(
            matches!(events.first(), Some(StreamEvent::ToolStarted { .. })),
            "the search starts first"
        );
        let Some(StreamEvent::ToolFinished { output, .. }) = events.get(1) else {
            panic!("and finishes with its hits");
        };
        let hits = SearchHit::from_json(output.as_deref().unwrap()).unwrap();
        assert_eq!(hits.len(), 3);
        let reply: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reply, DEMO_REPLY);
    }

    #[test]
    fn the_mock_streams_its_whole_response() {
        let mut got = String::new();
        MockAgent {
            response: "Hello there",
            chunk_chars: 4,
            tools: Vec::new(),
            script: None,
        }
        .stream(&[], None, &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                got.push_str(&t)
            }
        })
        .unwrap();
        assert_eq!(got, "Hello there");
    }

    #[test]
    fn the_mock_streams_its_tools_before_the_reply() {
        let mut names = Vec::new();
        let mut text = String::new();
        MockAgent {
            response: "Hi",
            chunk_chars: 4,
            tools: mock_tools(),
            script: None,
        }
        .stream(&[], None, &mut |ev| match ev {
            StreamEvent::ToolUsed(t) => names.push(t.name),
            StreamEvent::TextDelta(t) => text.push_str(&t),
            _ => {}
        })
        .unwrap();
        assert_eq!(names, ["load_skill_tool", "schedule_task_tool"]);
        assert_eq!(text, "Hi");
    }

    fn confetti_agent() -> MockAgent {
        MockAgent {
            response: "",
            chunk_chars: 4,
            tools: Vec::new(),
            script: Some(MockScript::Confetti),
        }
    }

    #[test]
    fn the_confetti_script_ends_its_first_request_on_a_pending_client_call() {
        let mut events = Vec::new();
        confetti_agent()
            .stream(&[ChatMessage::user("u1", "party")], None, &mut |e| {
                events.push(e)
            })
            .unwrap();
        let Some(StreamEvent::ClientToolsPending(calls)) = events.last() else {
            panic!("the request ends on the pending call: {events:?}");
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "throw_confetti");
        assert_eq!(calls[0].call_id, CONFETTI_CALL_ID);
    }

    #[test]
    fn the_confetti_script_answers_a_history_that_ends_with_the_calls_result() {
        let mut reply = ChatMessage::assistant("m2");
        reply.parts = vec![
            Part::StepStart,
            Part::Tool(ToolPart {
                call_id: CONFETTI_CALL_ID.into(),
                name: "throw_confetti".into(),
                input: json!({ "count": 50 }),
                state: ToolState::OutputAvailable,
                output: Some(json!({ "ok": true })),
                error: None,
                provider_metadata: None,
            }),
        ];
        let history = [ChatMessage::user("u1", "party"), reply];
        let mut events = Vec::new();
        confetti_agent()
            .stream(&history, None, &mut |e| events.push(e))
            .unwrap();
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, StreamEvent::ClientToolsPending(_)))
        );
        assert!(events.contains(&StreamEvent::TextDelta("Confetti is flying.".into())));
    }

    fn open_url_result_history() -> [ChatMessage; 2] {
        let mut reply = ChatMessage::assistant("m2");
        reply.parts = vec![
            Part::StepStart,
            Part::Tool(ToolPart {
                call_id: DEMO_CLIENT_CALL_ID.into(),
                name: "open_url".into(),
                input: json!({ "url": DEMO_GUIDE_URL }),
                state: ToolState::OutputAvailable,
                output: Some(json!({ "opened": DEMO_GUIDE_URL })),
                error: None,
                provider_metadata: None,
            }),
        ];
        [ChatMessage::user("u1", "open the guide"), reply]
    }

    #[test]
    fn the_demo_client_tool_agent_ends_its_first_request_on_an_open_url_call() {
        let mut events = Vec::new();
        DemoClientToolAgent
            .stream(
                &[ChatMessage::user("u1", "open the guide")],
                None,
                &mut |e| events.push(e),
            )
            .unwrap();
        let Some(StreamEvent::ClientToolsPending(calls)) = events.last() else {
            panic!("the request ends on the pending call: {events:?}");
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "open_url");
        assert_eq!(calls[0].call_id, DEMO_CLIENT_CALL_ID);
        assert_eq!(calls[0].input, json!({ "url": DEMO_GUIDE_URL }));
    }

    #[test]
    fn the_demo_client_tool_agent_answers_a_history_that_ends_with_the_calls_result() {
        let mut events = Vec::new();
        DemoClientToolAgent
            .stream(&open_url_result_history(), None, &mut |e| events.push(e))
            .unwrap();
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, StreamEvent::ClientToolsPending(_)))
        );
        let reply: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reply, DEMO_CLIENT_REPLY);
    }

    #[test]
    fn the_mock_directory_lists_its_agents() {
        let agents = vec![AgentInfo {
            agent_id: "a1".into(),
            name: "One".into(),
            description: None,
            status: AgentStatus::Published,
            model: None,
        }];
        assert_eq!(
            MockDirectory {
                agents: agents.clone(),
                sessions: Vec::new(),
                conversation: Vec::new(),
            }
            .list()
            .unwrap(),
            agents
        );
    }
}
