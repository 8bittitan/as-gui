use super::*;
use crate::command_bar::Group;

fn bar_chat() -> Runtime<Chat> {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| {
        s.agents = vec![
            agent("a1", "One", AgentStatus::Published),
            agent("a2", "Two", AgentStatus::Published),
        ];
        s.selected_agent = Some(s.agents[0].clone());
    });
    rt.frame(0.0);
    rt
}

fn press(rt: &mut Runtime<Chat>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    rt.frame(0.0);
}

fn chord(rt: &mut Runtime<Chat>, c: char) {
    press(rt, Key::Command(c));
}

fn open(rt: &mut Runtime<Chat>) {
    chord(rt, 'k');
    settle(rt);
}

fn type_text(rt: &mut Runtime<Chat>, text: &str) {
    rt.paste(text);
    rt.frame(0.0);
}

fn field_focused(rt: &Runtime<Chat>) -> bool {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("command-bar-field"))
        .is_some_and(|e| e.focused)
}

#[test]
fn cmd_k_opens_the_bar_with_the_field_focused() {
    let mut rt = bar_chat();
    assert!(rt.find("command-bar").is_none());
    open(&mut rt);
    assert!(rt.state.command_bar.open);
    assert!(rt.find("command-bar").is_some(), "the panel is on screen");
    assert!(field_focused(&rt), "the caret is in the field");
}

#[test]
fn the_field_takes_typing_without_a_click() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "dark");
    assert_eq!(rt.state.command_bar.query.text, "dark");
}

#[test]
fn cmd_k_closes_an_open_bar() {
    let mut rt = bar_chat();
    open(&mut rt);
    chord(&mut rt, 'k');
    assert!(!rt.state.command_bar.open);
}

#[test]
fn escape_closes_the_bar() {
    let mut rt = bar_chat();
    open(&mut rt);
    press(&mut rt, Key::Escape);
    settle(&mut rt);
    assert!(!rt.state.command_bar.open);
    assert!(rt.find("command-bar").is_none(), "and the panel goes");
}

#[test]
fn a_click_outside_closes_the_bar() {
    let mut rt = bar_chat();
    open(&mut rt);
    rt.pointer_move(4.0, 4.0);
    rt.frame(0.0);
    rt.pointer_down(4.0, 4.0);
    rt.frame(0.0);
    rt.pointer_up(4.0, 4.0);
    rt.frame(0.0);
    assert!(!rt.state.command_bar.open);
}

#[test]
fn a_click_inside_keeps_it_open() {
    let mut rt = bar_chat();
    open(&mut rt);
    tap(&mut rt, "command-bar-header");
    assert!(rt.state.command_bar.open);
}

#[test]
fn groups_are_headed() {
    let mut rt = bar_chat();
    open(&mut rt);
    for group in ["Conversations", "Agents", "Actions"] {
        assert!(
            rt.find(&format!("command-bar-group-{group}")).is_some(),
            "{group} has a header"
        );
    }
    assert!(rt.find("command-bar-group-Applications").is_none() == cfg!(not(debug_assertions)));
}

#[test]
fn down_and_up_move_the_selection_and_wrap() {
    let mut rt = bar_chat();
    open(&mut rt);
    let last = rt.state.command_entries().len() - 1;
    press(&mut rt, Key::Down);
    assert_eq!(rt.state.command_bar.selected, 1);
    press(&mut rt, Key::Up);
    press(&mut rt, Key::Up);
    assert_eq!(rt.state.command_bar.selected, last, "up from the top wraps");
    press(&mut rt, Key::Down);
    assert_eq!(rt.state.command_bar.selected, 0, "down from the end wraps");
    assert!(rt.state.command_bar.open);
}

#[test]
fn the_selected_row_is_marked() {
    let mut rt = bar_chat();
    open(&mut rt);
    press(&mut rt, Key::Down);
    rt.frame(0.0);
    let selected: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.item_selected)
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with("command-bar-row-"))
        .collect();
    assert_eq!(selected, ["command-bar-row-1"]);
}

#[test]
fn typing_narrows_the_list_and_returns_to_the_first_row() {
    let mut rt = bar_chat();
    open(&mut rt);
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Down);
    type_text(&mut rt, "theme: dark");
    assert_eq!(rt.state.command_bar.selected, 0);
    rt.frame(0.0);
    assert!(rt.find("command-bar-row-0").is_some());
    assert!(rt.find("command-bar-row-3").is_none(), "few rows match");
    assert_eq!(rt.state.command_entries()[0].title, "Theme: Dark");
}

#[test]
fn enter_runs_the_selection_and_closes_the_bar() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "theme: dark");
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.appearance.theme, ThemePref::Dark);
    assert!(!rt.state.command_bar.open);
}

