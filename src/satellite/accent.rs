//! The settings' accents as Satellite scales. A fill takes white text, so it's the first step
//! of its scale white reads on (Satellite's primary is xenon 600). The ink is accent text and
//! icons on the page: the fill's step in light, a pale step in dark.

use rune::Color;

use super::scale::Hue;
use crate::chat::Accent;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tones {
    pub fill: Color,
    pub ink: Color,
}

pub fn hue(accent: Accent) -> Hue {
    match accent {
        Accent::Blue => Hue::Xenon,
        Accent::Purple => Hue::Purple,
        Accent::Pink => Hue::Pink,
        Accent::Orange => Hue::Orange,
        Accent::Green => Hue::Green,
        Accent::Graphite => Hue::Grey,
    }
}

/// (fill, light ink, dark ink) steps.
fn steps(accent: Accent) -> (u16, u16, u16) {
    match accent {
        Accent::Blue | Accent::Purple => (600, 600, 300),
        Accent::Pink => (700, 700, 300),
        Accent::Orange | Accent::Green => (800, 800, 400),
        Accent::Graphite => (600, 700, 300),
    }
}

pub fn tones(accent: Accent, dark: bool) -> Tones {
    let hue = hue(accent);
    let (fill, light, dark_ink) = steps(accent);
    Tones {
        fill: hue.at(fill),
        ink: hue.at(if dark { dark_ink } else { light }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::satellite::contrast;

    #[test]
    fn blue_is_satellite_xenon_600() {
        assert_eq!(tones(Accent::Blue, false).fill, Hue::Xenon.at(600));
        assert_eq!(tones(Accent::Blue, false).ink, Hue::Xenon.at(600));
    }

    #[test]
    fn white_reads_on_every_fill() {
        for accent in Accent::ALL {
            for dark in [false, true] {
                let ratio = contrast(Color::WHITE, tones(accent, dark).fill);
                assert!(ratio >= 4.5, "{accent:?} dark={dark}: {ratio:.2}");
            }
        }
    }
}
