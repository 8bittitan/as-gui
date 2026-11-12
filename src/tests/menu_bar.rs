use super::*;

#[cfg(target_os = "macos")]
#[test]
fn the_open_at_launch_switch_in_settings_turns_the_setting_on() {
    let mut rt = test_chat_sized((720.0, 1900.0));
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.settings_open = true;
    });
    settle(&mut rt);
    tap(&mut rt, "open-at-launch");
    rt.frame(0.0);
    assert!(rt.state.appearance.open_at_launch);
}
