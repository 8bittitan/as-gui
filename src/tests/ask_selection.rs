use super::*;
use crate::selection::fakes::FakeSource;
use crate::selection::{Quote, Source};

fn panel_with(source: FakeSource) -> Runtime<QuickAsk> {
    let mut rt = quick_ask();
    rt.update(move |s, _| s.selection = Arc::new(source));
    rt.frame(1.0 / 60.0);
    rt
}

fn recording(rt: &mut Runtime<QuickAsk>) -> Arc<HistoryRecorder> {
    let recorder = Arc::new(HistoryRecorder {
        sent: Default::default(),
    });
    let client = recorder.clone();
    rt.update(move |s, _| s.snapshot.lock().unwrap().client = client);
    recorder
}

fn sent(recorder: &HistoryRecorder) -> Vec<String> {
    recorder
        .sent
        .lock()
        .unwrap()
        .iter()
        .flat_map(|turn| turn.iter().map(|(_, text)| text.clone()))
        .collect()
}

fn quoted(text: &str) -> Quote {
    Quote {
        text: text.into(),
        source: Source::Selection,
        trimmed: false,
    }
}

/// The panel's rows spring into place, so their hit boxes lag their layout a few frames.
fn open_on_selection(rt: &mut Runtime<QuickAsk>) {
    rt.update(quick_ask::ask_about_selection);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn field_focused(rt: &Runtime<QuickAsk>) -> bool {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("qa-field"))
        .is_some_and(|e| e.focused)
}

fn shows(rt: &Runtime<QuickAsk>, text: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.text.as_deref() == Some(text))
}

#[test]
fn an_instruction_is_sent_with_the_quote_fenced_beneath_it() {
    let mut rt = quick_ask();
    let recorder = recording(&mut rt);
    rt.update(|s, _| {
        s.quote = Some(quoted("TypeError: x is undefined"));
        s.composer.insert("Why does this happen?");
        s.submit();
    });
    settle_quick_ask(&mut rt);
    assert_eq!(
        sent(&recorder),
        ["Why does this happen?\n\n```\nTypeError: x is undefined\n```"]
    );
    assert!(rt.state.quote.is_none(), "the quote goes with the question");
}

#[test]
fn enter_alone_with_a_quote_asks_to_explain_it() {
    let mut rt = quick_ask();
    let recorder = recording(&mut rt);
    rt.update(|s, tx| {
        s.quote = Some(quoted("TypeError"));
        quick_ask::ask(s, tx);
    });
    settle_quick_ask(&mut rt);
    assert_eq!(sent(&recorder), ["Explain this\n\n```\nTypeError\n```"]);
}

#[test]
fn a_quote_holding_a_fence_is_fenced_with_a_longer_one() {
    let mut rt = quick_ask();
    let recorder = recording(&mut rt);
    rt.update(|s, _| {
        s.quote = Some(quoted("```rust\nlet x = 1;\n```"));
        s.submit();
    });
    settle_quick_ask(&mut rt);
    assert_eq!(
        sent(&recorder),
        ["Explain this\n\n````\n```rust\nlet x = 1;\n```\n````"]
    );
}

#[test]
fn removing_the_quote_leaves_a_plain_question() {
    let mut rt = panel_with(FakeSource::selection("TypeError"));
    let recorder = recording(&mut rt);
    open_on_selection(&mut rt);
    tap(&mut rt, "qa-quote-remove");
    rt.frame(1.0 / 60.0);
    assert!(rt.state.quote.is_none());
    assert!(rt.find("qa-quote").is_none());
    rt.update(|s, _| {
        s.composer.insert("What is new?");
        s.submit();
    });
    settle_quick_ask(&mut rt);
    assert_eq!(sent(&recorder), ["What is new?"]);
}

