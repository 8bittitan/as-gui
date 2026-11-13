//! Satellite's `MarkdownContent` and `ChatCodeDisplay` looks for rune-kit's markdown: grey 100
//! code, a grey 200 quote bar, semibold headings and its prism token colours.

use rune::TextStyle;
use rune_kit::Kit;
use rune_kit::markdown::MarkdownStyle;
use rune_kit::syntax::{Syntax, SyntaxTheme};

use super::scale::{Hue, grey};
use super::typography::{BODY_BOLD, CODE, DISPLAY_MEDIUM, DISPLAY_SMALL, MEDIUM, SEMIBOLD};

pub fn syntax(k: &Kit) -> SyntaxTheme {
    // ChatCodeDisplay's prism theme; dark takes the same hues' pale steps.
    let (pink, xenon, green, blue, orange, comment) = if k.dark {
        (
            Hue::Pink.at(300),
            Hue::Xenon.at(200),
            Hue::Green.at(300),
            Hue::Blue.at(400),
            Hue::Orange.at(400),
            grey(300),
        )
    } else {
        (
            Hue::Pink.at(700),
            Hue::Xenon.at(600),
            Hue::Green.at(800),
            Hue::Blue.at(800),
            Hue::Orange.at(700),
            Hue::Nebula.at(900),
        )
    };
    SyntaxTheme::from_kit(k)
        .with(Syntax::Keyword, pink)
        .with(Syntax::Attribute, pink)
        .with(Syntax::String, green)
        .with(Syntax::Inserted, green)
        .with(Syntax::Property, green)
        .with(Syntax::Number, xenon)
        .with(Syntax::Link, xenon)
        .with(Syntax::Function, blue)
        .with(Syntax::Type, orange)
        .with(Syntax::Comment, comment)
}

pub fn markdown(k: &Kit) -> MarkdownStyle {
    let c = &k.c;
    let code_fill = c.layer_02;
    let code_text = if k.dark { grey(200) } else { grey(700) };
    let body = TextStyle {
        line_height: 20.0,
        ..k.ty.body
    };
    MarkdownStyle::from_kit(k)
        .text(c.text_base)
        .muted(c.text_muted)
        .link(c.text_accent)
        .body_font(body)
        .heading_font(1, DISPLAY_MEDIUM)
        .heading_font(2, DISPLAY_SMALL)
        .heading_font(3, BODY_BOLD)
        .heading_font(
            4,
            TextStyle {
                weight: MEDIUM,
                ..BODY_BOLD
            },
        )
        .strong_weight(SEMIBOLD)
        .inline_code_bg(code_fill)
        .inline_code_font(0.93, 400.0)
        .inline_code_pad(4.0)
        .code_block(code_fill, code_fill)
        .code_block_padding(16.0, 8.0)
        .code_block_font(CODE)
        .code_block_text(code_text)
        .quote(c.border_base, 2.0, 16.0, c.text_muted)
        .rule(c.border_base)
        .table(c.border_base, c.bg_base)
        .table_rules(c.border_muted)
        .table_header_weight(400.0)
        .syntax_theme(syntax(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Accent;

    #[test]
    fn light_code_takes_satellites_prism_colours() {
        let k = crate::satellite::kit(false, Accent::Blue);
        let s = syntax(k);
        assert_eq!(s.color(Syntax::Keyword), Hue::Pink.at(700));
        assert_eq!(s.color(Syntax::String), Hue::Green.at(800));
        assert_eq!(s.color(Syntax::Number), Hue::Xenon.at(600));
        assert_eq!(s.color(Syntax::Function), Hue::Blue.at(800));
        assert_eq!(s.color(Syntax::Comment), Hue::Nebula.at(900));
    }

    #[test]
    fn code_colours_read_on_the_code_fill() {
        for dark in [false, true] {
            let k = crate::satellite::kit(dark, Accent::Blue);
            let s = syntax(k);
            for kind in [
                Syntax::Keyword,
                Syntax::String,
                Syntax::Number,
                Syntax::Function,
                Syntax::Comment,
            ] {
                let ratio = crate::satellite::contrast(s.color(kind), k.c.layer_02);
                assert!(ratio >= 4.5, "{kind:?} dark={dark}: {ratio:.2}");
            }
        }
    }
}
