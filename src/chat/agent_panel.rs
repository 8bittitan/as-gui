//! The agent sheet's state: the agent's configuration load, and the handler the user chose
//! for each of its client tools. Bindings live under the application and agent they were
//! made for, because a tool name means whatever that agent's config says.

use std::collections::BTreeMap;
use std::sync::Arc;

use rune::TextEdit;

use super::Chat;
use crate::agent::{AgentConfig, CommandBinding, StreamEvent, is_builtin};
use crate::loading::Load;
use crate::store::{SavedAgent, ToolBinding};

pub const MAX_TIMEOUT_SECS: u64 = 600;

/// What the sheet is showing right now.
#[derive(Default)]
pub struct AgentPanel {
    /// The tool whose row takes focus when the sheet opens.
    pub focus: Option<String>,
    /// The tool whose command is being edited in place.
    pub editing: Option<String>,
    /// The tool whose handler select is open.
    pub select_open: Option<String>,
    pub focus_wanted: bool,
}

/// The command form of the tool being edited.
#[derive(Default)]
pub struct BindingDraft {
    pub program: TextEdit,
    /// One argument per line, so no quoting is needed.
    pub args: TextEdit,
    pub timeout: TextEdit,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct AgentConfigState {
    pub load: Load,
    pub config: Option<AgentConfig>,
}

impl Chat {
    pub fn open_agent_panel(&mut self, focus: Option<String>) {
        if self.selected_agent.is_none() || self.agent_panel.is_some() {
            return;
        }
        self.settings_open = false;
        self.agent_panel = Some(AgentPanel {
            focus_wanted: true,
            focus,
            ..AgentPanel::default()
        });
        self.load_agent_config();
    }

    /// The agent changed: its sheet and config belong to the one left.
    pub(super) fn forget_agent_config(&mut self) {
        self.agent_panel = None;
        self.agent_config = AgentConfigState::default();
    }

    pub fn close_agent_panel(&mut self) {
        self.agent_panel = None;
    }

    pub fn toggle_agent_panel(&mut self) {
        if self.agent_panel.is_some() {
            self.close_agent_panel();
        } else {
            self.open_agent_panel(None);
        }
    }

    pub fn retry_agent_config(&mut self) {
        self.load_agent_config();
    }

