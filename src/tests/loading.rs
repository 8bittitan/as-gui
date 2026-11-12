use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;
use crate::agent::AgentDirectory;
use crate::loading::{Load, PaneView, Screen, SidebarView};

/// A directory whose every call waits for the test to let it through, so a test decides
/// exactly when each load lands.
struct Gated {
    agents: Vec<AgentInfo>,
    sessions: Mutex<Vec<Vec<SessionInfo>>>,
    history: Vec<ChatMessage>,
    history_error: Option<String>,
    gate: Mutex<Receiver<()>>,
}

impl Gated {
    fn wait(&self) {
        let _ = self.gate.lock().unwrap().recv();
    }
}

impl AgentDirectory for Gated {
    fn list(&self) -> Result<Vec<AgentInfo>, String> {
        self.wait();
        Ok(self.agents.clone())
    }

    fn sessions(&self, agent_id: &str) -> Result<Vec<SessionInfo>, String> {
        self.wait();
        let lists = self.sessions.lock().unwrap();
        let which = usize::from(agent_id == "a2" && lists.len() > 1);
        Ok(lists.get(which).cloned().unwrap_or_default())
    }

    fn conversation(&self, _: &str, _: &str) -> Result<Vec<ChatMessage>, String> {
        self.wait();
        match &self.history_error {
            Some(e) => Err(e.clone()),
            None => Ok(self.history.clone()),
        }
    }
}

fn info(id: &str, title: &str) -> SessionInfo {
    SessionInfo {
        id: id.into(),
        title: Some(title.into()),
    }
}

/// A chat on a gated directory with loads switched on, as the real app starts them.
fn launched(
    sessions: Vec<Vec<SessionInfo>>,
    remembered: Option<&str>,
) -> (Runtime<Chat>, Sender<()>) {
    let (release, gate) = channel();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.mock_forced = false;
        s.directory = Arc::new(Gated {
            agents: vec![agent_one(), agent("a2", "Two", AgentStatus::Published)],
            sessions: Mutex::new(sessions),
            history: vec![ChatMessage::user("h1", "Earlier question"), {
                let mut m = ChatMessage::assistant("h2");
                m.text = "Earlier answer".into();
                m.streaming = false;
                m
            }],
            history_error: None,
            gate: Mutex::new(gate),
        });
        if let Some(id) = remembered {
            s.restore(&Persisted {
                agent_id: Some(id.into()),
                ..Persisted::default()
            });
        }
    });
    (rt, release)
}

type Views = (Screen, SidebarView, PaneView);

fn views(rt: &Runtime<Chat>) -> Views {
    let s = &rt.state;
    (s.screen(s.now), s.sidebar_view(s.now), s.pane_view(s.now))
}

