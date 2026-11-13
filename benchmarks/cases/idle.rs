//! A settled conversation screen with nothing happening per frame: the floor every other
//! scenario stands above.

use crate::bench::{Scenario, fill_conversation, runtime};

pub(super) fn scenario() -> Scenario {
    Scenario::new("idle", || runtime(|s| fill_conversation(s, 6)), |_, _| {})
}

#[cfg(test)]
mod tests {
    use super::super::one_frame;

    #[test]
    fn the_idle_scenario_sits_on_a_settled_conversation() {
        let rt = one_frame("idle");
        assert!(rt.find("composer").is_some());
        assert!(rt.find("thread").is_some());
    }
}