#[test]
fn a_selection_opens_the_panel_with_its_quote_and_a_focused_field() {
    let mut rt = panel_with(FakeSource::selection("TypeError: x is undefined"));
    open_on_selection(&mut rt);
    assert_eq!(rt.state.quote, Some(quoted("TypeError: x is undefined")));
    assert!(rt.find("qa-quote").is_some());
    assert!(shows(&rt, "Selected text"));
    assert!(shows(&rt, "TypeError: x is undefined"));
    assert!(field_focused(&rt));
    assert!(
        rt.state.wanted_size().1 > 160.0,
        "the panel grows to fit the quote"
    );
}

#[test]
fn clipboard_text_is_labelled_as_from_the_clipboard() {
    let mut rt = panel_with(FakeSource::clipboard("copied earlier"));
    open_on_selection(&mut rt);
    assert_eq!(
        rt.state.quote.as_ref().map(|q| q.source),
        Some(Source::Clipboard)
    );
    assert!(shows(&rt, "From clipboard"));
    assert!(!shows(&rt, "Selected text"));
}

#[test]
fn with_nothing_to_quote_the_panel_opens_as_plain_quick_ask() {
    let mut rt = panel_with(FakeSource::nothing());
    open_on_selection(&mut rt);
    assert!(rt.state.quote.is_none());
    assert!(rt.find("qa-quote").is_none());
    assert!(rt.find("qa-chips").is_none());
    assert!(field_focused(&rt));
    assert_eq!(rt.state.wanted_size(), (480.0, 160.0));
}

#[test]
fn a_long_selection_is_quoted_trimmed_and_says_so() {
    let long = "word ".repeat(3_000);
    let mut rt = panel_with(FakeSource::selection(&long));
    open_on_selection(&mut rt);
    let quote = rt.state.quote.clone().unwrap();
    assert!(quote.trimmed);
    assert!(quote.text.chars().count() <= crate::selection::MAX_CHARS);
    assert!(shows(&rt, "(trimmed)"));
}

#[test]
fn plain_quick_ask_drops_a_quote_left_from_before() {
    let mut rt = panel_with(FakeSource::selection("TypeError"));
    rt.update(quick_ask::ask_about_selection);
    rt.update(quick_ask::toggled);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.quote.is_none());
}

#[test]
fn a_chip_sends_its_instruction_at_once() {
    let mut rt = panel_with(FakeSource::selection("Bonjour"));
    let recorder = recording(&mut rt);
    open_on_selection(&mut rt);
    for chip in [
        "qa-chip-explain",
        "qa-chip-summarize",
        "qa-chip-translate",
        "qa-chip-fix",
    ] {
        assert!(rt.find(chip).is_some(), "{chip}");
    }
    tap(&mut rt, "qa-chip-summarize");
    settle_quick_ask(&mut rt);
    assert_eq!(sent(&recorder), ["Summarize this\n\n```\nBonjour\n```"]);
    assert!(rt.find("qa-chips").is_none(), "no quote left to act on");
}

#[test]
fn the_question_bubble_shows_the_instruction_with_the_quote_collapsed_beneath() {
    let mut rt = quick_ask();
    rt.update(|s, _| {
        s.quote = Some(quoted("TypeError: x is undefined\nat line 3"));
        s.composer.insert("Why?");
        s.submit();
    });
    settle_quick_ask(&mut rt);
    let id = rt.state.messages[0].id.clone();
    assert!(shows(&rt, "Why?"));
    assert!(
        rt.find(&format!("qa-{id}-quote")).is_some(),
        "the quote sits under the instruction"
    );
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("```"))),
        "the fenced message isn't shown raw"
    );
}

#[test]
fn the_promoted_exchange_opens_with_the_quote_in_its_first_message() {
    let mut rt = quick_ask();
    rt.update(|s, _| {
        s.quote = Some(quoted("TypeError"));
        s.submit();
    });
    settle_quick_ask(&mut rt);
    assert_eq!(
        rt.state.messages[0].text, "Explain this\n\n```\nTypeError\n```",
        "what `promote` hands the main window"
    );
}
