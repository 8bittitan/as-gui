//! The command bar (⌘K): one text field over a filtered list of everything the app can do —
//! switch agent or application, jump to a conversation, start a session, change appearance,
//! open settings. `entries` is the pure core; `impl Chat` here runs them; `overlay` draws it.

use crate::satellite::typography::{BODY, SUBHEADING};
use crate::satellite::{RADIUS_CONTROL, icons};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, text_input};
use rune_kit::Badge;

use crate::agent::AgentStatus;
use crate::chat::{Accent, Chat, ChatWidth, MOCK_AVAILABLE, ThemePref};
use crate::keymap::Action;
use crate::shell::{modal_panel, push_top_modal_layer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Agents,
    Applications,
    Conversations,
    Actions,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Agents => "Agents",
            Group::Applications => "Applications",
            Group::Conversations => "Conversations",
            Group::Actions => "Actions",
        }
    }
}

/// What running an entry does; indices point into the `Chat` lists the entry was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Agent(usize),
    Application(usize),
    Mock,
    Session(usize),
    NewSession,
    ToggleSidebar,
    Settings,
    AddApplication,
    Theme(ThemePref),
    Accent(Accent),
    Width(ChatWidth),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub group: Group,
    pub title: String,
    pub subtitle: Option<String>,
    pub hint: String,
    pub command: Command,
}

#[derive(Default)]
pub struct CommandBar {
    pub open: bool,
    pub query: TextEdit,
    pub selected: usize,
    pub scope: Option<Group>,
}

const RECENT_CONVERSATIONS: usize = 5;

/// How well `query` fits `text`, higher being better, or none when its letters don't all
/// appear in order: exact, then prefix, then a word start, then inside a word, then scattered.
pub fn fuzzy_score(query: &str, text: &str) -> Option<u32> {
    let query: Vec<char> = query.trim().to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    if query.is_empty() {
        return Some(0);
    }
    if query == text {
        return Some(1000);
    }
    if let Some(at) = text.windows(query.len()).position(|w| w == query) {
        let nearness = at.min(50) as u32;
        return Some(if at == 0 {
            900
        } else if !text[at - 1].is_alphanumeric() {
            800 - nearness
        } else {
            700 - nearness
        });
    }
    let mut letters = query.iter();
    let mut wanted = letters.next();
    let (mut first, mut last) = (None, 0);
    for (i, c) in text.iter().enumerate() {
        if Some(c) == wanted {
            first.get_or_insert(i);
            last = i;
            wanted = letters.next();
        }
    }
    if wanted.is_some() {
        return None;
    }
    let spread = last + 1 - first.unwrap_or(0) - query.len();
    Some(500 - spread.min(200) as u32)
}

fn candidate(group: Group, title: impl Into<String>, command: Command) -> Entry {
    Entry {
        group,
        title: title.into(),
        subtitle: None,
        hint: String::new(),
        command,
    }
}

fn with_subtitle(mut entry: Entry, subtitle: Option<String>) -> Entry {
    entry.subtitle = subtitle;
    entry
}

fn agent_entries(s: &Chat) -> Vec<Entry> {
    let current = s.selected_agent.as_ref().map(|a| a.agent_id.as_str());
    s.agents
        .iter()
        .enumerate()
        .filter(|(_, a)| a.status == AgentStatus::Published)
        .map(|(i, a)| {
            let subtitle = if current == Some(a.agent_id.as_str()) {
                Some("current agent".to_string())
            } else {
                a.description.clone()
            };
            with_subtitle(
                candidate(Group::Agents, &a.name, Command::Agent(i)),
                subtitle,
            )
        })
        .collect()
}

fn application_entries(s: &Chat) -> Vec<Entry> {
    let names = s.application_options();
    let mut found: Vec<Entry> = s
        .applications
        .iter()
        .enumerate()
        .filter(|(i, _)| s.active != Some(*i))
        .map(|(i, a)| {
            let subtitle = (names[i] != a.app_id).then(|| a.app_id.clone());
            with_subtitle(
                candidate(Group::Applications, &names[i], Command::Application(i)),
                subtitle,
            )
        })
        .collect();
    if MOCK_AVAILABLE && s.active.is_some() {
        found.push(candidate(Group::Applications, "Mock", Command::Mock));
    }
    found
}

