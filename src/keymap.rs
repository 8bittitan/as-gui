//! The app's rebindable shortcuts: the actions, their defaults, the user's changes, and the
//! rules a new binding must pass.

use std::collections::BTreeMap;

use rune::{Key, KeyEvent, PhysicalKeyCode, Shortcut, ShortcutError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    ToggleSidebar,
    CommandBar,
    Settings,
    NewSession,
    QuickAsk,
    AskSelection,
}

impl Action {
    pub const COUNT: usize = 6;

    pub const ALL: [Action; Action::COUNT] = [
        Action::ToggleSidebar,
        Action::CommandBar,
        Action::Settings,
        Action::NewSession,
        Action::QuickAsk,
        Action::AskSelection,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Registered with the system, so it fires whichever app is in front.
    pub fn is_global(self) -> bool {
        matches!(self, Action::QuickAsk | Action::AskSelection)
    }

    /// The name the action is saved under.
    pub fn id(self) -> &'static str {
        match self {
            Action::ToggleSidebar => "toggle-sidebar",
            Action::CommandBar => "command-bar",
            Action::Settings => "settings",
            Action::NewSession => "new-session",
            Action::QuickAsk => "quick-ask",
            Action::AskSelection => "ask-selection",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::ToggleSidebar => "Toggle sidebar",
            Action::CommandBar => "Command bar",
            Action::Settings => "Settings",
            Action::NewSession => "New conversation",
            Action::QuickAsk => "Quick Ask",
            Action::AskSelection => "Ask about selection",
        }
    }

    /// Also reads the id the command bar's predecessor, the agent switcher, was saved under.
    fn from_id(id: &str) -> Option<Action> {
        if id == "agent-switcher" {
            return Some(Action::CommandBar);
        }
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    fn default_chord(self) -> &'static str {
        match self {
            Action::ToggleSidebar => "cmd+b",
            Action::CommandBar => "cmd+k",
            Action::Settings => "cmd+comma",
            Action::NewSession => "cmd+n",
            Action::QuickAsk => "cmd+shift+space",
            Action::AskSelection => "cmd+shift+a",
        }
    }
}

/// Why a chord can't be bound to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The system or text editing owns it.
    Reserved,
    UsedBy(Action),
}

impl Refusal {
    pub fn message(self) -> String {
        match self {
            Refusal::Reserved => "That shortcut is reserved by macOS or text editing".into(),
            Refusal::UsedBy(action) => format!("Already used by {}", action.label()),
        }
    }
}

const RESERVED: &[&str] = &[
    "cmd+q",
    "cmd+w",
    "cmd+h",
    "cmd+m",
    "cmd+c",
    "cmd+v",
    "cmd+x",
    "cmd+a",
    "cmd+z",
    "cmd+shift+z",
    #[cfg(feature = "knobs")]
    "cmd+shift+k",
];

/// A held global registration, as the registrar names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registration(pub u32);

/// What registers the system-wide chords (Quick Ask, Ask about selection) with the system, so tests can stand in for it.
pub trait ShortcutRegistrar {
    fn register(&self, action: Action, shortcut: &Shortcut) -> Result<Registration, ShortcutError>;
    fn unregister(&self, registration: Registration) -> Result<(), ShortcutError>;
    /// Who holds a chord that couldn't be registered, when the system says.
    fn owner(&self, shortcut: &Shortcut) -> Option<String>;
}

/// Each action's current chord, or none when the user unbound it.
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    bindings: [Option<Shortcut>; Action::COUNT],
}

fn default_shortcut(action: Action) -> Shortcut {
    action
        .default_chord()
        .parse()
        .expect("the default chords parse")
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap {
            bindings: Action::ALL.map(|a| Some(default_shortcut(a))),
        }
    }
}

impl Keymap {
    pub fn get(&self, action: Action) -> Option<&Shortcut> {
        self.bindings[action.index()].as_ref()
    }

    pub fn set(&mut self, action: Action, shortcut: Option<Shortcut>) {
        self.bindings[action.index()] = shortcut;
    }

    pub fn is_default(&self, action: Action) -> bool {
        self.get(action) == Some(&default_shortcut(action))
    }

