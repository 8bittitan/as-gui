use super::*;

const SEARCH_INPUT: &str = r#"{"queries":[{"indexName":"products_markdown","query":"wide running shoes","facet_lang":["en-US"],"hitsPerPage":5}],"userIntent":"why"}"#;

const SEARCH_OUTPUT: &str = r#"{"hits":[{"objectID":"a1","title":"Fit guide","path":"/guides/wide-fit","content":"Best shoes"}]}"#;

/// Stands in for the index: records each search and answers with a canned reply.
struct RecordingSearch {
    calls: std::sync::Mutex<Vec<(String, String, String, serde_json::Value)>>,
    reply: Result<SearchPage, String>,
}

impl RecordingSearch {
    fn answering(body: &str) -> Arc<RecordingSearch> {
        Arc::new(RecordingSearch {
            calls: Default::default(),
            reply: parse_search_page(body),
        })
    }
}

impl IndexSearch for RecordingSearch {
    fn search(
        &self,
        app_id: &str,
        api_key: &str,
        index: &str,
        params: &serde_json::Value,
    ) -> Result<SearchPage, String> {
        self.calls.lock().unwrap().push((
            app_id.into(),
            api_key.into(),
            index.into(),
            params.clone(),
        ));
        self.reply.clone()
    }
}

/// A chat whose live turn made one search call: `tool` is its name, `input` its arguments.
fn searched_chat(tool: &str, input: &str) -> (Runtime<Chat>, String) {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    let _sender = rt.state.begin_turn("Find a shoe");
    rt.update(|s, _| {
        s.apply_at(
            StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: tool.into(),
            },
            0.1,
        );
        s.apply_at(
            StreamEvent::ToolUsed(ToolCall {
                name: tool.into(),
                input: Some(input.into()),
                request: None,
                open: false,
                error: None,
            }),
            0.2,
        );
        s.apply_at(
            StreamEvent::ToolFinished {
                call_id: "c1".into(),
                name: tool.into(),
                output: Some(SEARCH_OUTPUT.into()),
            },
            0.9,
        );
        s.apply_at(StreamEvent::Finished, 1.0);
    });
    rt.frame(1.0 / 60.0);
    let id = rt.state.sessions[0].messages[1].id.clone();
    (rt, id)
}

#[test]
fn a_finished_tool_call_keeps_its_request_beside_its_result() {
    let (rt, _) = searched_chat("search_tool", SEARCH_INPUT);
    let tool = &rt.state.sessions[0].messages[1].tools[0];
    assert_eq!(tool.request.as_deref(), Some(SEARCH_INPUT));
    assert_eq!(tool.input.as_deref(), Some(SEARCH_OUTPUT));
    assert_eq!(tool.search_request().unwrap().query, "wide running shoes");
}

#[test]
fn opening_a_search_from_a_tool_prefills_the_fields_from_its_call() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    rt.update(|s, _| s.open_search(&id, 0));
    let panel = &rt.state.search;
    assert!(panel.open);
    assert_eq!(panel.index.text, "products_markdown");
    assert_eq!(panel.query.text, "wide running shoes");
    let params: serde_json::Value = serde_json::from_str(&panel.params.text).unwrap();
    assert_eq!(
        params,
        serde_json::json!({ "hitsPerPage": 5 }),
        "facet_lang is the tool's own param, not Algolia's"
    );
    assert_eq!(panel.not_sent, ["facet_lang"]);
    assert!(panel.load.is_idle(), "opening doesn't run the search");
    assert!(panel.page.is_none());
    let shown = crate::agent::SearchHit::from_json(SEARCH_OUTPUT).unwrap();
    assert_eq!(
        panel.agent_saw,
        crate::agent::agent_saw(&shown),
        "the hits the agent got are remembered for marking"
    );
}

