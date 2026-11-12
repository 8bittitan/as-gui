//! The macOS menu bar app: as-gui launches to a status item, its menu opens Quick Ask, the main
//! window and settings, and the Dock icon shows only while the main window does. The decisions
//! are pure (`launch_mode`, `menu_items`); the OS sits behind `AppShell` and `StatusItem`.

#[cfg(target_os = "macos")]
pub mod mac;

use std::rc::Rc;

use rune::{AppHandle, Shortcut};

use crate::chat::Chat;
use crate::keymap::Action;
use crate::quick_ask::{self, QuickAsk};

/// What the process starts as, decided once before the event loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchMode {
    pub show_window: bool,
    pub status_item: bool,
}

/// What `launch_mode` reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaunchInputs {
    pub macos: bool,
    pub headless: bool,
    pub demo: bool,
    pub needs_credentials: bool,
    pub preset_agent: bool,
    pub open_at_launch: bool,
    /// `AS_GUI_DEMO_MENU_BAR=1`: the recording draws the menu bar itself.
    pub menu_bar_demo: bool,
}

pub fn launch_mode(inputs: LaunchInputs) -> LaunchMode {
    let status_item = inputs.macos && !inputs.headless && !inputs.demo;
    LaunchMode {
        show_window: !inputs.menu_bar_demo
            && (!status_item
                || inputs.needs_credentials
                || inputs.preset_agent
                || inputs.open_at_launch),
        status_item,
    }
}

/// What a menu item does; its `id` is what the native menu hands back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    QuickAsk,
    OpenWindow,
    NewSession,
    Settings,
    Quit,
}

impl MenuAction {
    pub const ALL: [MenuAction; 5] = [
        MenuAction::QuickAsk,
        MenuAction::OpenWindow,
        MenuAction::NewSession,
        MenuAction::Settings,
        MenuAction::Quit,
    ];

    pub fn id(self) -> &'static str {
        match self {
            MenuAction::QuickAsk => "quick-ask",
            MenuAction::OpenWindow => "open-window",
            MenuAction::NewSession => "new-session",
            MenuAction::Settings => "settings",
            MenuAction::Quit => "quit",
        }
    }

    pub fn from_id(id: &str) -> Option<MenuAction> {
        MenuAction::ALL.into_iter().find(|a| a.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Action {
        action: MenuAction,
        label: &'static str,
        shortcut: Option<Shortcut>,
        enabled: bool,
    },
    Info(String),
    Separator,
}

pub fn menu_items(s: &Chat) -> Vec<MenuEntry> {
    let bound = |action: Action| s.keymap.get(action).cloned();
    let item = |action, label, shortcut, enabled| MenuEntry::Action {
        action,
        label,
        shortcut,
        enabled,
    };
    vec![
        item(
            MenuAction::QuickAsk,
            "Quick Ask",
            bound(Action::QuickAsk),
            s.selected_agent.is_some(),
        ),
        item(MenuAction::OpenWindow, "Open as-gui", None, true),
        item(
            MenuAction::NewSession,
            "New conversation",
            bound(Action::NewSession),
            true,
        ),
        item(
            MenuAction::Settings,
            "Settings…",
            bound(Action::Settings),
            true,
        ),
        MenuEntry::Separator,
        MenuEntry::Info(info_line(s)),
        MenuEntry::Separator,
        item(MenuAction::Quit, "Quit as-gui", "cmd+q".parse().ok(), true),
    ]
}

fn info_line(s: &Chat) -> String {
    let agent = s
        .selected_agent
        .as_ref()
        .map_or("No agent selected", |a| a.name.as_str());
    match s.application_label() {
        Some(application) => format!("{agent} · {application}"),
        None => agent.to_string(),
    }
}

/// The app's windows and process, as the menu drives them; tests stand in for it.
pub trait AppShell {
    fn open_window(&self);
    fn set_dock_icon(&self, visible: bool);
    fn toggle_quick_ask(&self);
    fn quit(&self);
}

/// The real shell: Rune's handles to the main window and the Quick Ask panel.
pub struct RuneShell {
    pub main: AppHandle<Chat>,
    pub panel: AppHandle<QuickAsk>,
}

impl AppShell for RuneShell {
    fn open_window(&self) {
        self.main.open();
    }
    fn set_dock_icon(&self, visible: bool) {
        self.main.set_dock_icon(visible);
    }
    fn toggle_quick_ask(&self) {
        self.panel.update(quick_ask::toggled);
    }
    fn quit(&self) {
        self.main.quit();
    }
}

/// Puts as-gui in the menu bar (main thread, from an `update`). The window open at launch
/// gets its Dock icon; without a status item the window is the only way in, so it opens.
pub fn attach_status_item(chat: &mut Chat, target: AppHandle<Chat>, window_shown: bool) {
    #[cfg(target_os = "macos")]
    match mac::MacStatusItem::new(target) {
        Ok(item) => {
            chat.attach_status_item(Box::new(item));
            if window_shown {
                chat.open_window();
            }
            return;
        }
        Err(error) => tracing::warn!(%error, "no menu bar icon"),
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (target, window_shown);
    if let Some(shell) = &chat.shell {
        shell.set_dock_icon(true);
        shell.open_window();
    }
}

/// The icon in the menu bar and its menu.
pub trait StatusItem {
    fn set_menu(&self, entries: &[MenuEntry]);
}

impl Chat {
    pub fn attach_shell(&mut self, shell: Rc<dyn AppShell>) {
        self.shell = Some(shell);
    }

    pub fn attach_status_item(&mut self, item: Box<dyn StatusItem>) {
        self.status_item = Some(item);
        self.refresh_menu();
    }

    pub(crate) fn refresh_menu(&self) {
        if let Some(item) = &self.status_item {
            item.set_menu(&menu_items(self));
        }
    }

    pub fn run_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::QuickAsk => {
                if let Some(shell) = &self.shell {
                    shell.toggle_quick_ask();
                }
            }
            MenuAction::OpenWindow => self.open_window(),
            MenuAction::NewSession => {
                self.open_window();
                self.new_session();
            }
            MenuAction::Settings => {
                self.open_window();
                self.settings_open = true;
            }
            MenuAction::Quit => {
                if let Some(shell) = &self.shell {
                    shell.quit();
                }
            }
        }
    }

    /// Brings the main window forward; in the menu bar, with the Dock icon it has while open.
    pub fn open_window(&mut self) {
        let Some(shell) = &self.shell else {
            return;
        };
        if self.status_item.is_some() {
            shell.set_dock_icon(true);
        }
        shell.open_window();
    }

    /// The main window closed (it hides, the app runs on): the Dock icon goes with it.
    pub fn window_closed(&mut self) {
        if let Some(shell) = &self.shell
            && self.status_item.is_some()
        {
            shell.set_dock_icon(false);
        }
    }
}