    /// Whether `shortcut` may be bound to `action`.
    pub fn check(&self, action: Action, shortcut: &Shortcut) -> Result<(), Refusal> {
        if RESERVED
            .iter()
            .any(|r| r.parse::<Shortcut>().is_ok_and(|r| &r == shortcut))
        {
            return Err(Refusal::Reserved);
        }
        match Action::ALL
            .into_iter()
            .find(|&other| other != action && self.get(other) == Some(shortcut))
        {
            Some(other) => Err(Refusal::UsedBy(other)),
            None => Ok(()),
        }
    }

    /// The action `ev` triggers: by the physical key when the event has one, else by the
    /// character it typed (scripted keys and tests have no physical origin).
    pub fn action_for(&self, ev: &KeyEvent, physical: Option<PhysicalKeyCode>) -> Option<Action> {
        Action::ALL.into_iter().find(|&action| {
            self.get(action)
                .is_some_and(|shortcut| matches(shortcut, ev, physical))
        })
    }

    /// Only the bindings that differ from the defaults, so a later change of default reaches
    /// everyone who never customised it. An unbound action is saved as `null`.
    pub fn to_saved(&self) -> BTreeMap<String, Option<String>> {
        Action::ALL
            .into_iter()
            .filter(|&a| !self.is_default(a))
            .map(|a| (a.id().to_string(), self.get(a).map(|s| s.to_string())))
            .collect()
    }

    /// Entries that name no action or hold no readable chord keep their default.
    pub fn load(saved: &BTreeMap<String, Option<String>>) -> Keymap {
        let mut keymap = Keymap::default();
        for (id, chord) in saved {
            let Some(action) = Action::from_id(id) else {
                continue;
            };
            match chord {
                None => keymap.set(action, None),
                Some(text) => {
                    if let Ok(shortcut) = text.parse::<Shortcut>() {
                        keymap.set(action, Some(shortcut));
                    }
                }
            }
        }
        keymap
    }

    /// The chord as a hint reads it ("⌘B"), empty when unbound.
    pub fn hint(&self, action: Action) -> String {
        let separator = if cfg!(target_os = "macos") { "" } else { "+" };
        self.get(action)
            .map(|s| s.keycaps().join(separator))
            .unwrap_or_default()
    }
}

fn matches(shortcut: &Shortcut, ev: &KeyEvent, physical: Option<PhysicalKeyCode>) -> bool {
    let want = shortcut.modifiers();
    let have = ev.mods;
    // `Key::Command` is already the primary modifier plus a key, whatever `mods` says.
    let have_cmd = have.cmd || matches!(ev.key, Key::Command(_));
    if want.shift != have.shift
        || want.alt != have.alt
        || want.ctrl != have.ctrl
        || want.cmd != have_cmd
    {
        return false;
    }
    if let Some(physical) = physical {
        return physical == shortcut.physical_key();
    }
    let name = shortcut.to_string();
    let name = name.rsplit('+').next().unwrap_or_default();
    let typed = match &ev.key {
        Key::Command(c) => c.to_ascii_lowercase().to_string(),
        Key::Text(t) => t.to_lowercase(),
        _ => return false,
    };
    match name {
        "comma" => typed == ",",
        "period" => typed == ".",
        "slash" => typed == "/",
        "space" => typed == " ",
        single => typed == single,
    }
}

#[cfg(test)]
mod tests {
    use rune::{Key, KeyEvent, Mods, PhysicalKeyCode, Shortcut};

    use super::*;

    fn chord(text: &str) -> Shortcut {
        text.parse().unwrap()
    }

    fn command(c: char) -> KeyEvent {
        KeyEvent {
            key: Key::Command(c),
            mods: Mods {
                cmd: true,
                logo: true,
                ..Mods::default()
            },
        }
    }

