//! The macOS status item, through `tray-icon`: a template icon whose menu opens on any click.
//! Swap for `rune::Tray` once it has a macOS backend (anomalyco/rune#141).

use std::cell::RefCell;

use rune::{AppHandle, Shortcut};
use tray_icon::menu::accelerator::Accelerator;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::{MenuAction, MenuEntry, StatusItem};
use crate::chat::Chat;

const ICON_SIZE: u32 = 44;
const ICON: &[u8] = include_bytes!("../../assets/MenuBarIcon.rgba");

pub struct MacStatusItem {
    tray: TrayIcon,
    shown: RefCell<Vec<MenuEntry>>,
}

impl MacStatusItem {
    /// Puts the icon in the menu bar; its menu picks run on `chat`. Main thread only.
    pub fn new(chat: AppHandle<Chat>) -> Result<MacStatusItem, String> {
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(action) = MenuAction::from_id(event.id.as_ref()) {
                chat.update(move |c, _| c.run_menu_action(action));
            }
        }));
        let icon =
            Icon::from_rgba(ICON.to_vec(), ICON_SIZE, ICON_SIZE).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_icon_templated(icon)
            .with_tooltip("as-gui")
            .with_menu_on_left_click(true)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(MacStatusItem {
            tray,
            shown: RefCell::new(Vec::new()),
        })
    }
}

impl StatusItem for MacStatusItem {
    fn set_menu(&self, entries: &[MenuEntry]) {
        if *self.shown.borrow() == entries {
            return;
        }
        let menu = Menu::new();
        for entry in entries {
            let appended = match entry {
                MenuEntry::Action {
                    action,
                    label,
                    shortcut,
                    enabled,
                } => menu.append(&MenuItem::with_id(
                    action.id(),
                    label,
                    *enabled,
                    shortcut.as_ref().and_then(accelerator),
                )),
                MenuEntry::Info(text) => menu.append(&MenuItem::new(text, false, None)),
                MenuEntry::Separator => menu.append(&PredefinedMenuItem::separator()),
            };
            if let Err(error) = appended {
                tracing::warn!(%error, "menu bar item");
            }
        }
        self.tray.set_menu(Some(Box::new(menu)));
        *self.shown.borrow_mut() = entries.to_vec();
    }
}

fn accelerator(shortcut: &Shortcut) -> Option<Accelerator> {
    shortcut.to_string().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::menu::accelerator::{Code, Modifiers};

    #[test]
    fn shortcuts_become_menu_accelerators() {
        let cases = [
            (
                "cmd+shift+space",
                Accelerator::new(Modifiers::META | Modifiers::SHIFT, Code::Space),
            ),
            ("cmd+n", Accelerator::new(Modifiers::META, Code::KeyN)),
            ("cmd+comma", Accelerator::new(Modifiers::META, Code::Comma)),
            (
                "ctrl+alt+k",
                Accelerator::new(Modifiers::CONTROL | Modifiers::ALT, Code::KeyK),
            ),
        ];
        for (chord, expected) in cases {
            let shortcut: Shortcut = chord.parse().unwrap();
            assert_eq!(accelerator(&shortcut), Some(expected), "{chord}");
        }
    }

    #[test]
    fn the_icon_is_a_22pt_retina_square() {
        assert_eq!(ICON.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(tray_icon::Icon::from_rgba(ICON.to_vec(), ICON_SIZE, ICON_SIZE).is_ok());
    }
}
