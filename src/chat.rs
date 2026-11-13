//! Chat state: the [`Chat`] struct, and the per-frame [`poll`] that drives it. Each feature's
//! behaviour lives in its own child module as an `impl Chat` block — the seam where that
//! feature can change without editing this file. What to draw lives in `shell` and
//! `conversation`; responses stream in from `agent::AgentClient` over channels that [`poll`]
//! drains each frame.

mod appearance;
mod applications;
mod directory;
mod keybinds;
mod message_actions;
mod notifications;
mod persistence;
mod search;
mod sessions;
mod snapshot;
mod turn;

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use rune::{TextEdit, Tx, Waker};
use rune_kit::ShortcutRecorderState;

use crate::agent::{
    AgentClient, AgentDirectory, AgentInfo, AgentSnapshot, ChatMessage, IndexSearch, StreamEvent,
};
use crate::command_bar::CommandBar;
use crate::keymap::{Action, Keymap, Registration, ShortcutRegistrar};
use crate::loading::Load;
use crate::menubar::{AppShell, StatusItem};
use crate::notify::Notifier;
use crate::search_panel::SearchPanel;
use crate::selection::SelectionSource;

pub use appearance::{Accent, Appearance, ChatWidth, ThemePref};
#[cfg(test)]
pub use applications::AddApplicationError;
use applications::algolia_connector;
pub use applications::{AddApplicationForm, Application, Connector, MOCK_AVAILABLE};
use directory::Loaded;
pub use notifications::focus_changed;
pub use sessions::Session;
use turn::Timed;