fn conversation_entries(s: &Chat) -> Vec<Entry> {
    if s.selected_agent.is_none() {
        return Vec::new();
    }
    s.sessions
        .iter()
        .enumerate()
        .map(|(i, session)| {
            let subtitle = session
                .hidden
                .then(|| "hidden — selecting restores it".to_string());
            with_subtitle(
                candidate(Group::Conversations, &session.title, Command::Session(i)),
                subtitle,
            )
        })
        .collect()
}

fn action_entries(s: &Chat) -> Vec<Entry> {
    let action = |title: &str, command: Command, bound: Option<Action>| Entry {
        hint: bound.map(|a| s.keymap.hint(a)).unwrap_or_default(),
        ..candidate(Group::Actions, title, command)
    };
    let mut found = vec![
        action(
            "New conversation",
            Command::NewSession,
            Some(Action::NewSession),
        ),
        action(
            "Toggle sidebar",
            Command::ToggleSidebar,
            Some(Action::ToggleSidebar),
        ),
        action("Settings", Command::Settings, Some(Action::Settings)),
        action("Add application", Command::AddApplication, None),
    ];
    for (label, theme) in
        ThemePref::LABELS
            .iter()
            .zip([ThemePref::Auto, ThemePref::Light, ThemePref::Dark])
    {
        found.push(action(
            &format!("Theme: {label}"),
            Command::Theme(theme),
            None,
        ));
    }
    for accent in Accent::ALL {
        found.push(action(
            &format!("Accent colour: {}", accent.label()),
            Command::Accent(accent),
            None,
        ));
    }
    for (label, width) in [
        ("Comfortable", ChatWidth::Comfortable),
        ("Wide", ChatWidth::Wide),
    ] {
        found.push(action(
            &format!("Chat width: {label}"),
            Command::Width(width),
            None,
        ));
    }
    found
}

/// What the bar lists for `query`. Empty: the recent conversations, the agents, the other
/// applications, then the actions. Typed: whatever fuzzy-matches a title, each group kept
/// together with the group holding the best match first (ties in group order) and its entries
/// ranked. A scope keeps only its group.
pub fn entries(s: &Chat, query: &str) -> Vec<Entry> {
    let mut found = Vec::new();
    found.extend(agent_entries(s));
    found.extend(application_entries(s));
    found.extend(conversation_entries(s));
    found.extend(action_entries(s));
    if let Some(scope) = s.command_bar.scope {
        found.retain(|e| e.group == scope);
    }
    if query.trim().is_empty() {
        let mut recent = 0;
        found.retain(|e| {
            e.group != Group::Conversations || {
                recent += 1;
                recent <= RECENT_CONVERSATIONS
            }
        });
        found.sort_by_key(|e| match e.group {
            Group::Conversations => 0,
            Group::Agents => 1,
            Group::Applications => 2,
            Group::Actions => 3,
        });
        return found;
    }
    let mut scored: Vec<(u32, Entry)> = found
        .into_iter()
        .filter_map(|e| Some((fuzzy_score(query, &e.title)?, e)))
        .collect();
    let mut best = std::collections::BTreeMap::new();
    for (score, e) in &scored {
        let top = best.entry(e.group).or_insert(0);
        *top = (*top).max(*score);
    }
    scored.sort_by_key(|(score, e)| {
        (
            std::cmp::Reverse(best[&e.group]),
            e.group,
            std::cmp::Reverse(*score),
        )
    });
    scored.into_iter().map(|(_, e)| e).collect()
}

impl Chat {
    pub fn open_command_bar(&mut self, scope: Option<Group>) {
        let mut query = TextEdit::new("");
        query.focused = true;
        self.command_bar = CommandBar {
            open: true,
            query,
            selected: 0,
            scope,
        };
    }

    pub fn close_command_bar(&mut self) {
        self.command_bar.open = false;
    }

    pub fn toggle_command_bar(&mut self) {
        if self.command_bar.open {
            self.close_command_bar();
        } else {
            self.open_command_bar(None);
        }
    }

    pub fn command_entries(&self) -> Vec<Entry> {
        entries(self, &self.command_bar.query.text)
    }

