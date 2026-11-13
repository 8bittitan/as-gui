//! Satellite's colour roles on rune-kit's palette. Light is Satellite as documented: grey 50
//! page, white surfaces, grey 900 text, grey 600 subdued text and icons, grey 200 borders,
//! grey 500 form borders. Satellite has no dark theme, so dark is built from its navy greys
//! and the pale steps of each scale, keeping the same roles.

use rune::Color;
use rune_kit::{Palette, StateColors};

use super::accent::Tones;
use super::scale::{Hue, grey};

/// A translucent colour as painted over `bg`, re-solved when the window blends in linear light.
fn over(c: Color, a: f32, bg: Color) -> Color {
    rune::theme::for_blend(c.alpha(a), bg)
}

fn state(hue: Hue, dark: bool, bg_base: Color) -> StateColors {
    if dark {
        StateColors {
            bg: bg_base.mix(hue.at(900), 0.55),
            fg: hue.at(300),
            border: hue.at(800),
            icon: hue.at(400),
        }
    } else {
        // Satellite's Badge and Tag tints: a 100 fill, a 200/300 border, deep text; the icon
        // is the role colour (green, orange, red, xenon 700).
        let (border, fg) = match hue {
            Hue::Green => (300, 900),
            Hue::Orange => (300, 800),
            _ => (200, 700),
        };
        StateColors {
            bg: hue.at(100),
            fg: hue.at(fg),
            border: hue.at(border),
            icon: hue.at(700),
        }
    }
}

pub fn palette(dark: bool, accent: Tones) -> Palette {
    if dark {
        dark_palette(accent)
    } else {
        light_palette(accent)
    }
}

fn light_palette(accent: Tones) -> Palette {
    let white = Color::WHITE;
    let bg = white;
    let ink = grey(950);
    Palette {
        page: grey(50),
        bg_base: bg,
        bg_deep: grey(100),
        layer_01: white,
        layer_02: grey(100),
        layer_03: grey(200),
        layer_04: grey(500),
        bg_inverse: grey(900),
        bg_contrast: grey(900),
        bg_button_neutral: white,
        bg_icon_button_contrast: accent.fill,
        bg_accent: accent.fill,
        submit_highlight: Color::TRANSPARENT,
        submit_hover: over(ink, 0.3, accent.fill),
        submit_fg: white,
        submit_icon: white,
        text_base: grey(900),
        text_muted: grey(600),
        text_faint: grey(600),
        text_inverse: white,
        text_contrast: white,
        text_accent: accent.ink,
        icon_base: grey(700),
        icon_muted: grey(600),
        icon_faint: grey(400),
        icon_inverse: white,
        icon_contrast: white,
        icon_accent: accent.ink,
        border_muted: grey(100),
        border_base: grey(200),
        border_strong: grey(300),
        border_focus: accent.fill,
        // Grey 600 at these alphas over white lands on grey 100 and grey 200, Satellite's
        // hover and active fills.
        hover: over(grey(600), 0.06, bg),
        pressed: over(grey(600), 0.25, bg),
        contrast_hover: over(white, 0.12, grey(900)),
        contrast_pressed: over(ink, 0.45, accent.fill),
        scrim: over(grey(900), 0.3, bg),
        depth_top: white,
        depth_bottom: grey(50),
        white,
        black: Color::BLACK,
        knob: white,
        knob_on: white,
        success: state(Hue::Green, false, bg),
        warning: state(Hue::Orange, false, bg),
        danger: state(Hue::Red, false, bg),
        info: state(Hue::Xenon, false, bg),
        surface_base: grey(50),
        surface_weak: grey(100),
        surface_hover: grey(100),
        surface_raised: white,
        surface_raised_base: grey(100),
        surface_raised_hover: grey(200),
        surface_float: grey(900),
        surface_disabled: grey(100),
        border_weak: grey(200),
        border_hover: grey(600),
        border_active: accent.fill,
        border_disabled: grey(200),
        text_strong: grey(900),
        text_weak: grey(600),
        text_invert_strong: white,
        icon_weaker: grey(400),
        std_border: grey(200),
        std_icon: grey(600),
        std_icon_invert: white,
        std_icon_strong: grey(900),
        std_text: grey(700),
    }
}

