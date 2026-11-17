use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Builtins, CommandBindings, CommandRunner, is_builtin, run_command};

/// How long a client tool may run before its call fails.
pub const CLIENT_TOOL_TIMEOUT: Duration = Duration::from_secs(20);

pub const TIMED_OUT: &str =
    "The tool call timed out before a result was received. The operation may have completed.";

/// What pressing Run does for a tool name right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handler {
    Builtin,
    /// A user-defined command, started without a shell.
    Command {
        program: String,
        args: Vec<String>,
    },
    /// Nothing handles it: the user types the result.
    Ask,
}

/// Runs the tools an agent calls on this machine. It is a seam so tests, the mock and the real
/// tools can stand behind the same call.
pub trait ClientTools: Send + Sync {
    fn run(&self, name: &str, input: &Value) -> Result<Value, String>;

    /// Who handles `name` now. It is asked again when Run is pressed, so a handler set up
    /// while a card waits still applies.
    fn handler(&self, _name: &str) -> Handler {
        Handler::Builtin
    }

    /// How long a call to `name` may run.
    fn timeout(&self, _name: &str) -> Duration {
        CLIENT_TOOL_TIMEOUT
    }
}

/// Handles nothing: every call asks the user.
pub struct NoClientTools;

impl ClientTools for NoClientTools {
    fn run(&self, name: &str, _input: &Value) -> Result<Value, String> {
        Err(format!("No tool implemented for \"{name}\"."))
    }

    fn handler(&self, _name: &str) -> Handler {
        Handler::Ask
    }
}

/// The real tools: a command bound to the name, else a built-in, else the user.
pub struct Registry {
    builtins: Builtins,
    bindings: Arc<CommandBindings>,
    runner: Arc<dyn CommandRunner>,
}

impl Registry {
    pub fn new(
        builtins: Builtins,
        bindings: Arc<CommandBindings>,
        runner: Arc<dyn CommandRunner>,
    ) -> Registry {
        Registry {
            builtins,
            bindings,
            runner,
        }
    }
}

impl ClientTools for Registry {
    fn run(&self, name: &str, input: &Value) -> Result<Value, String> {
        if let Some(binding) = self.bindings.get(name) {
            return run_command(self.runner.as_ref(), &binding, input);
        }
        self.builtins
            .run(name, input)
            .unwrap_or_else(|| Err(format!("No tool implemented for \"{name}\".")))
    }

    fn handler(&self, name: &str) -> Handler {
        if let Some(binding) = self.bindings.get(name) {
            Handler::Command {
                program: binding.program,
                args: binding.args,
            }
        } else if is_builtin(name) {
            Handler::Builtin
        } else {
            Handler::Ask
        }
    }

    /// A command gets its own timeout, and a second more so that it reports it itself.
    fn timeout(&self, name: &str) -> Duration {
        self.bindings
            .get(name)
            .map_or(CLIENT_TOOL_TIMEOUT, |binding| {
                binding.timeout() + Duration::from_secs(1)
            })
    }
}

/// The tools a recorded demo runs: `open_url` reports success without opening a browser on the
/// recording Mac, after a beat so the running state shows.
pub struct DemoTools;

impl ClientTools for DemoTools {
    fn run(&self, name: &str, input: &Value) -> Result<Value, String> {
        if name != "open_url" {
            return Err(format!("No tool implemented for \"{name}\"."));
        }
        let url = input["url"].as_str().ok_or("url is required.")?;
        std::thread::sleep(Duration::from_millis(700));
        Ok(json!({ "opened": url }))
    }
}

/// Succeeds at every call without doing anything, for tests.
#[cfg(test)]
pub struct FakeClientTools;

