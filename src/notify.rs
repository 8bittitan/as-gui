//! Notifications for a reply that finished in the background: the [`Notifier`] seam over
//! `rune-notify`, and the [`first_line`] helper that picks the reply's notification body.

use rune_notify::Notification;

/// Who the app's notifications post through; tests inject a fake.
pub trait Notifier: Send + Sync {
    fn post(&self, n: &Notification) -> bool;
    fn remove(&self, ids: &[&str]);
    fn focused(&self) -> bool;
}

/// The real thing: `rune-notify`, which quietly does nothing when the app can't post (a
/// `cargo run` build on macOS isn't bundled) and asks for permission on the first post
/// from one that is.
pub struct RuneNotifier;

impl Notifier for RuneNotifier {
    fn post(&self, n: &Notification) -> bool {
        rune_notify::post(n)
    }

    fn remove(&self, ids: &[&str]) {
        rune_notify::remove(ids);
    }

    fn focused(&self) -> bool {
        rune_notify::app_focused()
    }
}

/// About how much of the reply a notification body shows.
pub const MAX_BODY_CHARS: usize = 140;

/// The reply's first prose line: markdown markers stripped, truncated on a word boundary
/// with an ellipsis. Empty when the reply has no prose, in which case nothing posts.
pub fn first_line(text: &str) -> String {
    first_line_within(text, MAX_BODY_CHARS)
}

fn first_line_within(text: &str, max: usize) -> String {
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let stripped = strip_markdown(trimmed);
        if !stripped.is_empty() {
            return truncate(&stripped, max);
        }
    }
    String::new()
}

fn strip_markdown(line: &str) -> String {
    let mut s = line.trim().trim_start_matches('#').trim_start();
    if matches!(s, "---" | "***" | "___") {
        return String::new();
    }
    for marker in ["- ", "* ", "+ ", "> "] {
        if let Some(rest) = s.strip_prefix(marker) {
            s = rest;
            break;
        }
    }
    if matches!(s, "-" | "*" | "+") {
        return String::new();
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && s[digits..].starts_with(". ") {
        s = &s[digits + 2..];
    }
    s.chars()
        .filter(|c| !matches!(c, '*' | '`'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let budget = max.saturating_sub(1);
    let mut taken = String::new();
    let mut len = 0;
    for word in s.split_whitespace() {
        let extra = word.chars().count() + usize::from(!taken.is_empty());
        if len + extra > budget {
            break;
        }
        if !taken.is_empty() {
            taken.push(' ');
        }
        taken.push_str(word);
        len += extra;
    }
    if taken.is_empty() {
        taken = s.chars().take(budget).collect();
    }
    format!("{taken}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_its_own_first_line() {
        assert_eq!(first_line("Hello there"), "Hello there");
    }

    #[test]
    fn the_first_prose_line_wins() {
        assert_eq!(first_line("\n\n  Hello there  "), "Hello there");
    }

    #[test]
    fn a_heading_loses_its_hash() {
        assert_eq!(first_line("# The answer"), "The answer");
    }

    #[test]
    fn a_list_marker_is_stripped() {
        assert_eq!(first_line("- first item"), "first item");
    }

    #[test]
    fn an_ordered_list_marker_is_stripped() {
        assert_eq!(first_line("1. step one"), "step one");
    }

    #[test]
    fn a_blockquote_marker_is_stripped() {
        assert_eq!(first_line("> quoted wisdom"), "quoted wisdom");
    }

    #[test]
    fn emphasis_marks_are_stripped() {
        assert_eq!(first_line("**bold** and *italic*"), "bold and italic");
    }

    #[test]
    fn inline_code_backticks_are_stripped() {
        assert_eq!(first_line("run `cargo test` now"), "run cargo test now");
    }

    #[test]
    fn fenced_code_is_skipped_for_prose() {
        assert_eq!(
            first_line("```\nlet x = 1;\n```"),
            "",
            "a reply that is only code has no body"
        );
        assert_eq!(first_line("```\ncode\n```\nReal answer"), "Real answer");
    }

    #[test]
    fn rules_and_empty_markdown_are_skipped() {
        assert_eq!(first_line("---\n***\n\n- \nAnswer"), "Answer");
    }

    #[test]
    fn an_empty_reply_has_no_first_line() {
        assert_eq!(first_line(""), "");
        assert_eq!(first_line("   \n  "), "");
    }

    #[test]
    fn long_text_truncates_on_a_word_boundary_with_an_ellipsis() {
        assert_eq!(first_line_within("one two three", 8), "one two…");
        assert_eq!(first_line_within("aaaa bbbb cccc", 10), "aaaa bbbb…");
    }

    #[test]
    fn a_word_that_doesnt_fit_backs_off_to_the_previous_boundary() {
        assert_eq!(first_line_within("aaaa bbbb", 8), "aaaa…");
    }

    #[test]
    fn a_long_single_word_truncates_at_the_limit() {
        let out = first_line_within(&"x".repeat(200), 140);
        assert_eq!(out.chars().count(), 140);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn text_at_the_limit_is_untouched() {
        assert_eq!(first_line_within("abcdefgh", 8), "abcdefgh");
    }
}
