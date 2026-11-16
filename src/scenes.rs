//! Deterministic state fixtures for rune's `--scene` argument, so headless snapshots and
//! trees can start from a chosen screen without credentials or scripts.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::agent::{
    AgentConfig, AgentInfo, AgentStatus, ChatMessage, ClientToolDecl, ClientTools, PendingCall,
    ToolCall, TraceSpan, TurnTrace,
};
use crate::chat::agent_panel::AgentPanel;
use crate::chat::{Application, CallState, Chat, Session};
use crate::loading::Load;
use crate::palette::AppColour;
use crate::store::ToolBinding;

fn with_scene_agent(s: &mut Chat) {
    s.selected_agent = Some(AgentInfo {
        agent_id: "scene".into(),
        name: "Scene agent".into(),
        description: None,
        status: AgentStatus::Published,
        model: Some("gpt-5".into()),
    });
}

pub fn agents(s: &mut Chat) {
    s.agents = vec![
        AgentInfo {
            agent_id: "alg_agt_shopping".into(),
            name: "Shopping assistant".into(),
            description: Some(
                "Answers questions about the product catalog and helps compare items".into(),
            ),
            status: AgentStatus::Published,
            model: Some("gpt-5".into()),
        },
        AgentInfo {
            agent_id: "alg_agt_support".into(),
            name: "Support agent".into(),
            description: Some("Triages customer questions from your help-center index".into()),
            status: AgentStatus::Published,
            model: Some("claude-sonnet-5".into()),
        },
        AgentInfo {
            agent_id: "alg_agt_draft".into(),
            name: "Merch copywriter".into(),
            description: Some("Drafts product blurbs — not published yet".into()),
            status: AgentStatus::Draft,
            model: None,
        },
    ];
    s.agents_load = Load::Idle.finished(0.0);
}

pub fn conversation(s: &mut Chat) {
    with_scene_agent(s);
    let session = &mut s.sessions[s.current];
    session.title = "Rune layout".into();
    session.messages.push(ChatMessage::user(
        "u1",
        "How do I make a column scroll in Rune?",
    ));
    let mut a = ChatMessage::assistant("a1");
    a.streaming = false;
    a.thought = Some(4.0);
    a.text = "Bind an ancestor's height and put `.min_h(0.0)` on every column between it and the viewport:\n\n```rust\ncol().h_full()\n    .child(col().grow(1.0).min_h(0.0)\n        .scroll(ScrollSpec { stick_bottom: true }))\n```\n\nA grown `.scroll(..)` inside an **auto-height** column collapses to 0 pt — Taffy gives it no free space.".into();
    session.messages.push(a);
}

pub fn settings(s: &mut Chat) {
    conversation(s);
    s.settings_open = true;
}

pub fn settings_wide(s: &mut Chat) {
    settings(s);
    s.appearance.width = crate::chat::ChatWidth::Wide;
}

/// The settings open on a forced dark appearance with a non-default accent, so snapshots show
/// both dials working whatever the system appearance is.
pub fn settings_styled(s: &mut Chat) {
    settings(s);
    s.appearance.theme = crate::chat::ThemePref::Dark;
    s.appearance.accent = crate::chat::Accent::Purple;
}

/// The first-run welcome dialog: no credentials yet, and the mock escape hatch off.
pub fn welcome(s: &mut Chat) {
    s.mock_forced = false;
    s.active = None;
}

/// The tuning panel open beside a conversation with search hits.
pub fn knobs(s: &mut Chat) {
    tools(s);
    s.knobs.show();
}

/// The reader open on a search hit.
pub fn reader(s: &mut Chat) {
    tools(s);
    s.open_reader("search_tool", 0);
}

/// An agent in use with nothing asked yet: the empty state.
pub fn empty(s: &mut Chat) {
    with_scene_agent(s);
    s.selected_agent.as_mut().unwrap().description =
        Some("Answers questions about the product catalog and helps compare items".into());
}

pub fn streaming(s: &mut Chat) {
    with_scene_agent(s);
    let session = &mut s.sessions[s.current];
    session.title = "Streaming".into();
    session.messages.push(ChatMessage::user("u1", "Say hello"));
    let mut a = ChatMessage::assistant("a1");
    a.text = "Hello! I am your Algolia agent, streaming a".into();
    session.messages.push(a);
    s.pending = true;
}

