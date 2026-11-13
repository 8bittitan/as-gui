use super::*;

#[test]
fn a_quick_ask_sends_the_question_to_the_agent() {
    let mut rt = quick_ask();
    let recorder = Arc::new(HistoryRecorder {
        sent: Default::default(),
    });
    rt.update(|s, _| {
        s.snapshot.lock().unwrap().client = recorder.clone();
        s.composer.insert("What is new?");
        s.submit();
    });
    settle_quick_ask(&mut rt);
    assert_eq!(
        *recorder.sent.lock().unwrap(),
        vec![vec![(Role::User, "What is new?".to_string())]],
        "the question, and not the empty reply waiting for it"
    );
}

/// A Quick Ask panel mid-exchange: the question asked, the reply as `reply` left it.
#[test]
fn the_quick_ask_header_names_the_application_it_asks() {
    let mut rt = quick_ask();
    assert!(
        rt.find("qa-application").is_none(),
        "no application, no chip"
    );
    rt.update(|s, _| {
        s.snapshot.lock().unwrap().application =
            Some(("Production".into(), crate::palette::AppColour::Red));
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("qa-application").is_some());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Production")),
        "the name shows beside the dot"
    );
}

#[test]
fn the_quick_ask_transcript_is_inset_from_the_divider() {
    let rt = quick_ask_with(|m| m.streaming = false, false);
    let (header, _) = rt.presented("qa-header").unwrap();
    let (bubble, _) = rt.presented("qa-qa1").unwrap();
    assert!(
        bubble[1] - (header[1] + header[3]) >= 12.0,
        "the first message clears the divider: {bubble:?} under {header:?}"
    );
}

#[test]
fn a_waiting_quick_ask_says_thinking_once() {
    let rt = quick_ask_with(|_| {}, true);
    let thinking = rt
        .elements()
        .iter()
        .filter(|e| e.text.as_deref() == Some("Thinking…"))
        .count();
    assert_eq!(thinking, 1);
}

#[test]
fn the_sidebar_speaks_of_conversations_not_sessions() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    settle(&mut rt);
    rt.update(|s, _| s.new_session());
    rt.frame(0.0);
    let texts: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|e| e.text.clone())
        .collect();
    assert!(texts.iter().any(|t| t == "Conversations"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "New conversation"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains("ession")),
        "no \"session\" left in the UI: {texts:?}"
    );
}

#[test]
fn a_failed_quick_ask_shows_the_error_in_a_callout() {
    let mut rt = quick_ask_with(|m| m.streaming = false, false);
    rt.update(|s, _| s.error = Some("agent studio returned HTTP 422 — messages: required".into()));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("qa-error").is_some());
    let shown = rt.elements().iter().any(|e| {
        e.path.contains("qa-error")
            && e.text.as_deref() == Some("agent studio returned HTTP 422 — messages: required")
    });
    assert!(shown, "the API's message, verbatim");
    assert!(
        rt.find("qa-reply").is_none(),
        "a reply that never came leaves no empty block"
    );
}

#[test]
fn open_in_as_gui_sits_in_the_header_once_there_is_an_answer() {
    let rt = quick_ask();
    assert!(rt.find("qa-promote").is_none(), "nothing to open yet");

    let rt = quick_ask_with(|m| m.streaming = false, false);
    let promote = rt
        .elements()
        .iter()
        .find(|e| e.key.as_deref() == Some("qa-promote"))
        .map(|e| e.path.clone())
        .expect("the button is there");
    assert!(promote.contains("qa-header"), "in the header: {promote}");
}

#[test]
fn a_quick_ask_streams_a_reply_into_the_panel() {
    let mut rt = quick_ask();
    rt.update(|s, _| s.composer.insert("What is new?"));
    rt.update(|s, _| s.submit());
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
        if !rt.state.pending {
            break;
        }
    }
    assert!(!rt.state.pending, "the quick ask settles");
    assert_eq!(rt.state.messages.len(), 2);
    assert_eq!(rt.state.messages[0].text, "What is new?");
    assert_eq!(rt.state.messages[1].text, "Quick answer");
    assert!(
        rt.find("qa-reply").is_some(),
        "the reply renders in the panel"
    );
    assert_eq!(rt.state.composer.text, "", "the field clears on submit");
}

