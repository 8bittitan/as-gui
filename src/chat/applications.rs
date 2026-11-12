//! The saved Algolia applications: adding, switching, renaming and removing them, and the
//! [`Connector`] that turns one into an API client and agent directory.

use std::rc::Rc;
use std::sync::Arc;

use rune::TextEdit;

use super::{Chat, Session};
use crate::agent::{AgentClient, AgentDirectory, AgentInfo, AgentStatus, AlgoliaClient};
use crate::loading::Load;
use crate::palette::AppColour;
use crate::store;

/// Whether the mock agent can be chosen: debug builds only.
pub const MOCK_AVAILABLE: bool = cfg!(debug_assertions);

pub const ADD_APPLICATION: &str = "Add application…";
pub const MANAGE_APPLICATIONS: &str = "Manage…";
const MOCK_APPLICATION: &str = "Mock";

/// One saved Algolia application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    pub app_id: String,
    pub api_key: String,
    pub label: Option<String>,
    /// The agent last open in it, as `(id, name)`, so switching back reopens it.
    pub last_agent: Option<(String, String)>,
    /// What tells it apart at a glance: the header dot and the window's top bar.
    pub colour: AppColour,
}

impl Application {
    pub fn new(app_id: &str, api_key: &str) -> Application {
        Application {
            app_id: app_id.into(),
            api_key: api_key.into(),
            label: None,
            last_agent: None,
            colour: AppColour::default(),
        }
    }

