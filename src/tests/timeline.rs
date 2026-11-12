use super::*;

#[test]
fn a_settled_live_turn_offers_its_timing() {
    let (rt, id) = timed_turn_chat();
    assert!(rt.find(&format!("timing-{id}")).is_some());
    assert!(
        rt.find(&format!("timeline-{id}")).is_none(),
        "the timeline starts collapsed"
    );
}

#[test]
fn a_turn_still_streaming_offers_no_timing_yet() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    let sender = rt.state.begin_turn("Hi");
    sender.send(StreamEvent::TextDelta("Hel".into())).unwrap();
    rt.frame(1.0 / 60.0);
    let id = rt.state.sessions[0].messages[1].id.clone();
    assert!(rt.find(&format!("timing-{id}")).is_none());
}

#[test]
fn a_message_loaded_from_history_offers_no_timing() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        let mut reply = ChatMessage::assistant("h1");
        reply.text = "An old answer".into();
        reply.streaming = false;
        s.sessions[0].messages = vec![ChatMessage::user("h0", "Old question"), reply];
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("actions-h1").is_some(), "the reply is on screen");
    assert!(rt.find("timing-h1").is_none());
}

#[test]
fn expanding_the_timeline_shows_one_row_per_span_and_a_summary() {
    let (mut rt, id) = timed_turn_chat();
    tap(&mut rt, &format!("timing-{id}"));
    assert!(rt.find(&format!("timeline-{id}")).is_some());
    assert!(rt.find(&format!("timeline-request-{id}")).is_some());
    assert!(rt.find(&format!("timeline-tool-{id}-0")).is_some());
    assert!(rt.find(&format!("timeline-tool-{id}-1")).is_none());
    assert!(rt.find(&format!("timeline-reply-{id}")).is_some());
    assert!(rt.find(&format!("timeline-summary-{id}")).is_some());

    tap(&mut rt, &format!("timing-{id}"));
    assert!(
        rt.find(&format!("timeline-{id}")).is_none(),
        "pressing again collapses it"
    );
}

#[test]
fn clicking_a_timeline_row_shows_the_tools_json() {
    let (mut rt, id) = timed_turn_chat();
    tap(&mut rt, &format!("timing-{id}"));
    assert!(rt.find(&format!("timeline-json-{id}-0")).is_none());

    tap(&mut rt, &format!("timeline-tool-{id}-0"));
    assert!(rt.find(&format!("timeline-json-{id}-0")).is_some());
    assert!(shows_text(&rt, "OUTPUT", &format!("timeline-json-{id}-0")));

    tap(&mut rt, &format!("timeline-tool-{id}-0"));
    assert!(rt.find(&format!("timeline-json-{id}-0")).is_none());
}

#[test]
fn the_composer_glows_only_while_a_turn_is_streaming() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    settle(&mut rt);
    assert!(rt.find("composer-glow").is_none(), "no glow at rest");

    let sender = rt.state.begin_turn("Hi");
    settle(&mut rt);
    assert!(
        rt.find("composer-glow").is_some(),
        "glowing while streaming"
    );

    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    settle(&mut rt);
    assert!(
        rt.find("composer-glow").is_none(),
        "the glow ends with the turn"
    );
}

#[test]
fn the_composer_glow_goes_all_the_way_around() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    let _sender = rt.state.begin_turn("Hi");
    settle(&mut rt);
    let (card, _) = rt.presented("composer").unwrap();
    let (outline, _) = rt.presented("composer-glow").unwrap();
    assert_eq!(
        outline, card,
        "the glow traces the whole card: top, sides and bottom"
    );
    assert!(
        rt.find("composer-glow-band").is_none(),
        "nothing clips it to the top"
    );
}
