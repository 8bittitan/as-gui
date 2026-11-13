use super::*;

#[test]
fn a_later_tool_input_fills_the_announced_call() {
    let mut rt = test_chat();
    let sender = rt.state.begin_turn("Hi");
    sender
        .send(StreamEvent::ToolUsed(ToolCall {
            name: "search_tool".into(),
            input: None,
            open: false,
            request: None,
            error: None,
        }))
        .unwrap();
    sender
        .send(StreamEvent::ToolUsed(ToolCall {
            name: "search_tool".into(),
            input: Some("{\n  \"query\": \"x\"\n}".into()),
            open: false,
            request: None,
            error: None,
        }))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(m.tools.len(), 1, "the two events are one call");
    assert_eq!(
        m.tools[0].input.as_deref(),
        Some("{\n  \"query\": \"x\"\n}")
    );
}

#[test]
fn a_scripted_turn_records_its_trace_at_the_event_times() {
    let mut rt = test_chat();
    rt.update(|s, _| s.now = 10.0);
    let _sender = rt.state.begin_turn("Hi");
    rt.update(|s, _| {
        s.apply_at(
            StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "search_tool".into(),
            },
            10.5,
        );
        s.apply_at(
            StreamEvent::ToolUsed(ToolCall {
                name: "search_tool".into(),
                input: Some("{}".into()),
                open: false,
                request: None,
                error: None,
            }),
            10.6,
        );
        s.apply_at(
            StreamEvent::ToolFinished {
                call_id: "c1".into(),
                name: "search_tool".into(),
                output: Some("{\"hits\": []}".into()),
            },
            11.9,
        );
        s.apply_at(StreamEvent::TextDelta("abcdefgh".into()), 12.0);
        s.apply_at(StreamEvent::TextDelta("ijklmnop".into()), 13.0);
        s.apply_at(StreamEvent::Finished, 13.2);
    });

    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(
        m.trace,
        Some(TurnTrace {
            sent: 10.0,
            first_token: Some(12.0),
            last_token: Some(13.0),
            chars: 16,
            finished: Some(13.2),
            spans: vec![TraceSpan {
                call_id: "c1".into(),
                name: "search_tool".into(),
                start: 10.5,
                end: Some(11.9),
                input: Some("{}".into()),
                output: Some("{\"hits\": []}".into()),
                error: None,
                open: false,
            }],
            open: false,
        })
    );
    assert_eq!(m.tools.len(), 1, "the tools group still sees one call");
    assert_eq!(m.tools[0].input.as_deref(), Some("{\"hits\": []}"));
}

#[test]
fn a_call_whose_start_and_end_arrive_in_one_event_is_a_zero_width_marker() {
    let mut rt = test_chat();
    rt.update(|s, _| s.now = 1.0);
    let _sender = rt.state.begin_turn("Hi");
    rt.update(|s, _| {
        s.apply_at(
            StreamEvent::ToolUsed(ToolCall {
                name: "load_skill_tool".into(),
                input: Some("{}".into()),
                open: false,
                request: None,
                error: None,
            }),
            1.4,
        );
    });
    let spans = &rt.state.sessions[0].messages[1]
        .trace
        .as_ref()
        .unwrap()
        .spans;
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].start, spans[0].end), (1.4, Some(1.4)));
}

#[test]
fn a_failed_tool_call_records_why_and_closes_its_span() {
    let (rt, _) = failed_tool_chat();
    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(
        m.tools[0].error.as_deref(),
        Some("Cannot search in `brand` attribute")
    );
    assert_eq!(
        m.tools[0].input.as_deref(),
        Some("{\"facetName\": \"brand\"}"),
        "its input is kept"
    );
    let span = &m.trace.as_ref().unwrap().spans[0];
    assert_eq!(span.end, Some(0.9), "the call ended when it failed");
    assert_eq!(
        span.error.as_deref(),
        Some("Cannot search in `brand` attribute")
    );
}

#[test]
fn a_tool_failure_does_not_end_the_turn_or_raise_an_error() {
    let (rt, _) = failed_tool_chat();
    let m = &rt.state.sessions[0].messages[1];
    assert_eq!(m.text, "Let me try another way.", "the reply carried on");
    assert!(
        rt.state.error.is_none(),
        "no request-failed toast for a tool's error"
    );
}

#[test]
fn a_later_success_of_the_same_tool_clears_its_failure() {
    let (mut rt, _) = failed_tool_chat();
    rt.update(|s, _| {
        s.sessions[0].messages[1].streaming = true;
        s.apply_at(
            StreamEvent::ToolFinished {
                call_id: "c2".into(),
                name: "facet_tool".into(),
                output: Some("{\"facetHits\": []}".into()),
            },
            3.0,
        );
    });
    let tool = &rt.state.sessions[0].messages[1].tools[0];
    assert!(tool.error.is_none());
    assert_eq!(tool.input.as_deref(), Some("{\"facetHits\": []}"));
}

