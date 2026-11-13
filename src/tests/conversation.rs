use super::*;

#[test]
fn a_turn_streams_into_the_transcript_and_settles() {
    let mut rt = test_chat();
    let sender = rt.state.begin_turn("Hello agent");
    sender.send(StreamEvent::TextDelta("He".into())).unwrap();
    rt.frame(1.0 / 60.0);
    let session = &rt.state.sessions[0];
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[1].text, "He");
    assert!(session.messages[1].streaming);
    assert!(rt.state.pending);

    sender.send(StreamEvent::TextDelta("llo".into())).unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    let session = &rt.state.sessions[0];
    assert_eq!(session.messages[1].text, "Hello");
    assert!(!session.messages[1].streaming);
    assert!(!rt.state.pending);
    assert!(rt.state.error.is_none());
}

#[test]
fn message_actions_wait_until_the_reply_has_finished_streaming() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    let sender = rt.state.begin_turn("Hello agent");
    sender.send(StreamEvent::TextDelta("Hel".into())).unwrap();
    rt.frame(1.0 / 60.0);
    let actions = format!("actions-{}", rt.state.sessions[0].messages[1].id);
    assert!(
        rt.find(&actions).is_none(),
        "no copy, retry, votes or timing while the reply streams"
    );

    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(2.0 / 60.0);
    assert!(
        rt.find(&actions).is_some(),
        "the settled reply has its actions"
    );
}

#[test]
fn a_dropped_sender_fails_the_turn() {
    let mut rt = test_chat();
    let sender = rt.state.begin_turn("Hello agent");
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.pending);
    assert!(rt.state.error.is_some());
}

#[test]
fn the_first_message_names_the_session() {
    let mut rt = test_chat();
    let sender = rt.state.begin_turn("What is Agent Studio?");
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.sessions[0].title, "What is Agent Studio?");
}

#[test]
fn the_composer_and_send_button_render_and_submit_clears() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("composer").is_some(), "the composer is on screen");
    assert!(rt.find("send").is_some(), "the send button is on screen");
    rt.state.composer.insert("A question");
    let sender = rt.state.begin_turn("A question");
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.sessions[0].messages[0].text, "A question");
}

#[test]
fn new_and_select_sessions() {
    let mut rt = test_chat();
    rt.state.composer.insert("First");
    let sender = rt.state.begin_turn("First");
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    rt.state.new_session();
    assert_eq!(rt.state.current, 1);
    assert!(rt.state.sessions[1].messages.is_empty());
    rt.state.select_session(0);
    assert_eq!(rt.state.sessions[0].messages.len(), 2);
}

#[test]
fn the_directory_loads_into_the_picker_and_drafts_cannot_be_chosen() {
    let mut rt = test_chat();
    rt.frame(0.0);
    assert!(!rt.state.agents_load.is_settled());
    assert!(rt.state.selected_agent.is_none());

    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![
            AgentInfo {
                agent_id: "a1".into(),
                name: "Shopping".into(),
                description: Some("Catalog answers".into()),
                status: AgentStatus::Published,
                model: Some("gpt-5".into()),
            },
            AgentInfo {
                agent_id: "a2".into(),
                name: "Draft".into(),
                description: None,
                status: AgentStatus::Draft,
                model: None,
            },
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.agents_load.is_settled());
    assert!(rt.state.agents_load.error().is_none());
    assert_eq!(rt.state.agents.len(), 2);

    rt.state.choose_agent(1);
    assert!(
        rt.state.selected_agent.is_none(),
        "a draft cannot be chosen"
    );

    rt.state.choose_agent(0);
    let selected = rt.state.selected_agent.as_ref().unwrap();
    assert_eq!(selected.agent_id, "a1");
}

#[test]
fn the_picker_and_the_switcher_render_the_loaded_agents() {
    let mut rt = test_chat();
    rt.frame(0.0);
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![
            AgentInfo {
                agent_id: "a1".into(),
                name: "Shopping".into(),
                description: Some("Catalog answers".into()),
                status: AgentStatus::Published,
                model: Some("gpt-5".into()),
            },
            AgentInfo {
                agent_id: "a2".into(),
                name: "Draft".into(),
                description: None,
                status: AgentStatus::Draft,
                model: None,
            },
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    assert!(
        rt.find("agent-a1").is_some(),
        "the published agent's row is on screen"
    );
    assert!(
        rt.find("agent-a2").is_some(),
        "the draft's row is on screen too"
    );

    rt.update(|s, _| s.choose_agent(0));
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("agent-title").is_some(),
        "the agent's name is in the header"
    );
    assert!(rt.find("composer").is_some(), "and so is the chat");
}