    #[test]
    fn the_defaults_are_the_chords_the_app_shipped_with() {
        let keymap = Keymap::default();
        let expected = [
            (Action::ToggleSidebar, "cmd+b"),
            (Action::CommandBar, "cmd+k"),
            (Action::Settings, "cmd+comma"),
            (Action::NewSession, "cmd+n"),
            (Action::QuickAsk, "cmd+shift+space"),
            (Action::AskSelection, "cmd+shift+a"),
        ];
        for (action, text) in expected {
            assert_eq!(keymap.get(action), Some(&chord(text)), "{action:?}");
            assert!(keymap.is_default(action));
        }
    }

    #[test]
    fn a_key_event_resolves_to_the_action_bound_to_it() {
        let keymap = Keymap::default();
        assert_eq!(
            keymap.action_for(&command('b'), None),
            Some(Action::ToggleSidebar)
        );
        assert_eq!(
            keymap.action_for(&command('k'), None),
            Some(Action::CommandBar)
        );
        assert_eq!(
            keymap.action_for(&command(','), None),
            Some(Action::Settings)
        );
        assert_eq!(
            keymap.action_for(&command('n'), None),
            Some(Action::NewSession)
        );
        assert_eq!(keymap.action_for(&command('j'), None), None);
    }

    #[test]
    fn the_physical_key_wins_over_the_typed_character() {
        let keymap = Keymap::default();
        assert_eq!(
            keymap.action_for(&command('x'), Some(PhysicalKeyCode::KeyB)),
            Some(Action::ToggleSidebar),
            "⌘B on a layout where that position types x"
        );
        assert_eq!(
            keymap.action_for(&command('b'), Some(PhysicalKeyCode::KeyX)),
            None
        );
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let keymap = Keymap::default();
        let mut with_shift = command('b');
        with_shift.mods.shift = true;
        assert_eq!(keymap.action_for(&with_shift, None), None);
        let plain = KeyEvent {
            key: Key::Text("b".into()),
            mods: Mods::default(),
        };
        assert_eq!(keymap.action_for(&plain, None), None);
    }

    #[test]
    fn a_remapped_action_answers_to_its_new_chord_only() {
        let mut keymap = Keymap::default();
        keymap.set(Action::ToggleSidebar, Some(chord("cmd+j")));
        assert_eq!(
            keymap.action_for(&command('j'), None),
            Some(Action::ToggleSidebar)
        );
        assert_eq!(keymap.action_for(&command('b'), None), None);
        assert!(!keymap.is_default(Action::ToggleSidebar));
    }

    #[test]
    fn an_unbound_action_fires_for_nothing() {
        let mut keymap = Keymap::default();
        keymap.set(Action::NewSession, None);
        assert_eq!(keymap.action_for(&command('n'), None), None);
        assert_eq!(keymap.get(Action::NewSession), None);
        assert!(!keymap.is_default(Action::NewSession));
    }

    #[test]
    fn a_chord_another_action_holds_is_refused_naming_it() {
        let keymap = Keymap::default();
        assert_eq!(
            keymap.check(Action::Settings, &chord("cmd+n")),
            Err(Refusal::UsedBy(Action::NewSession))
        );
        assert_eq!(
            keymap.check(Action::NewSession, &chord("cmd+n")),
            Ok(()),
            "its own chord"
        );
        assert_eq!(
            Refusal::UsedBy(Action::NewSession).message(),
            "Already used by New conversation"
        );
    }

    #[test]
    fn chords_the_system_or_text_editing_owns_are_refused() {
        let keymap = Keymap::default();
        for reserved in [
            "cmd+q",
            "cmd+w",
            "cmd+h",
            "cmd+m",
            "cmd+c",
            "cmd+v",
            "cmd+x",
            "cmd+a",
            "cmd+z",
            "cmd+shift+z",
        ] {
            assert_eq!(
                keymap.check(Action::Settings, &chord(reserved)),
                Err(Refusal::Reserved),
                "{reserved}"
            );
        }
        assert_eq!(keymap.check(Action::Settings, &chord("cmd+j")), Ok(()));
    }

