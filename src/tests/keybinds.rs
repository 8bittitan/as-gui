use std::cell::RefCell;
use std::rc::Rc;

use rune::{PhysicalInput, PhysicalKeyCode, Shortcut, ShortcutError};

use super::*;
use crate::keymap::{Action, Registration, ShortcutRegistrar};

fn chord(text: &str) -> Shortcut {
    text.parse().unwrap()
}

fn command(c: char) -> KeyEvent {
    KeyEvent {
        key: Key::Command(c),
        mods: Mods::default(),
    }
}

fn with_keys() -> Runtime<Chat> {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt
}

fn open_settings(rt: &mut Runtime<Chat>) {
    rt.update(|s, _| s.settings_open = true);
    settle(rt);
}

#[test]
fn remapping_an_action_moves_it_to_the_new_chord_in_the_real_key_handler() {
    let mut rt = with_keys();
    let open = |rt: &Runtime<Chat>| rt.state.sidebar.is_open();
    assert!(open(&rt));

    rt.update(|s, _| {
        s.rebind(Action::ToggleSidebar, Some(&chord("cmd+j")))
            .unwrap();
    });
    rt.key(command('b'));
    rt.frame(0.0);
    assert!(open(&rt), "the old chord does nothing");
    rt.key(command('j'));
    rt.frame(0.0);
    assert!(!open(&rt), "the new chord toggles the sidebar");
}

#[test]
fn an_unbound_action_cannot_be_triggered() {
    let mut rt = with_keys();
    rt.update(|s, _| {
        s.rebind(Action::CommandBar, None).unwrap();
    });
    rt.key(command('k'));
    rt.frame(0.0);
    assert!(!rt.state.command_bar.open);
}

#[test]
fn a_duplicate_chord_is_refused_and_the_old_binding_stays() {
    let mut rt = with_keys();
    let result = rt.state.rebind(Action::Settings, Some(&chord("cmd+n")));
    assert_eq!(result, Err(ShortcutError::Conflict));
    assert_eq!(
        rt.state.keybind_note.as_deref(),
        Some("Already used by New conversation")
    );
    assert!(rt.state.keymap.is_default(Action::Settings));
}

#[test]
fn a_reserved_chord_is_refused() {
    let mut rt = with_keys();
    assert!(
        rt.state
            .rebind(Action::Settings, Some(&chord("cmd+q")))
            .is_err()
    );
    assert!(rt.state.keymap.is_default(Action::Settings));
}

#[test]
fn rebinding_saves_only_the_change_and_a_restart_restores_it() {
    let path = std::env::temp_dir().join("as-gui-keybinds-save/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.persist_path = Some(path.clone());
        s.rebind(Action::NewSession, Some(&chord("cmd+shift+n")))
            .unwrap();
    });
    let saved = crate::store::load_from(&path);
    assert_eq!(saved.keybinds.len(), 1);
    assert_eq!(
        saved.keybinds["new-session"].as_deref(),
        Some("shift+super+n")
    );

    let mut restarted = test_chat();
    restarted.update(|s, _| s.restore(&saved));
    assert_eq!(
        restarted.state.keymap.get(Action::NewSession),
        Some(&chord("cmd+shift+n"))
    );
    assert!(
        restarted.state.recorders[Action::NewSession.index()]
            .shortcut()
            .is_some_and(|s| s == &chord("cmd+shift+n"))
    );
}

#[test]
fn the_hints_follow_a_remap() {
    let mut rt = with_keys();
    let shows = |rt: &Runtime<Chat>, text: &str| {
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some(text))
    };
    assert!(
        shows(&rt, "⌘,"),
        "the settings footer hint starts at the default"
    );
    rt.update(|s, _| {
        s.rebind(Action::Settings, Some(&chord("cmd+j"))).unwrap();
    });
    rt.frame(0.0);
    assert!(shows(&rt, "⌘J"));
    assert!(!shows(&rt, "⌘,"));
}

#[test]
fn the_settings_dialog_lists_every_action_with_its_recorder() {
    let mut rt = with_keys();
    open_settings(&mut rt);
    for action in Action::ALL {
        assert!(
            rt.find(&format!("keybind-{}", action.id())).is_some(),
            "a recorder for {action:?}"
        );
    }
    assert!(
        rt.find("keybind-reset-new-session").is_none(),
        "no reset at the default"
    );
}