/// A reply streaming in a purple-accented dark window, so the composer's glow shows.
pub fn glow(s: &mut Chat) {
    streaming(s);
    s.appearance.theme = crate::chat::ThemePref::Dark;
    s.appearance.accent = crate::chat::Accent::Purple;
}

pub fn tools(s: &mut Chat) {
    with_scene_agent(s);
    let session = &mut s.sessions[s.current];
    session.title = "Tool calls".into();
    session.messages.push(ChatMessage::user(
        "u1",
        "Which running shoes suit wide feet?",
    ));
    let mut a = ChatMessage::assistant("a1");
    a.streaming = false;
    a.thought = Some(4.0);
    a.tools = vec![ToolCall {
        call_id: String::new(),
        provider_metadata: None,
        name: "algolia_search_index_products_markdown".into(),
        input: Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "clickAnalytics": true,
                "queries": [{
                    "facet_brand": null,
                    "facet_lang": ["en-US"],
                    "query": "wide running shoes",
                }],
                "sessionId": "7a1d2b3c-4e5f-6789-abcd-0123456789ab",
                "userIntent": "User asks which running shoes suit wide feet, so the index is searched for fit guides and shoe recommendations.",
            }))
            .unwrap(),
        ),
        open: true, request: None,
     error: None, }]
    .into_iter()
    .chain([ToolCall {
        call_id: String::new(),
        provider_metadata: None,
        name: "search_tool".into(),
        input: Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "hits": [
                    { "title": "Fit guide: wide feet", "path": "/guides/wide-fit",
                      "content": "###### *Updated for the 2026 season*\n\nThe current best **shoe and insole** pairings for wide feet.\n\n- W = Wide\n- XW = Extra wide" },
                    { "title": "Shoe tier list", "path": "/guides/shoes",
                      "content": "Where every shoe lands **after the latest release** — including the new arrivals." },
                    { "title": "Sizing cheat sheet", "path": "/guides/sizing",
                      "content": "How to compare shoes *without* running the numbers yourself." },
                    { "title": "Insole pairings", "path": "/guides/insoles",
                      "content": "Pairings that cover **arch support** needs." },
                    { "title": "Sizing spreadsheet", "path": "/guides/sizing-sheet",
                      "content": "Full sheet: (https://example.com/sizing-sheet)" }
                ]
            }))
            .unwrap(),
        ),
        open: true, request: None,
     error: None, }])
    .collect();
    a.text = "Wide feet work best with a **Cushioned Series** shoe — the search above pulled the current guides.".into();
    a.suggestions = Some(vec![
        "Show me the best insoles for wide feet".into(),
        "Explain how sizing works".into(),
        "Help me pick shoes for a marathon".into(),
    ]);
    session.messages.push(a);
}

/// The "search it yourself" panel beside a conversation, after a run: the first two of three
/// hits are ones the agent also showed.
pub fn search(s: &mut Chat) {
    use crate::agent::{IndexSearch, MockSearch};
    tools(s);
    s.open_search("a1", 0);
    s.search.index = rune::TextEdit::new("products_markdown");
    s.search.page = MockSearch
        .search("", "", "products_markdown", &serde_json::json!({}))
        .ok();
    s.search.load = Load::Idle.finished(0.0);
    s.search
        .agent_saw
        .extend(["id:mock-1".into(), "id:mock-2".into()]);
}

/// A tool call that errored, its row open on the reason, and its timeline beside it: the turn
/// went on and answered anyway.
pub fn tool_error(s: &mut Chat) {
    timeline(s);
    let m = s.sessions[s.current].messages.last_mut().unwrap();
    let reason = "Cannot search in `brand` attribute, you need to add `searchable(brand)` to attributesForFaceting. (Correlation-ID: EXAMPLE-0aBcD-0123456789abcdef)";
    m.tools.push(ToolCall {
        call_id: String::new(),
        provider_metadata: None,
        name: "algolia_search_for_facet_values".into(),
        input: Some(
            "{\n  \"facetName\": \"brand\",\n  \"indexName\": \"products_markdown\"\n}".into(),
        ),
        request: None,
        error: Some(reason.into()),
        open: true,
    });
    let trace = m.trace.as_mut().unwrap();
    trace.spans.push(TraceSpan {
        call_id: "c3".into(),
        name: "algolia_search_for_facet_values".into(),
        start: 0.7,
        end: Some(1.3),
        input: None,
        output: None,
        error: Some(reason.into()),
        open: false,
        client: None,
    });
}

