use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};

use super::*;
use crate::agent::{
    AgentClient, Builtins, ClientTools, CommandBinding, CommandBindings, CommandError,
    CommandOutput, CommandRequest, CommandRunner, FakeClientTools, MockScript, NoClientTools, Part,
    PendingCall, Registry, ToolState,
};
use crate::selection::NoSelection;

fn confetti_agent() -> MockAgent {
    MockAgent {
        response: "",
        chunk_chars: 4,
        tools: Vec::new(),
        script: Some(MockScript::Confetti),
    }
}

/// Remembers the history of every request, then answers as `inner` does.
struct RecordingAgent {
    inner: MockAgent,
    requests: Mutex<Vec<Vec<ChatMessage>>>,
}

impl RecordingAgent {
    fn confetti() -> Arc<Self> {
        Arc::new(RecordingAgent {
            inner: confetti_agent(),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<Vec<ChatMessage>> {
        self.requests.lock().unwrap().clone()
    }
}

impl AgentClient for RecordingAgent {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn send_feedback(&self, _: &str, _: &str, _: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        history: &[ChatMessage],
        conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        self.requests.lock().unwrap().push(history.to_vec());
        self.inner.stream(history, conversation, emit)
    }
}

/// An agent that answers every request with another client tool call.
struct LoopingAgent {
    requests: AtomicUsize,
}

impl AgentClient for LoopingAgent {
    fn name(&self) -> &'static str {
        "looping"
    }

    fn send_feedback(&self, _: &str, _: &str, _: i8) -> Result<(), String> {
        Ok(())
    }

    fn stream(
        &self,
        _: &[ChatMessage],
        _: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        let round = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
        let call_id = format!("loop_{round}");
        emit(StreamEvent::StepStart);
        emit(StreamEvent::TextDelta(format!("Round {round}.")));
        emit(StreamEvent::ToolUsed(ToolCall {
            call_id: call_id.clone(),
            provider_metadata: None,
            name: "again".into(),
            input: Some("{}".into()),
            request: None,
            error: None,
            open: false,
        }));
        emit(StreamEvent::ClientToolsPending(vec![PendingCall {
            call_id,
            name: "again".into(),
            input: json!({}),
        }]));
        Ok(())
    }
}

/// Counts the calls it runs and holds each one until released.
struct GatedTools {
    started: AtomicUsize,
    release: Mutex<Receiver<()>>,
}

impl GatedTools {
    fn new() -> (Arc<Self>, Sender<()>) {
        let (open, release) = channel();
        let tools = Arc::new(GatedTools {
            started: AtomicUsize::new(0),
            release: Mutex::new(release),
        });
        (tools, open)
    }
}

impl ClientTools for GatedTools {
    fn run(&self, _: &str, _: &Value) -> Result<Value, String> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let _ = self.release.lock().unwrap().recv();
        Ok(json!({ "released": true }))
    }
}

struct CountingTools(AtomicUsize);

impl ClientTools for CountingTools {
    fn run(&self, _: &str, _: &Value) -> Result<Value, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(json!({ "ran": true }))
    }
}

struct FailingTools;

impl ClientTools for FailingTools {
    fn run(&self, name: &str, _: &Value) -> Result<Value, String> {
        Err(format!("{name} broke"))
    }
}

fn chat_with(client: Arc<dyn AgentClient>, tools: Arc<dyn ClientTools>) -> Runtime<Chat> {
    let mut rt = test_chat();
    rt.update(|s, _| {
        s.client = client;
        s.client_tools = tools;
        s.selected_agent = Some(agent_one());
    });
    rt
}

/// Presses Run on every call waiting for the user.
fn approve_waiting(rt: &mut Runtime<Chat>) {
    let waiting: Vec<String> = rt
        .state
        .awaiting_calls()
        .map(|call| call.call_id.clone())
        .collect();
    if !waiting.is_empty() {
        rt.update(move |s, _| waiting.iter().for_each(|id| s.approve_client_call(id)));
    }
}

