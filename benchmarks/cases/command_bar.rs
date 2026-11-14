//! ⌘K opens the bar, a query types one character per frame over a large entry set, Escape
//! closes; the cycle repeats for the whole run so every measured frame carries work.

use rune::{Key, KeyEvent, Mods};

use crate::bench::{Scenario, published, runtime};
use crate::chat::Session;
use crate::loading::Load;

const QUERY: [char; 10] = ['s', 'h', 'o', 'e', 's', ' ', 'w', 'i', 'd', 'e'];

pub(super) fn scenario() -> Scenario {
    Scenario::new(
        "command-bar",
        || runtime(many_entries),
        |rt, i| {
            let beat = i % 15;
            let key = if beat == 0 {
                Key::Command('k')
            } else if (1..=8).contains(&beat) {
                Key::Text(QUERY[(i - 1) % QUERY.len()].to_string())
            } else if beat == 9 {
                Key::Escape
            } else {
                return;
            };
            rt.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
        },
    )
}

fn many_entries(s: &mut crate::chat::Chat) {
    s.agents = (0..50)
        .map(|i| published(&format!("agent{i}"), &format!("Agent number {i}")))
        .collect();
    s.sessions = (0..50)
        .map(|i| Session {
            id: None,
            hidden: false,
            title: format!("conversation {i} about shoes and insoles"),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::super::one_frame;

    #[test]
    fn the_command_bar_scenario_opens_the_bar_on_frame_one() {
        let rt = one_frame("command-bar");
        assert!(rt.state.command_bar.open, "⌘K opened the bar");
    }
}