#[test]
fn the_quick_ask_field_takes_focus_when_the_panel_shows() {
    let mut rt = quick_ask();
    let focused = |rt: &Runtime<QuickAsk>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("qa-field"))
            .is_some_and(|e| e.focused)
    };
    assert!(!focused(&rt), "nothing is focused before the panel shows");

    rt.update(quick_ask::shown);
    rt.frame(1.0 / 60.0);
    assert!(focused(&rt), "showing the panel focuses the field");

    // A re-show after focus moved elsewhere (a click on a panel button) refocuses it.
    rt.update(|_, tx| tx.blur());
    rt.frame(1.0 / 60.0);
    assert!(!focused(&rt));
    rt.update(quick_ask::shown);
    rt.frame(1.0 / 60.0);
    assert!(focused(&rt), "every show refocuses the field");
}

#[test]
fn the_quick_ask_window_is_tall_enough_only_when_there_is_an_exchange() {
    let mut rt = quick_ask();
    assert_eq!(
        rt.state.wanted_size(),
        (480.0, 160.0),
        "empty: just the field"
    );

    rt.update(|s, tx| {
        s.composer.insert("What is new?");
        quick_ask::ask(s, tx);
    });
    let (_, height) = rt.state.wanted_size();
    assert!(height > 300.0, "room for the reply: {height}");
}

#[test]
fn the_quick_ask_panel_ends_in_a_powered_by_algolia_footer_that_links_out() {
    let mut rt = quick_ask();
    let footer = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("qa-footer"))
        .expect("the footer is there before anything is asked");
    assert_eq!(footer.label.as_deref(), Some("Powered by Algolia"));

    let composer_bottom = rt
        .presented("qa-composer")
        .map(|(r, _)| r[1] + r[3])
        .unwrap();
    let footer_top = rt.presented("qa-footer").map(|(r, _)| r[1]).unwrap();
    assert!(
        footer_top >= composer_bottom - 0.5,
        "the footer sits below the composer"
    );

    tap(&mut rt, "qa-footer");
    let urls = rt.take_url_requests();
    assert_eq!(urls.len(), 1);
    assert!(
        urls[0].starts_with("https://www.algolia.com"),
        "{:?}",
        urls[0]
    );
}

#[test]
fn the_footer_logo_is_right_aligned_with_the_send_button() {
    let rt = quick_ask();
    let right = |key: &str| rt.presented(key).map(|(r, _)| r[0] + r[2]).unwrap();
    assert!(
        (right("qa-footer-logo") - right("qa-send")).abs() < 0.5,
        "logo right edge {} vs send button {}",
        right("qa-footer-logo"),
        right("qa-send")
    );
}

#[test]
fn the_quick_ask_panel_follows_the_chosen_theme_not_the_system_one() {
    let panel_luma = |rt: &Runtime<QuickAsk>| {
        let fill = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("quick-ask"))
            .map(|e| e.fill)
            .unwrap();
        (fill.r + fill.g + fill.b) / 3.0
    };
    let mut rt = quick_ask();
    assert!(panel_luma(&rt) < 0.3, "auto follows the dark system");

    rt.state.snapshot.lock().unwrap().theme = ThemePref::Light;
    rt.update(|_, _| {});
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    assert!(panel_luma(&rt) > 0.7, "light overrides a dark system");

    rt.state.snapshot.lock().unwrap().theme = ThemePref::Dark;
    rt.update(|_, _| {});
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    assert!(panel_luma(&rt) < 0.3, "dark again");
}