/// Frames of no elapsed time, until `done` holds: lets a released load land without
/// moving the clock the 150ms and 300ms rules read.
fn land(rt: &mut Runtime<Chat>, done: impl Fn(&Chat) -> bool) {
    for _ in 0..400 {
        rt.frame(0.0);
        if done(&rt.state) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the load never landed");
}

fn at(rt: &mut Runtime<Chat>, seconds: f64) {
    rt.frame(seconds);
}

#[test]
fn the_screen_is_a_splash_until_a_restored_launch_has_its_conversation_list() {
    let (mut rt, release) = launched(
        vec![vec![info("c1", "First"), info("c2", "Second")]],
        Some("a1"),
    );
    let mut trace: Vec<Views> = Vec::new();
    let mut note = |rt: &Runtime<Chat>| trace.push(views(rt));

    rt.frame(0.0);
    assert_eq!(
        rt.state.screen(rt.state.now),
        Screen::Splash { spinner: false }
    );
    note(&rt);

    at(&mut rt, 0.2);
    assert_eq!(
        rt.state.screen(rt.state.now),
        Screen::Splash { spinner: true }
    );

    release.send(()).unwrap();
    land(&mut rt, |s| s.agents_load.is_settled());
    assert!(
        matches!(rt.state.screen(rt.state.now), Screen::Splash { .. }),
        "the agent list alone doesn't end the splash"
    );
    note(&rt);

    release.send(()).unwrap();
    land(&mut rt, |s| s.sessions_load.is_settled());
    note(&rt);

    at(&mut rt, 0.3);
    let (screen, sidebar, pane) = views(&rt);
    assert_eq!(
        screen,
        Screen::Chat,
        "the splash gives way to the real layout"
    );
    assert_eq!(
        sidebar,
        SidebarView::Sessions,
        "the sidebar is already the real list"
    );
    assert_eq!(
        pane,
        PaneView::Skeleton,
        "while the chat section still loads"
    );
    assert_eq!(rt.state.sessions.len(), 2);
    assert_eq!(rt.state.sessions[0].title, "First");

    release.send(()).unwrap();
    land(&mut rt, |s| s.sessions[0].history.is_settled());
    at(&mut rt, 0.4);
    assert_eq!(rt.state.pane_view(rt.state.now), PaneView::History);
    assert_eq!(rt.state.sessions[0].messages.len(), 2);

    assert!(
        trace.iter().all(|(screen, _, pane)| {
            !matches!(screen, Screen::Picker | Screen::Chat) || *pane != PaneView::Empty
        }),
        "never the picker, never starter prompts: {trace:?}"
    );
}

#[test]
fn a_launch_without_a_remembered_agent_goes_from_the_splash_to_the_picker() {
    let (mut rt, release) = launched(vec![vec![]], None);
    rt.frame(0.0);
    assert!(matches!(
        rt.state.screen(rt.state.now),
        Screen::Splash { .. }
    ));
    release.send(()).unwrap();
    land(&mut rt, |s| s.agents_load.is_settled());
    at(&mut rt, 0.0);
    assert_eq!(rt.state.screen(rt.state.now), Screen::Picker);
}

#[test]
fn a_remembered_agent_the_directory_no_longer_lists_leads_to_the_picker() {
    let (mut rt, release) = launched(vec![vec![]], Some("gone"));
    rt.frame(0.0);
    release.send(()).unwrap();
    land(&mut rt, |s| s.agents_load.is_settled());
    at(&mut rt, 0.0);
    assert_eq!(rt.state.screen(rt.state.now), Screen::Picker);
}

#[test]
fn a_failed_agent_list_shows_the_error_screen() {
    let mut chat = test_chat();
    chat.update(|s, _| {
        s.agents_load = Load::Failed("HTTP 403".into());
    });
    assert_eq!(
        chat.state.screen(0.0),
        Screen::AgentsFailed("HTTP 403".into())
    );
}

#[test]
fn fast_loads_never_show_a_placeholder() {
    let (mut rt, release) = launched(vec![vec![info("c1", "First")]], Some("a1"));
    let mut trace: Vec<Views> = Vec::new();
    rt.frame(0.0);
    trace.push(views(&rt));
    for step in [
        (|s: &Chat| s.agents_load.is_settled()) as fn(&Chat) -> bool,
        |s: &Chat| s.sessions_load.is_settled(),
        |s: &Chat| s.sessions[0].history.is_settled(),
    ] {
        release.send(()).unwrap();
        land(&mut rt, step);
        at(&mut rt, 0.02);
        trace.push(views(&rt));
    }
    at(&mut rt, 0.5);
    let last = views(&rt);
    assert_eq!(
        last,
        (Screen::Chat, SidebarView::Sessions, PaneView::History)
    );
    assert!(
        trace.iter().all(|(screen, sidebar, pane)| {
            *screen != Screen::Splash { spinner: true }
                && *sidebar != SidebarView::Skeleton
                && *pane != PaneView::Skeleton
        }),
        "no spinner or skeleton under 150ms: {trace:?}"
    );
}

#[test]
fn switching_agents_blanks_then_skeletons_both_areas_and_never_shows_a_new_session() {
    let (mut rt, release) = launched(vec![vec![info("c1", "First")], vec![]], Some("a1"));
    rt.frame(0.0);
    for step in [
        (|s: &Chat| s.agents_load.is_settled()) as fn(&Chat) -> bool,
        |s: &Chat| s.sessions_load.is_settled(),
        |s: &Chat| s.sessions[0].history.is_settled(),
    ] {
        release.send(()).unwrap();
        land(&mut rt, step);
    }
    at(&mut rt, 2.0);
    assert_eq!(
        views(&rt),
        (Screen::Chat, SidebarView::Sessions, PaneView::History)
    );

    rt.update(|s, _| s.choose_agent(1));
    rt.frame(0.0);
    let (screen, sidebar, pane) = views(&rt);
    assert_eq!(screen, Screen::Chat, "no splash after launch");
    assert_eq!(
        sidebar,
        SidebarView::Blank,
        "no stale list, no fake new session"
    );
    assert_eq!(
        pane,
        PaneView::Blank,
        "no starter prompts while the list loads"
    );

    at(&mut rt, 0.2);
    let (_, sidebar, pane) = views(&rt);
    assert_eq!((sidebar, pane), (SidebarView::Skeleton, PaneView::Skeleton));

    release.send(()).unwrap();
    land(&mut rt, |s| s.sessions_load.is_settled());
    at(&mut rt, 0.5);
    let (_, sidebar, pane) = views(&rt);
    assert_eq!(sidebar, SidebarView::Sessions);
    assert_eq!(
        pane,
        PaneView::Empty,
        "an agent with no conversations opens a new session"
    );
}

#[test]
fn selecting_a_past_conversation_skeletons_only_the_chat_section() {
    let (mut rt, release) = launched(
        vec![vec![info("c1", "First"), info("c2", "Second")]],
        Some("a1"),
    );
    rt.frame(0.0);
    for step in [
        (|s: &Chat| s.agents_load.is_settled()) as fn(&Chat) -> bool,
        |s: &Chat| s.sessions_load.is_settled(),
        |s: &Chat| s.sessions[0].history.is_settled(),
    ] {
        release.send(()).unwrap();
        land(&mut rt, step);
    }
    at(&mut rt, 2.0);

    rt.update(|s, _| s.select_session(1));
    rt.frame(0.0);
    let (_, sidebar, pane) = views(&rt);
    assert_eq!(sidebar, SidebarView::Sessions, "the sidebar stays put");
    assert_eq!(
        pane,
        PaneView::Blank,
        "never starter prompts for a past conversation"
    );
    assert_eq!(rt.state.current, 1);

    at(&mut rt, 0.2);
    assert_eq!(rt.state.pane_view(rt.state.now), PaneView::Skeleton);
}

#[test]
fn a_load_for_an_agent_that_was_switched_away_from_is_dropped() {
    let (mut rt, release) = launched(
        vec![
            vec![info("old", "From the first agent")],
            vec![info("new", "From the second")],
        ],
        None,
    );
    rt.frame(0.0);
    release.send(()).unwrap();
    land(&mut rt, |s| s.agents_load.is_settled());

    rt.update(|s, _| s.choose_agent(0));
    rt.update(|s, _| s.choose_agent(1));
    release.send(()).unwrap();
    release.send(()).unwrap();
    land(&mut rt, |s| s.sessions_load.is_settled());
    for _ in 0..20 {
        rt.frame(0.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(rt.state.sessions.len(), 1);
    assert_eq!(rt.state.sessions[0].title, "From the second");
}

#[test]
fn a_failed_conversation_load_marks_the_conversation_it_belongs_to() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.sessions = ["c1", "c2"]
            .iter()
            .map(|id| Session {
                id: Some((*id).into()),
                hidden: false,
                title: (*id).into(),
                messages: Vec::new(),
                agent_message_id: None,
                history: Load::Idle,
            })
            .collect();
        s.current = 0;
        s.apply(StreamEvent::ConversationFailed {
            session_id: "c2".into(),
            error: "HTTP 500".into(),
        });
    });
    assert_eq!(rt.state.sessions[1].history.error(), Some("HTTP 500"));
    assert!(
        rt.state.sessions[0].history.is_idle(),
        "the current one is untouched"
    );
}

#[test]
fn a_failed_conversation_list_shows_in_the_sidebar_and_opens_a_new_session() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.sessions_load = Load::Failed("HTTP 500".into());
    });
    assert_eq!(
        rt.state.sidebar_view(0.0),
        SidebarView::Failed("HTTP 500".into())
    );
    assert_eq!(rt.state.pane_view(0.0), PaneView::Empty);
}

#[test]
fn a_failed_conversation_shows_inline_in_the_chat_section() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.sessions = vec![Session {
            id: Some("c1".into()),
            hidden: false,
            title: "c1".into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Failed("HTTP 500".into()),
        }];
    });
    assert_eq!(rt.state.pane_view(0.0), PaneView::Failed("HTTP 500".into()));
}
