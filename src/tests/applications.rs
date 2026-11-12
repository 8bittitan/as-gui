use std::rc::Rc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;
use crate::agent::AgentDirectory;
use crate::chat::{AddApplicationError, Application};
use crate::loading::{PaneView, Screen};
use crate::palette::AppColour;

fn app(id: &str) -> Application {
    Application::new(id, &format!("key-{id}"))
}

/// A chat holding two saved applications, A active, on an offline connector whose
/// directory lists a different agent and conversation per application.
fn two_apps() -> Runtime<Chat> {
    with_two_apps(test_chat())
}

fn with_two_apps(mut rt: Runtime<Chat>) -> Runtime<Chat> {
    rt.update(|s, _| {
        s.mock_forced = false;
        s.applications = vec![app("AAAAAAAAAA"), app("BBBBBBBBBB")];
        s.active = Some(0);
        s.connector = Rc::new(|application: &Application| {
            let client: Arc<dyn crate::agent::AgentClient> = Arc::new(MockAgent {
                response: "Hi!",
                chunk_chars: 2,
                tools: Vec::new(),
            });
            let id = application.app_id[..3].to_lowercase();
            let directory: Arc<dyn crate::agent::AgentDirectory> = Arc::new(MockDirectory {
                agents: vec![
                    agent(
                        &format!("{id}-1"),
                        &format!("{id} agent one"),
                        AgentStatus::Published,
                    ),
                    agent(
                        &format!("{id}-2"),
                        &format!("{id} agent two"),
                        AgentStatus::Published,
                    ),
                ],
                sessions: vec![SessionInfo {
                    id: format!("{id}-c1"),
                    title: Some(format!("{id} conversation")),
                }],
                conversation: vec![ChatMessage::user("h1", "Earlier")],
            });
            (client, directory)
        });
    });
    rt
}

fn land(rt: &mut Runtime<Chat>, done: impl Fn(&Chat) -> bool) {
    for _ in 0..400 {
        rt.frame(0.0);
        if done(&rt.state) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("never landed");
}

#[test]
fn an_application_shows_its_name_or_else_its_app_id() {
    let mut named = app("AAAAAAAAAA");
    assert_eq!(named.name(), "AAAAAAAAAA");
    named.label = Some("Production".into());
    assert_eq!(named.name(), "Production");
}

#[test]
fn adding_an_application_activates_it_and_a_duplicate_app_id_is_refused() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        assert_eq!(
            s.add_application("CCCCCCCCCC", "key", Some("Staging")),
            Ok(())
        );
    });
    assert_eq!(rt.state.applications.len(), 3);
    assert_eq!(rt.state.active, Some(2));
    assert_eq!(rt.state.credentials(), Some(("CCCCCCCCCC", "key")));
    assert_eq!(rt.state.applications[2].name(), "Staging");

    let again = rt.state.add_application("AAAAAAAAAA", "other", None);
    assert_eq!(again, Err(AddApplicationError::Duplicate));
    assert_eq!(rt.state.applications.len(), 3, "nothing was saved");
    assert_eq!(rt.state.active, Some(2), "and nothing switched");
}

struct Held {
    inner: Arc<dyn AgentDirectory>,
    gate: Arc<Mutex<Receiver<()>>>,
}

impl Held {
    fn wait(&self) {
        let _ = self.gate.lock().unwrap().recv();
    }
}

impl AgentDirectory for Held {
    fn list(&self) -> Result<Vec<AgentInfo>, String> {
        self.wait();
        self.inner.list()
    }

    fn sessions(&self, agent_id: &str) -> Result<Vec<SessionInfo>, String> {
        self.wait();
        self.inner.sessions(agent_id)
    }

    fn conversation(&self, agent_id: &str, id: &str) -> Result<Vec<ChatMessage>, String> {
        self.wait();
        self.inner.conversation(agent_id, id)
    }
}