    fn load_agent_config(&mut self) {
        let Some(agent_id) = self.selected_agent.as_ref().map(|a| a.agent_id.clone()) else {
            return;
        };
        self.agent_config.load = Load::started(self.now);
        if !self.loads_from_directory() {
            self.agent_config.load = Load::Failed("No application is connected.".into());
            return;
        }
        let sender = self.sender();
        let directory = Arc::clone(&self.directory);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match directory.agent(&agent_id) {
                Ok(config) => StreamEvent::AgentConfigLoaded { agent_id, config },
                Err(error) => StreamEvent::AgentConfigFailed { agent_id, error },
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    pub(super) fn apply_agent_config(&mut self, ev: StreamEvent) {
        let selected = self.selected_agent.as_ref().map(|a| a.agent_id.as_str());
        match ev {
            StreamEvent::AgentConfigLoaded { agent_id, config } if selected == Some(&agent_id) => {
                self.agent_config.config = Some(config);
                self.agent_config.load = self.agent_config.load.finished(self.now);
            }
            StreamEvent::AgentConfigFailed { agent_id, error } if selected == Some(&agent_id) => {
                self.agent_config.load = Load::Failed(error);
            }
            _ => {}
        }
    }

    /// Where the selected agent's settings are kept: its application's, else the session's.
    fn agent_settings(&self) -> Option<&SavedAgent> {
        let id = &self.selected_agent.as_ref()?.agent_id;
        match self.active.and_then(|i| self.applications.get(i)) {
            Some(application) => application.agents.get(id),
            None => self.session_agents.get(id),
        }
    }

    fn agent_settings_mut(&mut self) -> Option<&mut SavedAgent> {
        let id = self.selected_agent.as_ref()?.agent_id.clone();
        let agents = match self.active.and_then(|i| self.applications.get_mut(i)) {
            Some(application) => &mut application.agents,
            None => &mut self.session_agents,
        };
        Some(agents.entry(id).or_default())
    }

    /// The selected agent's saved handlers, by tool name.
    pub fn tool_bindings(&self) -> BTreeMap<String, ToolBinding> {
        self.agent_settings()
            .map(|agent| agent.tools.clone())
            .unwrap_or_default()
    }

    /// Bound tools the agent's config no longer declares.
    pub fn dropped_tools(&self) -> Vec<String> {
        let Some(config) = &self.agent_config.config else {
            return Vec::new();
        };
        self.tool_bindings()
            .into_keys()
            .filter(|name| !config.client_tools.iter().any(|tool| &tool.name == name))
            .collect()
    }

    pub fn set_binding(&mut self, tool: &str, binding: ToolBinding) -> Result<(), String> {
        if let ToolBinding::Command {
            program,
            timeout_secs,
            ..
        } = &binding
        {
            if is_builtin(tool) {
                return Err(format!("{tool} is a built-in tool and can't be replaced."));
            }
            if program.trim().is_empty() {
                return Err("A command needs a program to run.".into());
            }
            if !(1..=MAX_TIMEOUT_SECS).contains(timeout_secs) {
                return Err(format!(
                    "The timeout is between 1 and {MAX_TIMEOUT_SECS} seconds."
                ));
            }
        }
        if binding == ToolBinding::Ask {
            self.remove_binding(tool);
            return Ok(());
        }
        let agent = self
            .agent_settings_mut()
            .ok_or("No agent is open.".to_string())?;
        agent.tools.insert(tool.into(), binding);
        self.persist();
        Ok(())
    }

    pub fn remove_binding(&mut self, tool: &str) {
        if let Some(agent) = self.agent_settings_mut() {
            agent.tools.remove(tool);
        }
        self.prune_empty_agents();
        self.persist();
    }

    fn prune_empty_agents(&mut self) {
        let agents = match self.active.and_then(|i| self.applications.get_mut(i)) {
            Some(application) => &mut application.agents,
            None => &mut self.session_agents,
        };
        agents.retain(|_, agent| !agent.tools.is_empty());
    }

    /// Loads the selected agent's commands into the registry's table.
    pub(super) fn sync_bindings(&self) {
        let commands =
            self.tool_bindings()
                .into_iter()
                .filter_map(|(name, binding)| match binding {
                    ToolBinding::Command {
                        program,
                        args,
                        timeout_secs,
                    } => Some((
                        name,
                        CommandBinding {
                            program,
                            args,
                            timeout_secs,
                        },
                    )),
                    ToolBinding::Ask => None,
                });
        self.bindings.replace(commands);
    }

    /// Opens the command form of `tool`, filled from its binding.
    pub fn edit_binding(&mut self, tool: &str) {
        let draft = match self.tool_bindings().get(tool) {
            Some(ToolBinding::Command {
                program,
                args,
                timeout_secs,
            }) => BindingDraft {
                program: TextEdit::new(program.clone()),
                args: TextEdit::new(args.join("\n")),
                timeout: TextEdit::new(timeout_secs.to_string()),
                error: None,
            },
            _ => BindingDraft {
                timeout: TextEdit::new(CommandBinding::default().timeout_secs.to_string()),
                ..BindingDraft::default()
            },
        };
        self.binding_draft = draft;
        if let Some(panel) = &mut self.agent_panel {
            panel.editing = Some(tool.into());
            panel.select_open = None;
        }
    }

    pub fn cancel_binding_edit(&mut self) {
        if let Some(panel) = &mut self.agent_panel {
            panel.editing = None;
        }
    }

    pub fn save_binding_edit(&mut self) {
        let Some(panel) = &self.agent_panel else {
            return;
        };
        let Some(tool) = panel.editing.clone() else {
            return;
        };
        let draft = &self.binding_draft;
        let args = draft
            .args
            .text
            .lines()
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect();
        let timeout = draft.timeout.text.trim().parse::<u64>();
        let binding = timeout
            .map_err(|_| "The timeout is a whole number of seconds.".to_string())
            .map(|timeout_secs| ToolBinding::Command {
                program: draft.program.text.trim().to_string(),
                args,
                timeout_secs,
            });
        match binding.and_then(|binding| self.set_binding(&tool, binding)) {
            Ok(()) => self.cancel_binding_edit(),
            Err(error) => self.binding_draft.error = Some(error),
        }
    }

    pub fn toggle_handler_select(&mut self, tool: &str) {
        if let Some(panel) = &mut self.agent_panel {
            panel.select_open = if panel.select_open.as_deref() == Some(tool) {
                None
            } else {
                Some(tool.into())
            };
        }
    }

    pub fn close_handler_select(&mut self) {
        if let Some(panel) = &mut self.agent_panel {
            panel.select_open = None;
        }
    }

    pub fn choose_handler(&mut self, tool: &str, choice: &str) {
        self.close_handler_select();
        match choice {
            COMMAND_LABEL => self.edit_binding(tool),
            ASK_LABEL => {
                self.remove_binding(tool);
                self.cancel_binding_edit();
            }
            _ => {}
        }
    }

    /// Moves focus to the row the sheet was opened for, once its config has loaded.
    pub(super) fn focus_agent_panel(&mut self, tx: &mut rune::Tx) -> bool {
        let ready = self.agent_config.load.phase(self.now) == crate::loading::Phase::Done;
        let Some(panel) = &mut self.agent_panel else {
            return false;
        };
        if !panel.focus_wanted {
            return false;
        }
        let Some(tool) = panel.focus.clone() else {
            panel.focus_wanted = false;
            return false;
        };
        if !ready {
            return false;
        }
        panel.focus_wanted = false;
        tx.focus(format!("{}-trigger", handler_key(&tool)), true);
        true
    }
}

pub const COMMAND_LABEL: &str = "Command…";
pub const ASK_LABEL: &str = "Ask me";

pub fn handler_key(tool: &str) -> String {
    format!("agent-tool-handler-{tool}")
}
