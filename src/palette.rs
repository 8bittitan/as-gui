//! The application colours: one per saved application, so Production and Staging read apart
//! at a glance. Independent of the accent, which is the app's personality, so the tones sit
//! beside the accent's rather than on them.

use rune::{Color, color};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppColour {
    #[default]
    Blue,
    Orange,
    Green,
    Purple,
    Red,
    Teal,
    Yellow,
    Pink,
    Indigo,
    Graphite,
    /// The debug mock's: never assigned, never saved.
    Neutral,
}

impl AppColour {
    /// The assignable colours, most distinct first, so the first few applications differ most.
    pub const ALL: [AppColour; 10] = [
        AppColour::Blue,
        AppColour::Orange,
        AppColour::Green,
        AppColour::Purple,
        AppColour::Red,
        AppColour::Teal,
        AppColour::Yellow,
        AppColour::Pink,
        AppColour::Indigo,
        AppColour::Graphite,
    ];

    /// The stable id the state file stores.
    pub fn id(self) -> &'static str {
        match self {
            AppColour::Blue => "blue",
            AppColour::Orange => "orange",
            AppColour::Green => "green",
            AppColour::Purple => "purple",
            AppColour::Red => "red",
            AppColour::Teal => "teal",
            AppColour::Yellow => "yellow",
            AppColour::Pink => "pink",
            AppColour::Indigo => "indigo",
            AppColour::Graphite => "graphite",
            AppColour::Neutral => "neutral",
        }
    }

    pub fn from_id(id: &str) -> Option<AppColour> {
        AppColour::ALL.into_iter().find(|c| c.id() == id)
    }

    /// The colour an application at `index` gets when the file names none it knows: by
    /// position, so a restart doesn't reshuffle.
    pub fn by_position(index: usize) -> AppColour {
        AppColour::ALL[index % AppColour::ALL.len()]
    }

    /// The least used of `taken`, ties going to the palette's order.
    pub fn least_used(taken: impl IntoIterator<Item = AppColour>) -> AppColour {
        let mut uses = [0usize; AppColour::ALL.len()];
        for colour in taken {
            if let Some(i) = AppColour::ALL.iter().position(|&c| c == colour) {
                uses[i] += 1;
            }
        }
        let fewest = uses.iter().copied().min().unwrap_or(0);
        let i = uses.iter().position(|&n| n == fewest).unwrap_or(0);
        AppColour::ALL[i]
    }

    pub fn label(self) -> &'static str {
        match self {
            AppColour::Blue => "Blue",
            AppColour::Orange => "Orange",
            AppColour::Green => "Green",
            AppColour::Purple => "Purple",
            AppColour::Red => "Red",
            AppColour::Teal => "Teal",
            AppColour::Yellow => "Yellow",
            AppColour::Pink => "Pink",
            AppColour::Indigo => "Indigo",
            AppColour::Graphite => "Graphite",
            AppColour::Neutral => "Grey",
        }
    }

    /// The fill for a dot or the window's top bar, tuned to stand out on each theme's
    /// background.
    pub fn hex(self, dark: bool) -> &'static str {
        match (self, dark) {
            (AppColour::Blue, true) => "#4aa3ff",
            (AppColour::Blue, false) => "#1f6fe0",
            (AppColour::Orange, true) => "#ff9a3d",
            (AppColour::Orange, false) => "#d9670b",
            (AppColour::Green, true) => "#3fcf73",
            (AppColour::Green, false) => "#1b9a47",
            (AppColour::Purple, true) => "#b884ff",
            (AppColour::Purple, false) => "#8448d6",
            (AppColour::Red, true) => "#ff5f57",
            (AppColour::Red, false) => "#d63a31",
            (AppColour::Teal, true) => "#40d0c8",
            (AppColour::Teal, false) => "#0e968f",
            (AppColour::Yellow, true) => "#ffd43b",
            (AppColour::Yellow, false) => "#c49200",
            (AppColour::Pink, true) => "#ff70b0",
            (AppColour::Pink, false) => "#d6448a",
            (AppColour::Indigo, true) => "#8586ff",
            (AppColour::Indigo, false) => "#5352d4",
            (AppColour::Graphite, true) => "#a5a5ad",
            (AppColour::Graphite, false) => "#6b6b73",
            (AppColour::Neutral, true) => "#8e8e93",
            (AppColour::Neutral, false) => "#8e8e93",
        }
    }

    pub fn color(self, dark: bool) -> Color {
        color::hex(self.hex(dark)).expect("application colour hexes parse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_assignable_colour_round_trips_through_its_id() {
        for colour in AppColour::ALL {
            assert_eq!(AppColour::from_id(colour.id()), Some(colour));
        }
        assert_eq!(AppColour::from_id("chartreuse"), None);
        assert_eq!(
            AppColour::from_id("neutral"),
            None,
            "the mock's grey is never saved"
        );
    }

    #[test]
    fn the_least_used_colour_is_picked_in_palette_order() {
        assert_eq!(AppColour::least_used([]), AppColour::ALL[0]);
        assert_eq!(
            AppColour::least_used([AppColour::ALL[0], AppColour::ALL[2]]),
            AppColour::ALL[1]
        );
        assert_eq!(AppColour::least_used(AppColour::ALL), AppColour::ALL[0]);
    }

    #[test]
    fn every_colour_parses_on_both_themes() {
        for colour in AppColour::ALL.into_iter().chain([AppColour::Neutral]) {
            colour.color(true);
            colour.color(false);
        }
    }

    #[test]
    fn no_colour_repeats_an_accent_tone_exactly() {
        let accents = [
            "#0a84ff", "#0071e3", "#bf5af2", "#9d3fdb", "#ff375f", "#e02d58", "#ff9f0a", "#c77700",
            "#30d158", "#23a144", "#98989d", "#7c7c82",
        ];
        for colour in AppColour::ALL {
            for dark in [true, false] {
                assert!(!accents.contains(&colour.hex(dark)), "{colour:?}");
            }
        }
    }
}