#[test]
fn a_failed_tool_row_says_it_failed_and_shows_why_when_opened() {
    let (mut rt, id) = failed_tool_chat();
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("tool-failed-facet_tool").is_some(),
        "the row is marked failed"
    );
    assert!(
        rt.find("tool-error-facet_tool").is_none(),
        "the reason waits for the row to open"
    );

    rt.update(|s, _| s.toggle_tool_open(&id, 0));
    rt.frame(1.0 / 60.0);
    assert!(shows_text(
        &rt,
        "Cannot search in `brand` attribute",
        "tool-error-facet_tool"
    ));
}

#[test]
fn a_failed_call_is_flagged_in_the_timeline() {
    let (mut rt, id) = failed_tool_chat();
    tap(&mut rt, &format!("timing-{id}"));
    assert!(shows_text(&rt, "failed", &format!("timeline-tool-{id}-0")));
}

#[test]
fn cmd_k_toggles_the_command_bar() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    assert!(!rt.state.command_bar.open);

    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(rt.state.command_bar.open);

    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(!rt.state.command_bar.open);

    // Shift-⌘K stays free for the app to bind later.
    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    rt.frame(0.0);
    assert!(!rt.state.command_bar.open);
}

#[test]
fn a_search_call_reads_as_a_step_with_its_index_and_query() {
    let mut rt = test_chat();
    rt.update(|s, _| crate::scenes::tools(s));
    rt.frame(1.0 / 60.0);
    let row = "tool-algolia_search_index_products_markdown";
    assert!(
        shows_text(&rt, "Searched products_markdown", row),
        "the index, not the raw tool name"
    );
    assert!(
        shows_text(&rt, "wide running shoes", row),
        "the query beside it"
    );
    assert!(!shows_text(&rt, "Called tool", row));
}

#[test]
fn search_results_list_the_first_three_hits_until_show_all() {
    let mut rt = test_chat_sized((720.0, 900.0));
    rt.update(|s, _| crate::scenes::tools(s));
    rt.frame(1.0 / 60.0);
    assert!(
        shows_text(&rt, "5 results", "tool-search_tool"),
        "the step counts its hits"
    );
    for i in 0..3 {
        assert!(
            rt.find(&format!("hit-search_tool-{i}")).is_some(),
            "hit {i} shows"
        );
    }
    assert!(
        rt.find("hit-search_tool-3").is_none(),
        "the rest wait behind Show all"
    );
    assert!(shows_text(
        &rt,
        "Show all 5 results",
        "hits-more-search_tool"
    ));

    tap(&mut rt, "hits-more-search_tool");
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("hit-search_tool-4").is_some(),
        "Show all lists every hit"
    );
    assert!(shows_text(&rt, "Show fewer", "hits-more-search_tool"));
}

#[test]
fn a_result_row_shows_a_plain_excerpt_not_the_markdown() {
    let mut rt = test_chat();
    rt.update(|s, _| crate::scenes::tools(s));
    rt.frame(1.0 / 60.0);
    assert!(shows_text(
        &rt,
        "The current best shoe and insole pairings",
        "hit-search_tool-0"
    ));
    assert!(
        !shows_text(&rt, "Updated for the 2026 season", "hit-search_tool-0"),
        "a snippet's heading is metadata, left out of the excerpt"
    );
}

#[test]
fn a_cards_leading_icon_is_the_tool_wrench_not_a_second_magnifying_glass() {
    let tool = |name: &str, error: Option<&str>| ToolCall {
        name: name.into(),
        input: Some(SEARCH_ARGS.into()),
        open: false,
        request: None,
        error: error.map(String::from),
    };
    let glyph = |t: &ToolCall| crate::conversation::step_glyph(t).name;
    assert_eq!(
        glyph(&tool("algolia_search_index_products", None)),
        crate::satellite::icons::WRENCH.name
    );
    assert_ne!(
        glyph(&tool("search_tool", None)),
        crate::satellite::icons::SEARCH.name,
        "the search action keeps the only magnifying glass"
    );
    assert_eq!(
        glyph(&tool("algolia_search_index_products", Some("boom"))),
        "lucide-circle-alert"
    );
}

const SEARCH_ARGS: &str = r#"{"query": "shoes"}"#;

#[test]
fn a_steps_results_sit_inside_its_card_under_its_header() {
    let mut rt = test_chat();
    rt.update(|s, _| crate::scenes::tools(s));
    settle(&mut rt);
    let rect = |rt: &Runtime<Chat>, key: &str| rt.presented(key).map(|(r, _)| r).expect(key);
    let card = rect(&rt, "tool-step-search_tool");
    let header = rect(&rt, "tool-search_tool");
    let hit = rect(&rt, "hit-search_tool-0");
    let inside = |r: [f32; 4]| {
        r[0] >= card[0] - 0.5
            && r[0] + r[2] <= card[0] + card[2] + 0.5
            && r[1] >= card[1] - 0.5
            && r[1] + r[3] <= card[1] + card[3] + 0.5
    };
    assert!(inside(header) && inside(hit), "one surface holds the step");
    assert!(
        (header[2] - hit[2]).abs() < 0.5,
        "the header and results share the card's width"
    );
    assert!(
        hit[1] >= header[1] + header[3] - 0.5,
        "results follow the header"
    );
}