#[test]
fn the_main_windows_theme_and_accent_reach_the_quick_ask_snapshot() {
    let mut rt = test_chat();
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
    rt.update(|s, _| {
        s.snapshot = Some(snapshot.clone());
        s.set_theme(ThemePref::Light);
        s.set_accent(Accent::Pink);
    });
    let shared = snapshot.lock().unwrap();
    assert_eq!(shared.theme, ThemePref::Light);
    assert_eq!(shared.accent, Accent::Pink);
}

#[test]
fn a_restored_appearance_reaches_quick_ask_too() {
    let mut rt = test_chat();
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
    rt.update(|s, _| {
        s.snapshot = Some(snapshot.clone());
        s.restore(&Persisted {
            theme: Some(ThemePref::Dark),
            accent: Some(Accent::Green),
            ..Persisted::default()
        });
    });
    let shared = snapshot.lock().unwrap();
    assert_eq!(shared.theme, ThemePref::Dark);
    assert_eq!(shared.accent, Accent::Green);
}

#[test]
fn a_quick_ask_without_an_agent_does_nothing() {
    let mut rt = quick_ask();
    rt.update(|s, _| {
        s.snapshot.lock().unwrap().agent_id = String::new();
        s.composer.insert("What is new?");
    });
    rt.update(|s, _| s.submit());
    rt.frame(1.0 / 60.0);
    assert!(rt.state.messages.is_empty(), "no agent, no turn");
    assert_eq!(rt.state.composer.text, "What is new?", "the draft stays");
}

#[test]
fn promoting_a_quick_ask_appends_it_as_a_session() {
    let mut rt = test_chat();
    rt.update(|s, _| s.receive_quick_ask("What is new?".into(), "Quick answer".into(), None));
    assert_eq!(
        rt.state.sessions.len(),
        2,
        "the exchange lands as a session"
    );
    assert_eq!(rt.state.current, 1, "and is selected");
    let session = &rt.state.sessions[1];
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[0].text, "What is new?");
    assert_eq!(session.messages[1].text, "Quick answer");
}

#[test]
fn choosing_an_agent_publishes_it_to_the_quick_ask_snapshot() {
    let mut rt = test_chat();
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
    rt.update(|s, _| s.snapshot = Some(snapshot.clone()));
    let sender = rt.state.begin_agent_load();
    sender
        .send(StreamEvent::AgentsLoaded(vec![
            agent_one(),
            agent("a2", "Two", AgentStatus::Published),
        ]))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| s.choose_agent(1));
    let published = snapshot.lock().unwrap();
    assert_eq!(published.agent_id, "a2");
    assert_eq!(published.agent_name, "Two");
}

#[test]
fn a_quick_ask_shows_the_tools_the_agent_ran() {
    let mut rt = quick_ask_sized((480.0, 480.0));
    rt.state.snapshot.lock().unwrap().client = Arc::new(MockAgent {
        response: "Quick answer",
        chunk_chars: 4,
        tools: mock_tools(),
    });
    rt.update(|s, _| s.composer.insert("What is new?"));
    rt.update(quick_ask::ask);
    settle_quick_ask(&mut rt);
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.find("qa-tool-load_skill_tool").is_some(),
        "each call draws its card in the panel"
    );
    let names: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|e| e.text.clone())
        .collect();
    assert!(
        names.iter().any(|t| t == "Ran load_skill_tool"),
        "the header says what the call did: {names:?}"
    );

    tap(&mut rt, "qa-tool-schedule_task_tool");
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("qa-tool-json-schedule_task_tool").is_some(),
        "tapping the header opens the call's payload"
    );
}

#[test]
fn a_quick_ask_shows_a_tool_only_reply_that_never_says_a_word() {
    let rt = quick_ask_with(
        |m| {
            m.streaming = false;
            m.tools = mock_tools();
        },
        false,
    );
    assert!(
        rt.find("qa-tool-load_skill_tool").is_some(),
        "the calls show even when the reply has no text"
    );
    assert!(
        rt.find("qa-copy-qa2").is_none(),
        "nothing to copy, no copy action"
    );
}