#[cfg(test)]
pub mod fakes {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Call {
        OpenWindow,
        DockIcon(bool),
        ToggleQuickAsk,
        Quit,
    }

    #[derive(Default)]
    pub struct FakeShell {
        pub calls: RefCell<Vec<Call>>,
    }

    impl AppShell for FakeShell {
        fn open_window(&self) {
            self.calls.borrow_mut().push(Call::OpenWindow);
        }
        fn set_dock_icon(&self, visible: bool) {
            self.calls.borrow_mut().push(Call::DockIcon(visible));
        }
        fn toggle_quick_ask(&self) {
            self.calls.borrow_mut().push(Call::ToggleQuickAsk);
        }
        fn quit(&self) {
            self.calls.borrow_mut().push(Call::Quit);
        }
    }

    #[derive(Clone, Default)]
    pub struct FakeStatusItem {
        pub menus: Rc<RefCell<Vec<Vec<MenuEntry>>>>,
    }

    impl FakeStatusItem {
        pub fn info(&self) -> Option<String> {
            self.menus.borrow().last()?.iter().find_map(|e| match e {
                MenuEntry::Info(text) => Some(text.clone()),
                _ => None,
            })
        }
    }

    impl StatusItem for FakeStatusItem {
        fn set_menu(&self, entries: &[MenuEntry]) {
            self.menus.borrow_mut().push(entries.to_vec());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentInfo, AgentStatus};
    use crate::chat::Application;
    use fakes::{Call, FakeShell, FakeStatusItem};

    fn chat() -> Chat {
        Chat::new(crate::agent::mock_client(), crate::agent::mock_directory())
    }

    fn with_agent(s: &mut Chat, name: &str) {
        s.selected_agent = Some(AgentInfo {
            agent_id: "a1".into(),
            name: name.into(),
            description: None,
            status: AgentStatus::Published,
            model: None,
        });
    }

    fn with_application(s: &mut Chat, name: &str) {
        let mut application = Application::new("APP1", "key");
        application.label = Some(name.into());
        s.set_applications(vec![application], Some(0));
    }

