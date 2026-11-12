//! Notifications when a reply settles in the background: what posts when a turn finishes,
//! what a click on it opens, and what is taken back once the conversation is on screen.

use rune::Tx;
use rune_notify::Notification;

use super::Chat;
use crate::agent::AgentStatus;
use crate::notify::first_line;

/// How long a turn must run before its settling is worth a notification.
const MIN_TURN_SECS: f64 = 5.0;

impl Chat {
    /// Posts a notification for the turn that just settled (`Finished` or `Failed`), when
    /// the setting is on, the app isn't in front, and the turn ran long enough to matter.
    pub(super) fn maybe_notify(&mut self) {
        if !self.notify_replies || self.notifier.focused() {
            return;
        }
        let duration = self.streaming_message().and_then(|m| m.thought);
        if duration.is_none_or(|d| d < MIN_TURN_SECS) {
            return;
        }
        let failed = self.error.clone();
        let Some(message) = self
            .sessions
            .get(self.current)
            .and_then(|session| session.messages.last())
        else {
            return;
        };
        let body = match &failed {
            Some(error) => first_line(&format!("Couldn't finish: {error}")),
            None => first_line(&message.text),
        };
        if body.is_empty() {
            return;
        }
        let title = self
            .selected_agent
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "as-gui".into());
        let agent_id = self
            .selected_agent
            .as_ref()
            .map(|a| a.agent_id.clone())
            .unwrap_or_default();
        // A session the server has named routes by conversation id; a local one by the
        // reply's own message id, which is all it has.
        let target = self.sessions[self.current]
            .id
            .clone()
            .unwrap_or_else(|| message.id.clone());
        let id = format!("reply-{target}");
        let posted = self.notifier.post(
            &Notification::new(&id, title)
                .body(body)
                .info("agent", &agent_id)
                .info("session", &target),
        );
        if posted {
            self.pending_notifications.insert(target, id);
        }
    }

    /// What a notification click stands for: back to `agent_id`, on the conversation —
    /// or, for a session the server hasn't named yet, the session holding that reply.
    pub fn open_session_by_id(&mut self, agent_id: &str, target: &str) {
        if self.selected_agent.as_ref().map(|a| a.agent_id.as_str()) != Some(agent_id) {
            let Some(index) = self
                .agents
                .iter()
                .position(|a| a.agent_id == agent_id && a.status == AgentStatus::Published)
            else {
                return;
            };
            self.choose_agent(index);
            // A remote conversation opens when its session list lands; a local session's
            // messages don't survive the switch, so only its agent comes back.
            self.pending_conversation = Some(target.to_string());
            return;
        }
        let session = self.sessions.iter().position(|s| {
            s.id.as_deref() == Some(target) || s.messages.iter().any(|m| m.id == target)
        });
        if let Some(i) = session {
            self.select_session(i);
        }
    }

    /// Takes the pending notifications for session `i` (its conversation, or any of its
    /// messages) out of the system's list: the conversation is on screen.
    pub(super) fn dismiss_notifications(&mut self, i: usize) {
        let Some(session) = self.sessions.get(i) else {
            return;
        };
        let mut keys: Vec<String> = session.messages.iter().map(|m| m.id.clone()).collect();
        keys.extend(session.id.clone());
        let ids: Vec<String> = self
            .pending_notifications
            .iter()
            .filter(|(target, _)| keys.contains(target))
            .map(|(_, id)| id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.notifier.remove(&refs);
        self.pending_notifications.retain(|_, id| !ids.contains(id));
    }

    pub fn set_notify_replies(&mut self, on: bool) {
        self.notify_replies = on;
        self.persist();
    }
}

/// The main window took focus (or gave it up): whatever was on screen has been seen, so
/// its notification comes out of the system's list.
pub fn focus_changed(s: &mut Chat, focused: bool, _tx: &mut Tx) {
    if focused {
        s.dismiss_notifications(s.current);
    }
}
