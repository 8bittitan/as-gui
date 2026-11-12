//! Everything the agent directory feeds: the agent list, choosing an agent, its conversation
//! list and a conversation's messages. Each load runs on a thread and lands on the load
//! channel tagged with the generation it started in, so a result for an agent or
//! application the user already left is dropped.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use tracing::warn;

use super::{Chat, Session};
use crate::agent::{AgentClient, AgentStatus, AlgoliaClient, StreamEvent};
use crate::loading::Load;

/// A load's result, tagged with the generation it started in.
pub struct Loaded {
    generation: u64,
    event: StreamEvent,
}

const ANY_GENERATION: u64 = 0;

/// The sending half of a load: results of an older generation are dropped on arrival.
pub struct LoadSender {
    tx: Sender<Loaded>,
    generation: u64,
}

impl LoadSender {
    pub fn send(&self, event: StreamEvent) -> Result<(), mpsc::SendError<StreamEvent>> {
        self.tx
            .send(Loaded {
                generation: self.generation,
                event,
            })
            .map_err(|e| mpsc::SendError(e.0.event))
    }
}

impl Chat {
    /// Starts a directory fetch and returns the sender the fetch thread streams into.
    pub fn begin_agent_load(&mut self) -> LoadSender {
        self.agents.clear();
        self.agents_load = Load::started(self.now);
        if self.selected_agent.is_none() && !matches!(self.launch, Load::Loading { .. }) {
            self.launch = Load::started(self.now);
        }
        LoadSender {
            tx: self.load_tx.clone(),
            generation: ANY_GENERATION,
        }
    }

    fn finish_launch(&mut self) {
        if matches!(self.launch, Load::Loading { .. }) {
            self.launch = self.launch.finished(self.now);
        }
    }

    pub fn choose_agent(&mut self, index: usize) {
        let Some(agent) = self.agents.get(index) else {
            return;
        };
        if agent.status != AgentStatus::Published {
            return;
        }
        self.client = self.client_for(&agent.agent_id);
        if let Some(i) = self.active
            && let Some(application) = self.applications.get_mut(i)
        {
            application.last_agent = Some((agent.agent_id.clone(), agent.name.clone()));
        }
        self.selected_agent = Some(agent.clone());
        self.cancel_turn();
        self.sessions = vec![Session {
            id: None,
            hidden: false,
            title: "New conversation".into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        }];
        self.current = 0;
        self.error = None;
        self.generation += 1;
        self.pending_conversation = None;
        self.sessions_load = Load::Idle;
        self.show_all_sessions = false;
        self.sync_snapshot();
        self.persist();
        if self.loads_from_directory() {
            self.load_sessions();
        } else {
            self.finish_launch();
        }
    }

    pub(super) fn client_for(&self, agent_id: &str) -> Arc<dyn AgentClient> {
        match self.credentials() {
            Some((app_id, api_key)) => Arc::new(AlgoliaClient {
                app_id: app_id.into(),
                api_key: api_key.into(),
                agent_id: agent_id.into(),
            }),
            None => crate::agent::mock_client(),
        }
    }

    /// Whether `poll` keeps the directory and conversations loading: with credentials, or on
    /// the mock agent chosen outright (credentials plus a forced mock is tests driving loads).
    pub(crate) fn loads_from_directory(&self) -> bool {
        self.active.is_some() != self.mock_forced
    }