/// Frames, with the pauses the worker threads need, until `done` holds. Calls that wait for
/// the user are run as they arrive.
fn drive(rt: &mut Runtime<Chat>, done: impl Fn(&Chat) -> bool) {
    for _ in 0..1000 {
        approve_waiting(rt);
        rt.frame(1.0 / 60.0);
        if done(&rt.state) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the turn never got there");
}

fn idle(chat: &Chat) -> bool {
    !chat.pending
}

/// Frames long enough for a worker released now to have reported, if it were going to count.
fn let_late_results_arrive(rt: &mut Runtime<Chat>) {
    for _ in 0..50 {
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

fn tool_parts(message: &ChatMessage) -> Vec<&crate::agent::ToolPart> {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Tool(tool) => Some(tool),
            _ => None,
        })
        .collect()
}

#[test]
fn a_continued_turn_keeps_its_message_and_starts_a_new_paragraph() {
    let mut rt = chat_with(Arc::new(confetti_agent()), Arc::new(FakeClientTools));
    rt.update(|s, _| s.send_prompt("Party"));
    let reply_id = rt.state.sessions[0].messages[1].id.clone();

    drive(&mut rt, idle);

    let messages = &rt.state.sessions[0].messages;
    assert_eq!(messages.len(), 2, "no second assistant message");
    assert_eq!(messages[1].id, reply_id);
    assert_eq!(messages[1].text, "Let's celebrate.\n\nConfetti is flying.");
    assert!(!messages[1].streaming);
    assert_eq!(messages[1].tools.len(), 1);
    assert!(
        messages[1].tools[0]
            .input
            .as_deref()
            .is_some_and(|result| result.contains("\"ok\": true")),
        "the card shows the call's result, an empty one made non-empty"
    );
    assert!(rt.state.error.is_none());
}

#[test]
fn the_continuation_sends_the_assistant_message_with_the_calls_result() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent.clone(), Arc::new(FakeClientTools));
    rt.update(|s, _| s.send_prompt("Party"));

    drive(&mut rt, idle);

    let requests = agent.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].len(),
        1,
        "the first request holds only the prompt"
    );
    let resent = requests[1]
        .last()
        .expect("the continuation carries the reply");
    assert_eq!(resent.role, Role::Assistant);
    assert_eq!(resent.parts[0], Part::StepStart);
    let tools = tool_parts(resent);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].state, ToolState::OutputAvailable);
    assert_eq!(tools[0].output, Some(json!({ "ok": true })));
}

#[test]
fn a_failed_tool_goes_back_as_an_error() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent.clone(), Arc::new(FailingTools));
    rt.update(|s, _| s.send_prompt("Party"));

    drive(&mut rt, idle);

    let requests = agent.requests();
    let tools = tool_parts(requests[1].last().unwrap());
    assert_eq!(tools[0].state, ToolState::OutputError);
    assert_eq!(tools[0].error.as_deref(), Some("throw_confetti broke"));
    assert!(
        rt.state.sessions[0].messages[1].tools[0].error.is_some(),
        "the card shows the failure"
    );
}

#[test]
fn the_composer_stays_locked_while_a_call_runs() {
    let agent = RecordingAgent::confetti();
    let (tools, release) = GatedTools::new();
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| s.send_prompt("Party"));

    drive(&mut rt, |_| tools.started.load(Ordering::SeqCst) == 1);
    for _ in 0..10 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.pending, "the reply isn't done");
    assert!(
        rt.state.sessions[0].messages[1].streaming,
        "the reply is still open"
    );
    rt.update(|s, _| {
        s.composer.text = "Another".into();
        s.send();
    });
    assert_eq!(rt.state.sessions[0].messages.len(), 2, "a send is refused");
    assert_eq!(agent.requests().len(), 1);

    release.send(()).unwrap();
    drive(&mut rt, idle);
    assert_eq!(agent.requests().len(), 2, "the result continued the turn");
}