/// Holds every directory call until the returned sender is dropped.
fn hold_directory_loads(rt: &mut Runtime<Chat>) -> Sender<()> {
    let (release, gate) = channel();
    let gate = Arc::new(Mutex::new(gate));
    rt.update(|s, _| {
        let inner = s.connector.clone();
        s.connector = Rc::new(move |application: &Application| {
            let (client, directory) = inner(application);
            let held: Arc<dyn AgentDirectory> = Arc::new(Held {
                inner: directory,
                gate: gate.clone(),
            });
            (client, held)
        });
    });
    release
}

#[test]
fn switching_to_an_application_with_a_remembered_agent_opens_it_without_a_splash() {
    let mut rt = two_apps();
    let release = hold_directory_loads(&mut rt);
    rt.update(|s, _| {
        s.applications[1].last_agent = Some(("bbb-2".into(), "bbb agent two".into()));
        s.switch_application(1);
    });
    rt.frame(0.0);
    assert_eq!(
        rt.state.credentials(),
        Some(("BBBBBBBBBB", "key-BBBBBBBBBB"))
    );
    assert_eq!(
        rt.state
            .selected_agent
            .as_ref()
            .map(|a| a.agent_id.as_str()),
        Some("bbb-2")
    );
    assert_eq!(rt.state.screen(rt.state.now), Screen::Chat, "no splash");
    assert_eq!(
        rt.state.pane_view(rt.state.now),
        PaneView::Blank,
        "no stale or starter content"
    );

    drop(release);
    land(&mut rt, |s| {
        s.sessions[0].history.is_settled()
            || s.sessions_load.is_settled() && s.sessions.len() == 1 && s.sessions[0].id.is_some()
    });
    assert_eq!(
        rt.state.sessions[0].title, "bbb conversation",
        "B's conversations, not A's"
    );
    land(&mut rt, |s| s.agents_load.is_settled());
    assert_eq!(
        rt.state.selected_agent.as_ref().map(|a| a.name.as_str()),
        Some("bbb agent two"),
        "the provisional agent is replaced by the listed one"
    );
}

#[test]
fn switching_to_an_application_without_a_remembered_agent_ends_at_the_picker() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.selected_agent = Some(agent("aaa-1", "aaa agent one", AgentStatus::Published));
        s.switch_application(1);
    });
    assert!(rt.state.selected_agent.is_none());
    land(&mut rt, |s| s.agents_load.is_settled());
    rt.frame(0.5);
    assert_eq!(rt.state.screen(rt.state.now), Screen::Picker);
    assert!(
        rt.state
            .agents
            .iter()
            .all(|a| a.agent_id.starts_with("bbb"))
    );
}

#[test]
fn switching_cancels_a_reply_in_progress_and_publishes_the_new_agent_to_quick_ask() {
    let mut rt = two_apps();
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
    rt.update(|s, _| {
        s.snapshot = Some(snapshot.clone());
        s.selected_agent = Some(agent("aaa-1", "aaa agent one", AgentStatus::Published));
        s.begin_turn("a question");
        s.applications[1].last_agent = Some(("bbb-1".into(), "bbb agent one".into()));
        s.switch_application(1);
    });
    assert!(!rt.state.pending, "the reply was cancelled");
    assert_eq!(snapshot.lock().unwrap().agent_id, "bbb-1");

    rt.update(|s, _| {
        s.selected_agent = Some(agent("bbb-1", "bbb agent one", AgentStatus::Published));
        s.switch_application(0);
    });
    assert!(
        snapshot.lock().unwrap().agent_id.is_empty(),
        "no agent, nothing to ask"
    );
}