pub struct Chat {
    pub client: Arc<dyn AgentClient>,
    pub directory: Arc<dyn AgentDirectory>,
    /// The saved Algolia applications, and the one in use (none on the mock or before the
    /// first is added).
    pub applications: Vec<Application>,
    pub active: Option<usize>,
    pub connector: Connector,
    /// One name field per application in settings, parallel to `applications`.
    pub label_edits: Vec<TextEdit>,
    pub application_menu_open: bool,
    waker: Option<Waker>,
    pub agents: Vec<AgentInfo>,
    pub agents_load: Load,
    pub selected_agent: Option<AgentInfo>,
    pub command_bar: CommandBar,
    pub settings_open: bool,
    /// The rebindable shortcuts, as the user has set them.
    pub keymap: Keymap,
    /// One recorder per action, indexed by `Action::index`.
    pub recorders: [ShortcutRecorderState; Action::COUNT],
    /// Why the last proposed chord was refused, for the recorder's error line.
    pub keybind_note: Option<String>,
    /// Why a system-wide chord couldn't be registered (taken at launch, say).
    shortcut_warnings: BTreeMap<Action, String>,
    registrar: Option<Rc<dyn ShortcutRegistrar>>,
    /// The windows and process the menu bar drives; `None` in tests and headless runs.
    pub(crate) shell: Option<Rc<dyn AppShell>>,
    /// The menu bar icon, when the app runs from the menu bar.
    pub(crate) status_item: Option<Box<dyn StatusItem>>,
    registrations: BTreeMap<Action, Registration>,
    /// The search hit open in the reader: its tool's name and its index in that tool's hits.
    pub reader: Option<(String, usize)>,
    /// The tool calls whose search results list every hit: (message id, tool index).
    hits_expanded: std::collections::HashSet<(String, usize)>,
    /// The "search it yourself" panel beside the chat.
    pub search: SearchPanel,
    pub index_search: Arc<dyn IndexSearch>,
    /// What Ask about selection captures through; settings shows its permission.
    pub selection: Arc<dyn SelectionSource>,
    pub width_select_open: bool,
    pub appearance: Appearance,
    /// The add-application dialog's state, which the first run reuses.
    pub add_form: AddApplicationForm,
    /// The application whose colour swatches are open in settings.
    pub colour_picker: Option<usize>,
    /// The agent to select once the directory lists it, from the persisted state; `None`
    /// leaves the picker up. Cleared when the list arrives, matched or not.
    pending_agent_id: Option<String>,
    /// Where state persists, set by the real app; tests run without it, so they never write.
    pub persist_path: Option<std::path::PathBuf>,
    /// Mock mode however it arose: `AS_GUI_MOCK=1`, a headless run, or the welcome dialog's
    /// mock button. Suppresses the credentials prompt.
    pub mock_forced: bool,
    /// The Quick Ask panel's view of the agent in use, published on every change.
    pub snapshot: Option<Arc<std::sync::Mutex<AgentSnapshot>>>,
    /// The selected agent's conversation list.
    pub sessions_load: Load,
    /// The launch: the agent list, and for a remembered agent its conversation list. The
    /// splash covers it, so the first real screen is the finished layout.
    pub launch: Load,
    /// The frame clock as of the last `poll`, which the loading rules read.
    pub now: f64,
    generation: u64,
    loads: Receiver<Loaded>,
    load_tx: Sender<Loaded>,
    pub(crate) last_poll: f64,
    /// Whether the sidebar lists every conversation, or just the recent few.
    pub show_all_sessions: bool,
    /// Whether the sidebar also shows conversations the user hid.
    pub show_hidden: bool,
    /// Whether a reply that finishes in the background posts a notification.
    pub notify_replies: bool,
    /// What posts the notifications; tests inject a fake.
    pub notifier: Arc<dyn Notifier>,
    /// Notifications awaiting their conversation's return: what the click routes to
    /// (conversation id or reply message id) → the notification's id in the system's list.
    pending_notifications: BTreeMap<String, String>,
    /// The conversation to open when the session list a notification click triggered lands.
    pending_conversation: Option<String>,
    /// The live tuning knobs (⌘⇧K); untouched, they hold the shipped design values.
    pub knobs: rune_knobs::Knobs,
    /// Conversation ids hidden from the sidebar, persisted across restarts.
    pub hidden_conversations: std::collections::HashSet<String>,
    pub sessions: Vec<Session>,
    pub current: usize,
    pub sidebar: rune_kit::PaneState,
    pub composer: TextEdit,
    pub pending: bool,
    pub error: Option<String>,
    turn_started: Option<f64>,
    events: Option<Receiver<Timed>>,
    turn_origin: Option<(Instant, f64)>,
    next_id: u64,
}

impl Chat {
    pub fn new(client: Arc<dyn AgentClient>, directory: Arc<dyn AgentDirectory>) -> Chat {
        let (load_tx, loads) = mpsc::channel();
        Chat {
            client,
            directory,
            applications: Vec::new(),
            active: None,
            connector: Rc::new(algolia_connector),
            label_edits: Vec::new(),
            application_menu_open: false,
            waker: None,
            agents: Vec::new(),
            agents_load: Load::Idle,
            selected_agent: None,
            command_bar: CommandBar::default(),
            settings_open: false,
            keymap: Keymap::default(),
            recorders: Action::ALL
                .map(|a| ShortcutRecorderState::new(Keymap::default().get(a).cloned())),
            keybind_note: None,
            shortcut_warnings: BTreeMap::new(),
            registrar: None,
            shell: None,
            status_item: None,
            registrations: BTreeMap::new(),
            reader: None,
            hits_expanded: Default::default(),
            search: SearchPanel::default(),
            index_search: Arc::new(crate::agent::AutoSearch),
            selection: Arc::new(crate::selection::NoSelection),
            width_select_open: false,
            appearance: Appearance::default(),
            add_form: AddApplicationForm::default(),
            colour_picker: None,
            pending_agent_id: None,
            persist_path: None,
            mock_forced: false,
            snapshot: None,
            sessions_load: Load::Idle,
            launch: Load::Idle,
            now: 0.0,
            generation: 1,
            loads,
            load_tx,
            last_poll: 0.0,
            show_all_sessions: false,
            show_hidden: false,
            knobs: rune_knobs::Knobs::default(),
            hidden_conversations: std::collections::HashSet::new(),
            notify_replies: true,
            notifier: Arc::new(crate::notify::RuneNotifier),
            pending_notifications: BTreeMap::new(),
            pending_conversation: None,
            sessions: vec![Session {
                id: None,
                hidden: false,
                title: "New conversation".into(),
                messages: Vec::new(),
                agent_message_id: None,
                history: Load::Idle,
            }],
            current: 0,
            sidebar: rune_kit::PaneState::new(276.0).range(200.0, 420.0),
            composer: TextEdit::new(""),
            pending: false,
            error: None,
            turn_started: None,
            events: None,
            turn_origin: None,
            next_id: 0,
        }
    }