#[test]
fn the_loop_stops_after_eight_rounds() {
    let agent = Arc::new(LoopingAgent {
        requests: AtomicUsize::new(0),
    });
    let tools = Arc::new(CountingTools(AtomicUsize::new(0)));
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| s.send_prompt("Go on"));

    drive(&mut rt, idle);

    assert_eq!(tools.0.load(Ordering::SeqCst), 8, "eight rounds ran");
    assert_eq!(
        agent.requests.load(Ordering::SeqCst),
        9,
        "the ninth call is refused, not run"
    );
    let error = rt.state.error.as_deref().expect("an error says why");
    assert!(error.contains("8 rounds"), "{error}");
    assert!(!rt.state.sessions[0].messages[1].streaming);
}

#[test]
fn a_new_user_turn_gets_a_fresh_round_budget() {
    let agent = Arc::new(LoopingAgent {
        requests: AtomicUsize::new(0),
    });
    let tools = Arc::new(CountingTools(AtomicUsize::new(0)));
    let mut rt = chat_with(agent, tools.clone());
    rt.update(|s, _| s.send_prompt("One"));
    drive(&mut rt, idle);
    rt.update(|s, _| s.send_prompt("Two"));
    drive(&mut rt, idle);
    assert_eq!(tools.0.load(Ordering::SeqCst), 16);
}

#[test]
fn a_result_that_arrives_after_a_cancel_is_dropped() {
    let agent = RecordingAgent::confetti();
    let (tools, release) = GatedTools::new();
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| s.send_prompt("Party"));
    drive(&mut rt, |_| tools.started.load(Ordering::SeqCst) == 1);

    rt.update(|s, _| s.new_session());
    release.send(()).unwrap();
    let_late_results_arrive(&mut rt);

    assert_eq!(agent.requests().len(), 1, "no continuation went out");
    assert!(!rt.state.pending);
    assert!(rt.state.sessions[1].messages.is_empty());
    let abandoned = &rt.state.sessions[0].messages[1];
    assert_eq!(
        tool_parts(abandoned)[0].state,
        ToolState::InputAvailable,
        "the late result never reached the old reply"
    );
}

#[test]
fn a_result_that_arrives_after_an_application_switch_is_dropped() {
    let agent = RecordingAgent::confetti();
    let (tools, release) = GatedTools::new();
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| {
        s.connector = offline_connector(vec![agent_one()]);
        s.applications
            .push(crate::chat::Application::new("BBBBBBBBBB", "key-b"));
        s.send_prompt("Party");
    });
    drive(&mut rt, |_| tools.started.load(Ordering::SeqCst) == 1);

    rt.update(|s, _| s.switch_application(1));
    release.send(()).unwrap();
    let_late_results_arrive(&mut rt);

    assert_eq!(agent.requests().len(), 1, "no continuation went out");
    assert!(!rt.state.pending);
    assert!(rt.state.sessions[rt.state.current].messages.is_empty());
}

#[test]
fn two_calls_of_one_tool_in_a_step_stay_two_cards() {
    let mut rt = test_chat();
    let _sender = rt.state.begin_turn("Twice");
    rt.update(|s, _| {
        for (id, count) in [("a", 1), ("b", 2)] {
            s.apply(StreamEvent::ToolStarted {
                call_id: id.into(),
                name: "throw_confetti".into(),
            });
            s.apply(StreamEvent::ToolUsed(ToolCall {
                call_id: id.into(),
                provider_metadata: None,
                name: "throw_confetti".into(),
                input: Some(format!("{{\"count\": {count}}}")),
                request: None,
                error: None,
                open: false,
            }));
        }
        s.apply(StreamEvent::ToolFinished {
            call_id: "b".into(),
            name: "throw_confetti".into(),
            output: Some("{\"ok\": true}".into()),
        });
    });

    let reply = &rt.state.sessions[0].messages[1];
    assert_eq!(reply.tools.len(), 2);
    assert!(reply.tools[0].input.as_deref().unwrap().contains("1"));
    assert!(
        reply.tools[0].error.is_none() && reply.tools[1].input.as_deref().unwrap().contains("ok"),
        "the result went to the second call alone"
    );
    let parts = tool_parts(reply);
    assert_eq!(parts[0].state, ToolState::InputAvailable);
    assert_eq!(parts[1].state, ToolState::OutputAvailable);
}