#[test]
fn choosing_an_agent_resets_sessions_and_the_next_turn_uses_it() {
    let mut rt = test_chat();
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![
            agent("a1", "One", AgentStatus::Published),
            agent("a2", "Two", AgentStatus::Published),
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    rt.state.choose_agent(0);
    let turn = rt.state.begin_turn("First");
    turn.send(StreamEvent::Finished).unwrap();
    drop(turn);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.sessions[0].messages.len(), 2);

    rt.state.choose_agent(1);
    assert_eq!(rt.state.selected_agent.as_ref().unwrap().agent_id, "a2");
    assert_eq!(
        rt.state.sessions.len(),
        1,
        "the new agent starts a fresh session"
    );
    assert!(rt.state.sessions[0].messages.is_empty());
    assert!(!rt.state.pending);
}

#[test]
fn a_failed_directory_shows_an_error_and_a_reload_clears_it() {
    let mut rt = test_chat();
    rt.frame(0.0);
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsFailed("HTTP 403".into()))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.agents_load.is_settled());
    assert_eq!(rt.state.agents_load.error(), Some("HTTP 403"));
    assert!(rt.state.agents.is_empty());

    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![agent_one()]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert!(
        rt.state.agents_load.error().is_none(),
        "a successful reload clears the error"
    );
}

#[test]
fn cmd_b_toggles_the_sidebar() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    assert!(rt.state.sidebar.is_open());

    rt.key(KeyEvent {
        key: Key::Command('b'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(!rt.state.sidebar.is_open());

    rt.key(KeyEvent {
        key: Key::Command('b'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(rt.state.sidebar.is_open());

    // Shift-⌘B is the shell's other default and stays free for the app to bind later.
    rt.key(KeyEvent {
        key: Key::Command('b'),
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    rt.frame(0.0);
    assert!(rt.state.sidebar.is_open());
}

#[test]
fn cmd_comma_opens_and_closes_the_settings_dialog() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    assert!(
        rt.find("settings-dialog").is_none(),
        "the dialog starts closed"
    );

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(rt.find("settings-dialog").is_some(), "⌘, opens the dialog");

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(rt.find("settings-dialog").is_none(), "⌘, again closes it");
}

#[test]
fn the_settings_dialog_switches_the_chat_width() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.appearance.width, ChatWidth::Comfortable);

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(rt.find("settings-dialog").is_some());
    assert!(rt.find("chat-width-trigger").is_some());

    tap(&mut rt, "chat-width-trigger");
    settle(&mut rt);
    tap(&mut rt, "option1");
    assert_eq!(
        rt.state.appearance.width,
        ChatWidth::Wide,
        "choosing Wide takes effect"
    );

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(
        rt.find("settings-dialog").is_none(),
        "⌘, closes the dialog again"
    );
    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(
        !rt.state.width_select_open,
        "the dropdown starts closed when the dialog reopens"
    );
    tap(&mut rt, "chat-width-trigger");
    settle(&mut rt);
    tap(&mut rt, "option0");
    assert_eq!(
        rt.state.appearance.width,
        ChatWidth::Comfortable,
        "the choice is still live after the dialog reopens"
    );
}

#[test]
fn the_settings_dialog_changes_theme_and_accent() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(rt.find("settings-dialog").is_some());
    assert!(rt.find("theme-segments").is_some());
    assert!(rt.find("accent-blue").is_some());
    assert!(rt.find("accent-purple").is_some());

    tap(&mut rt, "accent-purple");
    assert_eq!(
        rt.state.appearance.accent,
        Accent::Purple,
        "the swatch dials the accent"
    );

    tap(&mut rt, "segment2");
    assert_eq!(
        rt.state.appearance.theme,
        ThemePref::Dark,
        "the segment dials the theme"
    );
    rt.frame(1.0 / 60.0);

    // The whole app re-renders in the forced appearance, accent and all.
    rt.update(|s, _| s.appearance.theme = ThemePref::Light);
    rt.frame(1.0 / 60.0);
    assert!(rt.find("composer").is_some(), "light mode renders the chat");
}

#[test]
fn a_restored_appearance_applies_without_a_directory() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.restore(&Persisted {
            theme: Some(ThemePref::Dark),
            accent: Some(Accent::Purple),
            width: Some(ChatWidth::Wide),
            ..Persisted::default()
        })
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.appearance.theme, ThemePref::Dark);
    assert_eq!(rt.state.appearance.accent, Accent::Purple);
    assert_eq!(rt.state.appearance.width, ChatWidth::Wide);
}

#[test]
fn the_welcome_dialog_saves_credentials_and_lists_agents() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.applications.clear();
        s.active = None;
        s.mock_forced = false;
    });
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("credentials-dialog").is_some(),
        "first run asks for credentials"
    );
    assert!(rt.find("credentials-save").is_some());

    // Blank fields can't save.
    tap(&mut rt, "credentials-save");
    assert!(
        rt.state.credentials().is_none(),
        "blank credentials don't save"
    );

    rt.update(|s, _| {
        s.add_form.app_id.insert("ABCD123456");
        s.add_form.api_key.insert("search-key");
    });
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| {
        s.connector = offline_connector(vec![agent_one()]);
        s.submit_credentials();
    });
    // Connecting would reach for the real API; the test stays offline, and `poll` lists the
    // mock directory the frame after saving.
    rt.frame(1.0 / 60.0);

    assert_eq!(
        rt.state.credentials().map(|(a, _)| a),
        Some("ABCD123456"),
        "the saved credentials are remembered"
    );
    assert!(
        rt.find("credentials-dialog").is_none(),
        "saving closes the dialog"
    );

    // `poll` picks the load up the frame after saving; the mock directory answers.
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        if rt.state.agents_load.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        rt.state.agents_load.is_settled() && rt.state.agents.iter().any(|a| a.agent_id == "a1"),
        "the directory lists with the saved credentials"
    );
}