    fn action(entries: &[MenuEntry], wanted: MenuAction) -> (Option<Shortcut>, bool) {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Action {
                    action,
                    shortcut,
                    enabled,
                    ..
                } if *action == wanted => Some((shortcut.clone(), *enabled)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {wanted:?}"))
    }

    fn chord(text: &str) -> Shortcut {
        text.parse().unwrap()
    }

    #[test]
    fn the_menu_leads_with_quick_ask_and_ends_with_quit() {
        let mut s = chat();
        with_agent(&mut s, "Support");
        let labels: Vec<String> = menu_items(&s)
            .iter()
            .map(|e| match e {
                MenuEntry::Action { label, .. } => label.to_string(),
                MenuEntry::Info(text) => format!("({text})"),
                MenuEntry::Separator => "-".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Quick Ask",
                "Open as-gui",
                "New conversation",
                "Settings…",
                "-",
                "(Support)",
                "-",
                "Quit as-gui",
            ]
        );
    }

    #[test]
    fn hints_follow_the_keymap() {
        let mut s = chat();
        with_agent(&mut s, "Support");
        assert_eq!(
            action(&menu_items(&s), MenuAction::QuickAsk).0,
            Some(chord("cmd+shift+space"))
        );
        assert_eq!(
            action(&menu_items(&s), MenuAction::NewSession).0,
            Some(chord("cmd+n"))
        );
        assert_eq!(
            action(&menu_items(&s), MenuAction::Settings).0,
            Some(chord("cmd+comma"))
        );
        assert_eq!(
            action(&menu_items(&s), MenuAction::Quit).0,
            Some(chord("cmd+q"))
        );
        s.keymap.set(Action::QuickAsk, Some(chord("cmd+shift+j")));
        s.keymap.set(Action::NewSession, None);
        assert_eq!(
            action(&menu_items(&s), MenuAction::QuickAsk).0,
            Some(chord("cmd+shift+j"))
        );
        assert_eq!(action(&menu_items(&s), MenuAction::NewSession).0, None);
    }

    #[test]
    fn quick_ask_is_greyed_out_until_an_agent_is_chosen() {
        let mut s = chat();
        assert!(!action(&menu_items(&s), MenuAction::QuickAsk).1);
        assert!(action(&menu_items(&s), MenuAction::OpenWindow).1);
        with_agent(&mut s, "Support");
        assert!(action(&menu_items(&s), MenuAction::QuickAsk).1);
    }

    #[test]
    fn the_info_line_names_the_agent_and_the_application() {
        let mut s = chat();
        let info = |s: &Chat| {
            menu_items(s)
                .into_iter()
                .find_map(|e| match e {
                    MenuEntry::Info(text) => Some(text),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(info(&s), "No agent selected");
        with_application(&mut s, "Production");
        assert_eq!(info(&s), "No agent selected · Production");
        with_agent(&mut s, "Support");
        assert_eq!(info(&s), "Support · Production");
    }

    fn in_menu_bar(s: &mut Chat) -> (Rc<FakeShell>, FakeStatusItem) {
        let shell = Rc::new(FakeShell::default());
        let item = FakeStatusItem::default();
        s.attach_shell(shell.clone());
        s.attach_status_item(Box::new(item.clone()));
        (shell, item)
    }

    #[test]
    fn quick_ask_toggles_the_panel() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.run_menu_action(MenuAction::QuickAsk);
        assert_eq!(*shell.calls.borrow(), [Call::ToggleQuickAsk]);
    }

    #[test]
    fn open_shows_the_dock_icon_then_the_window() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.run_menu_action(MenuAction::OpenWindow);
        assert_eq!(
            *shell.calls.borrow(),
            [Call::DockIcon(true), Call::OpenWindow]
        );
    }

    #[test]
    fn new_session_opens_the_window_on_a_fresh_session() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.run_menu_action(MenuAction::NewSession);
        assert!(shell.calls.borrow().contains(&Call::OpenWindow));
        assert_eq!(s.sessions.len(), 2);
        assert_eq!(s.current, 1);
    }

    #[test]
    fn settings_opens_the_window_on_the_settings_dialog() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.run_menu_action(MenuAction::Settings);
        assert!(shell.calls.borrow().contains(&Call::OpenWindow));
        assert!(s.settings_open);
    }

    #[test]
    fn quit_quits() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.run_menu_action(MenuAction::Quit);
        assert_eq!(*shell.calls.borrow(), [Call::Quit]);
    }