    #[test]
    fn only_changes_are_saved_and_they_load_back() {
        let mut keymap = Keymap::default();
        assert!(keymap.to_saved().is_empty(), "defaults save nothing");

        keymap.set(Action::ToggleSidebar, Some(chord("cmd+j")));
        keymap.set(Action::NewSession, None);
        let saved = keymap.to_saved();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved["toggle-sidebar"].as_deref(), Some("super+j"));
        assert_eq!(saved["new-session"], None, "unbound is kept, as null");
        assert_eq!(Keymap::load(&saved), keymap);
    }

    #[test]
    fn unreadable_saved_entries_fall_back_to_the_default() {
        let mut saved = std::collections::BTreeMap::new();
        saved.insert(
            "toggle-sidebar".to_string(),
            Some("not a chord".to_string()),
        );
        saved.insert("no-such-action".to_string(), Some("cmd+j".to_string()));
        assert_eq!(Keymap::load(&saved), Keymap::default());
    }

    #[test]
    fn a_saved_agent_switcher_chord_applies_to_the_command_bar() {
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("agent-switcher".to_string(), Some("cmd+j".to_string()));
        let keymap = Keymap::load(&saved);
        assert_eq!(keymap.get(Action::CommandBar), Some(&chord("cmd+j")));
        assert_eq!(keymap.action_for(&command('k'), None), None);
    }

    #[test]
    fn a_saved_agent_switcher_unbinding_unbinds_the_command_bar() {
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("agent-switcher".to_string(), None);
        assert_eq!(Keymap::load(&saved).get(Action::CommandBar), None);
    }

    #[test]
    fn a_command_bar_entry_wins_over_the_old_agent_switcher_one() {
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("agent-switcher".to_string(), Some("cmd+j".to_string()));
        saved.insert("command-bar".to_string(), Some("cmd+u".to_string()));
        assert_eq!(
            Keymap::load(&saved).get(Action::CommandBar),
            Some(&chord("cmd+u"))
        );
    }

    #[test]
    fn the_command_bar_saves_under_its_own_id() {
        let mut keymap = Keymap::default();
        keymap.set(Action::CommandBar, Some(chord("cmd+j")));
        let saved = keymap.to_saved();
        assert_eq!(saved["command-bar"].as_deref(), Some("super+j"));
        assert!(!saved.contains_key("agent-switcher"));
        assert_eq!(Action::CommandBar.label(), "Command bar");
    }

    #[test]
    fn ask_about_selection_is_refused_a_chord_any_other_action_holds() {
        let keymap = Keymap::default();
        for other in Action::ALL
            .into_iter()
            .filter(|&a| a != Action::AskSelection)
        {
            let held = keymap.get(other).unwrap().clone();
            assert_eq!(
                keymap.check(Action::AskSelection, &held),
                Err(Refusal::UsedBy(other))
            );
        }
        assert_eq!(
            keymap.check(Action::QuickAsk, &chord("cmd+shift+a")),
            Err(Refusal::UsedBy(Action::AskSelection))
        );
        assert_eq!(
            Refusal::UsedBy(Action::AskSelection).message(),
            "Already used by Ask about selection"
        );
    }

    #[test]
    fn ask_about_selection_saves_under_its_own_id_and_loads_back() {
        let mut keymap = Keymap::default();
        keymap.set(Action::AskSelection, Some(chord("cmd+shift+e")));
        let saved = keymap.to_saved();
        assert_eq!(saved["ask-selection"].as_deref(), Some("shift+super+e"));
        assert_eq!(Keymap::load(&saved), keymap);
    }

    #[test]
    fn only_quick_ask_and_ask_about_selection_are_system_wide() {
        let global: Vec<Action> = Action::ALL.into_iter().filter(|a| a.is_global()).collect();
        assert_eq!(global, [Action::QuickAsk, Action::AskSelection]);
    }

    #[test]
    fn hints_read_the_current_chord_and_are_empty_when_unbound() {
        let mut keymap = Keymap::default();
        assert_eq!(keymap.hint(Action::ToggleSidebar), "⌘B");
        assert_eq!(keymap.hint(Action::Settings), "⌘,");
        keymap.set(Action::ToggleSidebar, Some(chord("cmd+shift+j")));
        assert_eq!(keymap.hint(Action::ToggleSidebar), "⇧⌘J");
        keymap.set(Action::ToggleSidebar, None);
        assert_eq!(keymap.hint(Action::ToggleSidebar), "");
    }
}