#[test]
fn the_welcome_dialogs_mock_button_skips_credentials_for_the_run() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.applications.clear();
        s.active = None;
        s.mock_forced = false;
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("credentials-dialog").is_some());

    settle(&mut rt);
    tap(&mut rt, "credentials-mock");
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("credentials-dialog").is_none(),
        "mock mode closes the dialog"
    );
    assert!(rt.state.mock_forced);
    for _ in 0..200 {
        if rt.find("agent-picker").is_some() {
            break;
        }
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        rt.find("agent-picker").is_some(),
        "the picker shows once the mock agents are listed"
    );
}

#[test]
fn the_collapsed_sidebar_leaves_a_rail_of_session_icons() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.new_session();
    });
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("sidebar-rail").is_none(),
        "the open sidebar needs no rail"
    );

    rt.update(|s, _| s.sidebar.toggle());
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("sidebar-rail").is_some(),
        "the closed sidebar a rail"
    );
    assert!(
        rt.find("rail-session-0").is_some() && rt.find("rail-session-1").is_some(),
        "each session is an icon"
    );
    assert!(
        rt.find("rail-settings").is_some(),
        "settings stays reachable from the rail"
    );

    // A rail icon selects its session.
    settle(&mut rt);
    tap(&mut rt, "rail-session-1");
    assert_eq!(rt.state.current, 1, "the rail icon selects the session");
}

#[test]
fn the_sidebar_footer_opens_the_settings_dialog() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("settings-footer").is_some());

    settle(&mut rt);
    tap(&mut rt, "settings-footer");
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("settings-dialog").is_some(),
        "the footer opens the settings dialog"
    );
}

#[test]
fn the_selected_agents_conversations_load_into_the_sidebar() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.mock_forced = false;
        s.selected_agent = Some(agent_one());
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: vec![
                SessionInfo {
                    id: "c1".into(),
                    title: Some("Past chat".into()),
                },
                SessionInfo {
                    id: "c2".into(),
                    title: None,
                },
            ],
            conversation: vec![ChatMessage::user("m1", "Hello"), {
                let mut m = ChatMessage::assistant("m2");
                m.text = "Hi there".into();
                m
            }],
        });
    });
    // `poll` chains the loads: the agent list, then the conversations, then the selected
    // conversation's messages.
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        if rt.state.sessions[0].history.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        rt.state.sessions.len(),
        2,
        "the sidebar lists the API sessions"
    );
    assert_eq!(rt.state.sessions[0].title, "Past chat");
    assert_eq!(
        rt.state.sessions[1].title, "Untitled conversation",
        "a null title gets a placeholder"
    );
    assert_eq!(rt.state.sessions[0].messages.len(), 2);
    assert_eq!(rt.state.sessions[0].messages[0].text, "Hello");

    // Selecting the other session fetches its messages too.
    rt.update(|s, _| s.select_session(1));
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        if rt.state.sessions[1].history.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(rt.state.sessions[1].history.is_settled());
    assert_eq!(rt.state.sessions[1].messages.len(), 2);
    assert_eq!(rt.state.current, 1);
}

#[test]
fn a_conversation_id_from_the_server_binds_the_local_session() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Hello");
    sender
        .send(StreamEvent::ConversationStarted("alg_cnv_1".into()))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state.sessions[0].id.as_deref(),
        Some("alg_cnv_1"),
        "the fresh session binds to the server's conversation"
    );

    // The next send continues it instead of starting a new conversation.
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| {
        s.composer.clear();
        s.composer.insert("Follow-up question");
    });
    rt.update(|s, _| s.send());
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state.sessions[0].id.as_deref(),
        Some("alg_cnv_1"),
        "the follow-up stays in the same conversation"
    );
}

