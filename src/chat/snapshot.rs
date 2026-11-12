//! What the Quick Ask panel shares with the main window: the agent and appearance it reads
//! (`AgentSnapshot`), and the exchange it hands back.

use std::sync::Arc;

use super::{Chat, Session};
use crate::agent::{AgentSnapshot, ChatMessage};
use crate::loading::Load;

impl Chat {
    /// Publishes the agent in use, the application it belongs to and the appearance for the Quick Ask panel (no agent:
    /// nothing to ask yet).
    pub(super) fn sync_snapshot(&self) {
        self.refresh_menu();
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let application = self.application_label().zip(self.active_colour());
        let mut shared = snapshot.lock().unwrap_or_else(|p| p.into_inner());
        *shared = match &self.selected_agent {
            Some(agent) => AgentSnapshot {
                agent_id: agent.agent_id.clone(),
                agent_name: agent.name.clone(),
                client: Arc::clone(&self.client),
                theme: self.appearance.theme,
                accent: self.appearance.accent,
                application,
            },
            None => AgentSnapshot {
                theme: self.appearance.theme,
                accent: self.appearance.accent,
                application,
                ..AgentSnapshot::default()
            },
        };
    }

    /// Receives a Quick Ask exchange as a session in the main window, selected. The copy
    /// starts fresh: continuing it opens a new conversation rather than replaying ids the
    /// server already stored under the panel's turn.
    pub fn receive_quick_ask(
        &mut self,
        prompt: String,
        reply: String,
        conversation_id: Option<String>,
    ) {
        let user = ChatMessage::user(self.fresh_id(), prompt.clone());
        let mut assistant = ChatMessage::assistant(self.fresh_id());
        assistant.text = reply;
        assistant.streaming = false;
        self.sessions.push(Session {
            id: conversation_id,
            hidden: false,
            title: prompt.chars().take(40).collect(),
            messages: vec![user, assistant],
            agent_message_id: None,
            // The panel's conversation already holds this exchange on the server.
            history: Load::Idle.finished(self.now),
        });
        self.current = self.sessions.len() - 1;
        self.open_window();
    }
}