#[test]
fn a_tool_without_an_id_still_merges_by_name() {
    let mut rt = test_chat();
    let _sender = rt.state.begin_turn("Search");
    rt.update(|s, _| {
        s.apply(StreamEvent::ToolStarted {
            call_id: String::new(),
            name: "search".into(),
        });
        s.apply(StreamEvent::ToolFinished {
            call_id: String::new(),
            name: "search".into(),
            output: Some("{}".into()),
        });
    });
    assert_eq!(rt.state.sessions[0].messages[1].tools.len(), 1);
}

#[test]
fn text_and_tools_are_recorded_as_parts_in_stream_order() {
    let mut rt = test_chat();
    let _sender = rt.state.begin_turn("Order");
    rt.update(|s, _| {
        s.apply(StreamEvent::StepStart);
        s.apply(StreamEvent::TextDelta("Looking.".into()));
        s.apply(StreamEvent::ToolUsed(ToolCall {
            call_id: "c1".into(),
            provider_metadata: Some(json!({ "k": "v" })),
            name: "search".into(),
            input: Some("{\"q\": 1}".into()),
            request: None,
            error: None,
            open: false,
        }));
        s.apply(StreamEvent::StepStart);
        s.apply(StreamEvent::TextDelta("\n\nFound.".into()));
    });
    let parts = &rt.state.sessions[0].messages[1].parts;
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0], Part::StepStart);
    assert_eq!(parts[1], Part::Text("Looking.".into()));
    let Part::Tool(tool) = &parts[2] else {
        panic!("the tool follows its text: {parts:?}");
    };
    assert_eq!(tool.input, json!({ "q": 1 }));
    assert_eq!(tool.provider_metadata, Some(json!({ "k": "v" })));
    assert_eq!(parts[3], Part::StepStart);
    assert_eq!(
        parts[4],
        Part::Text("Found.".into()),
        "the break that separated the paragraphs isn't part of the text"
    );
}

const CONFETTI: &str = "mock_confetti_1";

fn run_key() -> String {
    format!("approval-run-{CONFETTI}")
}

/// Frames until the element keyed `key` is on screen.
fn wait_for(rt: &mut Runtime<Chat>, key: &str) {
    for _ in 0..1000 {
        rt.frame(1.0 / 60.0);
        if rt.find(key).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("{key} never showed");
}

fn quiet_frames(rt: &mut Runtime<Chat>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn confetti_result(agent: &RecordingAgent) -> (ToolState, Option<Value>, Option<String>) {
    let requests = agent.requests();
    let tool = tool_parts(requests[1].last().unwrap())[0].clone();
    (tool.state, tool.output, tool.error)
}

#[test]
fn a_call_waits_for_the_user_until_run_is_pressed() {
    let agent = RecordingAgent::confetti();
    let tools = Arc::new(CountingTools(AtomicUsize::new(0)));
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| s.send_prompt("Party"));

    wait_for(&mut rt, &run_key());
    quiet_frames(&mut rt);
    assert!(rt.state.pending, "the reply waits");
    assert_eq!(tools.0.load(Ordering::SeqCst), 0, "nothing ran unasked");
    assert_eq!(agent.requests().len(), 1);

    tap(&mut rt, &run_key());
    drive(&mut rt, idle);

    assert_eq!(tools.0.load(Ordering::SeqCst), 1);
    assert_eq!(agent.requests().len(), 2);
    assert_eq!(confetti_result(&agent).0, ToolState::OutputAvailable);
    assert!(rt.find(&run_key()).is_none(), "the card's buttons are gone");
}

#[test]
fn declining_reaches_the_next_request_as_an_output_error() {
    let agent = RecordingAgent::confetti();
    let tools = Arc::new(CountingTools(AtomicUsize::new(0)));
    let mut rt = chat_with(agent.clone(), tools.clone());
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &run_key());

    tap(&mut rt, &format!("approval-decline-{CONFETTI}"));
    drive(&mut rt, idle);

    assert_eq!(
        tools.0.load(Ordering::SeqCst),
        0,
        "a declined call never runs"
    );
    let (state, output, error) = confetti_result(&agent);
    assert_eq!(state, ToolState::OutputError);
    assert_eq!(output, None);
    assert_eq!(
        error.as_deref(),
        Some("The user declined to run this tool.")
    );
}

