//! Algolia's Satellite design system (`@algolia/satellite`) as the app's rune-kit theme: its
//! colour scales and roles, elevations, radii, type and Lucide icons. Satellite is light only;
//! the dark appearance is derived from its navy greys (see `roles`).

mod accent;
pub mod controls;
pub mod elevation;
pub mod icons;
mod markdown;
mod roles;
pub mod scale;
pub mod typography;

use std::sync::OnceLock;

use rune::{Color, HelpLook, Shadow};
use rune_kit::{Glyphs, Kit, Motion, Radius, TooltipAppearance, TooltipStyle, glyphs};

pub use accent::tones;
pub use controls::{Fit, Size, button};
pub use elevation::Level;
pub use markdown::markdown;
pub use scale::{Hue, Planet, grey};

use crate::chat::Accent;

/// Fira Code, Satellite's code face. The sans is SF Pro, which macOS already has.
pub const FONTS: &[&[u8]] = &[include_bytes!("../assets/fonts/FiraCode-VF.ttf")];

/// `AppSpec::families`: (sans, mono). "System Font" is how Rune names SF Pro.
pub const FAMILIES: (&str, &str) = ("System Font", "Fira Code");

/// Satellite's radii: 4 for controls and tags, 8 for cards and popovers, 12 for the z500 card.
pub const RADIUS_CONTROL: f32 = 4.0;
pub const RADIUS_CARD: f32 = 8.0;
pub const RADIUS_MODAL: f32 = 12.0;

/// Modal widths: small and medium.
pub const MODAL_SMALL: f32 = 480.0;
pub const MODAL_MEDIUM: f32 = 640.0;

pub const GLYPHS: Glyphs = Glyphs {
    check: &icons::CHECK,
    check_small: &icons::CHECK,
    chevron_down: &icons::CHEVRON_DOWN,
    chevron_up: &icons::CHEVRON_UP,
    caret_right: &icons::CHEVRON_RIGHT,
    close: &icons::X,
    clear: &icons::X,
    copy: &icons::COPY,
    search: &icons::SEARCH,
    lock: &icons::LOCK,
    eye: &icons::EYE,
    eye_off: &icons::EYE_OFF,
    circle_x: &icons::CIRCLE_X,
    dash: &icons::MINUS,
    ..glyphs::DEFAULT
};

/// The kit for one appearance and accent, resolved for the blend mode of the window building
/// the view, like the theme crates' `kit`.
pub fn kit(dark: bool, accent: Accent) -> &'static Kit {
    static KITS: [[[OnceLock<Kit>; 2]; Accent::ALL.len()]; 2] =
        [const { [const { [const { OnceLock::new() }; 2] }; Accent::ALL.len()] }; 2];
    let srgb = rune::theme::blends_srgb();
    let a = Accent::ALL.iter().position(|x| *x == accent).unwrap_or(0);
    KITS[dark as usize][a][srgb as usize]
        .get_or_init(|| rune::theme::with_blend(srgb, || build(dark, accent)))
}

fn build(dark: bool, accent: Accent) -> Kit {
    let c = roles::palette(dark, tones(accent, dark));
    let elevation = elevation::elevation(dark, c.bg_base);
    Kit {
        dark,
        elevation,
        glyph: GLYPHS,
        ty: typography::kit_type(),
        radius: Radius {
            xs: 2.0,
            sm: RADIUS_CONTROL,
            md: RADIUS_CONTROL,
            lg: RADIUS_CARD,
            xl: RADIUS_MODAL,
        },
        // Satellite's 72-100 ms control and popover transitions and its 200-250 ms modal and
        // checkbox ones, as critically damped springs.
        motion: Motion {
            control: rune::Spring::new(0.1, 0.0),
            overlay: rune::Spring::new(0.18, 0.0),
            disclosure: rune::Spring::new(0.25, 0.0),
        },
        blend_over: Some(c.bg_base),
        ring: None,
        invalid_text: true,
        c,
    }
}

/// An elevation's layers for the kit's appearance.
pub fn shadow(k: &Kit, level: Level) -> Vec<Shadow> {
    elevation::level(level, k.dark, k.c.bg_base)
}

/// Satellite's tooltip: grey 900 with white body text, 8 by 12 padding, radius 8, z200. Dark
/// lifts it off the page with a grey 700 fill.
pub fn tooltip(k: &Kit) -> TooltipStyle {
    let fill = if k.dark { grey(700) } else { grey(900) };
    TooltipStyle::from_kit(k, TooltipAppearance::Standard)
        .fill(fill)
        .border(0.0, Color::TRANSPARENT)
        .ink(Color::WHITE)
        .font(typography::BODY)
        .padding(12.0, 8.0)
        .radius(RADIUS_CARD)
        .max_width(256.0)
        .shadows(shadow(k, Level::Z200))
        .motion(k.motion.overlay)
}

pub fn help_look(k: &Kit) -> HelpLook {
    tooltip(k).help_look()
}

/// The view-wide defaults: Satellite's focus outline (2 points of accent, 1 point out) and its
/// tooltip. Call on every run of a window's view.
pub fn install<S>(k: &Kit, cx: &mut rune::Cx<S>) {
    cx.focus_ring(2.0, 1.0, k.c.border_focus);
    cx.help_look(help_look(k));
}

/// WCAG contrast ratio of two opaque colours.
#[cfg(test)]
pub fn contrast(a: Color, b: Color) -> f32 {
    let (hi, lo) = {
        let (x, y) = (a.luminance(), b.luminance());
        if x > y { (x, y) } else { (y, x) }
    };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_kit_follows_the_appearance_and_accent() {
        for dark in [false, true] {
            for accent in Accent::ALL {
                let k = kit(dark, accent);
                assert_eq!(k.dark, dark);
                assert_eq!(k.c.bg_accent, tones(accent, dark).fill);
                assert_eq!(
                    k.c.bg_icon_button_contrast, k.c.bg_accent,
                    "Submit is Satellite's primary"
                );
            }
        }
    }

    #[test]
    fn the_kit_is_cached() {
        assert!(std::ptr::eq(
            kit(false, Accent::Blue),
            kit(false, Accent::Blue)
        ));
        assert!(!std::ptr::eq(
            kit(false, Accent::Blue),
            kit(false, Accent::Pink)
        ));
    }

    #[test]
    fn controls_are_rounded_4_and_cards_8() {
        let k = kit(false, Accent::Blue);
        assert_eq!(
            (k.radius.sm, k.radius.md, k.radius.lg, k.radius.xl),
            (4.0, 4.0, 8.0, 12.0)
        );
    }

    #[test]
    fn primary_and_focus_are_xenon_in_light() {
        let k = kit(false, Accent::Blue);
        assert_eq!(k.c.border_focus.hex(), "#003dffff");
        assert_eq!(k.c.submit_fg, Color::WHITE);
    }
}