    #[test]
    fn closing_the_window_takes_the_dock_icon_away() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.window_closed();
        assert_eq!(*shell.calls.borrow(), [Call::DockIcon(false)]);
    }

    #[test]
    fn without_a_status_item_the_dock_icon_is_left_alone() {
        let mut s = chat();
        let shell = Rc::new(FakeShell::default());
        s.attach_shell(shell.clone());
        s.open_window();
        s.window_closed();
        assert_eq!(*shell.calls.borrow(), [Call::OpenWindow]);
    }

    #[test]
    fn without_a_shell_actions_still_change_the_state() {
        let mut s = chat();
        s.run_menu_action(MenuAction::Settings);
        s.run_menu_action(MenuAction::Quit);
        assert!(s.settings_open);
    }

    #[test]
    fn attaching_the_status_item_gives_it_the_menu() {
        let mut s = chat();
        let (_, item) = in_menu_bar(&mut s);
        assert_eq!(item.menus.borrow().len(), 1);
        assert_eq!(item.info().as_deref(), Some("No agent selected"));
    }

    #[test]
    fn choosing_an_agent_refreshes_the_menu() {
        let mut s = chat();
        let (_, item) = in_menu_bar(&mut s);
        s.agents = vec![AgentInfo {
            agent_id: "a2".into(),
            name: "Support".into(),
            description: None,
            status: AgentStatus::Published,
            model: None,
        }];
        s.choose_agent(0);
        assert_eq!(item.info().as_deref(), Some("Support"));
    }

    #[test]
    fn rebinding_quick_ask_refreshes_its_hint() {
        let mut s = chat();
        let (_, item) = in_menu_bar(&mut s);
        s.rebind(Action::QuickAsk, Some(&chord("cmd+shift+j")))
            .unwrap();
        let last = item.menus.borrow().last().cloned().unwrap();
        assert_eq!(
            action(&last, MenuAction::QuickAsk).0,
            Some(chord("cmd+shift+j"))
        );
    }

    #[test]
    fn a_promoted_quick_ask_brings_the_window_forward() {
        let mut s = chat();
        let (shell, _) = in_menu_bar(&mut s);
        s.receive_quick_ask("Hi".into(), "Hello".into(), None);
        assert_eq!(
            *shell.calls.borrow(),
            [Call::DockIcon(true), Call::OpenWindow]
        );
    }

    #[test]
    fn the_open_at_launch_setting_saves_and_restores() {
        let path = std::env::temp_dir().join("as-gui-open-at-launch/state.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let mut s = chat();
        assert!(!s.appearance.open_at_launch, "off by default");
        s.persist_path = Some(path.clone());
        s.set_open_at_launch(true);
        let saved = crate::store::load_from(&path);
        assert_eq!(saved.open_at_launch, Some(true));
        let mut restarted = chat();
        restarted.restore(&saved);
        assert!(restarted.appearance.open_at_launch);
    }

    #[test]
    fn every_action_round_trips_through_its_id() {
        for action in MenuAction::ALL {
            assert!(!action.id().is_empty());
            assert_eq!(MenuAction::from_id(action.id()), Some(action));
        }
        assert_eq!(MenuAction::from_id("nope"), None);
    }

    fn mac() -> LaunchInputs {
        LaunchInputs {
            macos: true,
            ..LaunchInputs::default()
        }
    }

    #[test]
    fn a_configured_mac_launches_to_the_menu_bar_without_a_window() {
        assert_eq!(
            launch_mode(mac()),
            LaunchMode {
                show_window: false,
                status_item: true
            }
        );
    }

    #[test]
    fn the_first_run_opens_the_window_for_the_credentials_dialog() {
        let mode = launch_mode(LaunchInputs {
            needs_credentials: true,
            ..mac()
        });
        assert!(mode.show_window);
        assert!(mode.status_item);
    }

    #[test]
    fn a_preset_agent_opens_the_window() {
        let mode = launch_mode(LaunchInputs {
            preset_agent: true,
            ..mac()
        });
        assert!(mode.show_window);
        assert!(mode.status_item);
    }

    #[test]
    fn the_open_at_launch_setting_opens_the_window() {
        let mode = launch_mode(LaunchInputs {
            open_at_launch: true,
            ..mac()
        });
        assert!(mode.show_window);
        assert!(mode.status_item);
    }

    #[test]
    fn headless_runs_render_the_window_and_build_no_status_item() {
        assert_eq!(
            launch_mode(LaunchInputs {
                headless: true,
                ..mac()
            }),
            LaunchMode {
                show_window: true,
                status_item: false
            }
        );
    }

    #[test]
    fn the_demo_records_the_window_without_a_status_item() {
        assert_eq!(
            launch_mode(LaunchInputs {
                demo: true,
                ..mac()
            }),
            LaunchMode {
                show_window: true,
                status_item: false
            }
        );
    }

    #[test]
    fn the_menu_bar_demo_launches_windowless_without_a_real_status_item() {
        assert_eq!(
            launch_mode(LaunchInputs {
                headless: true,
                demo: true,
                menu_bar_demo: true,
                needs_credentials: true,
                ..mac()
            }),
            LaunchMode {
                show_window: false,
                status_item: false
            }
        );
    }

    #[test]
    fn other_platforms_launch_to_the_window_as_before() {
        assert_eq!(
            launch_mode(LaunchInputs::default()),
            LaunchMode {
                show_window: true,
                status_item: false
            }
        );
    }
}
