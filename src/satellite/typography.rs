//! Satellite's type scale in SF Pro (the system font) and Fira Code: body 14/20, caption 12/16,
//! semibold for emphasis, no tracking.

use rune::TextStyle;
use rune_kit::Type;

const fn sf(size: f32, weight: f32, line: f32) -> TextStyle {
    TextStyle::new(size, weight).lh(line)
}

pub const REGULAR: f32 = 400.0;
pub const MEDIUM: f32 = 500.0;
pub const SEMIBOLD: f32 = 600.0;

/// Display M and S, Heading and Subheading: Satellite's desktop scale.
pub const DISPLAY_MEDIUM: TextStyle = sf(26.0, SEMIBOLD, 32.0);
pub const DISPLAY_SMALL: TextStyle = sf(20.0, SEMIBOLD, 28.0);
pub const HEADING: TextStyle = sf(16.0, SEMIBOLD, 24.0);
/// Set in capitals: pass the text through `str::to_uppercase`.
pub const SUBHEADING: TextStyle = sf(12.0, SEMIBOLD, 16.0);
pub const BODY: TextStyle = sf(14.0, REGULAR, 20.0);
pub const BODY_BOLD: TextStyle = sf(14.0, SEMIBOLD, 20.0);
pub const CAPTION: TextStyle = sf(12.0, REGULAR, 16.0);
pub const CODE: TextStyle = TextStyle::new(12.0, REGULAR).mono().lh(20.0);

pub fn kit_type() -> Type {
    Type {
        ui: BODY,
        ui_medium: sf(14.0, MEDIUM, 20.0),
        ui_compact: sf(14.0, REGULAR, 16.0),
        small: CAPTION,
        small_medium: sf(12.0, SEMIBOLD, 16.0),
        body: BODY,
        body_medium: BODY_BOLD,
        text_small: sf(13.0, REGULAR, 20.0),
        text_small_medium: sf(13.0, SEMIBOLD, 20.0),
        text_xsmall: CAPTION,
        text_xsmall_medium: sf(12.0, SEMIBOLD, 16.0),
        mono: CODE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_14_on_20_and_caption_12_on_16() {
        let t = kit_type();
        assert_eq!(
            (t.ui.size, t.ui.weight, t.ui.line_height),
            (14.0, 400.0, 20.0)
        );
        assert_eq!((t.small.size, t.small.line_height), (12.0, 16.0));
    }

    #[test]
    fn satellite_sets_no_tracking() {
        let t = kit_type();
        for s in [
            t.ui,
            t.ui_medium,
            t.ui_compact,
            t.small,
            t.small_medium,
            t.body,
            t.mono,
            HEADING,
            DISPLAY_SMALL,
        ] {
            assert_eq!(s.tracking, 0.0);
        }
    }

    #[test]
    fn headings_are_semibold() {
        for s in [
            DISPLAY_MEDIUM,
            DISPLAY_SMALL,
            HEADING,
            SUBHEADING,
            BODY_BOLD,
        ] {
            assert_eq!(s.weight, SEMIBOLD);
        }
    }
}
