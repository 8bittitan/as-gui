//! One text delta per frame through the `begin_turn` sender, so the transcript re-renders
//! markdown and the composer glows every frame, over a conversation that makes the reply wrap.

use std::cell::RefCell;

use crate::agent::StreamEvent;
use crate::bench::{Scenario, fill_conversation, runtime};

pub(super) fn scenario() -> Scenario {
    let sender = RefCell::new(None);
    Scenario::new(
        "stream",
        || runtime(|s| fill_conversation(s, 6)),
        move |rt, i| {
            let mut sender = sender.borrow_mut();
            if i == 0 {
                *sender = Some(rt.state.begin_turn("Which shoes fit wide feet?"));
            } else if let Some(sender) = sender.as_ref() {
                let _ = sender.send(StreamEvent::TextDelta("shoe ".into()));
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::one_frame;

    #[test]
    fn the_stream_scenario_streams_its_first_delta() {
        let rt = one_frame("stream");
        assert!(rt.find("composer").is_some());
        assert!(rt.state.pending, "the turn is still streaming");
    }
}
