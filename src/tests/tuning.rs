#[cfg_attr(not(feature = "knobs"), allow(unused_imports))]
use super::*;

#[test]
#[cfg(feature = "knobs")]
fn a_hit_card_height_knob_resizes_the_rendered_cards() {
    let mut rt = test_chat();
    rt.update(|s, _| crate::scenes::tools(s));
    rt.frame(1.0 / 60.0);
    let height = |rt: &Runtime<Chat>| rt.presented("hit-search_tool-0").map(|(r, _)| r[3]);
    assert_eq!(height(&rt), Some(210.0), "the shipped card height");

    rt.update(|s, _| s.knobs.set(&knobs::HIT_CARD_HEIGHT, 320.0));
    settle(&mut rt);
    assert!(
        height(&rt).is_some_and(|h| (h - 320.0).abs() < 0.5),
        "the knob retunes the cards live"
    );
}

#[test]
#[cfg(feature = "knobs")]
fn a_columns_knob_overrides_the_width_setting() {
    let mut rt = test_chat();
    assert_eq!(rt.state.hit_columns(), 2);
    rt.update(|s, _| s.knobs.set(&knobs::HIT_COLUMNS, 4.0));
    assert_eq!(
        rt.state.hit_columns(),
        4,
        "the fifth choice is four columns"
    );
    rt.update(|s, _| s.knobs.set(&knobs::HIT_COLUMNS, 1.0));
    assert_eq!(rt.state.hit_columns(), 1);
    rt.update(|s, _| s.knobs.set(&knobs::HIT_COLUMNS, 0.0));
    assert_eq!(
        rt.state.hit_columns(),
        2,
        "Auto follows the width setting again"
    );
}

#[test]
#[cfg(feature = "knobs")]
fn the_chat_width_knobs_retune_the_thread() {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.add_exchange("Question", "Answer");
        s.sidebar.toggle();
        s.knobs.set(&knobs::COMFORTABLE_MIN, 100.0);
        s.knobs.set(&knobs::COMFORTABLE_MAX, 2000.0);
        s.knobs.set(&knobs::COMFORTABLE_FRACTION, 0.5);
    });
    rt.frame(1.0 / 60.0);
    let narrow = rt.presented("thread").map(|(r, _)| r[2]).unwrap();
    rt.update(|s, _| s.knobs.set(&knobs::COMFORTABLE_FRACTION, 0.9));
    settle(&mut rt);
    let broad = rt.presented("thread").map(|(r, _)| r[2]).unwrap();
    assert!(
        broad > narrow * 1.5,
        "a wider fraction widens the thread: {narrow} -> {broad}"
    );
}

#[test]
#[cfg(feature = "knobs")]
fn the_frost_alpha_knob_retunes_the_composer() {
    let mut rt = test_chat();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(1.0 / 60.0);
    let alpha = |rt: &Runtime<Chat>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .map(|e| e.fill.a)
            .unwrap()
    };
    assert!((alpha(&rt) - 0.72).abs() < 0.01, "the shipped frost");
    rt.update(|s, _| s.knobs.set(&knobs::FROST_ALPHA, 0.3));
    settle(&mut rt);
    assert!(
        (alpha(&rt) - 0.3).abs() < 0.01,
        "the knob retunes the frost"
    );
}

#[test]
#[cfg(feature = "knobs")]
fn cmd_shift_k_opens_the_knobs_panel_and_cmd_k_still_opens_the_command_bar() {
    let mut rt = test_chat();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    assert!(!rt.state.knobs.open);

    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.knobs.open, "⌘⇧K opens the knobs");
    assert!(
        !rt.state.command_bar.open,
        "and leaves the command bar alone"
    );
    assert!(rt.find("knobs-top").is_some(), "the panel is on screen");

    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(!rt.state.knobs.open, "Esc closes it");

    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(
        rt.state.command_bar.open,
        "plain ⌘K still toggles the command bar"
    );
}
