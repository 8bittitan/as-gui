//! What the controls on a message do: disclosure toggles and 👍/👎 feedback.

use std::sync::Arc;

use tracing::warn;

use super::Chat;

impl Chat {
    pub fn toggle_hits_expanded(&mut self, message_id: &str, index: usize) {
        let key = (message_id.to_string(), index);
        if !self.hits_expanded.remove(&key) {
            self.hits_expanded.insert(key);
        }
    }

    pub fn hits_expanded(&self, message_id: &str, index: usize) -> bool {
        self.hits_expanded
            .contains(&(message_id.to_string(), index))
    }

    pub fn toggle_tool_open(&mut self, message_id: &str, index: usize) {
        if let Some(t) = self
            .message_mut(message_id)
            .and_then(|m| m.tools.get_mut(index))
        {
            t.open = !t.open;
        }
    }

    pub fn toggle_timeline(&mut self, message_id: &str) {
        if let Some(trace) = self.message_mut(message_id).and_then(|m| m.trace.as_mut()) {
            trace.open = !trace.open;
        }
    }

    pub fn toggle_span_open(&mut self, message_id: &str, index: usize) {
        if let Some(span) = self
            .message_mut(message_id)
            .and_then(|m| m.trace.as_mut())
            .and_then(|t| t.spans.get_mut(index))
        {
            span.open = !span.open;
        }
    }

    /// Sends 👍/👎 for an assistant message to Agent Studio, best effort: only messages the
    /// server persisted can take feedback, and a failed vote logs instead of interrupting.
    pub fn vote(&mut self, message_id: &str, up: bool) {
        let value: i8 = if up { 1 } else { 0 };
        let Some(server_id) = self.message_mut(message_id).and_then(|m| {
            m.vote = Some(value);
            m.server_id.clone()
        }) else {
            return;
        };
        let Some(agent_id) = self.selected_agent.as_ref().map(|a| a.agent_id.clone()) else {
            return;
        };
        let client = Arc::clone(&self.client);
        std::thread::spawn(move || {
            if let Err(e) = client.send_feedback(&agent_id, &server_id, value) {
                warn!("feedback failed: {e}");
            }
        });
    }
}