#[test]
fn rapid_switches_end_on_the_last_application() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.applications[0].last_agent = Some(("aaa-1".into(), "aaa agent one".into()));
        s.applications[1].last_agent = Some(("bbb-1".into(), "bbb agent one".into()));
        s.switch_application(0);
        s.switch_application(1);
    });
    land(&mut rt, |s| s.sessions_load.is_settled());
    for _ in 0..20 {
        rt.frame(0.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(rt.state.sessions[0].title, "bbb conversation");
}

#[test]
fn removing_the_active_application_activates_the_next_and_removing_the_last_asks_again() {
    let mut rt = two_apps();
    rt.update(|s, _| s.remove_application(0));
    assert_eq!(rt.state.applications.len(), 1);
    assert_eq!(
        rt.state.credentials(),
        Some(("BBBBBBBBBB", "key-BBBBBBBBBB")),
        "B took over"
    );

    rt.update(|s, _| s.remove_application(0));
    assert!(rt.state.applications.is_empty());
    assert!(rt.state.needs_credentials(), "the first-run dialog is back");
}

#[test]
fn removing_an_application_before_the_active_one_keeps_the_active_one() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.switch_application(1);
        s.remove_application(0);
    });
    assert_eq!(
        rt.state.credentials(),
        Some(("BBBBBBBBBB", "key-BBBBBBBBBB"))
    );
    assert_eq!(rt.state.active, Some(0));
}

#[test]
fn choosing_an_agent_is_remembered_for_its_application() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.agents = vec![agent("aaa-2", "aaa agent two", AgentStatus::Published)];
        s.choose_agent(0);
    });
    assert_eq!(
        rt.state.applications[0].last_agent,
        Some(("aaa-2".into(), "aaa agent two".into()))
    );
    assert!(rt.state.applications[1].last_agent.is_none());
}

#[test]
fn applications_save_and_restore_with_the_active_one_and_its_agent() {
    let path = std::env::temp_dir().join("as-gui-applications-save/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.persist_path = Some(path.clone());
        s.applications[1].label = Some("Staging".into());
        s.applications[1].last_agent = Some(("bbb-1".into(), "bbb agent one".into()));
        s.switch_application(1);
    });
    let saved = crate::store::load_from(&path);
    assert_eq!(saved.applications.len(), 2);
    assert_eq!(saved.active_app.as_deref(), Some("BBBBBBBBBB"));

    let mut restarted = test_chat();
    restarted.update(|s, _| {
        s.applications.clear();
        s.active = None;
        s.restore(&saved);
    });
    assert_eq!(
        restarted.state.credentials(),
        Some(("BBBBBBBBBB", "key-BBBBBBBBBB"))
    );
    assert_eq!(restarted.state.applications[1].name(), "Staging");
    assert_eq!(
        restarted.state.applications[1]
            .last_agent
            .as_ref()
            .map(|a| a.0.as_str()),
        Some("bbb-1")
    );
}

#[test]
fn the_header_names_the_active_application_and_lists_the_rest_in_a_menu() {
    let mut rt = two_apps();
    rt.on_key = Some(on_key);
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    assert!(rt.find("application-switcher-trigger").is_some());
    let shows = |rt: &Runtime<Chat>, text: &str| {
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some(text))
    };
    assert!(shows(&rt, "AAAAAAAAAA"), "the active application's name");

    rt.update(|s, _| s.application_menu_open = true);
    settle(&mut rt);
    for entry in ["AAAAAAAAAA", "BBBBBBBBBB", "Add application…", "Manage…"] {
        assert!(shows(&rt, entry), "the menu lists {entry}");
    }
}

#[test]
fn choosing_the_menu_entries_switches_adds_and_manages() {
    let mut rt = two_apps();
    rt.update(|s, _| s.application_menu_chosen("BBBBBBBBBB"));
    assert_eq!(rt.state.active, Some(1));
    rt.update(|s, _| s.application_menu_chosen("Add application…"));
    assert!(rt.state.add_form.open);
    rt.update(|s, _| s.application_menu_chosen("Manage…"));
    assert!(rt.state.settings_open);
}

#[test]
fn the_add_dialog_serves_first_run_and_later_additions() {
    let mut rt = two_apps();
    rt.update(|s, _| s.add_form.open = true);
    settle(&mut rt);
    assert!(rt.find("credentials-dialog").is_some());
    assert!(
        rt.find("credentials-cancel").is_some(),
        "later additions can be cancelled"
    );
    assert!(
        rt.find("credentials-mock").is_none(),
        "the mock is a first-run escape only"
    );

    tap(&mut rt, "credentials-cancel");
    rt.frame(0.0);
    assert!(!rt.state.add_form.open);
}

