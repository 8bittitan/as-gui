//! The application colours: one per saved application, so Production and Staging read apart
//! at a glance. Independent of the accent, which is the app's personality, so the tones sit
//! beside the accent's rather than on them.

use rune::Color;

use crate::satellite::{Hue, Planet, grey};

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

    /// The fill for a dot or the window's top bar: Satellite's 700 step on light, its 400 on
    /// dark, so each stands off the page in both. Blue's 700 and the brand planets are too
    /// pale for light, so blue takes its 800 and the planets are deepened toward grey 900.
    /// Steps an accent tone already uses move one along, so a dot never matches the accent.
    pub fn color(self, dark: bool) -> Color {
        let step = if dark { 400 } else { 700 };
        let planet = |p: Planet, dark_step| {
            if dark {
                p.at(dark_step)
            } else {
                p.at(1).mix(grey(900), 0.35)
            }
        };
        match self {
            AppColour::Blue => Hue::Blue.at(if dark { 400 } else { 800 }),
            AppColour::Orange => Hue::Orange.at(if dark { 500 } else { 700 }),
            AppColour::Green => Hue::Green.at(if dark { 500 } else { 700 }),
            AppColour::Purple => Hue::Purple.at(step),
            AppColour::Red => Hue::Red.at(step),
            AppColour::Pink => Hue::Pink.at(if dark { 400 } else { 600 }),
            AppColour::Indigo => Hue::Nebula.at(step),
            AppColour::Graphite => Hue::Grey.at(if dark { 400 } else { 500 }),
            AppColour::Teal => planet(Planet::Jupiter, 2),
            AppColour::Yellow => planet(Planet::Saturn, 4),
            AppColour::Neutral => grey(500),
        }
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
    fn every_assignable_colour_stands_off_the_page_in_both_appearances() {
        for dark in [true, false] {
            let k = crate::satellite::kit(dark, crate::chat::Accent::Blue);
            for colour in AppColour::ALL {
                for (name, bg) in [("bg_base", k.c.bg_base), ("bg_deep", k.c.bg_deep)] {
                    let ratio = crate::satellite::contrast(colour.color(dark), bg);
                    assert!(
                        ratio >= 3.0,
                        "{colour:?} on {name} (dark {dark}) is {ratio:.2}:1"
                    );
                }
            }
        }
    }

    #[test]
    fn no_two_assignable_colours_are_the_same() {
        for dark in [true, false] {
            for (i, a) in AppColour::ALL.into_iter().enumerate() {
                for b in AppColour::ALL.into_iter().skip(i + 1) {
                    assert_ne!(
                        a.color(dark),
                        b.color(dark),
                        "{a:?} and {b:?} (dark {dark})"
                    );
                }
            }
        }
    }

    #[test]
    fn no_colour_repeats_an_accent_tone_exactly() {
        for accent in crate::chat::Accent::ALL {
            for dark in [true, false] {
                let tones = crate::satellite::tones(accent, dark);
                for colour in AppColour::ALL {
                    let c = colour.color(dark);
                    assert!(
                        c != tones.fill && c != tones.ink,
                        "{colour:?} repeats {accent:?}"
                    );
                }
            }
        }
    }
}
