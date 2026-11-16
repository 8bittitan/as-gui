use super::*;
use crate::agent::{AgentConfig, ClientToolDecl};
use crate::command_bar::Command;
use crate::store::{self, ToolBinding};

const TOOL: &str = "throw_confetti";

fn sheet_chat() -> Runtime<Chat> {
    let mut rt = test_chat_sized((720.0, 1000.0));
    rt.on_key = Some(on_key);
    rt.update(|s, _| {
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: Vec::new(),
            conversation: Vec::new(),
        });
        s.mock_forced = false;
        s.agents = vec![agent_one()];
        s.selected_agent = Some(agent_one());
    });
    settle(&mut rt);
    rt
}

fn command(program: &str) -> ToolBinding {
    ToolBinding::Command {
        program: program.into(),
        args: vec!["--now".into()],
        timeout_secs: 5,
    }
}

fn wait_for(rt: &mut Runtime<Chat>, key: &str) {
    for _ in 0..1000 {
        rt.frame(1.0 / 60.0);
        if rt.find(key).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("{key} never showed");
}

fn open_sheet(rt: &mut Runtime<Chat>) {
    rt.update(|s, _| s.open_agent_panel(None));
    wait_for(rt, &format!("agent-tool-{TOOL}"));
    settle(rt);
}

fn press(rt: &mut Runtime<Chat>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(0.0);
}

fn focused(rt: &Runtime<Chat>, key: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.key.as_deref() == Some(key) && e.focused)
}

fn temp_state(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("as-gui-sheet-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("state.json")
}

#[test]
fn the_header_button_opens_the_sheet() {
    let mut rt = sheet_chat();
    assert!(rt.find("agent-panel").is_none());
    tap(&mut rt, "agent-settings");
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_some());
}

#[test]
fn the_command_bar_entry_opens_the_sheet() {
    let mut rt = sheet_chat();
    rt.update(|s, _| s.run_command(Command::AgentSettings));
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_some());
}

#[test]
fn the_shortcut_opens_and_closes_the_sheet() {
    let mut rt = sheet_chat();
    let chord = Mods {
        shift: true,
        ..Mods::default()
    };
    press(&mut rt, Key::Command(','), chord);
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_some());
    press(&mut rt, Key::Command(','), chord);
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_none());
}

#[test]
fn no_agent_means_no_sheet() {
    let mut rt = test_chat();
    rt.update(|s, _| s.open_agent_panel(None));
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_none());
}

#[test]
fn escape_closes_the_sheet_and_focus_returns_to_its_button() {
    let mut rt = sheet_chat();
    tap(&mut rt, "agent-settings");
    wait_for(&mut rt, &format!("agent-tool-{TOOL}"));
    settle(&mut rt);
    press(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_none());
    assert!(
        focused(&rt, "agent-settings"),
        "focus is back on the button"
    );
}

#[test]
fn the_sheet_is_a_labelled_region_naming_the_agent() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    let panel = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("agent-panel"))
        .unwrap();
    assert_eq!(panel.label.as_deref(), Some("One settings"));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.label.as_deref() == Some("Handler for throw_confetti")),
        "the select is named for its tool"
    );
}

#[test]
fn the_sheet_lists_the_declared_tools_and_the_server_tools() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    assert!(shows_text(
        &rt,
        "Throws confetti",
        "agent-tool-throw_confetti"
    ));
    assert!(shows_text(&rt, "Asks you each time", "agent-tool-summary"));
    assert!(shows_text(&rt, "load_skill_tool", "agent-server-tools"));
}

#[test]
fn a_config_for_an_agent_left_behind_is_dropped() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    let stale = AgentConfig {
        name: "Elsewhere".into(),
        description: None,
        status: AgentStatus::Published,
        model: None,
        client_tools: vec![ClientToolDecl {
            name: "stale_tool".into(),
            description: None,
            input_schema: None,
            is_terminal: false,
        }],
        other_tools: Vec::new(),
    };
    rt.update(move |s, _| {
        s.apply_at(
            StreamEvent::AgentConfigLoaded {
                agent_id: "other".into(),
                config: stale,
            },
            0.0,
        )
    });
    rt.frame(0.0);
    assert!(rt.find("agent-tool-stale_tool").is_none());
    assert!(rt.find(&format!("agent-tool-{TOOL}")).is_some());
}

#[test]
fn changing_agent_closes_the_sheet_and_forgets_its_config() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    rt.update(|s, _| s.choose_agent(0));
    settle(&mut rt);
    assert!(rt.find("agent-panel").is_none());
    assert!(rt.state.agent_config.config.is_none());
}

#[test]
fn choosing_command_opens_the_form_in_place_and_save_stores_the_binding() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    rt.update(|s, _| s.choose_handler(TOOL, "Command…"));
    settle(&mut rt);
    assert!(rt.find("agent-binding-form").is_some());
    assert!(rt.find("agent-panel").is_some(), "no second dialog");

    rt.update(|s, _| {
        s.binding_draft.program.insert("/usr/local/bin/confetti");
        s.binding_draft.args.insert("--count\ntwo words");
        s.binding_draft.timeout.clear();
        s.binding_draft.timeout.insert("12");
    });
    settle(&mut rt);
    tap(&mut rt, "agent-binding-save");
    settle(&mut rt);

    assert!(rt.find("agent-binding-form").is_none());
    assert_eq!(
        rt.state.tool_bindings().get(TOOL),
        Some(&ToolBinding::Command {
            program: "/usr/local/bin/confetti".into(),
            args: vec!["--count".into(), "two words".into()],
            timeout_secs: 12,
        })
    );
    assert_eq!(
        rt.state.bindings.get(TOOL).unwrap().program,
        "/usr/local/bin/confetti",
        "the registry's table follows"
    );
    assert!(shows_text(
        &rt,
        "/usr/local/bin/confetti --count 'two words'",
        "agent-tool-summary"
    ));
}

