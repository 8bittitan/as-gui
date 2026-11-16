//! User-defined shell commands as client tools: a program, its arguments and a timeout,
//! started without a shell, with the call's input on stdin and its stdout as the result.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{CLIENT_TOOL_TIMEOUT, TIMED_OUT, is_builtin};

pub const MAX_OUTPUT_BYTES: usize = 1_000_000;
const STDERR_CHARS: usize = 200;
const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandBinding {
    pub program: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

impl CommandBinding {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

impl Default for CommandBinding {
    fn default() -> Self {
        CommandBinding {
            program: String::new(),
            args: Vec::new(),
            timeout_secs: CLIENT_TOOL_TIMEOUT.as_secs(),
        }
    }
}

/// The commands bound to tool names. A name a built-in tool owns can't be bound.
#[derive(Default)]
pub struct CommandBindings(Mutex<BTreeMap<String, CommandBinding>>);

impl CommandBindings {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bind(&self, name: &str, binding: CommandBinding) -> Result<(), String> {
        if is_builtin(name) {
            return Err(format!("{name} is a built-in tool and can't be replaced."));
        }
        if binding.program.trim().is_empty() {
            return Err("A command needs a program to run.".into());
        }
        self.0.lock().unwrap().insert(name.into(), binding);
        Ok(())
    }

    /// Makes `commands` the whole table; names a built-in owns are left out.
    pub fn replace(&self, commands: impl IntoIterator<Item = (String, CommandBinding)>) {
        *self.0.lock().unwrap() = commands
            .into_iter()
            .filter(|(name, binding)| !is_builtin(name) && !binding.program.trim().is_empty())
            .collect();
    }

    pub fn get(&self, name: &str) -> Option<CommandBinding> {
        self.0.lock().unwrap().get(name).cloned()
    }
}

/// What to start: no shell is involved, so every argument reaches the program as one string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_output: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    Spawn(String),
    TimedOut,
    TooMuchOutput,
}

/// Starts processes, so tests can stand in for the OS.
pub trait CommandRunner: Send + Sync {
    fn run(&self, request: &CommandRequest) -> Result<CommandOutput, CommandError>;
}

/// The command's result as the agent gets it: JSON when stdout parses, otherwise its text.
pub fn run_command(
    runner: &dyn CommandRunner,
    binding: &CommandBinding,
    input: &Value,
) -> Result<Value, String> {
    let request = CommandRequest {
        program: binding.program.clone(),
        args: binding.args.clone(),
        stdin: input.to_string().into_bytes(),
        env: minimal_env(),
        timeout: binding.timeout(),
        max_output: MAX_OUTPUT_BYTES,
    };
    let output = runner.run(&request).map_err(|error| match error {
        CommandError::Spawn(reason) => format!("Could not start {}: {reason}", binding.program),
        CommandError::TimedOut => TIMED_OUT.to_string(),
        CommandError::TooMuchOutput => format!(
            "The command wrote more than {} MB.",
            MAX_OUTPUT_BYTES / 1_000_000
        ),
    })?;
    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr: String = stderr.trim().chars().take(STDERR_CHARS).collect();
        let status = output
            .status
            .map_or("a signal".to_string(), |code| format!("status {code}"));
        return Err(if stderr.is_empty() {
            format!("{} exited with {status}.", binding.program)
        } else {
            format!("{} exited with {status}: {stderr}", binding.program)
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim_end();
    Ok(serde_json::from_str(stdout).unwrap_or_else(|_| Value::String(stdout.to_string())))
}

fn minimal_env() -> Vec<(String, String)> {
    ["PATH", "HOME"]
        .into_iter()
        .filter_map(|name| Some((name.to_string(), std::env::var(name).ok()?)))
        .collect()
}

/// Runs the process for real: no shell, a cleared environment, and the process killed when it
/// overruns its timeout or its output.
pub struct SystemRunner;

fn read_capped(
    mut stream: impl Read + Send + 'static,
    cap: usize,
    over: Arc<AtomicBool>,
) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return buffer,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    if buffer.len() > cap {
                        over.store(true, Ordering::SeqCst);
                        return buffer;
                    }
                }
            }
        }
    })
}