#[test]
fn a_call_nothing_handles_takes_a_result_typed_by_the_user() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent.clone(), Arc::new(NoClientTools));
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &format!("approval-field-{CONFETTI}"));
    assert!(rt.find(&run_key()).is_none(), "there is nothing to run");

    rt.update(|s, _| s.manual_reply(CONFETTI).insert("{\"thrown\": 3}"));
    tap(&mut rt, &format!("approval-send-{CONFETTI}"));
    drive(&mut rt, idle);

    let (state, output, _) = confetti_result(&agent);
    assert_eq!(state, ToolState::OutputAvailable);
    assert_eq!(output, Some(json!({ "thrown": 3 })));
}

#[test]
fn a_typed_result_that_is_not_json_goes_back_as_text() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent.clone(), Arc::new(NoClientTools));
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &format!("approval-send-{CONFETTI}"));

    rt.update(|s, _| s.manual_reply(CONFETTI).insert("it worked"));
    tap(&mut rt, &format!("approval-send-{CONFETTI}"));
    drive(&mut rt, idle);

    assert_eq!(confetti_result(&agent).1, Some(json!("it worked")));
}

struct ScriptedRunner {
    requests: Mutex<Vec<CommandRequest>>,
}

impl CommandRunner for ScriptedRunner {
    fn run(&self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(CommandOutput {
            status: Some(0),
            stdout: b"{\"thrown\": true}".to_vec(),
            stderr: Vec::new(),
        })
    }
}

struct QuietShell;

impl crate::menubar::AppShell for QuietShell {
    fn open_window(&self) {}
    fn set_dock_icon(&self, _: bool) {}
    fn toggle_quick_ask(&self) {}
    fn quit(&self) {}
    fn open_url(&self, _: &str) {}
    fn copy_text(&self, _: &str) {}
}

struct QuietNotifier;

impl crate::notify::Notifier for QuietNotifier {
    fn post(&self, _: &rune_notify::Notification) -> bool {
        true
    }
    fn remove(&self, _: &[&str]) {}
    fn focused(&self) -> bool {
        false
    }
}

#[test]
fn a_command_bound_while_the_card_waits_is_used_when_run_is_pressed() {
    let agent = RecordingAgent::confetti();
    let bindings = Arc::new(CommandBindings::default());
    let runner = Arc::new(ScriptedRunner {
        requests: Mutex::new(Vec::new()),
    });
    let registry = Registry::new(
        Builtins {
            shell: Arc::new(QuietShell),
            notifier: Arc::new(QuietNotifier),
            selection: Arc::new(NoSelection),
        },
        bindings.clone(),
        runner.clone(),
    );
    let mut rt = chat_with(agent.clone(), Arc::new(registry));
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &format!("approval-field-{CONFETTI}"));

    bindings
        .bind(
            "throw_confetti",
            CommandBinding {
                program: "/usr/local/bin/confetti".into(),
                args: vec!["--count".into(), "two words".into()],
                timeout_secs: 5,
            },
        )
        .unwrap();
    rt.update(|_, _| {});
    wait_for(&mut rt, &run_key());
    assert!(
        rt.find(&format!("approval-field-{CONFETTI}")).is_none(),
        "the field gives way to Run"
    );
    assert!(shows_text(
        &rt,
        "/usr/local/bin/confetti --count 'two words'",
        "approval-command"
    ));

    tap(&mut rt, &run_key());
    drive(&mut rt, idle);

    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].args, ["--count", "two words"]);
    assert_eq!(requests[0].stdin, b"{\"count\":50}");
    assert_eq!(confetti_result(&agent).1, Some(json!({ "thrown": true })));
}