#[test]
fn enter_runs_the_row_arrowed_to() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "theme");
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    assert_eq!(
        rt.state.appearance.theme,
        ThemePref::Dark,
        "the third Theme entry"
    );
}

#[test]
fn enter_with_no_results_keeps_the_bar_open() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "zzzzqqq");
    press(&mut rt, Key::Enter);
    assert!(rt.state.command_bar.open);
    assert!(rt.find("command-bar-empty").is_some(), "it says so");
}

#[test]
fn cmd_and_a_digit_run_that_result() {
    let mut rt = bar_chat();
    open(&mut rt);
    chord(&mut rt, '3');
    assert_eq!(
        rt.state.selected_agent.as_ref().unwrap().agent_id,
        "a2",
        "the third result is the agent Two"
    );
    assert!(!rt.state.command_bar.open);
}

#[test]
fn cmd_and_a_digit_past_the_results_do_nothing() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "theme: dark");
    chord(&mut rt, '9');
    assert!(rt.state.command_bar.open);
    assert_eq!(rt.state.appearance.theme, ThemePref::Auto);
}

#[test]
fn clicking_a_row_runs_it() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "accent colour: purple");
    rt.frame(0.0);
    tap(&mut rt, "command-bar-row-0");
    assert_eq!(rt.state.appearance.accent, Accent::Purple);
    assert!(!rt.state.command_bar.open);
}

#[test]
fn rows_show_their_keybind_and_number() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    open(&mut rt);
    type_text(&mut rt, "new conversation");
    rt.frame(0.0);
    assert!(shows_text(&rt, "⌘N", "command-bar-row-0"));
    assert!(shows_text(&rt, "⌘1", "command-bar-row-0"));
}

#[test]
fn other_shortcuts_wait_while_the_bar_is_open() {
    let mut rt = bar_chat();
    open(&mut rt);
    chord(&mut rt, 'n');
    chord(&mut rt, 'b');
    assert_eq!(rt.state.sessions.len(), 1, "⌘N did not start a session");
    assert!(rt.state.sidebar.is_open(), "⌘B did not toggle the sidebar");
    assert!(rt.state.command_bar.open);
}

#[test]
fn opening_again_forgets_the_last_search() {
    let mut rt = bar_chat();
    open(&mut rt);
    type_text(&mut rt, "dark");
    press(&mut rt, Key::Escape);
    settle(&mut rt);
    open(&mut rt);
    assert_eq!(rt.state.command_bar.query.text, "");
    assert!(field_focused(&rt));
}

#[test]
fn the_header_name_opens_the_bar_on_agents() {
    let mut rt = bar_chat();
    settle(&mut rt);
    tap(&mut rt, "agent-title");
    settle(&mut rt);
    assert!(rt.state.command_bar.open);
    assert_eq!(rt.state.command_bar.scope, Some(Group::Agents));
    assert!(field_focused(&rt));
    assert!(rt.find("command-bar-row-1").is_some());
    assert!(
        rt.find("command-bar-row-2").is_none(),
        "only the two agents"
    );
    assert!(
        rt.find("agent-picker-trigger").is_none(),
        "the dropdown is gone"
    );
}

#[test]
fn backspace_on_an_empty_field_widens_a_scoped_list() {
    let mut rt = bar_chat();
    settle(&mut rt);
    tap(&mut rt, "agent-title");
    settle(&mut rt);
    type_text(&mut rt, "o");
    press(&mut rt, Key::Backspace);
    assert_eq!(
        rt.state.command_bar.scope,
        Some(Group::Agents),
        "a letter is deleted first"
    );
    press(&mut rt, Key::Backspace);
    assert_eq!(rt.state.command_bar.scope, None);
    assert!(rt.state.command_bar.open);
}

#[test]
fn the_bar_opens_over_the_agent_picker_screen_too() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.agents = vec![agent("a1", "One", AgentStatus::Published)]);
    rt.frame(0.0);
    open(&mut rt);
    assert!(rt.find("command-bar").is_some());
    press(&mut rt, Key::Enter);
    assert_eq!(
        rt.state
            .selected_agent
            .as_ref()
            .map(|a| a.agent_id.as_str()),
        Some("a1"),
        "with no conversations yet, the first row is the agent"
    );
    assert!(!rt.state.command_bar.open);
}

#[test]
fn the_composer_card_renders_without_selects() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);

    assert!(
        rt.find("composer").is_some(),
        "the composer card is on screen"
    );
    assert!(rt.find("send").is_some(), "the send button is on screen");
    assert!(
        rt.find("composer-mode-trigger").is_none(),
        "the mode select is gone"
    );
    assert!(
        rt.find("composer-model-trigger").is_none(),
        "the model select is gone"
    );
    assert!(
        rt.find("composer-effort-trigger").is_none(),
        "the effort select is gone"
    );
}
