use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

/// How long a client tool may run before its call fails.
pub const CLIENT_TOOL_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs the tools an agent calls on this machine. It is a seam so tests, the mock and the real
/// tools can stand behind the same call.
pub trait ClientTools: Send + Sync {
    fn run(&self, name: &str, input: &Value) -> Result<Value, String>;
}

/// Answers every call as the reference client does for a tool it doesn't have.
pub struct NoClientTools;

impl ClientTools for NoClientTools {
    fn run(&self, name: &str, _input: &Value) -> Result<Value, String> {
        Ok(Value::String(format!(
            "No tool implemented for \"{name}\"."
        )))
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
    let label = name.clone();
    std::thread::spawn(move || {
        let _ = done.send(tools.run(&name, &input));
    });
    result
        .recv_timeout(timeout)
        .unwrap_or_else(|_| Err(format!("{label} timed out after {timeout:?}")))
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
        assert_eq!(result, Err("slow timed out after 20ms".into()));
    }

    #[test]
    fn an_unknown_tool_is_answered_not_failed() {
        assert_eq!(
            NoClientTools.run("mystery", &json!({})),
            Ok(json!("No tool implemented for \"mystery\"."))
        );
    }
}