    /// The name it goes by: its label, else its app id.
    pub fn name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.app_id)
    }

    /// The key as settings shows it: only its last four characters.
    pub fn masked_key(&self) -> String {
        let tail: String = self
            .api_key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("••••{tail}")
    }

    /// The application saved at `index`; a colour the file doesn't name (or names unknown)
    /// comes from the position.
    pub(super) fn from_saved(saved: &store::SavedApplication, index: usize) -> Application {
        Application {
            app_id: saved.app_id.clone(),
            api_key: saved.api_key.clone(),
            label: saved.label.clone(),
            last_agent: saved
                .last_agent_id
                .clone()
                .map(|id| (id.clone(), saved.last_agent_name.clone().unwrap_or(id))),
            colour: saved
                .colour
                .as_deref()
                .and_then(AppColour::from_id)
                .unwrap_or_else(|| AppColour::by_position(index)),
        }
    }

    pub(super) fn to_saved(&self) -> store::SavedApplication {
        store::SavedApplication {
            app_id: self.app_id.clone(),
            api_key: self.api_key.clone(),
            label: self.label.clone(),
            last_agent_id: self.last_agent.as_ref().map(|a| a.0.clone()),
            last_agent_name: self.last_agent.as_ref().map(|a| a.1.clone()),
            colour: Some(self.colour.id().to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddApplicationError {
    Blank,
    Invalid,
    Duplicate,
}

/// The add-application dialog. Until one application exists, the first run shows it.
#[derive(Default)]
pub struct AddApplicationForm {
    pub open: bool,
    pub error: Option<String>,
    pub app_id: TextEdit,
    pub api_key: TextEdit,
    pub label: TextEdit,
}

const APP_ID_LEN: usize = 10;

/// An Algolia application id is ten letters or digits. It becomes a hostname, and the API key
/// goes to that host, so nothing else is let through.
pub fn is_valid_app_id(app_id: &str) -> bool {
    app_id.len() == APP_ID_LEN && app_id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// What an application's API client and agent directory are made of: the real Algolia ones,
/// or in tests, ones that answer from memory.
pub type Connector = Rc<dyn Fn(&Application) -> (Arc<dyn AgentClient>, Arc<dyn AgentDirectory>)>;

pub(super) fn algolia_connector(
    application: &Application,
) -> (Arc<dyn AgentClient>, Arc<dyn AgentDirectory>) {
    let make = || AlgoliaClient {
        app_id: application.app_id.clone(),
        api_key: application.api_key.clone(),
        agent_id: String::new(),
    };
    (Arc::new(make()), Arc::new(make()))
}

impl Chat {
    /// The active application's app id and key.
    pub fn credentials(&self) -> Option<(&str, &str)> {
        self.active
            .and_then(|i| self.applications.get(i))
            .map(|a| (a.app_id.as_str(), a.api_key.as_str()))
    }

    /// Whether the dialog should ask for an application: until one exists, unless the run is
    /// explicitly mock.
    pub fn needs_credentials(&self) -> bool {
        !self.mock_forced && self.active.is_none()
    }

    /// The menu entry for what's in use.
    pub fn active_option(&self) -> Option<String> {
        let options = self.application_options();
        match self.active {
            Some(i) => options.get(i).cloned(),
            None => self.application_label(),
        }
    }

    /// The header's name for what's in use: the active application, or the debug mock.
    pub fn application_label(&self) -> Option<String> {
        match self.active.and_then(|i| self.applications.get(i)) {
            Some(application) => Some(application.name().to_string()),
            None if self.mock_forced && MOCK_AVAILABLE => Some(MOCK_APPLICATION.into()),
            None => None,
        }
    }

    /// The switcher menu: applications (told apart when they share a name), the debug mock,
    /// then the two entries that aren't applications.
    pub fn application_options(&self) -> Vec<String> {
        let mut options: Vec<String> = self
            .applications
            .iter()
            .map(|a| {
                let shared = self
                    .applications
                    .iter()
                    .filter(|o| o.name() == a.name())
                    .count()
                    > 1;
                if shared {
                    format!("{} ({})", a.name(), a.app_id)
                } else {
                    a.name().to_string()
                }
            })
            .collect();
        if MOCK_AVAILABLE {
            options.push(MOCK_APPLICATION.into());
        }
        options.push(ADD_APPLICATION.into());
        options.push(MANAGE_APPLICATIONS.into());
        options
    }

    pub fn application_menu_chosen(&mut self, choice: &str) {
        self.application_menu_open = false;
        let options = self.application_options();
        match choice {
            ADD_APPLICATION => {
                self.add_form.error = None;
                self.add_form.open = true;
            }
            MANAGE_APPLICATIONS => self.settings_open = true,
            MOCK_APPLICATION if MOCK_AVAILABLE => self.switch_to_mock(),
            _ => {
                if let Some(i) = options.iter().position(|o| o == choice)
                    && i < self.applications.len()
                {
                    self.switch_application(i);
                }
            }
        }
    }

    /// Adds an application and switches to it. An app id already saved is refused.
    pub fn add_application(
        &mut self,
        app_id: &str,
        api_key: &str,
        label: Option<&str>,
    ) -> Result<(), AddApplicationError> {
        let (app_id, api_key) = (app_id.trim(), api_key.trim());
        if app_id.is_empty() || api_key.is_empty() {
            return Err(AddApplicationError::Blank);
        }
        if !is_valid_app_id(app_id) {
            return Err(AddApplicationError::Invalid);
        }
        if self.applications.iter().any(|a| a.app_id == app_id) {
            return Err(AddApplicationError::Duplicate);
        }
        let mut application = Application::new(app_id, api_key);
        application.label = label
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from);
        application.colour = AppColour::least_used(self.applications.iter().map(|a| a.colour));
        self.applications.push(application);
        self.sync_label_edits();
        self.add_form.open = false;
        self.add_form.error = None;
        self.activate(self.applications.len() - 1);
        Ok(())
    }

    pub fn switch_application(&mut self, index: usize) {
        if index < self.applications.len() && self.active != Some(index) {
            self.activate(index);
        }
    }

    /// Makes `index` the application in use. Its last agent reopens straight away (named
    /// provisionally until the list arrives, its conversations loading beside it); without
    /// one, the picker follows the agent list. Replies in flight are cancelled, and results
    /// for the application left behind are dropped.
    fn activate(&mut self, index: usize) {
        self.active = Some(index);
        self.mock_forced = false;
        let (client, directory) = (self.connector)(&self.applications[index]);
        self.client = client;
        self.directory = directory;
        self.reset_for_application();
        if let Some((id, name)) = self.applications[index].last_agent.clone() {
            self.selected_agent = Some(AgentInfo {
                agent_id: id.clone(),
                name,
                description: None,
                status: AgentStatus::Published,
                model: None,
            });
            self.client = self.client_for(&id);
            self.load_sessions();
        }
        self.sync_snapshot();
        self.persist();
    }

    /// Debug builds: the mock agent as an application of its own.
    pub fn switch_to_mock(&mut self) {
        if !MOCK_AVAILABLE {
            return;
        }
        self.active = None;
        self.mock_forced = true;
        self.client = crate::agent::mock_client();
        self.directory = crate::agent::mock_directory();
        self.reset_for_application();
        self.sync_snapshot();
        self.persist();
    }

    /// Back to nothing chosen: no agent, no conversations, nothing loaded or loading.
    fn reset_for_application(&mut self) {
        self.cancel_turn();
        self.generation += 1;
        self.agents.clear();
        self.agents_load = Load::Idle;
        self.launch = Load::Idle;
        self.selected_agent = None;
        self.pending_agent_id = None;
        self.sessions = vec![Session {
            id: None,
            hidden: false,
            title: "New conversation".into(),
            messages: Vec::new(),
            agent_message_id: None,
            history: Load::Idle,
        }];
        self.current = 0;
        self.sessions_load = Load::Idle;
        self.show_all_sessions = false;
        self.show_hidden = false;
        self.application_menu_open = false;
        self.reader = None;
        self.error = None;
    }

    pub fn remove_application(&mut self, index: usize) {
        if index >= self.applications.len() {
            return;
        }
        let was_active = self.active == Some(index);
        self.applications.remove(index);
        self.colour_picker = None;
        if index < self.label_edits.len() {
            self.label_edits.remove(index);
        }
        if self.applications.is_empty() {
            self.active = None;
            self.reset_for_application();
            self.sync_snapshot();
            self.persist();
        } else if was_active {
            self.activate(index.min(self.applications.len() - 1));
        } else if let Some(active) = self.active
            && active > index
        {
            self.active = Some(active - 1);
            self.persist();
        } else {
            self.persist();
        }
    }

    /// Replaces the saved applications wholesale, name fields included.
    pub fn set_applications(&mut self, applications: Vec<Application>, active: Option<usize>) {
        self.applications = applications;
        self.active = active;
        self.label_edits.clear();
        self.sync_label_edits();
    }

    /// Keeps one name field per application, and applies what's typed: an empty name falls
    /// back to the app id. True when a name changed.
    pub(super) fn sync_label_edits(&mut self) -> bool {
        self.label_edits.truncate(self.applications.len());
        while self.label_edits.len() < self.applications.len() {
            let label = self.applications[self.label_edits.len()].label.clone();
            self.label_edits
                .push(TextEdit::new(label.unwrap_or_default()));
        }
        let mut changed = false;
        for (application, edit) in self.applications.iter_mut().zip(&self.label_edits) {
            let typed = edit.text.trim();
            let want = (!typed.is_empty()).then(|| typed.to_string());
            if application.label != want {
                application.label = want;
                changed = true;
            }
        }
        changed
    }

    /// The add dialog's Save.
    pub fn submit_credentials(&mut self) {
        let app_id = self.add_form.app_id.text.trim().to_string();
        let api_key = self.add_form.api_key.text.trim().to_string();
        let label = self.add_form.label.text.trim().to_string();
        match self.add_application(&app_id, &api_key, Some(&label)) {
            Ok(()) => {
                self.add_form.app_id.clear();
                self.add_form.api_key.clear();
                self.add_form.label.clear();
            }
            Err(AddApplicationError::Blank) => {}
            Err(AddApplicationError::Invalid) => {
                self.add_form.error = Some(format!(
                    "An application ID is {APP_ID_LEN} letters or digits."
                ));
            }
            Err(AddApplicationError::Duplicate) => {
                self.add_form.error = Some(format!("{app_id} is already added."));
            }
        }
    }

    pub fn cancel_add_application(&mut self) {
        self.add_form.open = false;
        self.add_form.error = None;
    }

    /// Opens the connection to the active application, as at launch.
    pub fn connect_active(&mut self) {
        if let Some(application) = self.active.and_then(|i| self.applications.get(i)) {
            let (client, directory) = (self.connector)(application);
            self.client = client;
            self.directory = directory;
        }
    }

    /// The welcome dialog's escape hatch: stay on the mock agent for this run.
    pub fn use_mock(&mut self) {
        self.switch_to_mock();
    }

    /// The colour of what's in use: the active application's, or the debug mock's grey.
    pub fn active_colour(&self) -> Option<AppColour> {
        match self.active.and_then(|i| self.applications.get(i)) {
            Some(application) => Some(application.colour),
            None if self.mock_forced && MOCK_AVAILABLE => Some(AppColour::Neutral),
            None => None,
        }
    }

    /// The colour of the window's top bar: only when there are applications to tell apart,
    /// and the setting is on.
    pub fn window_tint(&self) -> Option<AppColour> {
        let application = self.active.and_then(|i| self.applications.get(i))?;
        (self.appearance.tint_window && self.applications.len() > 1).then_some(application.colour)
    }

    /// Settings' swatch row for application `index`: chosen, it closes.
    pub fn set_application_colour(&mut self, index: usize, colour: AppColour) {
        let Some(application) = self.applications.get_mut(index) else {
            return;
        };
        application.colour = colour;
        self.colour_picker = None;
        self.sync_snapshot();
        self.persist();
    }

    pub fn toggle_colour_picker(&mut self, index: usize) {
        self.colour_picker = (self.colour_picker != Some(index)).then_some(index);
    }
}
