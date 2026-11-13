//! Satellite's colour scales, as `@algolia/satellite` ships them (`styles/colors.js` and
//! `styles/brandColors.js`).

use rune::{Color, color};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hue {
    Grey,
    Pink,
    Nebula,
    Xenon,
    Blue,
    Green,
    Orange,
    Red,
    Purple,
}

impl Hue {
    #[cfg(test)]
    pub const ALL: [Hue; 9] = [
        Hue::Grey,
        Hue::Pink,
        Hue::Nebula,
        Hue::Xenon,
        Hue::Blue,
        Hue::Green,
        Hue::Orange,
        Hue::Red,
        Hue::Purple,
    ];

    /// The scale as (step, hex), lightest first. Only grey has a 950, and nebula has no 50.
    pub fn steps(self) -> &'static [(u16, &'static str)] {
        match self {
            Hue::Grey => &[
                (50, "#fcfcfd"),
                (100, "#f5f5fa"),
                (200, "#d6d6e7"),
                (300, "#b6b7d5"),
                (400, "#9698c3"),
                (500, "#777aaf"),
                (600, "#5a5e9a"),
                (700, "#484c7a"),
                (800, "#36395a"),
                (900, "#23263b"),
                (950, "#111432"),
            ],
            Hue::Pink => &[
                (50, "#fff5fb"),
                (100, "#ffeaf6"),
                (200, "#feb9e2"),
                (300, "#fd89ce"),
                (400, "#fb5abc"),
                (500, "#f82caa"),
                (600, "#e90a96"),
                (700, "#b80979"),
                (800, "#88085c"),
                (900, "#59063d"),
            ],
            Hue::Nebula => &[
                (100, "#f2f3ff"),
                (200, "#cacfff"),
                (300, "#a3acff"),
                (400, "#7c8aff"),
                (500, "#5468ff"),
                (600, "#3c4fe0"),
                (700, "#2b3cbb"),
                (800, "#1e2b8f"),
                (900, "#141d61"),
            ],
            Hue::Xenon => &[
                (50, "#f9f9ff"),
                (100, "#f2f4ff"),
                (200, "#bbd1ff"),
                (300, "#76a0ff"),
                (400, "#457aff"),
                (500, "#1e59ff"),
                (600, "#003dff"),
                (700, "#022eb9"),
                (800, "#042077"),
                (900, "#000033"),
            ],
            Hue::Blue => &[
                (50, "#f3fdff"),
                (100, "#e8faff"),
                (200, "#b9efff"),
                (300, "#89e5ff"),
                (400, "#5adaff"),
                (500, "#2cc8f7"),
                (600, "#0db7eb"),
                (700, "#009bcb"),
                (800, "#00769b"),
                (900, "#00526c"),
            ],
            Hue::Green => &[
                (50, "#f2fef1"),
                (100, "#e5fde4"),
                (200, "#bffbc2"),
                (300, "#86f195"),
                (400, "#4ade6d"),
                (500, "#00c241"),
                (600, "#00a648"),
                (700, "#008b4a"),
                (800, "#007046"),
                (900, "#005d43"),
            ],
            Hue::Orange => &[
                (50, "#fffcf5"),
                (100, "#fff9ec"),
                (200, "#ffe9c3"),
                (300, "#fed59a"),
                (400, "#fcbc73"),
                (500, "#faa04b"),
                (600, "#f78125"),
                (700, "#e8600a"),
                (800, "#bf470a"),
                (900, "#963209"),
            ],
            Hue::Red => &[
                (50, "#fff3f4"),
                (100, "#ffe6e9"),
                (200, "#febdc5"),
                (300, "#fc95a1"),
                (400, "#f86e7e"),
                (500, "#f4495d"),
                (600, "#e9122c"),
                (700, "#d4142a"),
                (800, "#ab1325"),
                (900, "#83111e"),
            ],
            Hue::Purple => &[
                (50, "#fdf9ff"),
                (100, "#fcf4ff"),
                (200, "#f2d3ff"),
                (300, "#e2a7ff"),
                (400, "#cc7bff"),
                (500, "#b75aff"),
                (600, "#9524ff"),
                (700, "#731adb"),
                (800, "#5612b7"),
                (900, "#3c0b93"),
            ],
        }
    }

    /// One step's hex. Panics on a step the scale doesn't have.
    #[track_caller]
    pub fn hex(self, step: u16) -> &'static str {
        self.steps()
            .iter()
            .find(|(s, _)| *s == step)
            .map(|(_, h)| *h)
            .unwrap_or_else(|| panic!("{self:?} has no step {step}"))
    }

    #[track_caller]
    pub fn at(self, step: u16) -> Color {
        color::hex(self.hex(step)).expect("scale hexes parse")
    }
}