/// A chat on a real application (so conversations load from the directory), the agent's lists
/// already in, and nothing persisted server-side yet: a brand new empty session.
fn new_session_on_an_application() -> Runtime<Chat> {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.mock_forced = false;
        s.selected_agent = Some(agent_one());
        s.agents_load = Load::Idle.finished(0.0);
        s.sessions_load = Load::Idle.finished(0.0);
    });
    rt.frame(0.0);
    rt
}

fn run_frames(rt: &mut Runtime<Chat>, n: usize) {
    for _ in 0..n {
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
}

#[test]
fn sending_into_a_new_conversation_keeps_the_exchange_on_screen() {
    let mut rt = new_session_on_an_application();
    let sender = rt.state.begin_turn("Hello");
    sender
        .send(StreamEvent::ConversationStarted("alg_cnv_new".into()))
        .unwrap();
    sender
        .send(StreamEvent::TextDelta("Hi there".into()))
        .unwrap();
    run_frames(&mut rt, 60);

    assert_eq!(
        rt.state.pane_view(rt.state.now),
        crate::loading::PaneView::History,
        "the pane shows the exchange, not a blank"
    );
    assert!(
        shows_text(&rt, "Hello", "messages"),
        "the question is there"
    );
    assert!(
        shows_text(&rt, "Hi there", "messages"),
        "and the streaming reply"
    );
}

#[test]
fn a_conversation_this_session_just_started_is_not_refetched_over_its_messages() {
    let mut rt = new_session_on_an_application();
    let sender = rt.state.begin_turn("Hello");
    sender
        .send(StreamEvent::ConversationStarted("alg_cnv_new".into()))
        .unwrap();
    sender
        .send(StreamEvent::TextDelta("Hi there".into()))
        .unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    run_frames(&mut rt, 60);

    let messages = &rt.state.sessions[0].messages;
    assert_eq!(
        messages.len(),
        2,
        "the server had nothing persisted yet; ours stay"
    );
    assert_eq!(messages[1].text, "Hi there");
}

#[test]
fn retry_reruns_the_turn_against_the_same_history() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("What is 2 + 2?");
    sender.send(StreamEvent::TextDelta("5".into())).unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    let assistant_id = rt.state.sessions[0].messages[1].id.clone();

    rt.update(|s, _| s.retry(&assistant_id));
    let session = &rt.state.sessions[0];
    assert_eq!(session.messages.len(), 2, "the old answer is dropped");
    assert_eq!(session.messages[0].text, "What is 2 + 2?");
    assert_eq!(session.messages[0].role, Role::User);
    assert_ne!(
        session.messages[1].id, assistant_id,
        "a fresh placeholder streams"
    );
    assert!(rt.state.pending);

    for _ in 0..400 {
        rt.frame(1.0);
        if !rt.state.pending {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!rt.state.pending, "the mock answers and the turn settles");
    assert_eq!(rt.state.sessions[0].messages[1].text, "Hi!");
}

#[test]
fn message_actions_arrive_with_a_streamed_reply() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Hello");
    // The server's Start event names the id the reply persists under.
    sender
        .send(StreamEvent::Start {
            message_id: "alg_msg_9".into(),
        })
        .unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(m.server_id.as_deref(), Some("alg_msg_9"));
    let message_id = m.id.clone();

    // Copy, retry and both votes render for the settled reply.
    assert!(rt.find(&format!("copy-{message_id}")).is_some());
    assert!(rt.find(&format!("retry-{message_id}")).is_some());
    assert!(rt.find(&format!("up-{message_id}")).is_some());
    assert!(rt.find(&format!("down-{message_id}")).is_some());

    // Voting records itself on the message.
    rt.update(|s, _| s.vote(&message_id, true));
    assert_eq!(rt.state.sessions[0].messages[1].vote, Some(1));
}

#[test]
fn the_empty_state_introduces_the_agent_with_starter_prompts() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(AgentInfo {
            description: Some("Answers questions about the catalog".into()),
            ..agent_one()
        })
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("empty-state").is_some());
    assert!(rt.find("starter-0").is_some());

    // A starter prompt fills the composer to edit and send.
    settle(&mut rt);
    tap(&mut rt, "starter-0");
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.composer.text, "What can you help me with?");
}

#[test]
fn the_sidebar_shows_five_recent_conversations_until_expanded() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.sessions = (0..8)
            .map(|i| Session {
                id: None,
                hidden: false,
                title: format!("Conversation {i}"),
                messages: Vec::new(),
                agent_message_id: None,
                history: Load::Idle,
            })
            .collect();
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("session-4").is_some(), "the fifth is visible");
    assert!(
        rt.find("session-7").is_none(),
        "older conversations hide behind the toggle"
    );
    assert!(rt.find("sessions-toggle").is_some());

    // Expanding lists everything.
    settle(&mut rt);
    tap(&mut rt, "sessions-toggle");
    rt.frame(1.0 / 60.0);
    assert!(rt.find("session-7").is_some(), "expanded shows them all");

    // Collapsing again keeps the session in use visible, even when it's not recent.
    rt.update(|s, _| s.select_session(7));
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| s.show_all_sessions = false);
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("session-7").is_some(),
        "the session in use stays visible when collapsed"
    );
}

