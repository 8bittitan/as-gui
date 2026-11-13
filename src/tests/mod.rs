//! Runtime-based tests: they drive the whole app — state, view, keys and frames — so they
//! live together rather than inside the modules they span. Pure parsing and formatting
//! logic is tested next to its implementation (`agent`, `logging`).

use std::sync::Arc;

use rune::{App, AppSpec, Env, Key, KeyEvent, Mods, Runtime};

use crate::agent::AgentSnapshot;
use crate::agent::{
    AgentInfo, AgentStatus, ChatMessage, IndexSearch, MockAgent, MockDirectory, Role, SearchPage,
    SessionInfo, StreamEvent, ToolCall, TraceSpan, TurnTrace, parse_search_page,
};
use crate::chat::{self, Accent, Chat, ChatWidth, Session, ThemePref};
#[cfg(feature = "knobs")]
use crate::knobs;
use crate::loading::Load;
use crate::quick_ask::{self, QuickAsk};
use crate::shell::{on_key, view};
use crate::store::Persisted;

mod accessibility;
mod applications;
mod ask_selection;
mod buttons;
mod command_bar;
mod conversation;
mod hit_reader;
mod keybinds;
mod loading;
mod menu_bar;
mod notifications;
mod playground;
mod quick_ask_panel;
mod search;
mod timeline;
mod tool_calls;
mod tuning;

fn test_chat() -> Runtime<Chat> {
    test_chat_sized((720.0, 480.0))
}

fn test_chat_sized(size: (f32, f32)) -> Runtime<Chat> {
    let env = Env {
        system_dark: true,
        scale: 2.0,
    };
    let client = Arc::new(MockAgent {
        response: "Hi!",
        chunk_chars: 2,
        tools: Vec::new(),
    });
    let directory = Arc::new(MockDirectory {
        agents: Vec::new(),
        sessions: Vec::new(),
        conversation: Vec::new(),
    });
    let mut rt = Runtime::new(Chat::new(client, directory), view, size, env);
    rt.poll = Some(chat::poll);
    // The app normally runs with credentials; tests start there so the welcome dialog stays
    // out of the way. Mock is forced so `poll` doesn't auto-load the (empty) test directory
    // under the tests that drive loads themselves; the credentials tests opt back in.
    rt.update(|s, _| {
        s.applications = vec![crate::chat::Application::new("test-app", "test-key")];
        s.active = Some(0);
        s.mock_forced = true;
    });
    rt.frame(0.0);
    rt
}

fn agent(id: &str, name: &str, status: AgentStatus) -> AgentInfo {
    AgentInfo {
        agent_id: id.into(),
        name: name.into(),
        description: None,
        status,
        model: None,
    }
}

/// What stands in for the real API: a client and a directory that answer from memory.
fn offline_connector(agents: Vec<AgentInfo>) -> chat::Connector {
    std::rc::Rc::new(move |_| {
        let client: Arc<dyn crate::agent::AgentClient> = Arc::new(MockAgent {
            response: "Hi!",
            chunk_chars: 2,
            tools: Vec::new(),
        });
        let directory: Arc<dyn crate::agent::AgentDirectory> = Arc::new(MockDirectory {
            agents: agents.clone(),
            sessions: Vec::new(),
            conversation: Vec::new(),
        });
        (client, directory)
    })
}

fn agent_one() -> AgentInfo {
    agent("a1", "One", AgentStatus::Published)
}

/// The app as `main` builds it, with the playground attached (debug builds only), state from
/// `AppSpec::init` — the same mock a playground scene starts from.
fn playground_app() -> Runtime<Chat> {
    let env = Env {
        system_dark: true,
        scale: 2.0,
    };
    let app = App::new(AppSpec::<Chat> {
        scenes: crate::SCENES,
        fonts: crate::satellite::FONTS,
        families: crate::satellite::FAMILIES,
        poll: Some(chat::poll),
        on_key: Some(crate::shell::on_key),
        ..AppSpec::new("as-gui-test", (860.0, 600.0), crate::mock_chat, view)
    })
    .playground(crate::playground::playground());
    let mut rt = app.headless_runtime((860.0, 600.0), env);
    rt.frame(0.0);
    rt
}