#[test]
fn an_app_id_that_is_not_ten_letters_or_digits_is_refused_and_saves_nothing() {
    let mut rt = two_apps();
    for bad in [
        "evil.example/",
        "evil.example#",
        "ABC",
        "ABCDEFGHIJK",
        "ABCDE FGHI",
        "ABCDE-FGHI",
    ] {
        assert_eq!(
            rt.state.add_application(bad, "key", None),
            Err(AddApplicationError::Invalid),
            "{bad:?}"
        );
    }
    assert_eq!(rt.state.applications.len(), 2);
    assert_eq!(rt.state.active, Some(0));
}

#[test]
fn a_ten_character_app_id_is_accepted_in_either_case() {
    let mut rt = no_apps();
    rt.update(|s, _| {
        assert_eq!(s.add_application("prodapp001", "key", None), Ok(()));
        assert_eq!(s.add_application("ABCDEFGH12", "key", None), Ok(()));
    });
    assert_eq!(rt.state.applications.len(), 2);
}

#[test]
fn submitting_an_invalid_app_id_shows_an_error_in_the_dialog() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.add_form.open = true;
        s.add_form.app_id.insert("evil.example/");
        s.add_form.api_key.insert("key");
        s.submit_credentials();
    });
    settle(&mut rt);
    assert_eq!(rt.state.applications.len(), 2);
    assert!(rt.state.add_form.open);
    assert!(rt.find("credentials-error").is_some());
}

#[test]
fn restoring_drops_saved_applications_whose_app_id_is_invalid() {
    let mut rt = no_apps();
    rt.update(|s, _| {
        s.restore(&Persisted {
            applications: vec![
                crate::store::SavedApplication {
                    app_id: "evil.example/".into(),
                    api_key: "k".into(),
                    ..Default::default()
                },
                crate::store::SavedApplication {
                    app_id: "GOODAPP123".into(),
                    api_key: "k".into(),
                    ..Default::default()
                },
            ],
            active_app: Some("evil.example/".into()),
            ..Persisted::default()
        });
    });
    let ids: Vec<&str> = rt
        .state
        .applications
        .iter()
        .map(|a| a.app_id.as_str())
        .collect();
    assert_eq!(ids, ["GOODAPP123"]);
    assert_eq!(rt.state.active, Some(0));
}

#[test]
fn submitting_a_duplicate_shows_an_error_in_the_dialog_and_saves_nothing() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.add_form.open = true;
        s.add_form.app_id.insert("AAAAAAAAAA");
        s.add_form.api_key.insert("another-key");
        s.submit_credentials();
    });
    settle(&mut rt);
    assert_eq!(rt.state.applications.len(), 2);
    assert!(rt.state.add_form.open, "the dialog stays for a correction");
    assert!(rt.state.add_form.error.is_some());
    assert!(rt.find("credentials-error").is_some());
}

#[test]
fn the_mock_is_a_debug_build_application_only() {
    assert_eq!(chat::MOCK_AVAILABLE, cfg!(debug_assertions));
    let mut rt = two_apps();
    rt.update(|s, _| s.switch_to_mock());
    assert!(rt.state.mock_forced);
    assert_eq!(rt.state.active, None);
    assert_eq!(rt.state.application_label().as_deref(), Some("Mock"));
    assert!(!rt.state.needs_credentials());
}

#[test]
fn editing_a_name_in_settings_renames_the_application() {
    let mut rt = two_apps();
    rt.frame(0.0);
    rt.update(|s, _| {
        s.settings_open = true;
        s.label_edits[1].insert("Staging");
    });
    rt.frame(0.0);
    assert_eq!(rt.state.applications[1].name(), "Staging");
    rt.update(|s, _| s.label_edits[1].clear());
    rt.frame(0.0);
    assert_eq!(
        rt.state.applications[1].name(),
        "BBBBBBBBBB",
        "empty falls back to the app id"
    );
}