/// The dark pane: grey 950 taken toward black, so body text reads at better than 15:1 while
/// the page and the shell around it sit deeper still.
pub fn dark_base() -> Color {
    grey(950).mix(Color::BLACK, 0.4)
}

fn dark_palette(accent: Tones) -> Palette {
    let white = Color::WHITE;
    let bg = dark_base();
    Palette {
        page: grey(950).mix(Color::BLACK, 0.55),
        bg_base: bg,
        bg_deep: grey(950).mix(Color::BLACK, 0.65),
        layer_01: grey(900),
        layer_02: grey(800),
        layer_03: grey(700),
        layer_04: grey(600),
        bg_inverse: grey(100),
        bg_contrast: grey(700),
        bg_button_neutral: grey(900),
        bg_icon_button_contrast: accent.fill,
        bg_accent: accent.fill,
        submit_highlight: Color::TRANSPARENT,
        submit_hover: over(grey(950), 0.3, accent.fill),
        submit_fg: white,
        submit_icon: white,
        text_base: grey(100),
        text_muted: grey(300),
        text_faint: grey(300),
        text_inverse: grey(950),
        text_contrast: white,
        text_accent: accent.ink,
        icon_base: grey(200),
        icon_muted: grey(400),
        icon_faint: grey(500),
        icon_inverse: grey(950),
        icon_contrast: white,
        icon_accent: accent.ink,
        border_muted: over(white, 0.08, bg),
        border_base: over(white, 0.12, bg),
        border_strong: over(white, 0.2, bg),
        border_focus: accent.ink,
        hover: over(white, 0.06, bg),
        pressed: over(white, 0.12, bg),
        contrast_hover: over(white, 0.12, grey(700)),
        contrast_pressed: over(grey(950), 0.45, accent.fill),
        scrim: over(grey(950), 0.6, bg),
        depth_top: white,
        depth_bottom: grey(50),
        white,
        black: Color::BLACK,
        knob: white,
        knob_on: white,
        success: state(Hue::Green, true, bg),
        warning: state(Hue::Orange, true, bg),
        danger: state(Hue::Red, true, bg),
        info: state(Hue::Xenon, true, bg),
        surface_base: bg,
        surface_weak: grey(900),
        surface_hover: grey(800),
        surface_raised: grey(900),
        surface_raised_base: grey(800),
        surface_raised_hover: grey(700),
        surface_float: grey(800),
        surface_disabled: grey(900),
        border_weak: grey(800),
        border_hover: grey(500),
        border_active: accent.ink,
        border_disabled: grey(800),
        text_strong: grey(50),
        text_weak: grey(400),
        text_invert_strong: grey(950),
        icon_weaker: grey(500),
        std_border: grey(700),
        std_icon: grey(400),
        std_icon_invert: grey(950),
        std_icon_strong: grey(50),
        std_text: grey(300),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Accent;
    use crate::satellite::{accent, contrast};

    fn both() -> [Palette; 2] {
        [false, true].map(|dark| palette(dark, accent::tones(Accent::Blue, dark)))
    }

    /// `top` painted over opaque `bottom` in sRGB, as the default windows blend.
    fn composite(top: Color, bottom: Color) -> [u8; 3] {
        let enc = rune::color::srgb_encode;
        let ch = |t: f32, b: f32| ((enc(t) * top.a + enc(b) * (1.0 - top.a)) * 255.0).round() as u8;
        [
            ch(top.r, bottom.r),
            ch(top.g, bottom.g),
            ch(top.b, bottom.b),
        ]
    }

    fn rgb(c: Color) -> [u8; 3] {
        composite(c, c)
    }

    fn near(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
    }

    #[test]
    fn light_is_satellites_documented_roles() {
        let p = &both()[0];
        assert_eq!(p.page, grey(50), "Background Default");
        assert_eq!(p.bg_deep, grey(100), "Background Alt");
        assert_eq!(p.border_base, grey(200), "Border Subtle");
        assert_eq!(p.text_muted, grey(600), "Text Subtle");
        assert_eq!(p.text_base, grey(900), "Text Primary");
        assert_eq!(p.bg_accent, Hue::Xenon.at(600), "Accent");
        assert_eq!(p.danger.icon, Hue::Red.at(700), "Danger");
        assert_eq!(p.success.icon, Hue::Green.at(700), "Success");
        assert_eq!(p.warning.icon, Hue::Orange.at(700), "Warning");
        assert_eq!(p.info.icon, Hue::Xenon.at(700), "Info");
    }

    #[test]
    fn light_hover_and_press_land_on_grey_100_and_200() {
        let p = &both()[0];
        assert!(
            near(composite(p.hover, Color::WHITE), rgb(grey(100))),
            "{:?}",
            composite(p.hover, Color::WHITE)
        );
        assert!(
            near(composite(p.pressed, Color::WHITE), rgb(grey(200))),
            "{:?}",
            composite(p.pressed, Color::WHITE)
        );
    }

    #[test]
    fn body_and_subdued_text_read_on_every_surface() {
        for p in both() {
            for surface in [p.bg_base, p.layer_01, p.bg_deep, p.page] {
                assert!(
                    contrast(p.text_base, surface) >= 7.0,
                    "text_base on {}",
                    surface.hex()
                );
                assert!(
                    contrast(p.text_muted, surface) >= 4.5,
                    "text_muted on {}",
                    surface.hex()
                );
                assert!(
                    contrast(p.text_faint, surface) >= 4.5,
                    "text_faint on {}",
                    surface.hex()
                );
            }
            assert!(contrast(p.text_base, p.layer_02) >= 7.0, "the user bubble");
            assert!(
                contrast(p.text_faint, p.layer_02) >= 4.5,
                "faint text in the bubble"
            );
        }
    }

    #[test]
    fn accent_ink_reads_on_the_page_for_every_accent() {
        for a in Accent::ALL {
            for dark in [false, true] {
                let p = palette(dark, accent::tones(a, dark));
                let ratio = contrast(p.text_accent, p.bg_base);
                assert!(ratio >= 4.5, "{a:?} dark={dark}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn status_text_reads_on_its_own_tint() {
        for p in both() {
            for (name, s) in [
                ("success", p.success),
                ("warning", p.warning),
                ("danger", p.danger),
                ("info", p.info),
            ] {
                let ratio = contrast(s.fg, s.bg);
                assert!(
                    ratio >= 4.5,
                    "{name} dark={}: {ratio:.2}",
                    p.text_base == grey(100)
                );
            }
        }
    }

    #[test]
    fn dark_reads_on_a_near_black_navy_pane_with_a_deeper_shell() {
        let p = &both()[1];
        assert!(
            p.bg_base.luminance() < grey(950).luminance(),
            "the pane is below grey 950"
        );
        assert!(
            p.bg_deep.luminance() < p.bg_base.luminance(),
            "the shell sits deeper than the pane"
        );
        let body = contrast(p.text_base, p.bg_base);
        assert!(body >= 15.0, "body text on the pane: {body:.2}");
        let muted = contrast(p.text_muted, p.bg_base);
        assert!(muted >= 9.0, "subdued text on the pane: {muted:.2}");
    }

    #[test]
    fn dark_keeps_the_page_behind_the_pane() {
        let p = &both()[1];
        assert!(p.page.luminance() < p.bg_base.luminance());
        assert!(p.bg_base.luminance() < p.layer_01.luminance());
        assert!(p.layer_01.luminance() < p.layer_02.luminance());
    }
}
