//! Reading and writing the persisted state (`store`): the applications, the last agent, the
//! appearance, hidden conversations and changed shortcuts.

use super::Chat;
use super::applications::{Application, is_valid_app_id};
use crate::keymap::{Action, Keymap};
use crate::store::{self, Persisted};

impl Chat {
    /// Applies the persisted state: the appearance now, the agent once the directory lists it.
    pub fn restore(&mut self, state: &Persisted) {
        self.hidden_conversations = state.hidden.iter().cloned().collect();
        self.appearance.width = state.width.unwrap_or(self.appearance.width);
        self.appearance.theme = state.theme.unwrap_or(self.appearance.theme);
        self.appearance.accent = state.accent.unwrap_or(self.appearance.accent);
        self.appearance.tint_window = state.tint_window.unwrap_or(self.appearance.tint_window);
        self.appearance.open_at_launch = state
            .open_at_launch
            .unwrap_or(self.appearance.open_at_launch);
        self.notify_replies = state.notify_replies.unwrap_or(self.notify_replies);
        let saved: Vec<_> = state
            .applications
            .iter()
            .filter(|a| is_valid_app_id(&a.app_id))
            .collect();
        if !saved.is_empty() {
            let applications = saved
                .iter()
                .enumerate()
                .map(|(i, saved)| Application::from_saved(saved, i))
                .collect();
            let active = state
                .active_app
                .as_ref()
                .and_then(|id| saved.iter().position(|a| &a.app_id == id));
            self.set_applications(applications, active.or(Some(0)));
        }
        self.sync_snapshot();
        self.pending_agent_id = self
            .active
            .and_then(|i| self.applications.get(i))
            .and_then(|a| a.last_agent.as_ref().map(|(id, _)| id.clone()))
            .or_else(|| state.agent_id.clone());
        self.keymap = Keymap::load(&state.keybinds);
        for action in Action::ALL {
            self.sync_recorder(action);
        }
    }

    /// Writes the last selected agent and the appearance settings, when persistence is on.
    pub(super) fn persist(&self) {
        let Some(path) = self.persist_path.as_ref() else {
            return;
        };
        store::save_to(
            path,
            &Persisted {
                applications: self
                    .applications
                    .iter()
                    .map(Application::to_saved)
                    .collect(),
                active_app: self
                    .active
                    .and_then(|i| self.applications.get(i))
                    .map(|a| a.app_id.clone()),
                agent_id: None,
                app_id: None,
                api_key: None,
                theme: Some(self.appearance.theme),
                accent: Some(self.appearance.accent),
                width: Some(self.appearance.width),
                tint_window: Some(self.appearance.tint_window),
                open_at_launch: Some(self.appearance.open_at_launch),
                notify_replies: Some(self.notify_replies),
                hidden: self.hidden_conversations.iter().cloned().collect(),
                keybinds: self.keymap.to_saved(),
            },
        );
    }
}