/// A settled turn with its timeline open: a search that took 1.4s, a nested index query, and
/// a reply streaming at about 38 tokens a second.
pub fn timeline(s: &mut Chat) {
    tools(s);
    let m = s.sessions[s.current].messages.last_mut().unwrap();
    for t in &mut m.tools {
        t.open = false;
    }
    let span = |call_id: &str, name: &str, start: f64, end: f64, tool: usize| TraceSpan {
        call_id: call_id.into(),
        name: name.into(),
        start,
        end: Some(end),
        input: m.tools[tool].input.clone(),
        output: None,
        error: None,
        open: false,
        client: None,
    };
    let search = span("c1", "search_tool", 0.6, 2.0, 1);
    let index = span("c2", "algolia_search_index_products_markdown", 0.9, 1.5, 0);
    m.trace = Some(TurnTrace {
        sent: 0.0,
        first_token: Some(2.1),
        last_token: Some(4.0),
        chars: 290,
        finished: Some(4.2),
        spans: vec![search, index],
        open: true,
    });
}

/// Launching: the splash spinner while the agent list and conversations load.
pub fn splash(s: &mut Chat) {
    s.launch = Load::Loading { since: -1.0 };
    s.agents_load = Load::Loading { since: -1.0 };
}

/// An agent just chosen: skeletons in the sidebar and the chat section while its
/// conversation list loads.
pub fn loading_sessions(s: &mut Chat) {
    with_scene_agent(s);
    s.agents_load = Load::Idle.finished(0.0);
    s.sessions_load = Load::Loading { since: -1.0 };
}

/// A past conversation selected: the sidebar is in, the chat section shows its skeleton.
pub fn loading_conversation(s: &mut Chat) {
    with_scene_agent(s);
    s.agents_load = Load::Idle.finished(0.0);
    s.sessions_load = Load::Idle.finished(0.0);
    s.sessions = ["Rune layout", "Hit cards", "Quick Ask"]
        .iter()
        .enumerate()
        .map(|(i, title)| Session {
            id: Some(format!("c{i}")),
            hidden: false,
            title: (*title).into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: if i == 0 {
                Load::Loading { since: -1.0 }
            } else {
                Load::Idle
            },
        })
        .collect();
    s.current = 0;
}

/// The conversation list failed: an inline error with Retry in the sidebar.
pub fn sessions_failed(s: &mut Chat) {
    with_scene_agent(s);
    s.agents_load = Load::Idle.finished(0.0);
    s.sessions_load = Load::Failed("HTTP 500".into());
}

/// Production and a second application, each in its own colour.
fn scene_applications() -> Vec<Application> {
    let mut production = Application::new("PRODAPP001", "search-key-4f2a");
    production.label = Some("Production".into());
    production.colour = AppColour::Orange;
    let mut latency = Application::new("LATENCY001", "search-key-9c1d");
    latency.colour = AppColour::Teal;
    vec![production, latency]
}

/// Two saved applications, settings open on them, and the conversation behind.
pub fn applications(s: &mut Chat) {
    conversation(s);
    s.set_applications(scene_applications(), Some(0));
    s.settings_open = true;
}

/// Settings with Production's colour swatches open.
pub fn application_colours(s: &mut Chat) {
    applications(s);
    s.colour_picker = Some(0);
}

/// The same in the light theme, to check the colours read on both.
pub fn application_colours_light(s: &mut Chat) {
    application_colours(s);
    s.appearance.theme = crate::chat::ThemePref::Light;
}

/// The header's application menu open.
pub fn application_menu(s: &mut Chat) {
    conversation(s);
    s.set_applications(scene_applications(), Some(0));
    s.application_menu_open = true;
}