    /// A dialog or menu that takes Esc before the search panel does.
    pub fn has_modal_open(&self) -> bool {
        self.reader.is_some()
            || self.settings_open
            || self.command_bar.open
            || self.add_form.open
            || self.application_menu_open
            || self.needs_credentials()
    }

    fn fresh_id(&mut self) -> String {
        loop {
            self.next_id += 1;
            let id = format!("m{}", self.next_id);
            // A loaded conversation may already carry ids like "m1" (they persist across
            // restarts), so a fresh id never repeats one the session already has.
            let taken = self.sessions[self.current]
                .messages
                .iter()
                .any(|m| m.id == id);
            if !taken {
                return id;
            }
        }
    }

    fn message_mut(&mut self, id: &str) -> Option<&mut ChatMessage> {
        self.sessions
            .iter_mut()
            .flat_map(|s| s.messages.iter_mut())
            .find(|m| m.id == id)
    }

    pub(crate) fn apply(&mut self, ev: StreamEvent) {
        self.apply_at(ev, self.now);
    }

    /// Applies `ev` as having arrived at `at` seconds on the frame clock, the time the
    /// turn's trace records, handing it to the feature it belongs to.
    pub(crate) fn apply_at(&mut self, ev: StreamEvent, at: f64) {
        if let Some(trace) = self.streaming_message().and_then(|m| m.trace.as_mut()) {
            trace.record(&ev, at);
        }
        match ev {
            StreamEvent::Start { .. }
            | StreamEvent::ConversationStarted(_)
            | StreamEvent::Suggestions(_)
            | StreamEvent::TextDelta(_)
            | StreamEvent::ToolUsed(_)
            | StreamEvent::ToolStarted { .. }
            | StreamEvent::ToolFailed { .. }
            | StreamEvent::ToolFinished { .. }
            | StreamEvent::Finished
            | StreamEvent::Failed(_) => self.apply_turn(ev),
            StreamEvent::SearchLoaded { .. } | StreamEvent::SearchFailed { .. } => {
                self.apply_search(ev)
            }
            StreamEvent::AgentsLoaded(_)
            | StreamEvent::AgentsFailed(_)
            | StreamEvent::SessionsLoaded(_)
            | StreamEvent::SessionsFailed(_)
            | StreamEvent::ConversationLoaded { .. }
            | StreamEvent::ConversationFailed { .. } => self.apply_directory(ev),
        }
    }
}

pub fn setup(s: &mut Chat, waker: Waker) {
    s.waker = Some(waker);
}

/// Advances everything one frame: applies renamed applications, starts the next load, applies
/// landed loads, tracks the loading views and drains the turn. True when state changed.
pub fn poll(s: &mut Chat, t: f64, tx: &mut Tx) -> bool {
    s.now = t;
    let mut changed = false;
    if s.sync_label_edits() {
        s.sync_snapshot();
        s.persist();
        changed = true;
    }
    changed |= s.advance_loads();
    changed |= s.drain_loads();
    changed |= s.track_views(t, tx);
    changed |= s.drain_turn(t);
    changed
}
