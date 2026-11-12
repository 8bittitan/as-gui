//! The appearance settings the settings dialog dials: chat width, theme and accent.

use super::Chat;

use serde::{Deserialize, Serialize};

/// The settings the user dials in the settings dialog.
pub struct Appearance {
    pub width: ChatWidth,
    pub theme: ThemePref,
    pub accent: Accent,
    /// Whether the window's top edge carries the active application's colour.
    pub tint_window: bool,
    /// Whether the main window opens at launch instead of only the menu bar icon.
    pub open_at_launch: bool,
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance {
            width: ChatWidth::default(),
            theme: ThemePref::default(),
            accent: Accent::default(),
            tint_window: true,
            open_at_launch: false,
        }
    }
}

/// How wide the conversation renders, set in the settings dialog (⌘,).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatWidth {
    #[default]
    Comfortable,
    Wide,
}

/// Which appearance the app renders in, whatever the system does; the settings dialog sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemePref {
    pub const LABELS: [&str; 3] = ["Auto", "Light", "Dark"];

    pub fn resolve(self, system_dark: bool) -> bool {
        match self {
            ThemePref::Auto => system_dark,
            ThemePref::Light => false,
            ThemePref::Dark => true,
        }
    }
}

/// The accent the theme's interactive colours dial to; the settings dialog sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Blue,
    Purple,
    Pink,
    Orange,
    Green,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 6] = [
        Accent::Blue,
        Accent::Purple,
        Accent::Pink,
        Accent::Orange,
        Accent::Green,
        Accent::Graphite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Accent::Blue => "Blue",
            Accent::Purple => "Purple",
            Accent::Pink => "Pink",
            Accent::Orange => "Orange",
            Accent::Green => "Green",
            Accent::Graphite => "Graphite",
        }
    }
}

impl Chat {
    pub fn set_width(&mut self, width: ChatWidth) {
        self.appearance.width = width;
        self.persist();
    }

    pub fn set_theme(&mut self, theme: ThemePref) {
        self.appearance.theme = theme;
        self.sync_snapshot();
        self.persist();
    }

    pub fn set_accent(&mut self, accent: Accent) {
        self.appearance.accent = accent;
        self.sync_snapshot();
        self.persist();
    }

    pub fn set_tint_window(&mut self, on: bool) {
        self.appearance.tint_window = on;
        self.persist();
    }

    pub fn set_open_at_launch(&mut self, on: bool) {
        self.appearance.open_at_launch = on;
        self.persist();
    }
}
