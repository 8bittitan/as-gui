//! Satellite's five elevations (z100 to z500): a hairline ring at grey 900 / 5% under a drop
//! shadow that deepens with the level, plus its field, switch and knob shadows.

use rune::{Color, Shadow};
use rune_kit::Elevation;

use super::scale::grey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Z100,
    Z200,
    Z300,
    Z500,
}

fn drop(y: f32, blur: f32, spread: f32, color: Color) -> Shadow {
    Shadow {
        x: 0.0,
        y,
        blur,
        spread,
        color,
        inset: false,
    }
}

fn ring(width: f32, color: Color, inset: bool) -> Shadow {
    Shadow {
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: width,
        color,
        inset,
    }
}

fn inset(y: f32, blur: f32, color: Color) -> Shadow {
    Shadow {
        x: 0.0,
        y,
        blur,
        spread: 0.0,
        color,
        inset: true,
    }
}

struct Ink {
    ring: Color,
    soft: Color,
    deep: Color,
}

fn ink(dark: bool, bg: Color) -> Ink {
    let solve = |c: Color, a: f32| rune::theme::for_blend(c.alpha(a), bg);
    if dark {
        Ink {
            ring: solve(Color::WHITE, 0.08),
            soft: solve(Color::BLACK, 0.3),
            deep: solve(Color::BLACK, 0.45),
        }
    } else {
        Ink {
            ring: solve(grey(900), 0.05),
            soft: solve(grey(900), 0.15),
            deep: solve(grey(900), 0.25),
        }
    }
}

pub fn level(level: Level, dark: bool, bg: Color) -> Vec<Shadow> {
    let i = ink(dark, bg);
    let lift = match level {
        Level::Z100 => drop(1.0, 3.0, 0.0, i.soft),
        Level::Z200 => drop(4.0, 8.0, -2.0, i.deep),
        Level::Z300 => drop(8.0, 16.0, -4.0, i.deep),
        Level::Z500 => drop(20.0, 32.0, -8.0, i.deep),
    };
    vec![ring(1.0, i.ring, false), lift]
}

/// Satellite's form-field border, grey 500: at least 3:1 on the field in both appearances.
pub fn field_border() -> Color {
    grey(500)
}

pub fn elevation(dark: bool, bg: Color) -> Elevation {
    let solve = |c: Color, a: f32| rune::theme::for_blend(c.alpha(a), bg);
    let (field_border, button_border, tick) = if dark {
        (
            field_border(),
            solve(Color::WHITE, 0.12),
            solve(Color::BLACK, 0.4),
        )
    } else {
        (field_border(), grey(200), solve(grey(900), 0.1))
    };
    let mut e = Elevation::default().with_input(vec![ring(1.0, field_border, true)]);
    e.raised = level(Level::Z100, dark, bg);
    e.md = level(Level::Z200, dark, bg);
    e.floating = level(Level::Z300, dark, bg);
    e.overlay = level(Level::Z500, dark, bg);
    e.button_neutral = vec![ring(1.0, button_border, true)];
    e.button_contrast = Vec::new();
    e.elements = vec![drop(1.0, 1.0, 0.0, tick)];
    e.knob = vec![drop(1.0, 1.0, 0.0, tick)];
    e.switch_off = vec![inset(1.0, 4.0, solve(grey(500), 0.2))];
    e.switch_on = vec![inset(1.0, 4.0, solve(grey(900), 0.1))];
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_is_a_hairline_ring_under_a_drop() {
        for l in [Level::Z100, Level::Z200, Level::Z300, Level::Z500] {
            let layers = level(l, false, Color::WHITE);
            assert_eq!(layers.len(), 2);
            assert_eq!((layers[0].blur, layers[0].spread), (0.0, 1.0));
            assert!((layers[0].color.a - 0.05).abs() < 1e-3);
        }
    }

    #[test]
    fn modals_sit_highest() {
        let e = elevation(false, Color::WHITE);
        assert_eq!(e.overlay[1].y, 20.0);
        assert_eq!(e.overlay[1].blur, 32.0);
        assert_eq!(e.floating[1].y, 8.0);
        assert_eq!(e.raised[1].y, 1.0);
    }

    #[test]
    fn fields_have_satellites_grey_500_border() {
        let e = elevation(false, Color::WHITE);
        let border = e.input()[0];
        assert!(border.inset && border.spread == 1.0);
        assert_eq!(border.color, grey(500));
    }

    #[test]
    fn field_borders_reach_3_to_1_in_both_appearances() {
        for dark in [false, true] {
            let k = crate::satellite::kit(dark, crate::chat::Accent::Blue);
            let ratio = crate::satellite::contrast(k.elevation.input()[0].color, k.c.bg_base);
            assert!(ratio >= 3.0, "dark={dark}: {ratio:.2}");
        }
    }
}