#[test]
fn a_command_without_a_program_is_refused_with_a_message() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    rt.update(|s, _| s.choose_handler(TOOL, "Command…"));
    settle(&mut rt);
    tap(&mut rt, "agent-binding-save");
    settle(&mut rt);
    assert!(rt.find("agent-binding-form").is_some());
    assert!(shows_text(&rt, "needs a program", "agent-binding-error"));
    assert!(rt.state.tool_bindings().is_empty());
}

#[test]
fn a_timeout_out_of_range_is_refused() {
    let mut rt = sheet_chat();
    open_sheet(&mut rt);
    rt.update(|s, _| {
        s.choose_handler(TOOL, "Command…");
        s.binding_draft.program.insert("/bin/x");
        s.binding_draft.timeout.clear();
        s.binding_draft.timeout.insert("9999");
    });
    settle(&mut rt);
    tap(&mut rt, "agent-binding-save");
    settle(&mut rt);
    assert!(shows_text(&rt, "between 1 and", "agent-binding-error"));
    assert!(rt.state.tool_bindings().is_empty());
}

#[test]
fn choosing_ask_me_removes_the_binding() {
    let mut rt = sheet_chat();
    rt.update(|s, _| s.set_binding(TOOL, command("/bin/x")).unwrap());
    open_sheet(&mut rt);
    rt.update(|s, _| s.choose_handler(TOOL, "Ask me"));
    rt.frame(0.0);
    assert!(rt.state.tool_bindings().is_empty());
    assert_eq!(rt.state.bindings.get(TOOL), None);
}

#[test]
fn a_binding_for_a_dropped_tool_is_listed_and_remove_deletes_it() {
    let mut rt = sheet_chat();
    rt.update(|s, _| s.set_binding("old_tool", command("/bin/old")).unwrap());
    open_sheet(&mut rt);
    assert!(rt.find("agent-dropped-old_tool").is_some());
    assert!(
        shows_text(&rt, "OLD_TOOL", "section-No longer declared")
            || rt.find("section-No longer declared").is_some()
    );

    tap(&mut rt, "agent-dropped-remove-old_tool");
    rt.frame(0.0);
    assert!(rt.find("agent-dropped-old_tool").is_none());
    assert!(rt.state.tool_bindings().is_empty());
}

#[test]
fn a_built_in_name_can_not_be_bound() {
    let mut rt = sheet_chat();
    let result = rt.state.set_binding("open_url", command("/bin/x"));
    assert!(result.is_err());
    assert!(rt.state.tool_bindings().is_empty());
}

#[test]
fn bindings_are_saved_under_the_application_and_agent_they_belong_to() {
    let path = temp_state("saved");
    let mut rt = sheet_chat();
    rt.update(|s, _| {
        s.applications
            .push(crate::chat::Application::new("second-app", "k"));
        s.persist_path = Some(path.clone());
        s.set_binding(TOOL, command("/bin/x")).unwrap();
    });
    let saved = store::load_from(&path);
    let first = &saved.applications[0];
    assert_eq!(first.app_id, "test-app");
    assert_eq!(first.agents["a1"].tools[TOOL], command("/bin/x"));
    assert!(saved.applications[1].agents.is_empty());
}

#[test]
fn removing_an_application_removes_its_agents_bindings() {
    let path = temp_state("removed");
    let mut rt = sheet_chat();
    rt.update(|s, _| {
        s.applications
            .push(crate::chat::Application::new("second-app", "k"));
        s.persist_path = Some(path.clone());
        s.set_binding(TOOL, command("/bin/x")).unwrap();
        s.remove_application(0);
    });
    let saved = store::load_from(&path);
    assert_eq!(saved.applications.len(), 1);
    assert!(saved.applications[0].agents.is_empty());
    assert!(!std::fs::read_to_string(&path).unwrap().contains("/bin/x"));
}

#[test]
fn bindings_follow_the_selected_agent() {
    let mut rt = sheet_chat();
    rt.update(|s, _| {
        s.agents = vec![agent_one(), agent("a2", "Two", AgentStatus::Published)];
        s.set_binding(TOOL, command("/bin/x")).unwrap();
        s.choose_agent(1);
    });
    assert_eq!(rt.state.bindings.get(TOOL), None);
    rt.update(|s, _| s.choose_agent(0));
    assert!(rt.state.bindings.get(TOOL).is_some());
}

#[test]
fn the_sheet_leaves_its_skeleton_without_any_input() {
    let mut rt = sheet_chat();
    rt.update(|s, _| s.open_agent_panel(None));
    for _ in 0..600 {
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
        if rt.find(&format!("agent-tool-{TOOL}")).is_some() {
            return;
        }
    }
    panic!("the sheet stayed on its skeleton");
}

#[test]
fn the_frame_that_releases_the_sheets_skeleton_reports_a_change() {
    let mut rt = sheet_chat();
    let released = std::cell::Cell::new(false);
    rt.update(|s, tx| {
        s.agent_panel = Some(Default::default());
        s.agent_config.load = Load::Loading { since: 0.0 }.finished(0.2);
        s.last_poll = 0.44;
        released.set(s.track_views(0.46, tx));
    });
    assert!(
        released.get(),
        "the view must rebuild when the placeholder lifts"
    );
}
