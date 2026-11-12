//! What "Ask about selection" quotes: the text selected in the frontmost app, else the
//! clipboard's. The macOS capture copies the selection with ⌘C and puts the clipboard back
//! (`copy_and_restore` over a `Desktop`); `SelectionSource` is the seam the panel and the
//! settings' Accessibility row read through.

#[cfg(target_os = "macos")]
mod mac;

use std::time::Duration;

/// About 8,000 characters reach the agent; longer text is cut and marked "(trimmed)".
pub const MAX_CHARS: usize = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Selection,
    Clipboard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub text: String,
    pub source: Source,
}

/// The text the next question is about, as the panel shows and sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub text: String,
    pub source: Source,
    pub trimmed: bool,
}

impl Quote {
    /// None when the capture is only whitespace.
    pub fn from_capture(capture: Capture) -> Option<Quote> {
        let (text, trimmed) = trim(&capture.text, MAX_CHARS);
        (!text.is_empty()).then_some(Quote {
            text,
            source: capture.source,
            trimmed,
        })
    }
}

/// Strips the blank lines around `text` and cuts it to `max` characters on a word boundary;
/// true when it was cut.
pub fn trim(text: &str, max: usize) -> (String, bool) {
    let Some(first) = text.find(|c: char| !c.is_whitespace()) else {
        return (String::new(), false);
    };
    let line_start = text[..first].rfind('\n').map_or(0, |i| i + 1);
    let body = text[line_start..].trim_end();
    let Some((cap, _)) = body.char_indices().nth(max) else {
        return (body.to_string(), false);
    };
    let head = &body[..cap];
    let cut = match head.rfind(char::is_whitespace) {
        Some(space) if !head[..space].trim().is_empty() => &head[..space],
        _ => head,
    };
    (cut.trim_end().to_string(), true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted,
    NotGranted,
    /// No permission applies here (other platforms, tests, headless runs).
    NotNeeded,
}

/// Where the selected text comes from, so tests and other platforms can stand in for macOS.
pub trait SelectionSource: Send + Sync {
    /// Blocks while the frontmost app copies; call it off the UI thread.
    fn capture(&self) -> Option<Capture>;
    fn access(&self) -> Access;
    /// Asks the system for the permission the capture needs.
    fn request_access(&self);
}

/// Captures nothing: the panel opens as a plain Quick Ask.
pub struct NoSelection;

impl SelectionSource for NoSelection {
    fn capture(&self) -> Option<Capture> {
        None
    }
    fn access(&self) -> Access {
        Access::NotNeeded
    }
    fn request_access(&self) {}
}

/// What the recorded demo (`just demo ask-selection`) has selected; the script paints the
/// same text on its backdrop.
pub const DEMO_SELECTION: &str = "Been running the Aero 2 for marathons, but the cushioning fades by mile \
twenty. Is it the shoe, or should I swap the insoles instead?";

/// The demo's capture: always `DEMO_SELECTION`, as if selected in another app.
pub struct DemoSelection;

impl SelectionSource for DemoSelection {
    fn capture(&self) -> Option<Capture> {
        Some(Capture {
            text: DEMO_SELECTION.into(),
            source: Source::Selection,
        })
    }
    fn access(&self) -> Access {
        Access::NotNeeded
    }
    fn request_access(&self) {}
}

/// The source this run captures through: the demo's in demo mode, the real one on a macOS
/// desktop.
pub fn system() -> std::sync::Arc<dyn SelectionSource> {
    source_for(crate::agent::demo_mode(), rune::is_headless())
}

fn source_for(demo: bool, headless: bool) -> std::sync::Arc<dyn SelectionSource> {
    if demo {
        return std::sync::Arc::new(DemoSelection);
    }
    #[cfg(target_os = "macos")]
    if !headless {
        return std::sync::Arc::new(mac::MacDesktop);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = headless;
    std::sync::Arc::new(NoSelection)
}

/// The pasteboard and keyboard the copy-and-restore capture drives.
pub trait Desktop {
    type Saved;
    /// Whether the process may post the ⌘C key event.
    fn trusted(&self) -> bool;
    fn change_count(&self) -> i64;
    fn text(&self) -> Option<String>;
    /// Whether the clipboard holds something its app marked as secret, such as a password
    /// copied from a password manager.
    fn concealed(&self) -> bool;
    fn save(&self) -> Self::Saved;
    fn restore(&self, saved: Self::Saved);
    /// Sends ⌘C to the frontmost app; false when the event couldn't be posted.
    fn send_copy(&self) -> bool;
    fn wait(&self, duration: Duration);
}

const COPY_POLL: Duration = Duration::from_millis(10);
const COPY_TIMEOUT: Duration = Duration::from_millis(200);

/// Copies the selection and puts the clipboard back as it was. Without the permission, or
/// when the frontmost app copies nothing, it's the clipboard's text instead.
pub fn copy_and_restore<D: Desktop>(desktop: &D) -> Option<Capture> {
    let clipboard = || {
        if desktop.concealed() {
            return None;
        }
        capture(desktop.text(), Source::Clipboard)
    };
    if !desktop.trusted() {
        return clipboard();
    }
    let before = desktop.change_count();
    let saved = desktop.save();
    if !desktop.send_copy() {
        return clipboard();
    }
    let mut waited = Duration::ZERO;
    while desktop.change_count() == before && waited < COPY_TIMEOUT {
        desktop.wait(COPY_POLL);
        waited += COPY_POLL;
    }
    if desktop.change_count() == before {
        return clipboard();
    }
    let selected = desktop.text();
    desktop.restore(saved);
    capture(selected, Source::Selection).or_else(clipboard)
}

fn capture(text: Option<String>, source: Source) -> Option<Capture> {
    text.filter(|t| !t.trim().is_empty())
        .map(|text| Capture { text, source })
}

#[cfg(test)]
pub mod fakes {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A source that hands back the same capture every time.
    pub struct FakeSource {
        pub capture: Option<Capture>,
        pub access: Access,
        pub requests: AtomicUsize,
    }

    impl FakeSource {
        pub fn selection(text: &str) -> FakeSource {
            FakeSource {
                capture: Some(Capture {
                    text: text.into(),
                    source: Source::Selection,
                }),
                access: Access::Granted,
                requests: AtomicUsize::new(0),
            }
        }

        pub fn clipboard(text: &str) -> FakeSource {
            FakeSource {
                capture: Some(Capture {
                    text: text.into(),
                    source: Source::Clipboard,
                }),
                access: Access::Granted,
                requests: AtomicUsize::new(0),
            }
        }

        pub fn with_access(access: Access) -> FakeSource {
            FakeSource {
                access,
                ..FakeSource::nothing()
            }
        }

        pub fn nothing() -> FakeSource {
            FakeSource {
                capture: None,
                access: Access::Granted,
                requests: AtomicUsize::new(0),
            }
        }
    }

    impl SelectionSource for FakeSource {
        fn capture(&self) -> Option<Capture> {
            self.capture.clone()
        }
        fn access(&self) -> Access {
            self.access
        }
        fn request_access(&self) {
            self.requests.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn the_demo_quotes_its_canned_selection_and_needs_no_permission() {
        let source = source_for(true, true);
        assert_eq!(
            source.capture(),
            Some(Capture {
                text: DEMO_SELECTION.into(),
                source: Source::Selection,
            })
        );
        assert_eq!(source.access(), Access::NotNeeded);
    }

    #[test]
    fn a_headless_run_captures_nothing() {
        let source = source_for(false, true);
        assert_eq!(source.capture(), None);
        assert_eq!(source.access(), Access::NotNeeded);
    }

    #[test]
    fn short_text_is_kept_whole() {
        assert_eq!(
            trim("TypeError: x is undefined", MAX_CHARS),
            ("TypeError: x is undefined".to_string(), false)
        );
    }

    #[test]
    fn the_blank_lines_around_the_text_are_stripped_but_its_indent_is_kept() {
        assert_eq!(
            trim("\n  \n    let x = 1;\n    let y = 2;\n\n \n", MAX_CHARS),
            ("    let x = 1;\n    let y = 2;".to_string(), false)
        );
    }

    #[test]
    fn text_over_the_cap_is_cut_on_a_word_boundary_and_flagged() {
        assert_eq!(
            trim("alpha beta gamma", 13),
            ("alpha beta".to_string(), true)
        );
    }

    #[test]
    fn a_single_word_over_the_cap_is_cut_mid_word() {
        assert_eq!(trim("abcdefghij", 4), ("abcd".to_string(), true));
    }

    #[test]
    fn the_cap_counts_characters_not_bytes() {
        assert_eq!(trim("ééé ééé", 7), ("ééé ééé".to_string(), false));
        assert_eq!(trim("ééé ééé", 6), ("ééé".to_string(), true));
    }

    #[test]
    fn a_quote_is_trimmed_and_whitespace_alone_is_no_quote() {
        let quote = Quote::from_capture(Capture {
            text: format!("\n{}\n", "word ".repeat(2_000)),
            source: Source::Clipboard,
        })
        .unwrap();
        assert!(quote.trimmed);
        assert!(quote.text.chars().count() <= MAX_CHARS);
        assert_eq!(quote.source, Source::Clipboard);
        assert_eq!(
            Quote::from_capture(Capture {
                text: " \n\t ".into(),
                source: Source::Selection,
            }),
            None
        );
    }

    /// A pasteboard holding `clipboard`, in front of an app with `selected` selected.
    struct FakeDesktop {
        trusted: bool,
        selected: Option<&'static str>,
        clipboard: RefCell<Option<String>>,
        concealed: bool,
        count: RefCell<i64>,
        log: RefCell<Vec<&'static str>>,
    }

    impl FakeDesktop {
        fn new(selected: Option<&'static str>, clipboard: Option<&str>) -> FakeDesktop {
            FakeDesktop {
                trusted: true,
                selected,
                clipboard: RefCell::new(clipboard.map(str::to_string)),
                concealed: false,
                count: RefCell::new(7),
                log: RefCell::new(Vec::new()),
            }
        }
    }

    impl Desktop for FakeDesktop {
        type Saved = Option<String>;
        fn trusted(&self) -> bool {
            self.trusted
        }
        fn change_count(&self) -> i64 {
            *self.count.borrow()
        }
        fn text(&self) -> Option<String> {
            self.clipboard.borrow().clone()
        }
        fn concealed(&self) -> bool {
            self.concealed
        }
        fn save(&self) -> Option<String> {
            self.log.borrow_mut().push("save");
            self.clipboard.borrow().clone()
        }
        fn restore(&self, saved: Option<String>) {
            self.log.borrow_mut().push("restore");
            *self.clipboard.borrow_mut() = saved;
            *self.count.borrow_mut() += 1;
        }
        fn send_copy(&self) -> bool {
            self.log.borrow_mut().push("copy");
            if let Some(selected) = self.selected {
                *self.clipboard.borrow_mut() = Some(selected.into());
                *self.count.borrow_mut() += 1;
            }
            true
        }
        fn wait(&self, _: Duration) {}
    }

    #[test]
    fn a_concealed_clipboard_is_never_quoted_without_permission() {
        let mut desktop = FakeDesktop::new(None, Some("hunter2"));
        desktop.concealed = true;
        desktop.trusted = false;
        assert_eq!(copy_and_restore(&desktop), None);
    }

    #[test]
    fn a_concealed_clipboard_is_never_quoted_when_nothing_was_selected() {
        let mut desktop = FakeDesktop::new(None, Some("hunter2"));
        desktop.concealed = true;
        assert_eq!(copy_and_restore(&desktop), None);
        assert_eq!(desktop.clipboard.borrow().as_deref(), Some("hunter2"));
    }

    #[test]
    fn the_selection_is_copied_and_the_clipboard_put_back() {
        let desktop = FakeDesktop::new(Some("the error"), Some("what I copied earlier"));
        assert_eq!(
            copy_and_restore(&desktop),
            Some(Capture {
                text: "the error".into(),
                source: Source::Selection,
            })
        );
        assert_eq!(*desktop.log.borrow(), ["save", "copy", "restore"]);
        assert_eq!(
            desktop.clipboard.borrow().as_deref(),
            Some("what I copied earlier")
        );
    }

    #[test]
    fn with_nothing_selected_it_is_the_clipboard_and_nothing_is_rewritten() {
        let desktop = FakeDesktop::new(None, Some("what I copied earlier"));
        assert_eq!(
            copy_and_restore(&desktop),
            Some(Capture {
                text: "what I copied earlier".into(),
                source: Source::Clipboard,
            })
        );
        assert!(!desktop.log.borrow().contains(&"restore"));
    }

    #[test]
    fn with_nothing_selected_or_copied_there_is_nothing() {
        let desktop = FakeDesktop::new(None, None);
        assert_eq!(copy_and_restore(&desktop), None);
    }

    #[test]
    fn a_blank_selection_falls_back_to_the_clipboard_after_restoring_it() {
        let desktop = FakeDesktop::new(Some("  \n"), Some("earlier"));
        assert_eq!(
            copy_and_restore(&desktop),
            Some(Capture {
                text: "earlier".into(),
                source: Source::Clipboard,
            })
        );
        assert_eq!(*desktop.log.borrow(), ["save", "copy", "restore"]);
    }

    #[test]
    fn without_the_permission_no_copy_is_sent_and_the_clipboard_is_used() {
        let mut desktop = FakeDesktop::new(Some("the error"), Some("earlier"));
        desktop.trusted = false;
        assert_eq!(
            copy_and_restore(&desktop),
            Some(Capture {
                text: "earlier".into(),
                source: Source::Clipboard,
            })
        );
        assert!(desktop.log.borrow().is_empty());
    }
}
