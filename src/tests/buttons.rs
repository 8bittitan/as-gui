use super::*;

/// Satellite's button heights: small 24, medium 32, large 40.
fn height(rt: &Runtime<Chat>, key: &str) -> f32 {
    rt.presented(key)
        .map(|(r, _)| r[3])
        .unwrap_or_else(|| panic!("no {key}"))
}

#[test]
fn primary_and_page_level_buttons_are_satellite_medium() {
    let mut rt = test_chat_sized((1100.0, 800.0));
    rt.update(|s, _| crate::scenes::empty(s));
    settle(&mut rt);
    assert_eq!(height(&rt, "send"), 32.0, "the composer's send button");
    assert_eq!(
        height(&rt, "new-session"),
        32.0,
        "the sidebar's New conversation"
    );
    assert_eq!(height(&rt, "starter-0"), 32.0, "a starter prompt");
}

#[test]
fn inline_row_actions_are_satellite_small() {
    let mut rt = test_chat_sized((1100.0, 800.0));
    rt.update(|s, _| crate::scenes::tools(s));
    settle(&mut rt);
    assert_eq!(height(&rt, "copy-a1"), 24.0, "a message action");
    rt.update(|s, _| s.settings_open = true);
    settle(&mut rt);
    assert_eq!(
        height(&rt, "applications-add"),
        24.0,
        "a settings row action"
    );
}

#[test]
fn button_labels_are_satellite_body_text() {
    let mut rt = test_chat_sized((1100.0, 800.0));
    rt.update(|s, _| crate::scenes::empty(s));
    settle(&mut rt);
    let label = rt
        .elements()
        .into_iter()
        .find(|e| e.path.contains("new-session") && e.text.as_deref() == Some("New conversation"))
        .expect("the label");
    assert!(
        (label.rect[3] - 20.0).abs() < 0.5,
        "14 on 20 body text: {:?}",
        label.rect
    );
}
