use std::sync::atomic::Ordering;

use super::*;
use crate::selection::Access;
use crate::selection::fakes::FakeSource;

fn settings_with(access: Access) -> (Runtime<Chat>, Arc<FakeSource>) {
    let source = Arc::new(FakeSource::with_access(access));
    let mut rt = test_chat_sized((720.0, 1900.0));
    let shared = source.clone();
    rt.update(move |s, _| {
        s.selection = shared;
        s.settings_open = true;
    });
    settle(&mut rt);
    (rt, source)
}

fn status(rt: &Runtime<Chat>) -> Vec<String> {
    rt.elements()
        .iter()
        .filter(|e| e.path.contains("accessibility-status"))
        .filter_map(|e| e.text.clone())
        .collect()
}

#[test]
fn settings_says_when_accessibility_access_is_missing_and_asks_for_it() {
    let (mut rt, source) = settings_with(Access::NotGranted);
    assert!(
        status(&rt).iter().any(|t| t == "Not granted"),
        "{:?}",
        status(&rt)
    );
    tap(&mut rt, "accessibility-grant");
    assert_eq!(source.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn settings_says_when_accessibility_access_is_granted() {
    let (rt, _) = settings_with(Access::Granted);
    assert!(
        status(&rt).iter().any(|t| t == "Granted"),
        "{:?}",
        status(&rt)
    );
    assert!(rt.find("accessibility-grant").is_none());
}

#[test]
fn where_no_permission_applies_there_is_no_row() {
    let (rt, _) = settings_with(Access::NotNeeded);
    assert!(rt.find("accessibility-status").is_none());
}
