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
            name: "load_skill_tool".into(),
            input: Some(skill),
            open: false,
            request: None,
            error: None,
        },
        ToolCall {
            name: "schedule_task_tool".into(),
            input: Some(schedule),
            open: false,
            request: None,
            error: None,
        },
    ]
}

/// Emits a canned response in small chunks, for tests, snapshots and `--mock`.
pub struct MockAgent {
    pub response: &'static str,
    pub chunk_chars: usize,
    pub tools: Vec<ToolCall>,
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
        _history: &[ChatMessage],
        _conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
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

/// The mock client: the demo's by `AS_GUI_DEMO=1`, otherwise the plain canned one.
pub fn mock_client() -> Arc<dyn AgentClient> {
    if demo_mode() {
        return Arc::new(DemoAgent);
    }
    Arc::new(MockAgent {
        response: MOCK_REPLY,
        chunk_chars: 24,
        tools: mock_tools(),
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
