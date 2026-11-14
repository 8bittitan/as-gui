//! A wheel step per frame through a 200-message transcript: up for the first half, back down
//! for the second, so the offset never runs out of range.

use crate::bench::{Scenario, fill_conversation, runtime};

pub(super) fn scenario() -> Scenario {
    Scenario::new(
        "scroll",
        || runtime(|s| fill_conversation(s, 200)),
        |rt, i| {
            let dy = if i < 60 { 40.0 } else { -40.0 };
            if let Some((x, y)) = rt.find("messages") {
                rt.wheel(x, y, dy);
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::one_frame;

    #[test]
    fn the_scroll_scenario_builds_two_hundred_messages() {
        let rt = one_frame("scroll");
        assert!(rt.find("composer").is_some());
        assert_eq!(rt.state.sessions[0].messages.len(), 400);
    }
}