#[test]
fn the_quick_ask_reply_carries_copy_and_feedback_actions() {
    let rt = quick_ask_with(
        |m| {
            m.streaming = false;
            m.text = "Quick answer".into();
            m.server_id = Some("alg_msg_1".into());
        },
        false,
    );
    assert!(rt.find("qa-copy-qa2").is_some(), "copy is there");
    assert!(rt.find("qa-up-qa2").is_some(), "👍 is there");
    assert!(rt.find("qa-down-qa2").is_some(), "👎 is there");
}

#[test]
fn a_quick_ask_reply_the_server_never_persisted_skips_the_vote() {
    let rt = quick_ask_with(
        |m| {
            m.streaming = false;
            m.text = "Quick answer".into();
        },
        false,
    );
    assert!(rt.find("qa-copy-qa2").is_some(), "copy still is there");
    assert!(rt.find("qa-up-qa2").is_none(), "no id, no 👍");
    assert!(rt.find("qa-down-qa2").is_none(), "no id, no 👎");
}

#[test]
fn voting_in_the_quick_ask_sends_feedback_for_the_reply() {
    let votes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut rt = quick_ask_sized((480.0, 480.0));
    rt.state.snapshot.lock().unwrap().client = Arc::new(FeedbackRecorder {
        votes: Arc::clone(&votes),
    });
    rt.update(|s, _| {
        s.messages.push(ChatMessage::user("qa1", "What is new?"));
        let mut m = ChatMessage::assistant("qa2");
        m.streaming = false;
        m.text = "Quick answer".into();
        m.server_id = Some("alg_msg_1".into());
        s.messages.push(m);
    });
    rt.update(quick_ask::shown);
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
    tap(&mut rt, "qa-up-qa2");
    rt.frame(1.0 / 60.0);
    let recorded = |votes: &std::sync::Mutex<Vec<(String, String, i8)>>| {
        for _ in 0..100 {
            if !votes.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        votes.lock().unwrap().clone()
    };
    assert_eq!(
        recorded(&votes),
        vec![("a1".to_string(), "alg_msg_1".to_string(), 1)],
        "the vote goes to Agent Studio for the persisted reply"
    );
    assert_eq!(rt.state.messages[1].vote, Some(1), "and shows as voted");
}

#[test]
fn the_quick_ask_reply_clears_the_composer_divider() {
    let mut rt = quick_ask_with(
        |m| {
            m.streaming = false;
            m.text = "A long reply.\n\n".repeat(30);
        },
        false,
    );
    rt.update(quick_ask::shown);
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
    let reply_bottom = rt
        .presented("qa-actions-qa2")
        .map(|(r, _)| r[1] + r[3])
        .expect("the actions are on screen");
    let composer_top = rt.presented("qa-composer").map(|(r, _)| r[1]).unwrap();
    assert!(
        composer_top - reply_bottom >= 20.0,
        "the last message does not crowd the composer: {reply_bottom} under {composer_top}"
    );
}

#[test]
fn hiding_a_session_removes_it_from_the_sidebar_and_keeps_its_data() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    rt.update(|s, _| {
        s.new_session();
        s.new_session();
        s.select_session(0);
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("session-0").is_some());

    rt.update(|s, _| s.hide_session(0));
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("session-0").is_none(),
        "the hidden session leaves the sidebar"
    );
    assert!(rt.find("session-1").is_some());
    assert_eq!(
        rt.state.sessions.len(),
        3,
        "hiding keeps the conversation's data"
    );
    assert_eq!(
        rt.state.current, 1,
        "the selection moves to the first visible session"
    );

    // The hidden ones come back through the toggle.
    assert!(rt.find("hidden-toggle").is_some());
    settle(&mut rt);
    tap(&mut rt, "hidden-toggle");
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("session-0").is_some(),
        "expanded shows the hidden one"
    );
    rt.update(|s, _| s.select_session(0));
    rt.frame(1.0 / 60.0);
    assert!(
        !rt.state.sessions[0].hidden,
        "selecting a hidden conversation restores it"
    );
}
