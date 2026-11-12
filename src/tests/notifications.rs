use std::sync::{Arc, Mutex};

use rune_notify::Notification;

use super::*;
use crate::agent::SessionInfo;
use crate::notify::Notifier;

struct FakeNotifier {
    focused: bool,
    posts: Mutex<Vec<Notification>>,
    removed: Mutex<Vec<String>>,
}

impl FakeNotifier {
    fn unfocused() -> Arc<FakeNotifier> {
        Arc::new(FakeNotifier {
            focused: false,
            posts: Mutex::new(Vec::new()),
            removed: Mutex::new(Vec::new()),
        })
    }

    fn focused() -> Arc<FakeNotifier> {
        Arc::new(FakeNotifier {
            focused: true,
            ..FakeNotifier::placeholder()
        })
    }

    fn placeholder() -> FakeNotifier {
        FakeNotifier {
            focused: false,
            posts: Mutex::new(Vec::new()),
            removed: Mutex::new(Vec::new()),
        }
    }

    fn posts(&self) -> Vec<Notification> {
        self.posts.lock().unwrap().clone()
    }

    fn removed(&self) -> Vec<String> {
        self.removed.lock().unwrap().clone()
    }
}

impl Notifier for FakeNotifier {
    fn post(&self, n: &Notification) -> bool {
        self.posts.lock().unwrap().push(n.clone());
        true
    }

    fn remove(&self, ids: &[&str]) {
        self.removed
            .lock()
            .unwrap()
            .extend(ids.iter().map(|s| s.to_string()));
    }

    fn focused(&self) -> bool {
        self.focused
    }
}

/// A turn that settles after more than the notification threshold, with the app in the
/// background. `reply` is what streams before it settles.
fn long_background_turn(notifier: &Arc<FakeNotifier>, reply: &str) -> Runtime<Chat> {
    let mut rt = test_chat();
    let shared = notifier.clone();
    rt.update(move |s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = shared;
    });
    settle_finished(&mut rt, reply);
    rt
}

fn settle_finished(rt: &mut Runtime<Chat>, reply: &str) {
    let sender = rt.state.begin_turn("A question");
    sender
        .send(StreamEvent::TextDelta(reply.to_string()))
        .unwrap();
    seconds(rt, 6.0);
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
}

/// Frames over `secs` seconds, the way the app's own frame clock ticks.
fn seconds(rt: &mut Runtime<Chat>, secs: f64) {
    for _ in 0..(secs * 60.0) as usize {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn a_long_background_reply_notifies_once_with_the_replys_first_line() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = long_background_turn(&notifier, "The **answer** arrived");
    rt.frame(7.5);
    assert_eq!(
        notifier.posts(),
        vec![
            Notification::new("reply-m2", "One")
                .body("The answer arrived")
                .info("agent", "a1")
                .info("session", "m2")
        ],
    );
}

#[test]
fn a_failed_background_reply_notifies_the_error() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = notifier.clone();
    });
    let sender = rt.state.begin_turn("A question");
    seconds(&mut rt, 6.0);
    sender
        .send(StreamEvent::Failed("the index is down".into()))
        .unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        notifier.posts(),
        vec![
            Notification::new("reply-m2", "One")
                .body("Couldn't finish: the index is down")
                .info("agent", "a1")
                .info("session", "m2")
        ],
    );
}

#[test]
fn a_background_reply_that_bound_a_conversation_routes_by_conversation_id() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = notifier.clone();
    });
    let sender = rt.state.begin_turn("A question");
    sender
        .send(StreamEvent::ConversationStarted("cnv-9".into()))
        .unwrap();
    sender
        .send(StreamEvent::TextDelta("Answer".into()))
        .unwrap();
    seconds(&mut rt, 6.0);
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        notifier.posts(),
        vec![
            Notification::new("reply-cnv-9", "One")
                .body("Answer")
                .info("agent", "a1")
                .info("session", "cnv-9")
        ],
    );
}

#[test]
fn no_notification_while_the_app_is_focused() {
    let notifier = FakeNotifier::focused();
    let _rt = long_background_turn(&notifier, "Answer");
    assert!(notifier.posts().is_empty());
}