#[cfg(test)]
impl ClientTools for FakeClientTools {
    fn run(&self, _name: &str, _input: &Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
}

/// The server counts an empty or falsy output (`{}`, `""`, `[]`, `0`, `false`, `null`) as no
/// result and rejects the request, so one is sent as `{"ok": true}`.
pub fn non_empty_output(output: Value) -> Value {
    let empty = match &output {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
    };
    if empty { json!({ "ok": true }) } else { output }
}

/// Runs one call on its own thread and waits at most `timeout` for it. A call that overruns
/// is abandoned: its thread finishes in the background and its result is dropped.
pub fn run_with_timeout(
    tools: Arc<dyn ClientTools>,
    name: String,
    input: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(tools.run(&name, &input));
    });
    result
        .recv_timeout(timeout)
        .unwrap_or_else(|_| Err(TIMED_OUT.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sleepy(Duration);

    impl ClientTools for Sleepy {
        fn run(&self, _: &str, _: &Value) -> Result<Value, String> {
            std::thread::sleep(self.0);
            Ok(json!({ "done": true }))
        }
    }

    #[test]
    fn an_empty_or_falsy_output_becomes_ok_true() {
        for empty in [
            json!(null),
            json!({}),
            json!([]),
            json!(""),
            json!(0),
            json!(false),
        ] {
            assert_eq!(
                non_empty_output(empty.clone()),
                json!({ "ok": true }),
                "{empty}"
            );
        }
    }

    #[test]
    fn a_real_output_is_kept() {
        for output in [
            json!({ "a": 1 }),
            json!("done"),
            json!([1]),
            json!(7),
            json!(true),
        ] {
            assert_eq!(non_empty_output(output.clone()), output);
        }
    }

    #[test]
    fn a_call_that_finishes_in_time_returns_its_result() {
        let tools = Arc::new(Sleepy(Duration::ZERO));
        assert_eq!(
            run_with_timeout(tools, "t".into(), json!({}), Duration::from_secs(5)),
            Ok(json!({ "done": true }))
        );
    }

    #[test]
    fn a_call_that_overruns_fails_with_a_timeout() {
        let tools = Arc::new(Sleepy(Duration::from_millis(500)));
        let result = run_with_timeout(tools, "slow".into(), json!({}), Duration::from_millis(20));
        assert_eq!(result, Err(TIMED_OUT.into()));
    }

    #[test]
    fn the_demo_tools_report_a_url_opened_without_opening_it() {
        assert_eq!(
            DemoTools.run("open_url", &json!({ "url": "https://a.com" })),
            Ok(json!({ "opened": "https://a.com" }))
        );
        assert_eq!(DemoTools.handler("open_url"), Handler::Builtin);
        assert!(DemoTools.run("open_url", &json!({})).is_err());
        assert!(DemoTools.run("mystery", &json!({})).is_err());
    }

    #[test]
    fn a_tool_nobody_handles_asks_the_user() {
        assert_eq!(NoClientTools.handler("mystery"), Handler::Ask);
        assert!(NoClientTools.run("mystery", &json!({})).is_err());
    }

    use crate::agent::CommandBinding;
    use crate::selection::NoSelection;
    use std::sync::Mutex;

    struct Shell;

    impl crate::menubar::AppShell for Shell {
        fn open_window(&self) {}
        fn set_dock_icon(&self, _: bool) {}
        fn toggle_quick_ask(&self) {}
        fn quit(&self) {}
        fn open_url(&self, _: &str) {}
        fn copy_text(&self, _: &str) {}
    }

    struct Notices;

    impl crate::notify::Notifier for Notices {
        fn post(&self, _: &rune_notify::Notification) -> bool {
            true
        }
        fn remove(&self, _: &[&str]) {}
        fn focused(&self) -> bool {
            false
        }
    }

    struct Runner(Mutex<Vec<String>>);

    impl CommandRunner for Runner {
        fn run(
            &self,
            request: &crate::agent::CommandRequest,
        ) -> Result<crate::agent::CommandOutput, crate::agent::CommandError> {
            self.0.lock().unwrap().push(request.program.clone());
            Ok(crate::agent::CommandOutput {
                status: Some(0),
                stdout: b"\"from the command\"".to_vec(),
                stderr: Vec::new(),
            })
        }
    }

    fn registry() -> (Registry, Arc<CommandBindings>, Arc<Runner>) {
        let bindings = Arc::new(CommandBindings::default());
        let runner = Arc::new(Runner(Mutex::new(Vec::new())));
        let registry = Registry::new(
            Builtins {
                shell: Arc::new(Shell),
                notifier: Arc::new(Notices),
                selection: Arc::new(NoSelection),
            },
            bindings.clone(),
            runner.clone(),
        );
        (registry, bindings, runner)
    }

    #[test]
    fn the_registry_resolves_a_command_then_a_built_in_then_the_user() {
        let (registry, bindings, runner) = registry();
        assert_eq!(registry.handler("throw_confetti"), Handler::Ask);
        assert_eq!(registry.handler("notify"), Handler::Builtin);

        bindings
            .bind(
                "throw_confetti",
                CommandBinding {
                    program: "/usr/bin/confetti".into(),
                    args: vec!["--fast".into()],
                    timeout_secs: 5,
                },
            )
            .unwrap();
        assert_eq!(
            registry.handler("throw_confetti"),
            Handler::Command {
                program: "/usr/bin/confetti".into(),
                args: vec!["--fast".into()],
            }
        );
        assert_eq!(
            registry.run("throw_confetti", &json!({})),
            Ok(json!("from the command"))
        );
        assert_eq!(*runner.0.lock().unwrap(), ["/usr/bin/confetti"]);
        assert_eq!(registry.timeout("throw_confetti"), Duration::from_secs(6));
        assert_eq!(registry.timeout("notify"), CLIENT_TOOL_TIMEOUT);
    }

    #[test]
    fn a_built_in_keeps_its_name_whatever_a_binding_says() {
        let (registry, bindings, runner) = registry();
        let evil = CommandBinding {
            program: "/bin/evil".into(),
            ..CommandBinding::default()
        };
        assert!(bindings.bind("notify", evil).is_err());
        assert_eq!(registry.handler("notify"), Handler::Builtin);
        assert!(registry.run("notify", &json!({ "title": "t" })).is_ok());
        assert!(runner.0.lock().unwrap().is_empty());
    }

    #[test]
    fn running_a_name_nobody_handles_is_an_error() {
        let (registry, _, _) = registry();
        assert_eq!(
            registry.run("mystery", &json!({})),
            Err("No tool implemented for \"mystery\".".into())
        );
    }
}