    pub fn command_selection(&self, listed: usize) -> usize {
        self.command_bar.selected.min(listed.saturating_sub(1))
    }

    pub fn move_command_selection(&mut self, delta: isize) {
        let listed = self.command_entries().len();
        if listed == 0 {
            self.command_bar.selected = 0;
            return;
        }
        let from = self.command_selection(listed) as isize;
        self.command_bar.selected = (from + delta).rem_euclid(listed as isize) as usize;
    }

    pub fn run_selected_command(&mut self) {
        let listed = self.command_entries().len();
        self.run_command_at(self.command_selection(listed));
    }

    pub fn run_command_at(&mut self, i: usize) {
        if let Some(entry) = self.command_entries().into_iter().nth(i) {
            self.run_command(entry.command);
        }
    }

    pub fn run_command(&mut self, command: Command) {
        self.close_command_bar();
        match command {
            Command::Agent(i) => self.choose_agent(i),
            Command::Application(i) => self.switch_application(i),
            Command::Mock => self.switch_to_mock(),
            Command::Session(i) => self.select_session(i),
            Command::NewSession => self.new_session(),
            Command::ToggleSidebar => self.sidebar.toggle(),
            Command::Settings => self.settings_open = true,
            Command::AddApplication => {
                self.add_form.error = None;
                self.add_form.open = true;
            }
            Command::Theme(theme) => self.set_theme(theme),
            Command::Accent(accent) => self.set_accent(accent),
            Command::Width(width) => self.set_width(width),
        }
    }
}

const LIST: &str = "command-bar-list";
const FIELD: &str = "command-bar-field";

fn row_key(i: usize) -> String {
    format!("command-bar-row-{i}")
}

/// The bar is on screen: put the caret in the field, by the model (so it holds across
/// frames) and by key (so it wins focus a click on the page took).
pub fn shown(s: &mut Chat, tx: &mut Tx) {
    s.command_bar.query.focused = true;
    tx.focus(FIELD, false);
}

pub fn toggled(s: &mut Chat, tx: &mut Tx) {
    s.toggle_command_bar();
    if s.command_bar.open {
        shown(s, tx);
    }
}

pub fn open_agents(s: &mut Chat, tx: &mut Tx) {
    s.open_command_bar(Some(Group::Agents));
    shown(s, tx);
}

/// The keys the bar owns while it's open: ↑/↓ move, Enter runs, ⌘1–⌘9 run by number, Esc
/// closes, and Backspace on an empty field widens a scoped list. True when consumed.
pub fn key(s: &mut Chat, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !s.command_bar.open {
        return false;
    }
    match &ev.key {
        Key::Escape => s.close_command_bar(),
        Key::Up | Key::Down => {
            s.move_command_selection(if ev.key == Key::Up { -1 } else { 1 });
            let listed = s.command_entries().len();
            tx.scroll_into_view_in(LIST, row_key(s.command_selection(listed)));
        }
        Key::Enter => s.run_selected_command(),
        Key::Backspace if s.command_bar.query.text.is_empty() && s.command_bar.scope.is_some() => {
            s.command_bar.scope = None;
            s.command_bar.selected = 0;
        }
        Key::Command(c @ '1'..='9') if !ev.mods.shift && !ev.mods.alt => {
            s.run_command_at(*c as usize - '1' as usize);
        }
        _ => return false,
    }
    true
}

fn number_hint(n: usize) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    };
    format!("{modifier}{n}")
}

fn key_cap(k: &rune_kit::Kit, caps: &str) -> El {
    text(caps).font(k.ty.small).color(k.c.text_faint)
}

