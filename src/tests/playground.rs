use super::*;

#[test]
fn the_conversation_trigger_stacks_an_exchange() {
    let mut rt = playground_app();
    rt.open_playground("conversation", None);
    let before = rt.playground_state::<Chat>().unwrap().sessions[0]
        .messages
        .len();

    rt.playground_trigger('a');
    let stage = rt.playground_state::<Chat>().unwrap();
    assert_eq!(
        stage.sessions[0].messages.len(),
        before + 2,
        "the trigger adds a user and an assistant message"
    );
    let last = stage.sessions[0].messages.last().unwrap();
    assert!(!last.streaming, "the added exchange is settled");
    assert!(
        rt.playground_element(&format!("m-{}", last.id)).is_some(),
        "the new bubbles render on the stage"
    );
    assert!(rt.state.sessions[0].messages.is_empty());
}

#[test]
fn the_page_draws_the_real_state_live() {
    let mut rt = playground_app();
    rt.open_playground("App/Conversation", None);
    assert!(
        rt.playground_state::<Chat>().is_none(),
        "a page's stage is the real state, not a copy"
    );
    assert!(
        rt.find("agent-picker").is_some(),
        "the real state starts on the picker"
    );

    rt.update(|s, _| s.agents_load = Load::Idle.finished(0.0));
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("agent-list").is_some(),
        "the page follows the real state's changes"
    );
    assert!(
        rt.playground_state::<Chat>().is_none(),
        "and the real state keeps its identity"
    );
}