#[test]
fn settings_lists_each_application_masked_and_can_remove_one() {
    let mut rt = test_chat_sized((720.0, 1900.0));
    rt.update(|s, _| {
        s.applications = vec![app("AAAAAAAAAA"), app("BBBBBBBBBB")];
        s.active = Some(0);
        s.settings_open = true;
    });
    settle(&mut rt);
    assert!(rt.find("application-row-0").is_some());
    assert!(rt.find("application-row-1").is_some());
    let shows = |rt: &Runtime<Chat>, text: &str| {
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains(text)))
    };
    assert!(shows(&rt, "AAAAAAAAAA"), "the app id");
    assert!(
        !shows(&rt, "key-AAAAAAAAAA"),
        "the key is never shown whole"
    );
    assert!(shows(&rt, "••••AAAA"), "only its last four characters");

    tap(&mut rt, "application-remove-1");
    rt.frame(0.0);
    assert_eq!(rt.state.applications.len(), 1);
}

fn no_apps() -> Runtime<Chat> {
    let mut rt = two_apps();
    rt.update(|s, _| s.set_applications(Vec::new(), None));
    rt
}

#[test]
fn added_applications_get_distinct_colours_until_the_palette_runs_out_then_wrap() {
    let mut rt = no_apps();
    rt.update(|s, _| {
        for i in 0..=AppColour::ALL.len() {
            s.add_application(&format!("APPNUM{i:04}"), "key", None)
                .unwrap();
        }
    });
    let colours: Vec<AppColour> = rt.state.applications.iter().map(|a| a.colour).collect();
    let first_round = &colours[..AppColour::ALL.len()];
    for colour in AppColour::ALL {
        assert_eq!(
            first_round.iter().filter(|&&c| c == colour).count(),
            1,
            "{colour:?} is used once before any repeats"
        );
    }
    assert_eq!(
        colours[AppColour::ALL.len()],
        AppColour::ALL[0],
        "then the palette wraps"
    );
}

#[test]
fn a_removed_applications_colour_is_reused_by_the_next_one_added() {
    let mut rt = no_apps();
    rt.update(|s, _| {
        for id in ["AAAAAAAAAA", "BBBBBBBBBB", "CCCCCCCCCC"] {
            s.add_application(id, "key", None).unwrap();
        }
    });
    let freed = rt.state.applications[1].colour;
    rt.update(|s, _| {
        s.remove_application(1);
        s.add_application("DDDDDDDDDD", "key", None).unwrap();
    });
    assert_eq!(rt.state.applications[2].colour, freed);
}

#[test]
fn an_application_colour_saves_and_restores() {
    let path = std::env::temp_dir().join("as-gui-application-colour/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.persist_path = Some(path.clone());
        s.set_application_colour(1, AppColour::Teal);
    });
    let saved = crate::store::load_from(&path);
    assert_eq!(saved.applications[1].colour.as_deref(), Some("teal"));

    let mut restarted = test_chat();
    restarted.update(|s, _| s.restore(&saved));
    assert_eq!(restarted.state.applications[1].colour, AppColour::Teal);
}

#[test]
fn colours_missing_or_unknown_in_the_file_come_from_the_position() {
    let saved = |app_id: &str, colour: Option<&str>| crate::store::SavedApplication {
        app_id: app_id.into(),
        api_key: "key".into(),
        colour: colour.map(String::from),
        ..Default::default()
    };
    let state = crate::store::Persisted {
        applications: vec![
            saved("AAAAAAAAAA", None),
            saved("BBBBBBBBBB", None),
            saved("CCCCCCCCCC", Some("chartreuse")),
        ],
        ..Default::default()
    };
    let mut rt = test_chat();
    rt.update(|s, _| s.restore(&state));
    let colours: Vec<AppColour> = rt.state.applications.iter().map(|a| a.colour).collect();
    assert_eq!(colours, AppColour::ALL[..3].to_vec());

    let mut again = test_chat();
    again.update(|s, _| s.restore(&state));
    assert_eq!(
        again.state.applications[2].colour, colours[2],
        "a restart doesn't reshuffle"
    );
}

