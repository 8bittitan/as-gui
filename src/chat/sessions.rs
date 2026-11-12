//! The current agent's sessions: starting, selecting, hiding and restoring them.

use crate::agent::ChatMessage;
use crate::loading::Load;

use super::Chat;

pub struct Session {
    /// The Agent Studio conversation id, when this session is one of the agent's own.
    pub id: Option<String>,
    /// Hidden conversations stay in the data but out of the sidebar and rail.
    pub hidden: bool,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub agent_message_id: Option<String>,
    /// The conversation's messages: loading, in, or unavailable. Local sessions never load.
    pub history: Load,
}

impl Chat {
    pub fn new_session(&mut self) {
        self.cancel_turn();
        self.composer.clear();
        self.sessions.push(Session {
            id: None,
            hidden: false,
            title: "New conversation".into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        });
        self.current = self.sessions.len() - 1;
    }

    pub fn select_session(&mut self, i: usize) {
        if i < self.sessions.len() {
            // Selecting a hidden conversation restores it.
            if self.sessions[i].hidden {
                self.unhide_session(i);
            }
            if i != self.current {
                self.cancel_turn();
                self.current = i;
                self.load_conversation();
            }
            // Whatever the conversation needed a nudge for, it's on screen now.
            self.dismiss_notifications(i);
        }
    }

    /// Hides a conversation: out of the sidebar and rail, data kept. Remote ids persist.
    pub fn hide_session(&mut self, i: usize) {
        let Some(session) = self.sessions.get_mut(i) else {
            return;
        };
        session.hidden = true;
        if let Some(id) = &session.id {
            self.hidden_conversations.insert(id.clone());
        }
        // Don't leave the selection on something the user can't see.
        if self.sessions.get(self.current).is_some_and(|s| s.hidden)
            && let Some(first) = self.sessions.iter().position(|s| !s.hidden)
        {
            self.current = first;
        }
        self.persist();
    }

    /// Brings a hidden conversation back.
    pub fn unhide_session(&mut self, i: usize) {
        let Some(session) = self.sessions.get_mut(i) else {
            return;
        };
        session.hidden = false;
        if let Some(id) = &session.id {
            self.hidden_conversations.remove(id);
        }
        self.persist();
    }

    /// Appends a settled user/assistant exchange to the current session, without touching the
    /// turn machinery; the playground's conversation trigger stacks these onto the scene.
    pub fn add_exchange(&mut self, user: impl Into<String>, answer: impl Into<String>) {
        let user = ChatMessage::user(self.fresh_id(), user.into());
        let mut reply = ChatMessage::assistant(self.fresh_id());
        reply.streaming = false;
        reply.text = answer.into();
        let session = &mut self.sessions[self.current];
        session.messages.push(user);
        session.messages.push(reply);
    }
}