/// A click at the middle of the element keyed `key`.
fn tap<S>(rt: &mut Runtime<S>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_move(x, y);
    rt.frame(0.0);
    rt.pointer_down(x, y);
    rt.frame(0.0);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

#[test]
fn a_restored_agent_selects_itself_when_the_directory_lists_it() {
    let mut rt = test_chat();
    rt.frame(0.0);
    rt.update(|s, _| {
        s.restore(&Persisted {
            agent_id: Some("a2".into()),
            ..Persisted::default()
        })
    });
    rt.frame(0.0);
    assert!(
        rt.state.selected_agent.is_none(),
        "nothing until the list lands"
    );

    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![
            agent("a1", "One", AgentStatus::Published),
            agent("a2", "Two", AgentStatus::Published),
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state
            .selected_agent
            .as_ref()
            .map(|a| a.agent_id.as_str()),
        Some("a2"),
        "the persisted agent selects itself"
    );
    assert!(rt.find("composer").is_some(), "the app opens to its chat");

    // A draft the user last had selected can't come back; the picker stays up.
    rt.update(|s, _| {
        s.selected_agent = None;
        s.restore(&Persisted {
            agent_id: Some("a3".into()),
            ..Persisted::default()
        })
    });
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![agent(
            "a1",
            "One",
            AgentStatus::Published,
        )]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert!(
        rt.state.selected_agent.is_none(),
        "an unknown id leaves the picker up"
    );
}

/// Frames for the dialog's overlay entrance to finish, so its controls take pointer input.
fn settle(rt: &mut Runtime<Chat>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn quick_ask() -> Runtime<QuickAsk> {
    let env = Env {
        system_dark: true,
        scale: 2.0,
    };
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot {
        agent_id: "a1".into(),
        agent_name: "One".into(),
        client: Arc::new(MockAgent {
            response: "Quick answer",
            chunk_chars: 4,
            tools: Vec::new(),
        }),
        ..AgentSnapshot::default()
    }));
    let mut rt = Runtime::new(
        QuickAsk::new(snapshot, None, None),
        quick_ask::view,
        (480.0, 300.0),
        env,
    );
    rt.poll = Some(quick_ask::poll);
    rt.frame(0.0);
    rt
}

/// An agent that remembers the messages each request sent.
struct HistoryRecorder {
    sent: std::sync::Mutex<Vec<Vec<(Role, String)>>>,
}

impl crate::agent::AgentClient for HistoryRecorder {
    fn name(&self) -> &'static str {
        "recorder"
    }

    fn send_feedback(&self, _: &str, _: &str, _: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        history: &[ChatMessage],
        _conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        self.sent
            .lock()
            .unwrap()
            .push(history.iter().map(|m| (m.role, m.text.clone())).collect());
        emit(StreamEvent::TextDelta("ok".into()));
        Ok(())
    }
}

fn settle_quick_ask(rt: &mut Runtime<QuickAsk>) {
    for _ in 0..200 {
        rt.frame(1.0 / 60.0);
        if !rt.state.pending {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn quick_ask_with(reply: impl FnOnce(&mut ChatMessage), pending: bool) -> Runtime<QuickAsk> {
    let mut rt = quick_ask();
    rt.update(|s, _| {
        s.messages.push(ChatMessage::user("qa1", "What is new?"));
        let mut m = ChatMessage::assistant("qa2");
        reply(&mut m);
        s.messages.push(m);
        s.pending = pending;
    });
    rt.frame(1.0 / 60.0);
    rt
}

/// A turn whose one tool call fails mid-stream, then the reply carries on and finishes.
fn failed_tool_chat() -> (Runtime<Chat>, String) {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.now = 0.0;
    });
    let _sender = rt.state.begin_turn("List the brands");
    rt.update(|s, _| {
        s.apply_at(
            StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "facet_tool".into(),
            },
            0.2,
        );
        s.apply_at(
            StreamEvent::ToolUsed(ToolCall {
                name: "facet_tool".into(),
                input: Some("{\"facetName\": \"brand\"}".into()),
                request: None,
                error: None,
                open: false,
            }),
            0.3,
        );
        s.apply_at(
            StreamEvent::ToolFailed {
                call_id: "c1".into(),
                name: "facet_tool".into(),
                error: "Cannot search in `brand` attribute".into(),
            },
            0.9,
        );
        s.apply_at(
            StreamEvent::TextDelta("Let me try another way.".into()),
            1.2,
        );
        s.apply_at(StreamEvent::Finished, 2.0);
    });
    rt.frame(1.0 / 60.0);
    let id = rt.state.sessions[0].messages[1].id.clone();
    (rt, id)
}

/// A chat whose one live turn searched for 1.4s, then streamed a reply, settled.
fn timed_turn_chat() -> (Runtime<Chat>, String) {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.now = 0.0;
    });
    let _sender = rt.state.begin_turn("Find a shoe");
    rt.update(|s, _| {
        s.apply_at(
            StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "search_tool".into(),
            },
            0.5,
        );
        s.apply_at(
            StreamEvent::ToolFinished {
                call_id: "c1".into(),
                name: "search_tool".into(),
                output: Some("{\n  \"marker\": \"OUTPUT\"\n}".into()),
            },
            1.9,
        );
        s.apply_at(StreamEvent::TextDelta("Use a Cushioned shoe.".into()), 2.1);
        s.apply_at(StreamEvent::Finished, 4.2);
    });
    rt.frame(1.0 / 60.0);
    let id = rt.state.sessions[0].messages[1].id.clone();
    (rt, id)
}

fn shows_text(rt: &Runtime<Chat>, needle: &str, key: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.text.as_deref().is_some_and(|t| t.contains(needle)) && e.path.contains(key))
}