#[test]
fn the_window_tint_shows_with_two_applications_and_the_setting_on() {
    let mut rt = two_apps();
    rt.update(|s, _| s.selected_agent = Some(agent_one()));
    rt.frame(0.0);
    assert!(rt.find("window-tint").is_some());

    rt.update(|s, _| s.set_tint_window(false));
    rt.frame(0.0);
    assert!(
        rt.find("window-tint").is_none(),
        "the setting hides the bar"
    );
    assert!(
        rt.find(&format!(
            "application-dot-{}",
            rt.state.applications[0].colour.id()
        ))
        .is_some(),
        "but not the header dot"
    );

    rt.update(|s, _| {
        s.set_tint_window(true);
        s.remove_application(1);
        s.selected_agent = Some(agent_one());
    });
    rt.frame(0.0);
    assert!(
        rt.find("window-tint").is_none(),
        "one application needs no telling apart"
    );
}

#[test]
fn the_mock_has_a_neutral_dot_and_no_window_tint() {
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.switch_to_mock();
        s.selected_agent = Some(agent_one());
    });
    rt.frame(0.0);
    assert_eq!(rt.state.active_colour(), Some(AppColour::Neutral));
    assert!(rt.find("window-tint").is_none());
}

#[test]
fn the_tint_setting_saves_and_restores() {
    let path = std::env::temp_dir().join("as-gui-tint-window/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = two_apps();
    rt.update(|s, _| {
        s.persist_path = Some(path.clone());
        s.set_tint_window(false);
    });
    let saved = crate::store::load_from(&path);
    assert_eq!(saved.tint_window, Some(false));
    let mut restarted = test_chat();
    restarted.update(|s, _| s.restore(&saved));
    assert!(!restarted.state.appearance.tint_window);
}

#[test]
fn choosing_a_colour_in_settings_changes_the_header_dot_and_persists() {
    let path = std::env::temp_dir().join("as-gui-colour-picker/state.json");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let mut rt = with_two_apps(test_chat_sized((720.0, 1900.0)));
    rt.update(|s, _| {
        s.persist_path = Some(path.clone());
        s.selected_agent = Some(agent_one());
        s.set_application_colour(0, AppColour::Blue);
        s.settings_open = true;
    });
    settle(&mut rt);
    assert!(
        rt.find("application-colour-0-teal").is_none(),
        "the swatches wait for the dot"
    );
    tap(&mut rt, "application-colour-0");
    settle(&mut rt);
    tap(&mut rt, "application-colour-0-teal");
    rt.frame(0.0);
    assert_eq!(rt.state.applications[0].colour, AppColour::Teal);
    assert!(rt.find("application-dot-teal").is_some());
    assert!(rt.find("application-colour-0-teal").is_none(), "and closes");
    assert_eq!(
        crate::store::load_from(&path).applications[0]
            .colour
            .as_deref(),
        Some("teal")
    );
}

#[test]
fn quick_ask_reads_the_active_applications_colour_and_follows_a_switch() {
    let mut rt = two_apps();
    let snapshot = Arc::new(std::sync::Mutex::new(AgentSnapshot::default()));
    rt.update(|s, _| {
        s.snapshot = Some(snapshot.clone());
        s.set_application_colour(0, AppColour::Orange);
        s.set_application_colour(1, AppColour::Indigo);
    });
    assert_eq!(
        snapshot.lock().unwrap().application,
        Some(("AAAAAAAAAA".into(), AppColour::Orange))
    );
    rt.update(|s, _| s.switch_application(1));
    assert_eq!(
        snapshot.lock().unwrap().application,
        Some(("BBBBBBBBBB".into(), AppColour::Indigo))
    );
}