#[test]
fn a_streaming_reply_reveals_word_by_word_and_settles() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Hello");
    // Words keep arriving, so some frame catches one mid-fade even on a loaded machine.
    let mut revealed = false;
    for i in 0..10 {
        sender
            .send(StreamEvent::TextDelta(format!("word{i} ")))
            .unwrap();
        rt.frame(1.0 / 60.0);
        if rt.elements().into_iter().any(|e| e.revealing) {
            revealed = true;
            break;
        }
    }
    assert!(revealed, "the streaming reply's words fade in");

    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        !rt.elements().into_iter().any(|e| e.revealing),
        "the settled reply stops revealing"
    );
}

#[test]
fn the_composer_overlays_the_transcript_as_translucent_frost() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);

    let composer = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("composer"))
        .expect("the composer is on screen");

    // The transcript runs under the composer: its rect reaches behind the card's top.
    let messages = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("messages"))
        .expect("the transcript is on screen");
    assert!(
        messages.rect[1] + messages.rect[3] > composer.rect[1],
        "the transcript scrolls behind the composer card"
    );

    // The card is translucent, so the blurred transcript shows through the frost.
    assert!(
        composer.fill.a < 1.0,
        "the composer's fill is translucent, not opaque"
    );

    // The last message clears the composer: scrolled to the bottom, the thread's bottom
    // edge sits above the card's top edge.
    let sender = rt.state.begin_turn("Hello");
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    let thread = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("thread"))
        .expect("the thread is on screen");
    assert!(
        thread.rect[1] + thread.rect[3] <= composer.rect[1] + 1.0,
        "the thread's bottom ({}) must clear the composer's top ({})",
        thread.rect[1] + thread.rect[3],
        composer.rect[1]
    );
}

#[test]
fn a_new_message_in_a_loaded_conversation_gets_an_id_that_does_not_collide() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.mock_forced = false;
        s.selected_agent = Some(agent_one());
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: vec![SessionInfo {
                id: "c1".into(),
                title: Some("Loaded".into()),
            }],
            // The stored conversation's user message id is "m1" — the id a restarted
            // app's `fresh_id` would hand out first.
            conversation: vec![ChatMessage::user("m1", "loaded question")],
        });
    });
    // `poll` chains the loads: agents, conversations, then the conversation's messages.
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        if rt.state.sessions[0].history.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(rt.state.sessions[0].history.is_settled());

    rt.update(|s, _| s.composer.insert("A follow-up"));
    rt.update(|s, _| s.send());
    rt.frame(1.0 / 60.0);

    let ids: Vec<&str> = rt.state.sessions[0]
        .messages
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        ids.len(),
        unique.len(),
        "message ids must be unique in a conversation: {ids:?}"
    );
}

#[test]
fn a_long_chat_scrolls_clear_of_the_composer() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    rt.update(|s, _| {
        for i in 0..30 {
            s.sessions[0]
                .messages
                .push(ChatMessage::user(format!("u{i}"), format!("question {i}")));
            let mut a = ChatMessage::assistant(format!("a{i}"));
            a.text = format!("answer {i} — a fairly long line of text to fill the transcript");
            a.streaming = false;
            s.sessions[0].messages.push(a);
        }
    });
    // stick_bottom scrolls to the newest message.
    rt.frame(1.0 / 60.0);

    let composer = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("composer"))
        .expect("the composer is on screen");
    let last = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("m-a29"))
        .expect("the last message is on screen");
    assert!(
        last.rect[1] + last.rect[3] <= composer.rect[1] + 1.0,
        "the last message's bottom ({}) hides behind the composer's top ({})",
        last.rect[1] + last.rect[3],
        composer.rect[1]
    );
}

#[test]
fn follow_ups_are_full_width_rows_under_a_heading_with_roomy_actions() {
    let mut rt = test_chat();
    rt.update(|s, _| crate::scenes::tools(s));
    settle(&mut rt);
    let rect = |key: &str| rt.presented(key).map(|(r, _)| r).expect(key);
    let list = rect("suggestions-a1");
    let row = rect("suggestion-0");
    let reply = rect("m-a1");
    assert!(
        (list[2] - reply[2]).abs() < 0.5 && row[2] >= list[2] - 2.5,
        "the follow-ups span the reply's width: {list:?} {row:?} {reply:?}"
    );
    assert!(
        row[3] >= 40.0,
        "a follow-up is a 40 point row, not a small chip: {}",
        row[3]
    );
    assert!(list[3] >= 3.0 * 40.0, "the follow-ups stack");
    assert!(
        shows_text(&rt, "Follow up", "follow-ups-a1"),
        "the list is labelled"
    );
    let copy = rect("copy-a1");
    assert!(
        copy[3] >= 24.0,
        "the action buttons are 24 points: {}",
        copy[3]
    );
}