/// The command bar open on an empty query: recent conversations, agents, the other
/// application, then the actions.
pub fn command_bar(s: &mut Chat) {
    conversation(s);
    s.set_applications(scene_applications(), Some(0));
    s.agents = vec![
        AgentInfo {
            agent_id: "scene".into(),
            name: "Scene agent".into(),
            description: None,
            status: AgentStatus::Published,
            model: Some("gpt-5".into()),
        },
        AgentInfo {
            agent_id: "alg_agt_support".into(),
            name: "Support agent".into(),
            description: Some("Triages customer questions".into()),
            status: AgentStatus::Published,
            model: None,
        },
    ];
    s.sessions = [
        ("Rune layout", false),
        ("Hit cards", false),
        ("Quick Ask", true),
    ]
    .iter()
    .enumerate()
    .map(|(i, (title, hidden))| Session {
        id: Some(format!("c{i}")),
        hidden: *hidden,
        title: (*title).into(),
        messages: Vec::new(),
        agent_message_id: None,
        history: Load::Idle.finished(0.0),
    })
    .collect();
    s.sessions[0].title = "Rune layout".into();
    s.open_command_bar(None);
}

/// The bar with a typed query: matches ranked across groups.
pub fn command_bar_query(s: &mut Chat) {
    command_bar(s);
    s.command_bar.query = rune::TextEdit::new("se");
    s.command_bar.query.focused = true;
}

/// Stands in for the real registry: it answers every call as a built-in would.
struct SceneTools;

impl ClientTools for SceneTools {
    fn run(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
}

/// A turn that ended on a call to `name`: the reply so far, the call's card, and the call
/// in `state`.
fn client_call_turn(s: &mut Chat, name: &str, input: Value, state: CallState) {
    with_scene_agent(s);
    let session = &mut s.sessions[s.current];
    session.title = "Client tool".into();
    session
        .messages
        .push(ChatMessage::user("u1", "Open the docs for InstantSearch"));
    let mut a = ChatMessage::assistant("a1");
    a.text = "I'll open the page for you.".into();
    a.tools = vec![ToolCall {
        call_id: "scene_call_1".into(),
        provider_metadata: None,
        name: name.into(),
        input: serde_json::to_string_pretty(&input).ok(),
        open: false,
        request: None,
        error: None,
    }];
    session.messages.push(a);
    s.pending = true;
    s.scene_call(
        PendingCall {
            call_id: "scene_call_1".into(),
            name: name.into(),
            input,
        },
        state,
    );
}

fn open_docs() -> Value {
    json!({ "url": "https://www.algolia.com/doc/guides/building-search-ui/what-is-instantsearch/js/" })
}

/// A built-in call waiting for Run or Decline.
pub fn client_tool_approval(s: &mut Chat) {
    s.client_tools = Arc::new(SceneTools);
    client_call_turn(s, "open_url", open_docs(), CallState::Awaiting);
}

/// A call nothing handles: the card asks for a result to send back.
pub fn client_tool_manual(s: &mut Chat) {
    client_call_turn(
        s,
        "export_report",
        json!({ "format": "csv", "range": "last_7_days" }),
        CallState::Awaiting,
    );
}

/// A call Run was pressed on, still going.
pub fn client_tool_running(s: &mut Chat) {
    s.client_tools = Arc::new(SceneTools);
    client_call_turn(s, "open_url", open_docs(), CallState::Running);
}

/// The agent sheet: one declared tool, and a binding for a tool the agent dropped.
pub fn agent_panel(s: &mut Chat) {
    conversation(s);
    s.persist_path = None;
    let _ = s.set_binding(
        "old_export",
        ToolBinding::Command {
            program: "/usr/local/bin/export-report".into(),
            args: vec!["--format".into(), "csv".into()],
            timeout_secs: 30,
        },
    );
    s.agent_config.config = Some(AgentConfig {
        name: "Scene agent".into(),
        description: Some(
            "Answers questions about the product catalog and can export a report on request."
                .into(),
        ),
        status: AgentStatus::Published,
        model: Some("gpt-5".into()),
        client_tools: vec![ClientToolDecl {
            name: "export_report".into(),
            description: Some("Exports a sales report as a file on the user's Mac".into()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["csv", "json"] },
                    "range": { "type": "string" }
                },
                "required": ["format"]
            })),
            is_terminal: false,
        }],
        other_tools: vec!["search_tool".into(), "load_skill_tool".into()],
    });
    s.agent_config.load = Load::Idle.finished(0.0);
    s.agent_panel = Some(AgentPanel::default());
}
