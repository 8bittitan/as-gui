//! The storybook ([`App::playground`](rune::App::playground), ⌘⇧D in debug builds): the app's
//! scenes with triggers and variants, plus a page of the real state, live on a stage that never
//! touches the running app.

use rune::{Playground, Stage, Story};

use crate::chat::Chat;

pub fn playground() -> Playground<Chat> {
    Playground::<Chat>::new()
        .stage("agents", Stage::Fill)
        .stage("conversation", Stage::Fill)
        .stage("streaming", Stage::Fill)
        .stage("tools", Stage::Fill)
        .page("App/Conversation", crate::shell::view)
        .story(
            Story::new("App/Tuning", tuning_state, crate::shell::view)
                .stage(Stage::Fill)
                .side_keys(
                    |s: &Chat, active, cx| {
                        rune_knobs::inline(
                            &s.knobs,
                            crate::knobs::GROUPS,
                            if cx.system_dark() {
                                rune_knobs::Style::dark()
                            } else {
                                rune_knobs::Style::light()
                            },
                            |s: &mut Chat| &mut s.knobs,
                            active,
                        )
                    },
                    rune_knobs::side_key(crate::knobs::GROUPS, |s: &mut Chat| &mut s.knobs),
                ),
        )
        .trigger("conversation", 'a', "Add exchange", |s, _| {
            s.add_exchange(
                "What else can this agent do?",
                "Quite a lot — press A again to stack another exchange onto the thread.",
            );
        })
        .variant("tools", "Collapsed", |s| {
            for session in &mut s.sessions {
                for m in &mut session.messages {
                    for t in &mut m.tools {
                        t.open = false;
                    }
                }
            }
        })
        .variant("agents", "Loading", |s| {
            s.agents.clear();
            s.agents_load = crate::loading::Load::Loading { since: -1.0 };
        })
        .variant("agents", "Empty", |s| s.agents.clear())
}

/// The tuning story's own state: a conversation with search hits to tune against.
fn tuning_state() -> Chat {
    let mut chat = crate::mock_chat();
    crate::scenes::tools(&mut chat);
    chat
}