#[test]
fn suggestions_after_a_reply_become_quick_actions_that_send() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Hello");
    sender
        .send(StreamEvent::Start {
            message_id: "alg_msg_1".into(),
        })
        .unwrap();
    sender
        .send(StreamEvent::TextDelta("Here you go".into()))
        .unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    sender
        .send(StreamEvent::Suggestions(vec![
            "Show me the best insoles for wide feet".into(),
            "Explain how sizing works".into(),
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(
        m.suggestions.as_deref(),
        Some(
            &[
                "Show me the best insoles for wide feet".to_string(),
                "Explain how sizing works".to_string(),
            ][..]
        )
    );
    assert!(rt.find("suggestion-0").is_some(), "the chips render");
    assert!(rt.find("suggestion-1").is_some());

    // Tapping a chip sends that prompt as a new message and clears the suggestions.
    settle(&mut rt);
    tap(&mut rt, "suggestion-0");
    rt.frame(1.0 / 60.0);
    let session = &rt.state.sessions[0];
    assert_eq!(session.messages.len(), 4, "user + placeholder appended");
    assert_eq!(session.messages[2].role, Role::User);
    assert_eq!(
        session.messages[2].text,
        "Show me the best insoles for wide feet"
    );
    assert!(
        session.messages.iter().all(|m| m.suggestions.is_none()),
        "stale suggestions clear once a new turn starts"
    );
    assert_eq!(rt.state.composer.text, "", "the composer stays untouched");
}

#[test]
fn cmd_n_starts_a_new_session() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    rt.state.composer.insert("Half-typed draft");
    let sender = rt.state.begin_turn("First");
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.sessions.len(), 1);

    rt.key(KeyEvent {
        key: Key::Command('n'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert_eq!(rt.state.sessions.len(), 2, "⌘N starts a new session");
    assert_eq!(rt.state.current, 1);
    assert!(
        rt.state.sessions[1].messages.is_empty(),
        "the new session starts empty"
    );
    assert_eq!(
        rt.state.composer.text, "",
        "the draft clears with the new session"
    );

    // Shift-⌘N stays free for the app to bind later.
    rt.key(KeyEvent {
        key: Key::Command('n'),
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    rt.frame(0.0);
    assert_eq!(rt.state.sessions.len(), 2);
}

#[test]
fn a_quick_ask_reply_renders_its_markdown() {
    let rt = quick_ask_with(
        |m| {
            m.streaming = false;
            m.text = "Use **Cushioned**".into();
        },
        false,
    );
    let texts: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|e| e.text.clone())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("**")),
        "no raw markdown: {texts:?}"
    );
    assert!(texts.iter().any(|t| t.contains("Cushioned")));
}

#[test]
fn a_hidden_conversation_stays_hidden_across_a_restart() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.mock_forced = false;
        s.selected_agent = Some(agent_one());
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: vec![
                SessionInfo {
                    id: "c1".into(),
                    title: Some("Kept".into()),
                },
                SessionInfo {
                    id: "c2".into(),
                    title: Some("Gone".into()),
                },
            ],
            conversation: Vec::new(),
        });
        s.hidden_conversations = ["c2".to_string()].into_iter().collect();
    });
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        if rt.state.sessions_load.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        rt.state.sessions[1].hidden,
        "the hidden id marks its session"
    );
    assert!(!rt.state.sessions[0].hidden);
    assert!(
        rt.find("session-1").is_none() && rt.find("session-0").is_some(),
        "the sidebar shows only the kept conversation"
    );
}

#[test]
fn the_collapsed_rail_shows_only_the_five_recent_conversations() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.sessions = (0..8)
            .map(|i| Session {
                id: None,
                hidden: false,
                title: format!("Conversation {i}"),
                messages: Vec::new(),
                agent_message_id: None,
                history: Load::Idle,
            })
            .collect();
        s.select_session(7);
    });
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| s.sidebar.toggle());
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("rail-session-4").is_some(),
        "the fifth is on the rail"
    );
    assert!(
        rt.find("rail-session-5").is_none() && rt.find("rail-session-6").is_none(),
        "older conversations stay off the rail"
    );
    assert!(
        rt.find("rail-session-7").is_some(),
        "the session in use stays on the rail"
    );
}