#[test]
fn a_changed_row_offers_a_reset_and_restore_defaults_resets_everything() {
    let mut rt = test_chat_sized((720.0, 1600.0));
    rt.on_key = Some(on_key);
    rt.update(|s, _| {
        s.rebind(Action::NewSession, Some(&chord("cmd+j"))).unwrap();
        s.rebind(Action::Settings, None).unwrap();
    });
    open_settings(&mut rt);
    assert!(rt.find("keybind-reset-new-session").is_some());

    tap(&mut rt, "keybind-reset-new-session");
    rt.frame(0.0);
    assert!(rt.state.keymap.is_default(Action::NewSession));
    assert!(!rt.state.keymap.is_default(Action::Settings));

    tap(&mut rt, "keybinds-restore");
    rt.frame(0.0);
    assert_eq!(rt.state.keymap, crate::keymap::Keymap::default());
    assert!(
        rt.state.recorders[Action::Settings.index()]
            .shortcut()
            .is_some()
    );
}

#[test]
fn recording_a_chord_captures_it_instead_of_running_the_action() {
    let mut rt = test_chat_sized((720.0, 1900.0));
    rt.on_key = Some(on_key);
    open_settings(&mut rt);
    tap(&mut rt, "keybind-settings");
    rt.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyB,
        mods: Mods {
            cmd: true,
            logo: true,
            ..Mods::default()
        },
        pressed: true,
        repeat: false,
    });
    rt.frame(0.0);
    assert!(
        rt.state.sidebar.is_open(),
        "⌘B didn't toggle the sidebar while recording"
    );
    assert!(
        rt.state.keymap.is_default(Action::Settings),
        "and it was refused: the sidebar owns ⌘B"
    );
    assert_eq!(
        rt.state.keybind_note.as_deref(),
        Some("Already used by Toggle sidebar")
    );
}

#[derive(Default)]
struct Fake {
    taken: RefCell<Vec<(Action, Shortcut)>>,
    log: RefCell<Vec<String>>,
    refuse: RefCell<Vec<Shortcut>>,
    owner: RefCell<Option<String>>,
}

struct FakeRegistrar(Rc<Fake>);

impl ShortcutRegistrar for FakeRegistrar {
    fn register(&self, action: Action, s: &Shortcut) -> Result<Registration, ShortcutError> {
        if self.0.refuse.borrow().contains(s) {
            self.0.log.borrow_mut().push(format!("refused {s}"));
            return Err(ShortcutError::Conflict);
        }
        self.0.taken.borrow_mut().push((action, s.clone()));
        self.0.log.borrow_mut().push(format!("register {s}"));
        Ok(Registration(self.0.taken.borrow().len() as u32))
    }
    fn unregister(&self, _: Registration) -> Result<(), ShortcutError> {
        self.0.log.borrow_mut().push("unregister".into());
        Ok(())
    }
    fn owner(&self, _: &Shortcut) -> Option<String> {
        self.0.owner.borrow().clone()
    }
}

fn with_registrar(fake: &Rc<Fake>) -> Runtime<Chat> {
    let mut rt = with_keys();
    let registrar = Rc::new(FakeRegistrar(fake.clone()));
    rt.update(move |s, _| s.attach_registrar(registrar));
    rt
}

#[test]
fn the_system_wide_chords_register_when_the_registrar_attaches() {
    let fake = Rc::new(Fake::default());
    let rt = with_registrar(&fake);
    assert_eq!(
        *fake.taken.borrow(),
        vec![
            (Action::QuickAsk, chord("cmd+shift+space")),
            (Action::AskSelection, chord("cmd+shift+a")),
        ]
    );
    assert!(rt.state.shortcut_warning(Action::QuickAsk).is_none());
    assert!(rt.state.shortcut_warning(Action::AskSelection).is_none());
}

#[test]
fn changing_ask_about_selection_registers_its_new_chord_for_it() {
    let fake = Rc::new(Fake::default());
    let mut rt = with_registrar(&fake);
    fake.log.borrow_mut().clear();
    rt.update(|s, _| {
        s.rebind(Action::AskSelection, Some(&chord("cmd+shift+e")))
            .unwrap();
    });
    assert_eq!(
        fake.taken.borrow().last(),
        Some(&(Action::AskSelection, chord("cmd+shift+e")))
    );
    assert_eq!(
        *fake.log.borrow(),
        vec![
            "register shift+super+e".to_string(),
            "unregister".to_string()
        ],
        "new before old, and Quick Ask's chord untouched"
    );
}

#[test]
fn ask_about_selection_cannot_take_quick_asks_chord() {
    let fake = Rc::new(Fake::default());
    let mut rt = with_registrar(&fake);
    fake.log.borrow_mut().clear();
    let result = rt
        .state
        .rebind(Action::AskSelection, Some(&chord("cmd+shift+space")));
    assert_eq!(result, Err(ShortcutError::Conflict));
    assert_eq!(
        rt.state.keybind_note.as_deref(),
        Some("Already used by Quick Ask")
    );
    assert!(fake.log.borrow().is_empty(), "the system is never asked");
}