fn result_row(k: &rune_kit::Kit, i: usize, entry: &Entry, selected: bool, total: usize) -> El {
    let mut labels = row().grow(1.0).min_w(0.0).gap(8.0).center_y().child(
        text(&entry.title)
            .font(k.ty.ui)
            .color(k.c.text_base)
            .truncate(),
    );
    if let Some(subtitle) = &entry.subtitle {
        labels = labels.child(
            text(subtitle)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        );
    }
    let mut result = row()
        .key(row_key(i))
        .role("option")
        .label(&entry.title)
        .position_in_set(i + 1, total)
        .focus_selected(selected)
        .w_full()
        .min_h(36.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(RADIUS_CONTROL)
        .hover(k.c.hover)
        .on_hover(move |s: &mut Chat| s.command_bar.selected = i)
        .on_press(move |s: &mut Chat| s.run_command_at(i))
        .child(labels);
    if selected {
        result = result.fill(k.c.layer_02);
    }
    if !entry.hint.is_empty() {
        result = result.child(key_cap(k, &entry.hint));
    }
    if i < 9 {
        result = result.child(key_cap(k, &number_hint(i + 1)));
    }
    result
}

fn group_header(k: &rune_kit::Kit, group: Group) -> El {
    text(group.label().to_uppercase())
        .key(format!("command-bar-group-{}", group.label()))
        .label(group.label())
        .font(SUBHEADING)
        .color(k.c.text_muted)
        .px(12.0)
        .pt(12.0)
        .pb(4.0)
}

fn result_list(k: &rune_kit::Kit, found: &[Entry], selected: usize) -> El {
    let mut list = col()
        .key(LIST)
        .role("listbox")
        .label("Results")
        .w_full()
        .padding(8.0);
    if found.is_empty() {
        return list.child(
            text("No results")
                .key("command-bar-empty")
                .font(BODY)
                .color(k.c.text_muted)
                .px(12.0)
                .py(12.0),
        );
    }
    let mut previous = None;
    for (i, entry) in found.iter().enumerate() {
        if previous != Some(entry.group) {
            list = list.child(group_header(k, entry.group));
            previous = Some(entry.group);
        }
        list = list.child(result_row(k, i, entry, i == selected, found.len()));
    }
    list
}

pub fn overlay(s: &Chat, k: &rune_kit::Kit, layer: &mut Vec<El>) {
    if !s.command_bar.open {
        return;
    }
    let found = s.command_entries();
    let selected = s.command_selection(found.len());
    let placeholder = match s.command_bar.scope {
        Some(Group::Agents) => "Switch agent…",
        _ => "Type a command or search…",
    };
    let mut field = text_input(&s.command_bar.query, BODY)
        .key(FIELD)
        .grow(1.0)
        .min_w(0.0)
        .color(k.c.text_base)
        .placeholder(placeholder, k.c.text_faint)
        .label("Command bar")
        .active_descendant(row_key(selected))
        .on_edit(|s: &mut Chat| &mut s.command_bar.query)
        .on_input_change(|s: &mut Chat, _: &mut Tx| s.command_bar.selected = 0)
        .on_input_key(|s: &mut Chat, ev: &KeyEvent, tx: &mut Tx| key(s, ev, tx));
    if found.is_empty() {
        field = field.active_descendant("command-bar-empty");
    }
    let mut header = row()
        .key("command-bar-header")
        .w_full()
        .min_h(56.0)
        .px(16.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(icon(&icons::SEARCH, 16.0).color(k.c.icon_muted));
    if let Some(scope) = s.command_bar.scope {
        header = header.child(Badge::new(scope.label()).build(k).key("command-bar-scope"));
    }
    let header = header.child(field);
    let divider = el()
        .key("command-bar-divider")
        .w_full()
        .h(1.0)
        .fill(k.c.border_muted);
    let body = col().w_full().shrink().min_h(0.0).child(divider).child(
        col()
            .key("command-bar-scroll")
            .w_full()
            .max_h(360.0)
            .shrink()
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(result_list(k, &found, selected)),
    );
    let close = |s: &mut Chat| s.close_command_bar();
    let panel = modal_panel(k, "command-bar", "Command bar", 560.0, header, body)
        .shrink()
        .min_h(0.0)
        .modal(true, close)
        .modal_click_outside(true)
        .token_motion(k.motion.overlay);
    push_top_modal_layer(k, layer, "command-bar", panel, TOP_OFFSET);
}

const TOP_OFFSET: f32 = 96.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentInfo, AgentStatus};
    use crate::chat::{Application, Session};
    use crate::loading::Load;

    fn chat() -> Chat {
        Chat::new(crate::agent::mock_client(), crate::agent::mock_directory())
    }

    fn agent(id: &str, name: &str, status: AgentStatus) -> AgentInfo {
        AgentInfo {
            agent_id: id.into(),
            name: name.into(),
            description: None,
            status,
            model: None,
        }
    }

    fn session(title: &str, hidden: bool) -> Session {
        Session {
            id: Some(title.into()),
            hidden,
            title: title.into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        }
    }

    fn with_agent(s: &mut Chat) {
        s.agents = vec![agent("a1", "Shopping", AgentStatus::Published)];
        s.selected_agent = Some(s.agents[0].clone());
    }

    fn with_applications(s: &mut Chat, names: &[&str]) {
        s.applications = names
            .iter()
            .map(|n| {
                let mut a = Application::new(&format!("id-{n}"), "key");
                a.label = Some((*n).into());
                a
            })
            .collect();
        s.active = Some(0);
    }

    fn titles(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.title.as_str()).collect()
    }

    fn groups(entries: &[Entry]) -> Vec<Group> {
        let mut seen: Vec<Group> = Vec::new();
        for e in entries {
            if seen.last() != Some(&e.group) {
                seen.push(e.group);
            }
        }
        seen
    }

    #[test]
    fn a_match_scores_by_how_well_it_fits() {
        let exact = fuzzy_score("settings", "Settings").unwrap();
        let prefix = fuzzy_score("set", "Settings").unwrap();
        let word = fuzzy_score("side", "Toggle sidebar").unwrap();
        let inside = fuzzy_score("ett", "Settings").unwrap();
        let scattered = fuzzy_score("tgsb", "Toggle sidebar").unwrap();
        assert!(exact > prefix, "exact beats prefix");
        assert!(prefix > word, "prefix beats a later word start");
        assert!(word > inside, "a word start beats the middle of a word");
        assert!(inside > scattered, "a substring beats scattered letters");
    }

    #[test]
    fn letters_out_of_order_do_not_match() {
        assert_eq!(fuzzy_score("xyz", "Settings"), None);
        assert_eq!(fuzzy_score("gt", "Settings"), None, "no t follows the g");
    }

    #[test]
    fn matching_ignores_case_and_surrounding_space() {
        assert!(fuzzy_score("  SETTINGS ", "settings").is_some());
    }

    #[test]
    fn an_empty_query_lists_conversations_agents_applications_then_actions() {
        let mut s = chat();
        with_agent(&mut s);
        with_applications(&mut s, &["Prod", "Staging"]);
        s.sessions = vec![session("Pricing question", false)];
        let found = entries(&s, "");
        assert_eq!(
            groups(&found),
            [
                Group::Conversations,
                Group::Agents,
                Group::Applications,
                Group::Actions
            ]
        );
    }

    #[test]
    fn groups_without_entries_are_left_out() {
        let found = entries(&chat(), "");
        assert_eq!(groups(&found), [Group::Actions]);
    }

    #[test]
    fn only_published_agents_are_listed() {
        let mut s = chat();
        s.agents = vec![
            agent("a1", "Shopping", AgentStatus::Published),
            agent("a2", "Draft", AgentStatus::Draft),
        ];
        let found = entries(&s, "");
        let listed: Vec<&Entry> = found.iter().filter(|e| e.group == Group::Agents).collect();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Shopping");
        assert_eq!(listed[0].command, Command::Agent(0));
    }

    #[test]
    fn an_agent_keeps_its_index_in_the_loaded_list_when_a_draft_precedes_it() {
        let mut s = chat();
        s.agents = vec![
            agent("a1", "Draft", AgentStatus::Draft),
            agent("a2", "Support", AgentStatus::Published),
        ];
        let found = entries(&s, "support");
        assert_eq!(found[0].command, Command::Agent(1));
    }

    #[test]
    fn the_application_in_use_is_not_a_switch_target() {
        let mut s = chat();
        with_applications(&mut s, &["Prod", "Staging"]);
        let found = entries(&s, "");
        let listed: Vec<&Entry> = found
            .iter()
            .filter(|e| e.group == Group::Applications && e.command != Command::Mock)
            .collect();
        assert_eq!(titles(&[listed[0].clone()]), ["Staging"]);
        assert_eq!(listed[0].command, Command::Application(1));
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn an_application_shows_its_id_only_when_the_name_says_something_else() {
        let mut s = chat();
        with_applications(&mut s, &["Prod", "Staging"]);
        s.applications[1].label = None;
        s.applications[1].app_id = "STAGING01".into();
        let listed = entries(&s, "staging");
        assert_eq!(listed[0].title, "STAGING01");
        assert_eq!(listed[0].subtitle, None, "the name is the id already");
        s.applications[1].label = Some("Staging".into());
        assert_eq!(
            entries(&s, "staging")[0].subtitle.as_deref(),
            Some("STAGING01")
        );
    }

    #[test]
    fn mock_is_offered_in_debug_builds_only() {
        let mut s = chat();
        with_applications(&mut s, &["Prod"]);
        let offered = entries(&s, "mock")
            .iter()
            .any(|e| e.command == Command::Mock && e.title == "Mock");
        assert_eq!(offered, cfg!(debug_assertions));
    }

    #[test]
    fn mock_is_not_offered_while_already_on_it() {
        let mut s = chat();
        s.mock_forced = true;
        assert!(
            !entries(&s, "mock")
                .iter()
                .any(|e| e.command == Command::Mock)
        );
    }

    #[test]
    fn a_hidden_conversation_is_listed_and_marked() {
        let mut s = chat();
        with_agent(&mut s);
        s.sessions = vec![session("Open one", false), session("Tucked away", true)];
        let found = entries(&s, "tucked");
        assert_eq!(found[0].command, Command::Session(1));
        assert!(found[0].subtitle.as_deref().unwrap().contains("hidden"));
        let open = entries(&s, "open one");
        assert_eq!(open[0].subtitle, None);
    }

    #[test]
    fn the_empty_query_shows_only_the_recent_conversations() {
        let mut s = chat();
        with_agent(&mut s);
        s.sessions = (0..8)
            .map(|i| session(&format!("Chat {i}"), false))
            .collect();
        let listed = entries(&s, "")
            .into_iter()
            .filter(|e| e.group == Group::Conversations)
            .count();
        assert_eq!(listed, 5);
        let typed = entries(&s, "chat")
            .into_iter()
            .filter(|e| e.group == Group::Conversations)
            .count();
        assert_eq!(typed, 8, "typing searches them all");
    }

    #[test]
    fn conversations_wait_for_an_agent() {
        let mut s = chat();
        s.sessions = vec![session("Orphan", false)];
        assert!(entries(&s, "orphan").is_empty());
    }

    #[test]
    fn the_actions_cover_everything_the_settings_dial() {
        let found = entries(&chat(), "");
        assert_eq!(
            titles(&found),
            [
                "New conversation",
                "Toggle sidebar",
                "Settings",
                "Add application",
                "Theme: Auto",
                "Theme: Light",
                "Theme: Dark",
                "Accent colour: Blue",
                "Accent colour: Purple",
                "Accent colour: Pink",
                "Accent colour: Orange",
                "Accent colour: Green",
                "Accent colour: Graphite",
                "Chat width: Comfortable",
                "Chat width: Wide",
            ]
        );
    }

    #[test]
    fn an_action_shows_the_chord_the_keymap_gives_it() {
        let mut s = chat();
        let hint = |s: &Chat, title: &str| {
            entries(s, title)
                .into_iter()
                .find(|e| e.title == title)
                .unwrap()
                .hint
        };
        assert_eq!(hint(&s, "New conversation"), "⌘N");
        assert_eq!(hint(&s, "Toggle sidebar"), "⌘B");
        assert_eq!(hint(&s, "Settings"), "⌘,");
        assert_eq!(hint(&s, "Add application"), "", "no chord, no hint");
        s.keymap.set(Action::NewSession, None);
        assert_eq!(hint(&s, "New conversation"), "", "unbound, no hint");
    }

    #[test]
    fn typing_filters_by_title() {
        let mut s = chat();
        s.agents = vec![
            agent("a1", "Shopping", AgentStatus::Published),
            agent("a2", "Support", AgentStatus::Published),
        ];
        let found = entries(&s, "support");
        assert_eq!(titles(&found), ["Support"]);
    }

    #[test]
    fn a_better_match_outranks_an_earlier_group() {
        let mut s = chat();
        s.agents = vec![agent("a1", "Asset helper", AgentStatus::Published)];
        let found = entries(&s, "set");
        assert_eq!(found[0].title, "Settings", "a prefix beats a substring");
        assert!(titles(&found).contains(&"Asset helper"));
    }

    #[test]
    fn a_typed_query_keeps_each_group_together_best_group_first() {
        let mut s = chat();
        s.agents = vec![
            agent("a1", "Scene agent", AgentStatus::Published),
            agent("a2", "Support agent", AgentStatus::Published),
        ];
        let found = entries(&s, "se");
        let seen = groups(&found);
        let mut unique = seen.clone();
        unique.dedup();
        assert_eq!(seen.len(), 2, "one run per group, not {seen:?}");
        assert_eq!(seen[0], Group::Actions, "Settings is a prefix match");
        let agents: Vec<&str> = found
            .iter()
            .filter(|e| e.group == Group::Agents)
            .map(|e| e.title.as_str())
            .collect();
        assert_eq!(
            agents,
            ["Scene agent", "Support agent"],
            "ranked inside the group"
        );
    }

    #[test]
    fn equal_matches_keep_the_group_order() {
        let mut s = chat();
        s.agents = vec![agent("a1", "Settings", AgentStatus::Published)];
        let found = entries(&s, "settings");
        assert_eq!(found[0].group, Group::Agents);
        assert_eq!(found[1].group, Group::Actions);
    }

    #[test]
    fn scattered_letters_still_find_an_action() {
        let found = entries(&chat(), "tgsb");
        assert_eq!(found[0].title, "Toggle sidebar");
    }

    #[test]
    fn a_query_nothing_matches_lists_nothing() {
        assert!(entries(&chat(), "zzzzqqq").is_empty());
    }

    #[test]
    fn opening_starts_from_a_clean_field_with_the_first_row_selected() {
        let mut s = chat();
        s.command_bar.query = TextEdit::new("old search");
        s.command_bar.selected = 3;
        s.command_bar.scope = Some(Group::Actions);
        s.open_command_bar(None);
        assert!(s.command_bar.open);
        assert_eq!(s.command_bar.query.text, "");
        assert_eq!(s.command_bar.selected, 0);
        assert_eq!(s.command_bar.scope, None);
        assert!(s.command_bar.query.focused);
    }

    #[test]
    fn opening_with_a_scope_keeps_it() {
        let mut s = chat();
        s.open_command_bar(Some(Group::Agents));
        assert_eq!(s.command_bar.scope, Some(Group::Agents));
    }

    #[test]
    fn toggling_opens_then_closes() {
        let mut s = chat();
        s.toggle_command_bar();
        assert!(s.command_bar.open);
        s.toggle_command_bar();
        assert!(!s.command_bar.open);
    }

    #[test]
    fn moving_the_selection_wraps_both_ways() {
        let mut s = chat();
        s.open_command_bar(None);
        let last = entries(&s, "").len() - 1;
        s.move_command_selection(-1);
        assert_eq!(
            s.command_bar.selected, last,
            "up from the top wraps to the end"
        );
        s.move_command_selection(1);
        assert_eq!(
            s.command_bar.selected, 0,
            "down from the end wraps to the top"
        );
        s.move_command_selection(1);
        assert_eq!(s.command_bar.selected, 1);
    }

    #[test]
    fn moving_with_nothing_listed_stays_put() {
        let mut s = chat();
        s.open_command_bar(None);
        s.command_bar.query = TextEdit::new("zzzzqqq");
        s.move_command_selection(1);
        s.move_command_selection(-1);
        assert_eq!(s.command_bar.selected, 0);
    }

    #[test]
    fn running_the_selected_entry_runs_it_and_closes_the_bar() {
        let mut s = chat();
        s.open_command_bar(None);
        s.command_bar.query = TextEdit::new("theme: dark");
        s.run_selected_command();
        assert_eq!(s.appearance.theme, ThemePref::Dark);
        assert!(!s.command_bar.open);
    }

    #[test]
    fn running_with_nothing_listed_leaves_the_bar_open() {
        let mut s = chat();
        s.open_command_bar(None);
        s.command_bar.query = TextEdit::new("zzzzqqq");
        s.run_selected_command();
        assert!(s.command_bar.open);
    }

    #[test]
    fn the_nth_result_runs_by_number() {
        let mut s = chat();
        s.open_command_bar(None);
        s.run_command_at(2);
        assert!(s.settings_open, "the third action is Settings");
        assert!(!s.command_bar.open);
    }

    #[test]
    fn a_number_past_the_last_result_runs_nothing() {
        let mut s = chat();
        s.open_command_bar(None);
        s.command_bar.query = TextEdit::new("theme: dark");
        s.run_command_at(8);
        assert!(s.command_bar.open);
        assert_eq!(s.appearance.theme, ThemePref::Auto);
    }

    #[test]
    fn choosing_an_agent_switches_to_it() {
        let mut s = chat();
        s.agents = vec![
            agent("a1", "Shopping", AgentStatus::Published),
            agent("a2", "Support", AgentStatus::Published),
        ];
        s.open_command_bar(None);
        s.run_command(Command::Agent(1));
        assert_eq!(s.selected_agent.as_ref().unwrap().agent_id, "a2");
        assert!(!s.command_bar.open);
    }

    #[test]
    fn choosing_a_conversation_selects_it() {
        let mut s = chat();
        with_agent(&mut s);
        s.sessions = vec![session("First", false), session("Second", false)];
        s.run_command(Command::Session(1));
        assert_eq!(s.current, 1);
    }

    #[test]
    fn choosing_a_hidden_conversation_restores_it() {
        let mut s = chat();
        with_agent(&mut s);
        s.sessions = vec![session("First", false), session("Tucked away", true)];
        s.run_command(Command::Session(1));
        assert_eq!(s.current, 1);
        assert!(!s.sessions[1].hidden);
    }

    #[test]
    fn new_session_starts_one() {
        let mut s = chat();
        s.run_command(Command::NewSession);
        assert_eq!(s.sessions.len(), 2);
        assert_eq!(s.current, 1);
    }

    #[test]
    fn toggle_sidebar_flips_it() {
        let mut s = chat();
        let open = s.sidebar.is_open();
        s.run_command(Command::ToggleSidebar);
        assert_eq!(s.sidebar.is_open(), !open);
    }

    #[test]
    fn settings_opens_the_dialog() {
        let mut s = chat();
        s.run_command(Command::Settings);
        assert!(s.settings_open);
    }

    #[test]
    fn add_application_opens_its_dialog() {
        let mut s = chat();
        s.add_form.error = Some("stale".into());
        s.run_command(Command::AddApplication);
        assert!(s.add_form.open);
        assert_eq!(s.add_form.error, None);
    }

    #[test]
    fn appearance_entries_change_theme_accent_and_width() {
        let mut s = chat();
        s.run_command(Command::Theme(ThemePref::Light));
        s.run_command(Command::Accent(Accent::Purple));
        s.run_command(Command::Width(ChatWidth::Wide));
        assert_eq!(s.appearance.theme, ThemePref::Light);
        assert_eq!(s.appearance.accent, Accent::Purple);
        assert_eq!(s.appearance.width, ChatWidth::Wide);
    }

    #[test]
    fn choosing_an_application_switches_to_it() {
        let mut s = chat();
        with_applications(&mut s, &["Prod", "Staging"]);
        s.run_command(Command::Application(1));
        assert_eq!(s.active, Some(1));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn choosing_mock_leaves_the_applications() {
        let mut s = chat();
        with_applications(&mut s, &["Prod"]);
        s.run_command(Command::Mock);
        assert_eq!(s.active, None);
        assert!(s.mock_forced);
    }

    #[test]
    fn running_a_stale_index_does_nothing() {
        let mut s = chat();
        s.run_command(Command::Agent(9));
        s.run_command(Command::Session(9));
        s.run_command(Command::Application(9));
        assert!(s.selected_agent.is_none());
        assert_eq!(s.current, 0);
        assert_eq!(s.active, None);
    }

    #[test]
    fn a_scope_keeps_only_its_group() {
        let mut s = chat();
        s.agents = vec![agent("a1", "Shopping", AgentStatus::Published)];
        s.command_bar.scope = Some(Group::Agents);
        let found = entries(&s, "");
        assert_eq!(groups(&found), [Group::Agents]);
        let typed = entries(&s, "settings");
        assert!(typed.is_empty(), "actions are out of scope");
    }
}