    /// Fetches the agent list on a background thread; `poll` applies the result. Runs
    /// against `directory`, which follows the credentials (`Chat::use_credentials`).
    pub fn load_agents(&mut self) {
        let sender = self.begin_agent_load();
        let directory = Arc::clone(&self.directory);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match directory.list() {
                Ok(agents) => StreamEvent::AgentsLoaded(agents),
                Err(e) => StreamEvent::AgentsFailed(e),
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    /// Fetches the selected agent's conversations on a background thread; `poll` applies
    /// them.
    pub fn load_sessions(&mut self) {
        let Some(agent_id) = self.selected_agent.as_ref().map(|a| a.agent_id.clone()) else {
            return;
        };
        self.sessions_load = Load::started(self.now);
        let sender = self.sender();
        let directory = Arc::clone(&self.directory);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match directory.sessions(&agent_id) {
                Ok(sessions) => StreamEvent::SessionsLoaded(sessions),
                Err(e) => StreamEvent::SessionsFailed(e),
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    pub fn retry_sessions(&mut self) {
        self.sessions_load = Load::Idle;
        if self.loads_from_directory() {
            self.load_sessions();
        }
    }

    /// Fetches the current remote session's messages, when it has some and they aren't in.
    pub fn load_conversation(&mut self) {
        self.begin_history(self.now);
    }

    pub fn retry_history(&mut self) {
        if let Some(session) = self.sessions.get_mut(self.current) {
            session.history = Load::Idle;
        }
        self.load_conversation();
    }

    /// Starts the current session's message load, counting its wait from `since`: the list
    /// that led here may already have waited, and the placeholder must not restart.
    fn begin_history(&mut self, since: f64) {
        let Some(agent_id) = self.selected_agent.as_ref().map(|a| a.agent_id.clone()) else {
            return;
        };
        let Some(session) = self.sessions.get_mut(self.current) else {
            return;
        };
        let Some(conversation_id) = session.id.clone() else {
            return;
        };
        if !session.history.is_idle() {
            return;
        }
        session.history = Load::Loading { since };
        let sender = self.sender();
        let directory = Arc::clone(&self.directory);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match directory.conversation(&agent_id, &conversation_id) {
                Ok(messages) => StreamEvent::ConversationLoaded {
                    session_id: conversation_id,
                    messages,
                },
                Err(error) => StreamEvent::ConversationFailed {
                    session_id: conversation_id,
                    error,
                },
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    /// A sender for a load of the current generation: if the agent or credentials change
    /// before it lands, its result is dropped.
    pub(super) fn sender(&self) -> LoadSender {
        LoadSender {
            tx: self.load_tx.clone(),
            generation: self.generation,
        }
    }

    /// Applies a directory load's result.
    pub(super) fn apply_directory(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::AgentsLoaded(agents) => {
                self.agents = agents;
                self.agents_load = self.agents_load.finished(self.now);
                // An agent opened by its remembered name is swapped for the listed one.
                let listed = self
                    .selected_agent
                    .as_ref()
                    .and_then(|sel| self.agents.iter().find(|a| a.agent_id == sel.agent_id))
                    .filter(|real| Some(*real) != self.selected_agent.as_ref())
                    .cloned();
                if listed.is_some() {
                    self.selected_agent = listed;
                    self.sync_snapshot();
                }
                // A persisted agent selects itself once the directory lists it; an id the
                // directory doesn't know (or a draft) leaves the picker up.
                let remembered = self.pending_agent_id.take().and_then(|id| {
                    self.agents
                        .iter()
                        .position(|a| a.agent_id == id && a.status == AgentStatus::Published)
                });
                match remembered {
                    Some(i) => self.choose_agent(i),
                    None => self.finish_launch(),
                }
            }
            StreamEvent::AgentsFailed(e) => {
                self.agents_load = Load::Failed(e);
                self.finish_launch();
            }
            StreamEvent::SessionsLoaded(sessions) => {
                let since = match self.sessions_load {
                    Load::Loading { since } => since,
                    _ => self.now,
                };
                self.sessions_load = self.sessions_load.finished(self.now);
                self.finish_launch();
                self.show_all_sessions = false;
                self.show_hidden = false;
                if !sessions.is_empty() {
                    self.sessions = sessions
                        .into_iter()
                        .map(|s| Session {
                            id: Some(s.id.clone()),
                            hidden: self.hidden_conversations.contains(&s.id),
                            title: s.title.unwrap_or_else(|| "Untitled conversation".into()),
                            messages: Vec::new(),
                            agent_message_id: None,
                            history: Load::Idle,
                        })
                        .collect();
                    // A notification click that led here opens the conversation it named.
                    let pending = self.pending_conversation.take();
                    self.current = pending
                        .and_then(|target| {
                            self.sessions
                                .iter()
                                .position(|s| s.id.as_deref() == Some(&target))
                        })
                        .unwrap_or(0);
                    if let Some(session) = self.sessions.get(self.current)
                        && session.hidden
                    {
                        self.unhide_session(self.current);
                    }
                    if self.loads_from_directory() {
                        self.begin_history(since);
                    }
                }
            }
            StreamEvent::SessionsFailed(e) => {
                warn!("listing conversations failed: {e}");
                self.sessions_load = Load::Failed(e);
                self.finish_launch();
            }
            StreamEvent::ConversationLoaded {
                session_id,
                messages,
            } => {
                let now = self.now;
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.id.as_deref() == Some(&session_id))
                {
                    session.messages = messages;
                    session.history = session.history.finished(now);
                }
            }
            StreamEvent::ConversationFailed { session_id, error } => {
                warn!("loading the conversation failed: {error}");
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.id.as_deref() == Some(&session_id))
                {
                    session.history = Load::Failed(error);
                }
            }
            _ => {}
        }
    }

    /// Starts the next load the screen is waiting on: with an application (or the mock chosen
    /// outright) the agent list loads once, then the selected agent's conversations, then the
    /// open one's messages, each when its predecessor has landed. True when one started.
    pub(super) fn advance_loads(&mut self) -> bool {
        if !self.loads_from_directory() {
            return false;
        }
        if self.agents_load.is_idle() {
            self.load_agents();
        } else if self.selected_agent.is_some() && self.sessions_load.is_idle() {
            self.load_sessions();
        } else if self
            .sessions
            .get(self.current)
            .is_some_and(|session| session.id.is_some() && session.history.is_idle())
        {
            self.load_conversation();
        } else {
            return false;
        }
        true
    }

    /// Applies the loads that landed, dropping those of an older generation.
    pub(super) fn drain_loads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(loaded) = self.loads.try_recv() {
            if loaded.generation == ANY_GENERATION || loaded.generation == self.generation {
                self.apply(loaded.event);
                changed = true;
            }
        }
        changed
    }
}