impl CommandRunner for SystemRunner {
    fn run(&self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
        let mut child = Command::new(&request.program)
            .args(&request.args)
            .env_clear()
            .envs(request.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CommandError::Spawn(e.to_string()))?;
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let input = request.stdin.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let over = Arc::new(AtomicBool::new(false));
        let stdout = read_capped(
            child.stdout.take().expect("stdout was piped"),
            request.max_output,
            over.clone(),
        );
        let stderr = read_capped(
            child.stderr.take().expect("stderr was piped"),
            request.max_output,
            over.clone(),
        );
        let deadline = Instant::now() + request.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(CommandError::Spawn(error.to_string())),
            }
            let failure = if over.load(Ordering::SeqCst) {
                Some(CommandError::TooMuchOutput)
            } else if Instant::now() >= deadline {
                Some(CommandError::TimedOut)
            } else {
                None
            };
            if let Some(failure) = failure {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure);
            }
            std::thread::sleep(POLL);
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        if over.load(Ordering::SeqCst) {
            return Err(CommandError::TooMuchOutput);
        }
        Ok(CommandOutput {
            status: status.code(),
            stdout,
            stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    struct FakeRunner {
        requests: Mutex<Vec<CommandRequest>>,
        answer: Result<CommandOutput, CommandError>,
    }

    impl FakeRunner {
        fn answering(answer: Result<CommandOutput, CommandError>) -> FakeRunner {
            FakeRunner {
                requests: Mutex::new(Vec::new()),
                answer,
            }
        }

        fn printing(stdout: &str) -> FakeRunner {
            FakeRunner::answering(Ok(CommandOutput {
                status: Some(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            }))
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            self.requests.lock().unwrap().push(request.clone());
            self.answer.clone()
        }
    }

    fn binding(program: &str, args: &[&str]) -> CommandBinding {
        CommandBinding {
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            timeout_secs: 7,
        }
    }

    #[test]
    fn replacing_the_table_swaps_every_binding_and_skips_built_ins() {
        let bindings = CommandBindings::default();
        let binding = |program: &str| CommandBinding {
            program: program.into(),
            ..CommandBinding::default()
        };
        bindings.bind("old_tool", binding("/bin/old")).unwrap();
        bindings.replace([
            ("new_tool".to_string(), binding("/bin/new")),
            ("open_url".to_string(), binding("/bin/evil")),
            ("blank".to_string(), binding("  ")),
        ]);
        assert_eq!(bindings.get("old_tool"), None);
        assert_eq!(bindings.get("new_tool").unwrap().program, "/bin/new");
        assert_eq!(bindings.get("open_url"), None);
        assert_eq!(bindings.get("blank"), None);
    }

    #[test]
    fn arguments_reach_the_program_as_separate_strings_and_never_through_a_shell() {
        let runner = FakeRunner::printing("");
        let tricky = "$(rm -rf ~); echo `id` | tee";
        run_command(
            &runner,
            &binding("/usr/bin/say", &["--voice", tricky]),
            &json!({}),
        )
        .unwrap();
        let request = runner.requests.lock().unwrap()[0].clone();
        assert_eq!(request.program, "/usr/bin/say");
        assert_eq!(request.args, ["--voice", tricky]);
    }

    #[test]
    fn the_input_goes_in_on_stdin_with_the_bindings_timeout_and_a_minimal_environment() {
        let runner = FakeRunner::printing("");
        run_command(&runner, &binding("tool", &[]), &json!({ "count": 3 })).unwrap();
        let request = runner.requests.lock().unwrap()[0].clone();
        assert_eq!(request.stdin, br#"{"count":3}"#);
        assert_eq!(request.timeout, Duration::from_secs(7));
        assert_eq!(request.max_output, MAX_OUTPUT_BYTES);
        assert!(
            request
                .env
                .iter()
                .all(|(name, _)| name == "PATH" || name == "HOME"),
            "{:?}",
            request.env
        );
    }

    #[test]
    fn stdout_is_json_when_it_parses_and_text_otherwise() {
        let json_out = run_command(
            &FakeRunner::printing("{\"a\": 1}\n"),
            &binding("t", &[]),
            &json!({}),
        );
        assert_eq!(json_out, Ok(json!({ "a": 1 })));
        let text_out = run_command(
            &FakeRunner::printing("all done\n"),
            &binding("t", &[]),
            &json!({}),
        );
        assert_eq!(text_out, Ok(json!("all done")));
    }

    #[test]
    fn a_non_zero_exit_is_an_error_with_stderr_trimmed_to_200_characters() {
        let runner = FakeRunner::answering(Ok(CommandOutput {
            status: Some(2),
            stdout: Vec::new(),
            stderr: format!("  {}  ", "x".repeat(500)).into_bytes(),
        }));
        let error = run_command(&runner, &binding("tool", &[]), &json!({})).unwrap_err();
        assert_eq!(
            error,
            format!("tool exited with status 2: {}", "x".repeat(200))
        );
    }

    #[test]
    fn a_failure_without_stderr_still_says_what_happened() {
        let runner = FakeRunner::answering(Ok(CommandOutput {
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }));
        assert_eq!(
            run_command(&runner, &binding("tool", &[]), &json!({})),
            Err("tool exited with a signal.".into())
        );
    }

    #[test]
    fn a_timeout_and_a_flood_and_a_missing_program_are_errors() {
        let timed_out = FakeRunner::answering(Err(CommandError::TimedOut));
        assert_eq!(
            run_command(&timed_out, &binding("t", &[]), &json!({})),
            Err(TIMED_OUT.into())
        );
        let flood = FakeRunner::answering(Err(CommandError::TooMuchOutput));
        assert!(
            run_command(&flood, &binding("t", &[]), &json!({}))
                .unwrap_err()
                .contains("1 MB")
        );
        let missing = FakeRunner::answering(Err(CommandError::Spawn("no such file".into())));
        assert_eq!(
            run_command(&missing, &binding("nope", &[]), &json!({})),
            Err("Could not start nope: no such file".into())
        );
    }

    #[test]
    fn a_name_that_clashes_with_a_built_in_cannot_be_bound() {
        let bindings = CommandBindings::default();
        for name in ["open_url", "copy_to_clipboard", "notify", "read_selection"] {
            assert!(bindings.bind(name, binding("tool", &[])).is_err(), "{name}");
            assert_eq!(bindings.get(name), None);
        }
        assert!(
            bindings
                .bind("throw_confetti", binding("tool", &[]))
                .is_ok()
        );
        assert_eq!(bindings.get("throw_confetti"), Some(binding("tool", &[])));
    }

    #[test]
    fn a_binding_without_a_program_is_refused() {
        let bindings = CommandBindings::default();
        assert!(bindings.bind("t", binding("  ", &[])).is_err());
    }

    fn request(program: &str, args: &[&str], stdin: &[u8], timeout: Duration) -> CommandRequest {
        CommandRequest {
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            stdin: stdin.to_vec(),
            env: minimal_env(),
            timeout,
            max_output: 1000,
        }
    }

    #[test]
    fn the_system_runner_feeds_stdin_and_reads_stdout() {
        let output = SystemRunner
            .run(&request("/bin/cat", &[], b"hello", Duration::from_secs(5)))
            .unwrap();
        assert_eq!(output.status, Some(0));
        assert_eq!(output.stdout, b"hello");
    }

    #[test]
    fn the_system_runner_kills_a_process_that_overruns_its_timeout() {
        let started = Instant::now();
        let result = SystemRunner.run(&request(
            "/bin/sleep",
            &["30"],
            b"",
            Duration::from_millis(100),
        ));
        assert_eq!(result, Err(CommandError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_system_runner_stops_a_process_that_writes_too_much() {
        let result = SystemRunner.run(&request("/usr/bin/yes", &[], b"", Duration::from_secs(10)));
        assert_eq!(result, Err(CommandError::TooMuchOutput));
    }

    #[test]
    fn the_system_runner_reports_a_program_that_is_not_there() {
        let result = SystemRunner.run(&request(
            "/no/such/program",
            &[],
            b"",
            Duration::from_secs(1),
        ));
        assert!(matches!(result, Err(CommandError::Spawn(_))));
    }
}