/// Shorthand for the greys, which most roles read.
#[track_caller]
pub fn grey(step: u16) -> Color {
    Hue::Grey.at(step)
}

/// The brand's planet colours, 1 (deepest) to 5 (palest).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Planet {
    Neptune,
    Mercury,
    Jupiter,
    Saturn,
    Mars,
    Venus,
}

impl Planet {
    pub fn steps(self) -> [&'static str; 5] {
        match self {
            Planet::Neptune => ["#3a46a1", "#5560b5", "#707bcc", "#8d97e3", "#a6b0f9"],
            Planet::Mercury => ["#008fba", "#2da7cb", "#5bbfdd", "#88d6ee", "#b5eeff"],
            Planet::Jupiter => ["#3ab2bd", "#61c5c8", "#89d9d3", "#b0ecde", "#d7ffe9"],
            Planet::Saturn => ["#ec8b63", "#f3a57e", "#f8be9a", "#fcd7b7", "#fdf1d4"],
            Planet::Mars => ["#ed5a6a", "#f27885", "#f695a0", "#fbb3ba", "#ffd0d5"],
            Planet::Venus => ["#ae3e88", "#d44fa4", "#ea71bc", "#f89ad3", "#ffcae9"],
        }
    }

    #[track_caller]
    pub fn at(self, step: usize) -> Color {
        assert!(
            (1..=5).contains(&step),
            "planet steps run 1 to 5, got {step}"
        );
        color::hex(self.steps()[step - 1]).expect("planet hexes parse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_is_xenon_600_algolia_blue() {
        assert_eq!(Hue::Xenon.hex(600), "#003dff");
    }

    #[test]
    fn the_greys_run_from_50_to_950_and_only_they_have_a_950() {
        let greys: Vec<u16> = Hue::Grey.steps().iter().map(|(s, _)| *s).collect();
        assert_eq!(
            greys,
            [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950]
        );
        for hue in Hue::ALL.into_iter().filter(|h| *h != Hue::Grey) {
            assert!(hue.steps().iter().all(|(s, _)| *s <= 900), "{hue:?}");
        }
    }

    #[test]
    fn each_scale_darkens_step_by_step() {
        for hue in Hue::ALL {
            let lum: Vec<f32> = hue
                .steps()
                .iter()
                .map(|(s, _)| hue.at(*s).luminance())
                .collect();
            assert!(lum.windows(2).all(|w| w[0] > w[1]), "{hue:?} {lum:?}");
        }
    }

    #[test]
    fn spot_values_match_satellite() {
        assert_eq!(Hue::Grey.hex(900), "#23263b");
        assert_eq!(Hue::Red.hex(700), "#d4142a");
        assert_eq!(Hue::Green.hex(700), "#008b4a");
        assert_eq!(Hue::Orange.hex(700), "#e8600a");
        assert_eq!(Hue::Nebula.hex(900), "#141d61");
        assert_eq!(Planet::Jupiter.steps()[0], "#3ab2bd");
    }

    #[test]
    #[should_panic(expected = "no step 50")]
    fn a_missing_step_panics() {
        Hue::Nebula.hex(50);
    }
}