#[test]
fn untouched_knobs_keep_the_shipped_design_values() {
    let rt = test_chat();
    let s = &rt.state;
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let (fraction, min, max) = s.width_spec();
    assert!(
        near(fraction, 0.6) && min == 500.0 && max == 860.0,
        "comfortable width"
    );
    assert_eq!(s.results_shown(), 3);
    assert_eq!(s.frost_blur(), 24.0);
    assert!(near(s.frost_alpha(), 0.72), "the shipped frost opacity");
    assert!(near(s.reveal_seconds(), 0.18), "the shipped word fade");

    let mut wide = test_chat();
    wide.update(|s, _| s.appearance.width = ChatWidth::Wide);
    let (fraction, min, max) = wide.state.width_spec();
    assert!(
        near(fraction, 0.8) && min == 620.0 && max == 1160.0,
        "wide width"
    );
}

#[test]
fn tool_events_build_a_collapsible_group_with_json() {
    let mut rt = test_chat();
    rt.frame(0.0);
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![agent_one()]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    rt.state.choose_agent(0);

    let sender = rt.state.begin_turn("Schedule something");
    sender
        .send(StreamEvent::ToolUsed(ToolCall {
            name: "schedule_task_tool".into(),
            input: Some("{\n  \"cron\": \"0 9 * * 1-5\"\n}".into()),
            open: false,
            request: None,
            error: None,
        }))
        .unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(m.tools.len(), 1);
    let message_id = m.id.clone();
    assert!(
        rt.find("tool-schedule_task_tool").is_some(),
        "each call shows as its own card, with no group to open first"
    );
    assert!(rt.find("tools-header").is_none(), "no Used N tools header");
    assert!(
        rt.find("tool-json-schedule_task_tool").is_none(),
        "the input JSON is hidden while the row is collapsed"
    );

    // The press path anchors the row: it stays where it shows while the content below opens.
    rt.update(|s, tx| {
        s.toggle_tool_open(&message_id, 0);
        tx.keep_on_screen("tool-schedule_task_tool");
    });
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("tool-json-schedule_task_tool").is_some(),
        "expanding the row shows its JSON"
    );
}

#[test]
fn the_sidebar_chips_sessions_without_a_directory() {
    let mut rt = test_chat();
    rt.update(|s, _| s.new_session());
    rt.frame(1.0 / 60.0);

    assert!(
        rt.find("cwd").is_none(),
        "the working directory is not in the sidebar"
    );
    assert!(
        rt.find("session-chip-0").is_some(),
        "each session row has a letter chip"
    );
    assert!(rt.find("session-chip-1").is_some());
}

#[test]
fn a_single_agent_shows_no_switcher() {
    let mut rt = test_chat();
    rt.frame(0.0);
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![agent_one()]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| s.choose_agent(0));
    rt.frame(1.0 / 60.0);

    let role = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("agent-title"))
        .map(|e| e.role);
    assert!(role.is_some(), "the agent's name is centred in the header");
    assert_ne!(role, Some("button"), "one agent needs no switcher");
    assert!(
        rt.find("session-header").is_some(),
        "the session title leads with its chip"
    );
    assert!(
        rt.find("composer").is_some(),
        "the chat shows behind the title"
    );
}

#[test]
fn a_new_session_mid_stream_cancels_the_turn() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Long question");
    sender
        .send(StreamEvent::TextDelta("partial".into()))
        .unwrap();
    rt.frame(1.0 / 60.0);
    assert!(rt.state.pending);

    rt.state.new_session();
    rt.frame(1.0 / 60.0);

    assert!(!rt.state.pending, "the turn is cancelled");
    assert!(
        rt.state
            .sessions
            .iter()
            .all(|s| s.messages.iter().all(|m| !m.streaming)),
        "no message is left streaming"
    );

    let _ = sender.send(StreamEvent::TextDelta("late".into()));
    rt.frame(1.0 / 60.0);
    assert!(
        rt.state
            .sessions
            .iter()
            .all(|s| s.messages.iter().all(|m| m.text != "partiallate")),
        "late deltas go nowhere"
    );
}

#[test]
fn selecting_another_session_mid_stream_cancels_the_turn() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Long question");
    sender
        .send(StreamEvent::TextDelta("partial".into()))
        .unwrap();
    rt.frame(1.0 / 60.0);
    rt.state.new_session();
    rt.frame(1.0 / 60.0);

    rt.state.select_session(0);
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.pending);
    assert!(
        rt.state.sessions[0]
            .messages
            .iter()
            .all(|m| !m.streaming && m.text != "partiallate"),
        "the visited session stays clean"
    );
}