#[test]
fn no_notification_for_a_quick_reply() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = notifier.clone();
    });
    let sender = rt.state.begin_turn("A question");
    sender.send(StreamEvent::TextDelta("Fast".into())).unwrap();
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    seconds(&mut rt, 2.0);
    assert!(notifier.posts().is_empty());
}

#[test]
fn no_notification_with_the_setting_off() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = notifier.clone();
        s.set_notify_replies(false);
    });
    settle_finished(&mut rt, "Answer");
    assert!(notifier.posts().is_empty());
}

#[test]
fn an_empty_reply_does_not_notify() {
    let notifier = FakeNotifier::unfocused();
    let _rt = long_background_turn(&notifier, "");
    assert!(notifier.posts().is_empty());
}

#[test]
fn a_click_selects_the_session_that_asked() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = long_background_turn(&notifier, "Answer");
    rt.state.new_session();
    rt.state.open_session_by_id("a1", "m2");
    assert_eq!(rt.state.current, 0, "the click returns to the conversation");
}

#[test]
fn a_click_selects_the_conversation_even_across_agents() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.notifier = notifier.clone();
    });
    let sender = rt.state.begin_turn("A question");
    sender
        .send(StreamEvent::ConversationStarted("cnv-a".into()))
        .unwrap();
    sender
        .send(StreamEvent::TextDelta("Answer".into()))
        .unwrap();
    seconds(&mut rt, 6.0);
    sender.send(StreamEvent::Finished).unwrap();
    drop(sender);
    rt.frame(1.0 / 60.0);

    // The user moved on to another agent before clicking.
    rt.update(|s, _| {
        s.agents = vec![agent_one(), agent("a2", "Two", AgentStatus::Published)];
        s.selected_agent = Some(agent("a2", "Two", AgentStatus::Published));
    });
    rt.state.open_session_by_id("a1", "cnv-a");
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| {
        s.apply(StreamEvent::SessionsLoaded(vec![SessionInfo {
            id: "cnv-a".into(),
            title: Some("A question".into()),
        }]))
    });
    assert_eq!(
        rt.state
            .selected_agent
            .as_ref()
            .map(|a| a.agent_id.as_str()),
        Some("a1"),
        "the click switches back to the agent"
    );
    assert_eq!(
        rt.state.sessions[rt.state.current].id.as_deref(),
        Some("cnv-a"),
        "and opens the conversation"
    );
}

#[test]
fn selecting_the_conversation_removes_its_notification() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = long_background_turn(&notifier, "Answer");
    rt.state.new_session();
    rt.state.select_session(0);
    rt.frame(1.0 / 60.0);
    assert_eq!(notifier.removed(), vec!["reply-m2".to_string()]);
}

#[test]
fn focusing_the_app_clears_the_open_conversations_notification() {
    let notifier = FakeNotifier::unfocused();
    let mut rt = long_background_turn(&notifier, "Answer");
    rt.update(|s, tx| chat::focus_changed(s, true, tx));
    rt.frame(1.0 / 60.0);
    assert_eq!(notifier.removed(), vec!["reply-m2".to_string()]);
    assert!(
        notifier.posts().len() == 1,
        "the notification posted once, then was taken back"
    );
}

#[test]
fn the_notification_setting_defaults_on_and_restores_from_state() {
    let mut rt = test_chat();
    assert!(rt.state.notify_replies, "on by default");
    rt.update(|s, _| {
        s.restore(&Persisted {
            notify_replies: Some(false),
            ..Persisted::default()
        })
    });
    assert!(!rt.state.notify_replies);
}

#[test]
fn the_settings_switch_turns_the_setting_off_and_it_saves() {
    let path = std::env::temp_dir().join("as-gui-notify-replies/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = test_chat_sized((720.0, 1900.0));
    rt.update(|s, _| {
        s.selected_agent = Some(agent_one());
        s.persist_path = Some(path.clone());
        s.settings_open = true;
    });
    settle(&mut rt);
    assert!(
        rt.find("notify-replies").is_some(),
        "the switch is in the settings dialog"
    );
    tap(&mut rt, "notify-replies");
    rt.frame(0.0);
    assert!(!rt.state.notify_replies);
    assert_eq!(
        crate::store::load_from(&path).notify_replies,
        Some(false),
        "the setting persisted"
    );
}
