//! The search tool's call and its hits go up front through the sender; after that the card
//! header toggles open and closed every 30 frames, the way `scripts/chat.json` taps it.

use std::cell::RefCell;

use crate::agent::StreamEvent;
use crate::bench::{Scenario, fill_conversation, runtime};
use rune::Runtime;

pub(super) fn scenario() -> Scenario {
    let sender = RefCell::new(None);
    Scenario::new(
        "tool-card",
        || runtime(|s| fill_conversation(s, 6)),
        move |rt, i| {
            let mut sender = sender.borrow_mut();
            if i == 0 {
                *sender = Some(rt.state.begin_turn("Which shoes fit wide feet?"));
                let sender = sender.as_ref().unwrap();
                let _ = sender.send(StreamEvent::ToolStarted {
                    call_id: "bench_search".into(),
                    name: "search_tool".into(),
                });
                let _ = sender.send(StreamEvent::ToolFinished {
                    call_id: "bench_search".into(),
                    name: "search_tool".into(),
                    output: Some(hits_json()),
                });
                let _ = sender.send(StreamEvent::Finished);
            } else if i % 30 == 0 {
                tap(rt, "tool-search_tool");
            }
        },
    )
}

fn tap(rt: &mut Runtime<crate::chat::Chat>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("no element keyed {key}"));
    rt.pointer_move(x, y);
    rt.frame(0.0);
    rt.pointer_down(x, y);
    rt.frame(0.0);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

fn hits_json() -> String {
    serde_json::json!({ "hits": [
        { "title": "Fit guide: wide feet", "path": "/guides/wide-fit",
          "content": "The current best **shoe and insole** pairings for wide feet." },
        { "title": "Shoe tier list", "path": "/guides/shoes",
          "content": "Where every shoe lands **after the latest release**." },
        { "title": "Insole pairings", "path": "/guides/insoles",
          "content": "Pairings that cover **arch support** needs." },
    ] })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::one_frame;

    #[test]
    fn the_tool_card_scenario_renders_the_tool_card() {
        let rt = one_frame("tool-card");
        assert!(
            rt.find("tool-search_tool").is_some(),
            "the card header is on screen"
        );
    }
}
