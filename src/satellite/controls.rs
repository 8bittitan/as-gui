//! Satellite's button sizes on rune-kit's buttons: small 24, medium 32 and large 40 points
//! tall, with 8, 12 and 16 points of padding, 4 between icon and label, and the label in body
//! text. Build a kit button with `.sized(k, Size)` in place of `.build(k)`.

use rune::{El, text};
use rune_kit::{Button, ControlSize, IconButton, Kit};

use super::typography;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Inline actions: a row's or a message's buttons.
    Small,
    /// The default: dialog and form actions, page-level buttons.
    Medium,
    #[allow(dead_code)]
    Large,
}

impl Size {
    pub fn height(self) -> f32 {
        match self {
            Size::Small => 24.0,
            Size::Medium => 32.0,
            Size::Large => 40.0,
        }
    }

    pub fn padding(self) -> f32 {
        match self {
            Size::Small => 8.0,
            Size::Medium => 12.0,
            Size::Large => 16.0,
        }
    }
}

/// A button whose label is set in Satellite's body text.
pub fn button(label: impl Into<String>) -> Button {
    let label = label.into();
    let text_label = label.clone();
    Button::new(label).label_with(move |_, ink| text(text_label).font(typography::BODY).color(ink))
}

pub trait Fit {
    fn sized(self, k: &Kit, size: Size) -> El;
}

impl Fit for Button {
    fn sized(self, k: &Kit, size: Size) -> El {
        self.control_size(ControlSize::Regular)
            .build(k)
            .h(size.height())
            .px(size.padding())
            .gap(4.0)
    }
}

/// An icon-only button is square at the size's height, its icon 16 points.
impl Fit for IconButton {
    fn sized(self, k: &Kit, size: Size) -> El {
        let side = size.height();
        self.control_size(ControlSize::Regular)
            .build(k)
            .size(side, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_satellite() {
        assert_eq!((Size::Small.height(), Size::Small.padding()), (24.0, 8.0));
        assert_eq!(
            (Size::Medium.height(), Size::Medium.padding()),
            (32.0, 12.0)
        );
        assert_eq!((Size::Large.height(), Size::Large.padding()), (40.0, 16.0));
    }
}