#[test]
fn a_search_tool_named_for_its_index_prefills_the_index() {
    let (mut rt, id) = searched_chat(
        "algolia_search_index_products_markdown",
        r#"{"queries":[{"query":"wide"}]}"#,
    );
    rt.update(|s, _| s.open_search(&id, 0));
    assert_eq!(rt.state.search.index.text, "products_markdown");
}

#[test]
fn opening_a_search_from_a_call_without_a_query_does_nothing() {
    let (mut rt, id) = searched_chat("other_tool", r#"{"foo":1}"#);
    rt.update(|s, _| s.open_search(&id, 0));
    assert!(!rt.state.search.open);
}

#[test]
fn run_searches_the_index_with_the_active_applications_key() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    let index = RecordingSearch::answering(
        r#"{"hits":[{"objectID":"a1","title":"Fit guide"}],"nbHits":1,"page":0,"nbPages":1,"processingTimeMS":3}"#,
    );
    rt.update(|s, _| {
        s.index_search = index.clone();
        s.open_search(&id, 0);
        s.search.query = rune::TextEdit::new("wide trail shoes");
        s.run_search();
    });
    assert!(matches!(rt.state.search.load, Load::Loading { .. }));
    for _ in 0..200 {
        rt.frame(1.0 / 60.0);
        if rt.state.search.page.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let page = rt.state.search.page.as_ref().expect("the page arrived");
    assert_eq!(page.nb_hits, 1);
    assert!(matches!(rt.state.search.load, Load::Ready { .. }));
    let calls = index.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (app_id, api_key, index_name, body) = &calls[0];
    assert_eq!(
        (app_id.as_str(), api_key.as_str()),
        ("test-app", "test-key")
    );
    assert_eq!(index_name, "products_markdown");
    assert_eq!(
        *body,
        serde_json::json!({ "query": "wide trail shoes", "hitsPerPage": 5 })
    );
}

#[test]
fn a_late_result_from_an_earlier_run_is_dropped() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    rt.update(|s, _| {
        s.index_search = RecordingSearch::answering(r#"{"hits":[],"nbHits":0}"#);
        s.open_search(&id, 0);
        s.run_search();
        s.run_search();
    });
    let stale = parse_search_page(r#"{"hits":[{"objectID":"old"}],"nbHits":9}"#).unwrap();
    let fresh = parse_search_page(r#"{"hits":[{"objectID":"new"}],"nbHits":1}"#).unwrap();
    rt.update(|s, _| {
        s.apply(StreamEvent::SearchLoaded {
            run: 1,
            page: stale,
        });
    });
    assert!(
        rt.state.search.page.is_none(),
        "run 1 was superseded by run 2"
    );
    assert!(matches!(rt.state.search.load, Load::Loading { .. }));
    rt.update(|s, _| {
        s.apply(StreamEvent::SearchLoaded {
            run: 2,
            page: fresh,
        });
    });
    assert_eq!(rt.state.search.page.as_ref().unwrap().nb_hits, 1);
}

#[test]
fn a_failed_run_keeps_the_apis_message_and_retry_runs_again() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    rt.update(|s, _| {
        s.open_search(&id, 0);
        s.run_search();
        s.apply(StreamEvent::SearchFailed {
            run: 1,
            error: "Index products_markdown does not exist (HTTP 404)".into(),
        });
    });
    assert_eq!(
        rt.state.search.load.error(),
        Some("Index products_markdown does not exist (HTTP 404)")
    );
}

#[test]
fn a_params_json_that_does_not_parse_blocks_run() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    let index = RecordingSearch::answering(r#"{"hits":[]}"#);
    rt.update(|s, _| {
        s.index_search = index.clone();
        s.open_search(&id, 0);
        s.search.params = rune::TextEdit::new("{\"hitsPerPage\": ");
        s.run_search();
    });
    assert!(rt.state.search.params_error().is_some());
    assert!(rt.state.search.load.is_idle(), "nothing ran");
    assert!(index.calls.lock().unwrap().is_empty());

    rt.update(|s, _| s.search.params = rune::TextEdit::new("[1]"));
    assert!(
        rt.state.search.params_error().is_some(),
        "params must be an object"
    );

    rt.update(|s, _| s.search.params = rune::TextEdit::new("{}"));
    assert!(rt.state.search.params_error().is_none());
}

#[test]
fn run_needs_an_index() {
    let (mut rt, id) = searched_chat("search_tool", r#"{"queries":[{"query":"x"}]}"#);
    rt.update(|s, _| {
        s.open_search(&id, 0);
        s.run_search();
    });
    assert!(rt.state.search.load.is_idle());
    assert!(rt.state.search.blocked().is_some());
}

const TWO_HITS: &str = r#"{"hits":[{"objectID":"a1","title":"Seen by the agent","content":"one"},{"objectID":"b2","title":"Missed by the agent","content":"two"}],"nbHits":57,"page":0,"nbPages":3,"hitsPerPage":2,"processingTimeMS":6}"#;

fn open_panel_chat() -> Runtime<Chat> {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    rt.update(|s, _| {
        s.index_search = RecordingSearch::answering(TWO_HITS);
        s.open_search(&id, 0);
    });
    rt.frame(1.0 / 60.0);
    rt
}

fn show_results(rt: &mut Runtime<Chat>) {
    rt.update(|s, _| {
        s.run_search();
        let page = parse_search_page(TWO_HITS).unwrap();
        s.apply(StreamEvent::SearchLoaded {
            run: s.search.run,
            page,
        });
    });
    settle(rt);
}

#[test]
fn a_tool_call_with_a_query_offers_search_it_yourself_under_its_row() {
    let (mut rt, id) = searched_chat("search_tool", SEARCH_INPUT);
    let key = format!("search-yourself-{id}-0");
    assert!(rt.find(&key).is_none(), "the collapsed group hides it");
    rt.update(|s, _| s.toggle_tools_open(&id));
    rt.frame(1.0 / 60.0);
    assert!(rt.find(&key).is_some());
    tap(&mut rt, &key);
    assert!(rt.state.search.open);
    assert!(rt.find("search-panel").is_some());
}

#[test]
fn a_tool_call_without_a_query_offers_no_search() {
    let (mut rt, id) = searched_chat("other_tool", r#"{"foo":1}"#);
    rt.update(|s, _| s.toggle_tools_open(&id));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("tool-other_tool").is_some());
    assert!(rt.find(&format!("search-yourself-{id}-0")).is_none());
}

#[test]
fn the_hit_reader_offers_search_it_yourself_and_hands_over_to_the_panel() {
    let (mut rt, _) = searched_chat("search_tool", SEARCH_INPUT);
    rt.update(|s, _| s.open_reader("search_tool", 0));
    settle(&mut rt);
    tap(&mut rt, "hit-reader-search");
    settle(&mut rt);
    assert!(rt.state.search.open);
    assert!(rt.state.reader.is_none(), "the reader steps aside");
}

#[test]
fn the_hit_reader_disables_search_it_yourself_without_a_recognisable_query() {
    let (mut rt, _) = searched_chat("other_tool", r#"{"foo":1}"#);
    rt.update(|s, _| s.open_reader("other_tool", 0));
    settle(&mut rt);
    assert!(rt.find("hit-reader-search").is_some());
    tap(&mut rt, "hit-reader-search");
    assert!(!rt.state.search.open);
    assert!(rt.state.reader.is_some());
}

#[test]
fn the_panel_shows_the_prefilled_index_and_query() {
    let rt = open_panel_chat();
    assert!(shows_text(&rt, "products_markdown", "search-index"));
    assert!(shows_text(&rt, "wide running shoes", "search-query"));
    assert!(rt.find("search-run").is_some());
    assert!(rt.find("search-params").is_none(), "Advanced starts folded");
}

#[test]
fn the_panel_says_which_of_the_calls_params_were_not_sent() {
    let rt = open_panel_chat();
    assert!(shows_text(&rt, "facet_lang", "search-not-sent"));
}

#[test]
fn the_advanced_expander_shows_the_params_json() {
    let mut rt = open_panel_chat();
    tap(&mut rt, "search-advanced");
    assert!(rt.find("search-params").is_some());
    assert!(shows_text(&rt, "hitsPerPage", "search-params"));
    tap(&mut rt, "search-advanced");
    assert!(rt.find("search-params").is_none());
}

#[test]
fn invalid_params_show_an_inline_message() {
    let mut rt = open_panel_chat();
    assert!(rt.find("search-blocked").is_none());
    rt.update(|s, _| s.search.params = rune::TextEdit::new("{"));
    rt.frame(1.0 / 60.0);
    assert!(rt.find("search-blocked").is_some());
}

#[test]
fn pressing_run_starts_the_search() {
    let mut rt = open_panel_chat();
    tap(&mut rt, "search-run");
    assert_eq!(rt.state.search.run, 1);
}

#[test]
fn a_running_search_shows_a_skeleton() {
    let mut rt = open_panel_chat();
    rt.update(|s, _| s.search.load = Load::Loading { since: -1.0 });
    rt.frame(1.0 / 60.0);
    assert!(rt.find("search-skeleton").is_some());
    assert!(rt.find("search-hit-1").is_none());
}

#[test]
fn results_show_rank_and_mark_what_the_agent_saw() {
    let mut rt = open_panel_chat();
    show_results(&mut rt);
    assert!(rt.find("search-skeleton").is_none());
    assert!(rt.find("search-hit-1").is_some());
    assert!(rt.find("search-hit-2").is_some());
    assert!(shows_text(&rt, "#1", "search-hit-1"));
    assert!(
        rt.find("search-saw-1").is_some(),
        "a1 was in the agent's result"
    );
    assert!(rt.find("search-saw-2").is_none(), "b2 was not");
}

#[test]
fn the_footer_shows_totals_and_timing() {
    let mut rt = open_panel_chat();
    show_results(&mut rt);
    assert!(shows_text(&rt, "57 hits", "search-footer"));
    assert!(shows_text(&rt, "6 ms", "search-footer"));
    assert!(shows_text(&rt, "Page 1 of 3", "search-footer"));
    assert!(rt.find("search-curl").is_some());
}

#[test]
fn a_failed_search_shows_the_error_with_retry() {
    let mut rt = open_panel_chat();
    rt.update(|s, _| {
        s.index_search =
            RecordingSearch::answering(r#"{"message":"Index nope does not exist","status":404}"#);
        s.run_search();
    });
    for _ in 0..200 {
        rt.frame(1.0 / 60.0);
        if rt.state.search.load.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    settle(&mut rt);
    assert!(shows_text(
        &rt,
        "Index nope does not exist (HTTP 404)",
        "search-error"
    ));
    tap(&mut rt, "search-retry");
    assert_eq!(rt.state.search.run, 2, "Retry runs again");
}

#[test]
fn the_next_page_control_runs_the_following_page() {
    let mut rt = open_panel_chat();
    show_results(&mut rt);
    tap(&mut rt, "search-prev");
    assert_eq!(rt.state.search.run, 1, "there is no page before the first");

    tap(&mut rt, "search-next");
    assert_eq!(rt.state.search.run, 2);
    let params: serde_json::Value = serde_json::from_str(&rt.state.search.params.text).unwrap();
    assert_eq!(params["page"], 1);
}

#[test]
fn the_close_button_and_escape_close_the_panel() {
    let mut rt = open_panel_chat();
    rt.on_key = Some(on_key);
    tap(&mut rt, "search-close");
    assert!(!rt.state.search.open);

    rt.update(|s, _| s.search.open = true);
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.search.open);
}