#[test]
fn a_launch_failure_for_ask_about_selection_warns_on_its_own_row() {
    let fake = Rc::new(Fake::default());
    fake.refuse.borrow_mut().push(chord("cmd+shift+a"));
    *fake.owner.borrow_mut() = Some("Chrome".into());
    let mut rt = with_registrar(&fake);
    assert!(
        rt.state
            .shortcut_warning(Action::AskSelection)
            .is_some_and(|w| w.contains("Chrome"))
    );
    assert!(rt.state.shortcut_warning(Action::QuickAsk).is_none());
    assert!(rt.state.keybinds_customised());

    open_settings(&mut rt);
    assert!(rt.find("keybind-ask-selection-warning").is_some());
    assert!(rt.find("keybind-quick-ask-warning").is_none());

    rt.update(|s, _| {
        s.rebind(Action::AskSelection, Some(&chord("cmd+shift+e")))
            .unwrap();
    });
    assert!(rt.state.shortcut_warning(Action::AskSelection).is_none());
}

#[test]
fn changing_quick_ask_registers_the_new_chord_before_releasing_the_old_one() {
    let fake = Rc::new(Fake::default());
    let mut rt = with_registrar(&fake);
    fake.log.borrow_mut().clear();
    rt.update(|s, _| {
        s.rebind(Action::QuickAsk, Some(&chord("cmd+shift+j")))
            .unwrap();
    });
    assert_eq!(
        *fake.log.borrow(),
        vec![
            "register shift+super+j".to_string(),
            "unregister".to_string()
        ]
    );
    assert_eq!(
        rt.state.keymap.get(Action::QuickAsk),
        Some(&chord("cmd+shift+j"))
    );
}

#[test]
fn a_refused_quick_ask_change_keeps_the_old_chord_and_names_the_owner() {
    let fake = Rc::new(Fake::default());
    let mut rt = with_registrar(&fake);
    fake.refuse.borrow_mut().push(chord("cmd+space"));
    *fake.owner.borrow_mut() = Some("Spotlight".into());
    fake.log.borrow_mut().clear();

    let result = rt.state.rebind(Action::QuickAsk, Some(&chord("cmd+space")));
    assert_eq!(result, Err(ShortcutError::Conflict));
    assert!(
        rt.state.keymap.is_default(Action::QuickAsk),
        "the old chord stays"
    );
    assert!(!fake.log.borrow().contains(&"unregister".to_string()));
    assert!(
        rt.state
            .keybind_note
            .as_deref()
            .is_some_and(|n| n.contains("Spotlight"))
    );
}

#[test]
fn a_launch_failure_warns_in_the_keyboard_section_and_retries_on_a_new_chord() {
    let fake = Rc::new(Fake::default());
    fake.refuse.borrow_mut().push(chord("cmd+shift+space"));
    *fake.owner.borrow_mut() = Some("Raycast".into());
    let mut rt = with_registrar(&fake);
    assert!(
        rt.state
            .shortcut_warning(Action::QuickAsk)
            .is_some_and(|w| w.contains("Raycast"))
    );

    open_settings(&mut rt);
    assert!(
        rt.find("keybind-quick-ask-warning").is_some(),
        "the warning shows in the section"
    );

    rt.update(|s, _| {
        s.rebind(Action::QuickAsk, Some(&chord("cmd+shift+j")))
            .unwrap();
    });
    assert!(
        rt.state.shortcut_warning(Action::QuickAsk).is_none(),
        "a working chord clears it"
    );
}

#[test]
fn unbinding_quick_ask_releases_its_chord_and_clears_the_warning() {
    let fake = Rc::new(Fake::default());
    fake.refuse.borrow_mut().push(chord("cmd+shift+space"));
    let mut rt = with_registrar(&fake);
    assert!(rt.state.shortcut_warning(Action::QuickAsk).is_some());
    rt.update(|s, _| {
        s.rebind(Action::QuickAsk, None).unwrap();
    });
    assert!(rt.state.shortcut_warning(Action::QuickAsk).is_none());
    assert_eq!(rt.state.keymap.get(Action::QuickAsk), None);
}

#[test]
fn the_dialog_stays_inside_a_short_window_and_fits_a_tall_one() {
    let mut short = test_chat_sized((720.0, 360.0));
    open_settings(&mut short);
    let (rect, _) = short.presented("settings-dialog").expect("the dialog");
    assert!(
        rect[1] >= 0.0 && rect[1] + rect[3] <= 360.0,
        "inside the window: {rect:?}"
    );

    let mut tall = test_chat_sized((720.0, 1600.0));
    open_settings(&mut tall);
    let (dialog, _) = tall.presented("settings-dialog").unwrap();
    let (restore, _) = tall
        .presented("keybinds-restore")
        .expect("the last control");
    assert!(
        dialog[3] < 1600.0 - 32.0,
        "it sizes to its content: {dialog:?}"
    );
    assert!(
        restore[1] + restore[3] <= dialog[1] + dialog[3],
        "everything is visible without scrolling"
    );
}