#[test]
fn a_new_approval_card_takes_focus_and_announces_itself() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent, Arc::new(FakeClientTools));
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &run_key());
    quiet_frames(&mut rt);

    let key = run_key();
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some(key.as_str()) && e.focused),
        "Run has focus"
    );
    assert!(shows_text(
        &rt,
        "Waiting for your approval",
        "approval-actions"
    ));
    let labels: Vec<String> = rt
        .elements()
        .iter()
        .filter_map(|e| e.label.clone())
        .collect();
    for expected in ["Run throw_confetti", "Decline throw_confetti"] {
        assert!(
            labels.iter().any(|l| l == expected),
            "{expected} in {labels:?}"
        );
    }
    assert!(
        labels
            .iter()
            .any(|l| l == "throw_confetti, waiting for your approval"),
        "the card says what it waits for"
    );
}

fn with_directory(rt: &mut Runtime<Chat>) {
    rt.update(|s, _| {
        s.directory = Arc::new(MockDirectory {
            agents: vec![agent_one()],
            sessions: Vec::new(),
            conversation: Vec::new(),
        });
        s.mock_forced = false;
        s.agents = vec![agent_one()];
    });
}

fn focused(rt: &Runtime<Chat>, key: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.key.as_deref() == Some(key) && e.focused)
}

#[test]
fn set_up_a_handler_opens_the_sheet_on_that_tool_and_focus_returns_to_the_card() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent, Arc::new(NoClientTools));
    with_directory(&mut rt);
    rt.update(|s, _| s.send_prompt("Party"));
    let link = format!("approval-setup-{CONFETTI}");
    wait_for(&mut rt, &link);
    quiet_frames(&mut rt);

    tap(&mut rt, &link);
    wait_for(&mut rt, "agent-tool-throw_confetti");
    quiet_frames(&mut rt);
    assert!(rt.find("agent-panel").is_some());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.focused && e.path.contains("agent-tool-handler-throw_confetti")),
        "the tool's handler select has focus"
    );

    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    quiet_frames(&mut rt);
    assert!(rt.find("agent-panel").is_none());
    assert!(focused(&rt, &link), "focus is back on the link");
}

#[test]
fn a_command_saved_in_the_sheet_runs_when_run_is_pressed() {
    let agent = RecordingAgent::confetti();
    let mut rt = chat_with(agent.clone(), Arc::new(NoClientTools));
    with_directory(&mut rt);
    let runner = Arc::new(ScriptedRunner {
        requests: Mutex::new(Vec::new()),
    });
    let registry = Registry::new(
        Builtins {
            shell: Arc::new(QuietShell),
            notifier: Arc::new(QuietNotifier),
            selection: Arc::new(NoSelection),
        },
        rt.state.bindings.clone(),
        runner.clone(),
    );
    rt.update(move |s, _| s.client_tools = Arc::new(registry));

    rt.update(|s, _| {
        s.open_agent_panel(None);
        s.edit_binding("throw_confetti");
        s.binding_draft.program.insert("/usr/local/bin/confetti");
        s.binding_draft.args.insert("--loud");
        s.save_binding_edit();
        s.close_agent_panel();
    });
    rt.update(|s, _| s.send_prompt("Party"));
    wait_for(&mut rt, &run_key());
    assert!(shows_text(
        &rt,
        "/usr/local/bin/confetti --loud",
        "approval-command"
    ));

    tap(&mut rt, &run_key());
    drive(&mut rt, idle);

    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].program, "/usr/local/bin/confetti");
    assert_eq!(requests[0].args, ["--loud"]);
}