#[test]
fn choosing_an_agent_mid_stream_cancels_cleanly() {
    let mut rt = test_chat();
    rt.frame(0.0);
    let loader = rt.state.begin_agent_load();
    loader
        .send(StreamEvent::AgentsLoaded(vec![
            agent_one(),
            agent("a2", "Two", AgentStatus::Published),
        ]))
        .unwrap();
    drop(loader);
    rt.frame(1.0 / 60.0);
    rt.state.choose_agent(0);

    let sender = rt.state.begin_turn("Long question");
    sender
        .send(StreamEvent::TextDelta("partial".into()))
        .unwrap();
    rt.frame(1.0 / 60.0);
    assert!(rt.state.pending);

    rt.state.choose_agent(1);
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.pending);
    assert!(
        rt.state.sessions[0].messages.is_empty(),
        "the fresh session has no leaked deltas"
    );
}

#[test]
fn a_finished_turn_records_thought_time() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    let sender = rt.state.begin_turn("Think about it");

    settle(&mut rt);
    assert!(rt.state.pending);
    assert!(rt.state.sessions[0].messages[1].thought.is_none());

    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    let m = &rt.state.sessions[0].messages[1];
    let thought = m.thought.expect("the turn's duration is recorded");
    assert!(
        (thought - 0.5).abs() < 0.05,
        "thought spans the virtual time between the first poll and the finish: {thought}"
    );
    assert!(
        rt.find(&format!("thought-{}", m.id)).is_some(),
        "the thought row renders after the turn settles"
    );
}

#[test]
fn the_playground_opens_a_scene_with_a_variant_on_its_stage() {
    let mut rt = playground_app();
    assert!(
        rt.playground_state::<Chat>().is_none(),
        "the stage state only exists while the playground is open"
    );

    rt.open_playground("tools", Some("Collapsed"));
    let stage = rt
        .playground_state::<Chat>()
        .expect("the scene's state is on the stage");
    assert!(
        stage.sessions[0].messages[1].tools.iter().all(|t| !t.open),
        "the variant collapses every tool card"
    );
    assert!(
        rt.playground_element("hits-search_tool").is_none(),
        "the collapsed card hides its results on the stage"
    );

    rt.open_playground("tools", None);
    let stage = rt.playground_state::<Chat>().unwrap();
    assert_eq!(
        stage.sessions[0].messages.len(),
        2,
        "reopening the scene remakes its fixture state"
    );
    assert!(
        stage.sessions[0].messages[1].tools.iter().all(|t| !t.open),
        "and the picked variant stays applied"
    );
    assert!(
        rt.state.sessions[0].messages.is_empty(),
        "the real state stays untouched throughout"
    );
}

fn two_settled_exchanges() -> Runtime<Chat> {
    let mut rt = test_chat_sized((900.0, 720.0));
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        for i in 0..2 {
            s.sessions[0]
                .messages
                .push(ChatMessage::user(format!("u{i}"), format!("question {i}")));
            let mut a = ChatMessage::assistant(format!("a{i}"));
            a.text = format!("answer {i}");
            a.streaming = false;
            s.sessions[0].messages.push(a);
        }
    });
    settle(&mut rt);
    rt
}

fn opacity(rt: &Runtime<Chat>, key: &str) -> f32 {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
        .opacity
}

#[test]
fn an_earlier_reply_shows_its_actions_on_hover_and_the_latest_always() {
    let mut rt = two_settled_exchanges();
    assert!(
        opacity(&rt, "actions-a0") < 0.01,
        "an earlier reply's actions rest hidden"
    );
    assert!(
        opacity(&rt, "actions-a1") > 0.99,
        "the latest reply's actions stay shown"
    );

    let (x, y) = rt.find("m-a0").unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert!(
        opacity(&rt, "actions-a0") > 0.99,
        "hovering the reply shows them"
    );
}

#[test]
fn keyboard_focus_on_an_earlier_reply_shows_its_actions() {
    let mut rt = two_settled_exchanges();
    assert!(opacity(&rt, "actions-a0") < 0.01);
    rt.update(|_, tx| tx.focus("copy-a0", true));
    settle(&mut rt);
    assert!(opacity(&rt, "actions-a0") > 0.99);
}

#[test]
fn a_long_question_wraps_within_seventy_percent_of_the_thread() {
    let mut rt = test_chat_sized((900.0, 720.0));
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.sessions[0].messages.push(ChatMessage::user(
            "u0",
            "a long question that goes on and on, well past the width a single line of the bubble could ever hold on screen",
        ));
    });
    settle(&mut rt);
    let rect = |key: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no {key}"))
            .rect
    };
    let (thread, bubble) = (rect("thread"), rect("bubble-u0"));
    assert!(
        bubble[2] <= thread[2] * 0.7 + 0.5,
        "{bubble:?} in {thread:?}"
    );
    assert!(
        bubble[2] > thread[2] * 0.6,
        "the long question fills its share"
    );
    assert!(
        (bubble[0] + bubble[2] - (thread[0] + thread[2])).abs() < 0.5,
        "it sits on the right"
    );
}
