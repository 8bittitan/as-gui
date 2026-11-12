use super::*;

fn long_hit_chat() -> Runtime<Chat> {
    let mut rt = test_chat();
    rt.update(|s, _| {
        crate::scenes::tools(s);
        let body = (0..40)
            .map(|i| format!("Paragraph {i} of the guide."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let input = serde_json::json!({
            "hits": [{ "title": "Long guide", "path": "/guides/long", "content": format!("{body}\n\nTAILMARK") }]
        });
        let session = &mut s.sessions[s.current];
        for t in session.messages.iter_mut().flat_map(|m| m.tools.iter_mut()) {
            if t.name == "search_tool" {
                t.input = Some(input.to_string());
            }
        }
    });
    settle(&mut rt);
    rt
}

#[test]
fn clicking_a_hit_card_opens_a_reader_with_the_whole_markdown() {
    let mut rt = long_hit_chat();
    assert!(rt.find("hit-reader").is_none());

    tap(&mut rt, "hit-search_tool-0");
    settle(&mut rt);
    assert!(rt.find("hit-reader").is_some(), "the reader is on screen");
    assert!(
        shows_text(&rt, "TAILMARK", "hit-reader"),
        "the reader holds the full content"
    );
}

#[test]
fn esc_and_the_close_button_close_the_reader_and_the_card_stays() {
    let mut rt = long_hit_chat();
    rt.on_key = Some(on_key);

    tap(&mut rt, "hit-search_tool-0");
    settle(&mut rt);
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(rt.state.reader.is_none(), "Esc closes it");
    assert!(rt.find("hit-reader").is_none());
    assert!(
        rt.find("hit-search_tool-0").is_some(),
        "the card is still in the grid"
    );

    tap(&mut rt, "hit-search_tool-0");
    settle(&mut rt);
    tap(&mut rt, "hit-reader-close");
    settle(&mut rt);
    assert!(rt.state.reader.is_none(), "the close button closes it");
}

#[test]
fn a_mock_run_without_credentials_lists_the_mock_agents() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.applications.clear();
        s.active = None;
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: Vec::new(),
            conversation: Vec::new(),
        });
    });
    for _ in 0..200 {
        rt.frame(1.0 / 60.0);
        if rt.state.agents_load.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        rt.state.agents_load.is_settled(),
        "the picker is not stuck loading"
    );
    assert_eq!(rt.state.agents.len(), 1);
}
