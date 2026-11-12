use tracing::{debug, warn};

use super::*;

pub fn parse_sse_line(line: &str) -> Option<StreamEvent> {
    let data = line.strip_prefix("data:")?.trim_start();
    if data == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    match v.get("type")?.as_str()? {
        "start" => Some(StreamEvent::Start {
            message_id: v
                .get("messageId")
                .and_then(|m| m.as_str())
                .unwrap_or_default()
                .into(),
        }),
        "text-delta" => Some(StreamEvent::TextDelta(v.get("delta")?.as_str()?.into())),
        "data-suggestions" => {
            let suggestions: Vec<String> = v
                .get("data")
                .and_then(|d| d.get("suggestions"))
                .and_then(|s| s.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            (!suggestions.is_empty()).then_some(StreamEvent::Suggestions(suggestions))
        }
        "tool-output-available" => {
            // The output event identifies its call by id alone: the name comes from the call's
            // earlier `tool-input-start` (see `ToolInputs::named`).
            let call_id = v
                .get("toolCallId")
                .and_then(|i| i.as_str())
                .unwrap_or_default();
            let name = v
                .get("toolName")
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            if call_id.is_empty() && name.is_empty() {
                return None;
            }
            Some(StreamEvent::ToolFinished {
                call_id: call_id.into(),
                name: name.into(),
                output: v
                    .get("output")
                    .and_then(|o| serde_json::to_string_pretty(o).ok()),
            })
        }
        "tool-output-error" | "tool-input-error" => {
            let call_id = v
                .get("toolCallId")
                .and_then(|i| i.as_str())
                .unwrap_or_default();
            let name = v
                .get("toolName")
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            if call_id.is_empty() && name.is_empty() {
                return None;
            }
            Some(StreamEvent::ToolFailed {
                call_id: call_id.into(),
                name: name.into(),
                error: tool_error_message(v.get("errorText").and_then(|e| e.as_str())?),
            })
        }
        "tool-input-start" | "tool-input-available" => {
            let Some(name) = v
                .get("toolName")
                .and_then(|t| t.as_str())
                .filter(|n| !n.is_empty())
            else {
                // A tool event without a name would only render as the placeholder "tool".
                return None;
            };
            let call_id = v
                .get("toolCallId")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string();
            if v.get("type").and_then(|t| t.as_str()) == Some("tool-input-start") {
                return Some(StreamEvent::ToolStarted {
                    call_id,
                    name: name.into(),
                });
            }
            Some(StreamEvent::ToolUsed(ToolCall {
                name: name.into(),
                input: v
                    .get("input")
                    .and_then(|i| serde_json::to_string_pretty(i).ok()),
                request: None,
                open: false,
                error: None,
            }))
        }
        _ => None,
    }
}

/// A tool's error text for display. Agent Studio wraps it as `{"error": "Error: …"}`; the
/// message inside reads better without the wrapper and its redundant "Error: " lead.
pub(super) fn tool_error_message(text: &str) -> String {
    let inner = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(String::from))
        .unwrap_or_else(|| text.to_string());
    let inner = inner.trim();
    inner.strip_prefix("Error: ").unwrap_or(inner).to_string()
}

/// The JSON part a `data:` line carries, else None (`[DONE]`, comments, blanks).
fn event_json(line: &str) -> Option<serde_json::Value> {
    serde_json::from_str(line.strip_prefix("data:")?.trim_start()).ok()
}

/// What a stream line is, for the log: its type and size. Its content is the user's and the
/// agent's, so it is never written.
fn describe(line: &str) -> String {
    let kind = event_json(line)
        .and_then(|part| part.get("type").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_else(|| "unparsed".into());
    format!("{kind} ({} bytes)", line.len())
}

/// The name of a value's variant (its `Debug` up to the first field), for the log.
fn variant_name(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}")
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Whether the line is a stream event the UI consumes nothing from but that is normal in the
/// AI SDK v5 protocol — lifecycle steps, text and reasoning part markers and `data-*`
/// payloads — so it logs at debug instead of warn.
fn recognised_but_ignored(line: &str) -> bool {
    let Some(v) = event_json(line) else {
        return false;
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some(
            "start-step"
            | "finish-step"
            | "text-start"
            | "text-end"
            | "reasoning-start"
            | "reasoning-delta"
            | "reasoning-end"
            | "finish"
            | "tool-input-start"
            | "tool-input-delta"
            | "tool-input-available"
            | "tool-output-available"
            | "tool-output-error"
            | "tool-input-error",
        ) => true,
        Some(t) => t.starts_with("data-"),
        None => false,
    }
}

/// The readable reason of the stream's `error` part. Agent Studio sends `errorText` as a JSON
/// payload (`message`, `type`, `statusCode`, `retryAfter`, `details`).
fn stream_error_message(part: &serde_json::Value) -> String {
    let Some(text) = part
        .get("errorText")
        .and_then(|e| e.as_str())
        .map(str::trim)
        .filter(|e| !e.is_empty())
    else {
        return "the agent stream failed".into();
    };
    let payload = serde_json::from_str::<serde_json::Value>(text).ok();
    let Some(message) = payload
        .as_ref()
        .and_then(|p| p.get("message")?.as_str())
        .filter(|m| !m.is_empty())
    else {
        return text.into();
    };
    match payload.as_ref().and_then(|p| p.get("retryAfter")?.as_f64()) {
        Some(seconds) => format!("{message} (retry after {seconds}s)"),
        None => message.into(),
    }
}

/// What to show for a turn a guardrail blocked (`data-guardrail-violation`): the agent's
/// fallback response, else why the reply is empty.
fn guardrail_notice(part: &serde_json::Value) -> String {
    let data = part.get("data");
    let field = |key: &str| {
        data.and_then(|d| d.get(key)?.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match (field("fallbackResponse"), field("category")) {
        (Some(fallback), _) => fallback.into(),
        (None, Some(category)) => {
            format!("This request was blocked by the agent's guardrails ({category}).")
        }
        (None, None) => "This request was blocked by the agent's guardrails.".into(),
    }
}

/// The input a `tool-input-error` carries: the model's emission, parsed or as raw text.
fn failed_tool_input(part: &serde_json::Value) -> Option<ToolCall> {
    let name = part.get("toolName")?.as_str().filter(|n| !n.is_empty())?;
    let input = match part.get("input")? {
        serde_json::Value::Null => None,
        serde_json::Value::String(raw) => pretty_json(raw),
        value => serde_json::to_string_pretty(value).ok(),
    }?;
    Some(ToolCall {
        name: name.into(),
        input: Some(input),
        request: None,
        error: None,
        open: false,
    })
}

/// The call id and text fragment a `tool-input-delta` line carries, else None.
fn tool_input_delta(line: &str) -> Option<(String, String)> {
    let v: serde_json::Value =
        serde_json::from_str(line.strip_prefix("data:")?.trim_start()).ok()?;
    if v.get("type")?.as_str()? != "tool-input-delta" {
        return None;
    }
    Some((
        v.get("toolCallId")?.as_str()?.into(),
        v.get("inputTextDelta")?.as_str()?.into(),
    ))
}

/// The `toolCallId` of a tool event line, else None.
fn tool_call_id(line: &str) -> Option<String> {
    let v: serde_json::Value =
        serde_json::from_str(line.strip_prefix("data:")?.trim_start()).ok()?;
    v.get("toolCallId")?.as_str().map(Into::into)
}

/// Accumulates `tool-input-delta` fragments per call, so a tool whose complete input never
/// arrives as `tool-input-available` still shows its input in the UI.
#[derive(Default)]
struct ToolInputs {
    calls: Vec<PendingCall>,
}

struct PendingCall {
    id: String,
    name: String,
    input: String,
    done: bool,
    failed: bool,
}

impl ToolInputs {
    fn note_call(&mut self, id: &str, name: &str) {
        if name.is_empty() {
            return;
        }
        if !self.calls.iter().any(|call| call.id == id) {
            self.calls.push(PendingCall {
                id: id.into(),
                name: name.into(),
                input: String::new(),
                done: false,
                failed: false,
            });
        }
    }

    fn call_mut(&mut self, id: &str) -> Option<&mut PendingCall> {
        self.calls.iter_mut().find(|call| call.id == id)
    }

    /// `ev` with the name of the call it ends filled in from the call's start, for the output
    /// events that carry only an id. An ending nobody started has no name to show.
    fn named(&self, ev: StreamEvent) -> Option<StreamEvent> {
        let lookup = |call_id: &str, name: String| -> Option<String> {
            if !name.is_empty() {
                return Some(name);
            }
            self.calls
                .iter()
                .find(|call| call.id == call_id)
                .map(|call| call.name.clone())
        };
        Some(match ev {
            StreamEvent::ToolFinished {
                call_id,
                name,
                output,
            } => StreamEvent::ToolFinished {
                name: lookup(&call_id, name)?,
                call_id,
                output,
            },
            StreamEvent::ToolFailed {
                call_id,
                name,
                error,
            } => StreamEvent::ToolFailed {
                name: lookup(&call_id, name)?,
                call_id,
                error,
            },
            other => other,
        })
    }

    /// Notes what `ev` (parsed from `line`) says about its call, and returns the event with its
    /// tool named; `None` for an output whose call never started, or a second failure of a
    /// call that already failed (`tool-input-error` is followed by its `tool-output-error`).
    fn track(&mut self, ev: StreamEvent, line: &str) -> Option<StreamEvent> {
        let ev = self.named(ev)?;
        if let Some(call_id) = tool_call_id(line) {
            match &ev {
                StreamEvent::ToolUsed(call) => {
                    self.note_call(&call_id, &call.name);
                    if call.input.is_some() {
                        self.finish(&call_id);
                    }
                }
                StreamEvent::ToolStarted { name, .. } => self.note_call(&call_id, name),
                StreamEvent::ToolFinished { name, .. } => {
                    self.note_call(&call_id, name);
                    self.finish(&call_id);
                }
                StreamEvent::ToolFailed { name, .. } => {
                    self.note_call(&call_id, name);
                    let call = self.call_mut(&call_id)?;
                    if call.failed {
                        return None;
                    }
                    call.failed = true;
                    call.done = true;
                }
                _ => {}
            }
        }
        Some(ev)
    }

    fn add_delta(&mut self, id: &str, fragment: &str) {
        if let Some(call) = self.call_mut(id) {
            call.input.push_str(fragment);
        }
    }

    pub(super) fn finish(&mut self, id: &str) {
        if let Some(call) = self.call_mut(id) {
            call.done = true;
        }
    }

    /// Unfinished calls with buffered input, pretty-printed.
    fn pending(&self) -> Vec<ToolCall> {
        self.calls
            .iter()
            .filter(|call| !call.input.is_empty() && !call.done)
            .map(|call| ToolCall {
                name: call.name.clone(),
                input: pretty_json(&call.input),
                open: false,
                request: None,
                error: None,
            })
            .collect()
    }
}

/// Turns the completions stream's lines into events: tool calls tracked across their parts,
/// each new text part opening a paragraph, guardrail blocks and approval requests shown as
/// text, and the stream's own `error` part ending the turn.
#[derive(Default)]
pub(super) struct Decoder {
    tools: ToolInputs,
    wrote_text: bool,
    paragraph_pending: bool,
    pub(super) deltas: usize,
}

impl Decoder {
    pub(super) fn line(&mut self, line: &str) -> Result<Vec<StreamEvent>, String> {
        if let Some((call_id, fragment)) = tool_input_delta(line) {
            self.tools.add_delta(&call_id, &fragment);
            return Ok(Vec::new());
        }
        let Some(part) = event_json(line) else {
            let quiet = line.trim().is_empty()
                || line.starts_with(':')
                || line.trim_end() == "data: [DONE]";
            if !quiet {
                warn!("unrecognised stream line: {}", describe(line));
            }
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        match part
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
        {
            "error" => {
                warn!("stream error: {}", describe(line));
                return Err(stream_error_message(&part));
            }
            "text-start" => {
                self.paragraph_pending = self.wrote_text;
                return Ok(events);
            }
            "data-guardrail-violation" => {
                debug!("guardrail blocked the turn");
                return Ok(self.notice(guardrail_notice(&part)));
            }
            "data-guardrail-error" => {
                warn!("the agent's guardrail provider failed");
                return Ok(events);
            }
            "data-tool-approval" => return Ok(self.approval(&part)),
            "tool-input-error" => {
                if let Some(call) = failed_tool_input(&part) {
                    events.extend(self.tools.track(StreamEvent::ToolUsed(call), line));
                }
            }
            _ => {}
        }
        match parse_sse_line(line) {
            Some(StreamEvent::TextDelta(text)) => events.extend(self.text(text)),
            Some(ev) => match self.tools.track(ev, line) {
                Some(ev) => {
                    debug!("stream event {}", variant_name(&ev));
                    events.push(ev);
                }
                None => debug!(
                    "a tool event for an unknown or settled call: {}",
                    describe(line)
                ),
            },
            None if recognised_but_ignored(line) => {
                debug!("ignored stream event: {}", describe(line))
            }
            None => warn!("unrecognised stream line: {}", describe(line)),
        }
        Ok(events)
    }

    /// The calls whose input only ever arrived as deltas, once the stream has ended.
    pub(super) fn finish(self) -> Vec<StreamEvent> {
        debug!("stream ended after {} text deltas", self.deltas);
        self.tools
            .pending()
            .into_iter()
            .map(|call| {
                debug!("finalizing buffered input for {}", call.name);
                StreamEvent::ToolUsed(call)
            })
            .collect()
    }

    fn text(&mut self, text: String) -> Vec<StreamEvent> {
        if text.is_empty() {
            return Vec::new();
        }
        if self.deltas == 0 {
            debug!("first text delta arrived");
        }
        self.deltas += 1;
        self.wrote_text = true;
        let text = if std::mem::take(&mut self.paragraph_pending) {
            format!("\n\n{text}")
        } else {
            text
        };
        vec![StreamEvent::TextDelta(text)]
    }

    /// Text the stream implies rather than streams, in a paragraph of its own.
    fn notice(&mut self, text: String) -> Vec<StreamEvent> {
        self.paragraph_pending = self.wrote_text;
        self.text(text)
    }

    /// A call that waits for the user's approval (`data-tool-approval`): its row shows the
    /// arguments, and the reply says why the turn stopped, since approving isn't supported.
    fn approval(&mut self, part: &serde_json::Value) -> Vec<StreamEvent> {
        let data = part.get("data");
        let Some(name) = data
            .and_then(|d| d.get("toolName")?.as_str())
            .filter(|n| !n.is_empty())
        else {
            return Vec::new();
        };
        if let Some(call_id) = data.and_then(|d| d.get("toolCallId")?.as_str()) {
            self.tools.note_call(call_id, name);
            self.tools.finish(call_id);
        }
        let call = ToolCall {
            name: name.into(),
            input: data
                .and_then(|d| d.get("args"))
                .and_then(|a| serde_json::to_string_pretty(a).ok()),
            request: None,
            error: None,
            open: false,
        };
        let mut events = vec![StreamEvent::ToolUsed(call)];
        events.extend(self.notice(format!(
            "`{name}` needs your approval before it runs, and as-gui can't approve tool calls yet."
        )));
        events
    }
}

fn pretty_json(raw: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .or_else(|| (!raw.trim().is_empty()).then(|| raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logged_stream_line_names_its_type_and_size_but_never_its_content() {
        let line = r#"data: {"type":"text-delta","delta":"a secret reply"}"#;
        let described = describe(line);
        assert_eq!(described, format!("text-delta ({} bytes)", line.len()));
        assert!(!described.contains("secret"));
        assert_eq!(describe("garbage with a secret"), "unparsed (21 bytes)");
    }

    #[test]
    fn a_logged_event_shows_its_variant_and_none_of_its_fields() {
        let event = StreamEvent::TextDelta("a secret reply".into());
        assert_eq!(variant_name(&event), "TextDelta");
    }

    fn deltas(lines: &[&str]) -> String {
        lines
            .iter()
            .filter_map(|l| match parse_sse_line(l) {
                Some(StreamEvent::TextDelta(t)) => Some(t),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_data_suggestions_line_becomes_a_suggestions_event() {
        let line = r#"data: {"type": "data-suggestions", "data": {"suggestions": ["Show me the best insoles for wide feet", "Explain how sizing works"]}}"#;
        match parse_sse_line(line) {
            Some(StreamEvent::Suggestions(suggestions)) => {
                assert_eq!(
                    suggestions,
                    [
                        "Show me the best insoles for wide feet",
                        "Explain how sizing works"
                    ]
                );
            }
            other => panic!("expected suggestions, got {other:?}"),
        }
    }

    #[test]
    fn a_suggestions_event_without_suggestions_is_dropped() {
        assert_eq!(
            parse_sse_line(r#"data: {"type": "data-suggestions", "data": {"suggestions": []}}"#),
            None
        );
        assert_eq!(
            parse_sse_line(r#"data: {"type": "data-suggestions"}"#),
            None
        );
    }

    #[test]
    fn the_v5_stream_assembles_text_deltas() {
        let text = deltas(&[
            "data: {\"type\":\"start\",\"messageId\":\"alg_msg_1\"}",
            "data: {\"type\":\"start-step\"}",
            "data: {\"type\":\"text-start\",\"id\":\"t1\"}",
            "data: {\"type\":\"text-delta\",\"id\":\"t1\",\"delta\":\"1 + 1\"}",
            "data: {\"type\":\"text-delta\",\"id\":\"t1\",\"delta\":\" = 2\"}",
            "data: {\"type\":\"text-end\",\"id\":\"t1\"}",
            "data: {\"type\":\"finish-step\"}",
            "data: {\"type\":\"finish\"}",
            "data: [DONE]",
        ]);
        assert_eq!(text, "1 + 1 = 2");
    }

    #[test]
    fn tool_parts_name_the_tool() {
        let ev = parse_sse_line(
            "data: {\"type\":\"tool-input-available\",\"toolCallId\":\"c1\",\"toolName\":\"algolia_search\"}",
        );
        assert_eq!(
            ev,
            Some(StreamEvent::ToolUsed(ToolCall {
                name: "algolia_search".into(),
                input: None,
                open: false,
                request: None,
                error: None,
            }))
        );
    }

    #[test]
    fn a_tool_input_start_announces_the_call_before_its_input() {
        let ev = parse_sse_line(
            "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\",\"toolName\":\"algolia_search_index\"}",
        );
        assert_eq!(
            ev,
            Some(StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "algolia_search_index".into(),
            })
        );
    }

    #[test]
    fn a_tool_output_finishes_the_call_it_started_with_the_same_id() {
        let started = parse_sse_line(
            "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\",\"toolName\":\"search_tool\"}",
        );
        let finished = parse_sse_line(
            "data: {\"type\":\"tool-output-available\",\"toolCallId\":\"c1\",\"toolName\":\"search_tool\",\"output\":{\"hits\":[]}}",
        );
        assert_eq!(
            started,
            Some(StreamEvent::ToolStarted {
                call_id: "c1".into(),
                name: "search_tool".into(),
            })
        );
        assert_eq!(
            finished,
            Some(StreamEvent::ToolFinished {
                call_id: "c1".into(),
                name: "search_tool".into(),
                output: Some("{\n  \"hits\": []\n}".into()),
            })
        );
    }

    #[test]
    fn a_tool_output_names_its_call_by_id_alone() {
        let ev = parse_sse_line(
            "data: {\"type\": \"tool-output-available\", \"toolCallId\": \"call_1\", \"output\": {\"hits\": [{\"title\": \"A\"}], \"nbHits\": 1}}",
        );
        let Some(StreamEvent::ToolFinished {
            call_id,
            name,
            output,
        }) = ev
        else {
            panic!("an output with a call id is a finished call: {ev:?}");
        };
        assert_eq!(call_id, "call_1");
        assert_eq!(name, "", "the stream's output event carries no tool name");
        let hits = SearchHit::from_json(&output.expect("the output JSON")).unwrap();
        assert_eq!(hits[0].title, "A");
    }

    #[test]
    fn a_tool_output_with_neither_id_nor_name_is_dropped() {
        let ev = parse_sse_line("data: {\"type\": \"tool-output-available\", \"output\": {}}");
        assert_eq!(ev, None);
    }

    #[test]
    fn a_finished_call_takes_the_name_its_start_announced() {
        let mut inputs = ToolInputs::default();
        inputs.note_call("call_1", "algolia_search_index_products");
        let finished = StreamEvent::ToolFinished {
            call_id: "call_1".into(),
            name: String::new(),
            output: Some("{}".into()),
        };
        assert_eq!(
            inputs.named(finished),
            Some(StreamEvent::ToolFinished {
                call_id: "call_1".into(),
                name: "algolia_search_index_products".into(),
                output: Some("{}".into()),
            })
        );
    }

    #[test]
    fn a_finished_call_nobody_started_is_dropped_not_shown_nameless() {
        let inputs = ToolInputs::default();
        let finished = StreamEvent::ToolFinished {
            call_id: "ghost".into(),
            name: String::new(),
            output: Some("{}".into()),
        };
        assert_eq!(inputs.named(finished), None);
    }

    #[test]
    fn a_finished_call_that_names_itself_keeps_its_name() {
        let inputs = ToolInputs::default();
        let finished = StreamEvent::ToolFinished {
            call_id: "c".into(),
            name: "search_tool".into(),
            output: None,
        };
        assert_eq!(inputs.named(finished.clone()), Some(finished));
    }

    /// Feeds stream lines through the parser and the call bookkeeping the stream loop uses.
    fn replay(lines: &[&str]) -> Vec<StreamEvent> {
        let mut inputs = ToolInputs::default();
        lines
            .iter()
            .filter_map(|line| parse_sse_line(line).and_then(|ev| inputs.track(ev, line)))
            .collect()
    }

    /// The tool part of an Agent Studio stream: the output event carries the call id and the
    /// output, and no tool name.
    const RECORDED_TOOL_STREAM: [&str; 4] = [
        "data: {\"type\": \"tool-input-start\", \"toolCallId\": \"call_search_1\", \"toolName\": \"algolia_search_index_products_markdown\"}",
        "data: {\"type\": \"tool-input-available\", \"toolCallId\": \"call_search_1\", \"toolName\": \"algolia_search_index_products_markdown\", \"input\": {\"queries\": [{\"query\": \"quick start\"}]}}",
        "data: {\"type\": \"tool-output-available\", \"toolCallId\": \"call_search_1\", \"output\": {\"hits\": [{\"title\": \"Quick Start Guide\", \"objectID\": \"guide-0\"}], \"nbHits\": 7}}",
        "data: {\"type\": \"data-tool-output-metadata\", \"data\": {\"toolCallId\": \"call_search_1\", \"metadata\": {}}}",
    ];

    /// The stream's reply to a search the index couldn't run.
    const RECORDED_TOOL_ERROR: &str = r#"data: {"type": "tool-output-error", "toolCallId": "call_facet_1", "errorText": "{\"error\": \"Error: Cannot search in `brand` attribute, you need to add `searchable(brand)` to attributesForFaceting. (Correlation-ID: EXAMPLE-0aBcD-0123456789abcdef)\"}"}"#;

    const CANNOT_SEARCH: &str = "Cannot search in `brand` attribute, you need to add `searchable(brand)` to attributesForFaceting. (Correlation-ID: EXAMPLE-0aBcD-0123456789abcdef)";

    #[test]
    fn a_tool_output_error_fails_its_call_with_the_readable_message() {
        assert_eq!(
            parse_sse_line(RECORDED_TOOL_ERROR),
            Some(StreamEvent::ToolFailed {
                call_id: "call_facet_1".into(),
                name: String::new(),
                error: CANNOT_SEARCH.into(),
            })
        );
    }

    #[test]
    fn a_plain_error_text_is_shown_as_it_came() {
        let ev = parse_sse_line(
            r#"data: {"type":"tool-output-error","toolCallId":"c1","errorText":"Timed out after 30s"}"#,
        );
        assert!(
            matches!(ev, Some(StreamEvent::ToolFailed { error, .. }) if error == "Timed out after 30s")
        );
    }

    #[test]
    fn a_tool_input_error_fails_its_call_too() {
        let ev = parse_sse_line(
            r#"data: {"type":"tool-input-error","toolCallId":"c1","toolName":"search_tool","input":{},"errorText":"Invalid input"}"#,
        );
        assert_eq!(
            ev,
            Some(StreamEvent::ToolFailed {
                call_id: "c1".into(),
                name: "search_tool".into(),
                error: "Invalid input".into(),
            })
        );
    }

    #[test]
    fn a_tool_error_nobody_can_place_is_dropped_without_ending_the_stream() {
        let line = r#"data: {"type":"tool-output-error","errorText":"boom"}"#;
        assert_eq!(parse_sse_line(line), None);
        assert_eq!(
            decode(&[line]),
            Ok(vec![]),
            "it isn't the stream's own error"
        );
    }

    #[test]
    fn the_streams_own_error_still_ends_it() {
        assert_eq!(
            decode(&[r#"data: {"type":"error","errorText":"boom"}"#]),
            Err("boom".into())
        );
    }

    #[test]
    fn a_stream_error_shows_the_message_inside_its_payload() {
        let line = r#"data: {"type": "error", "errorText": "{\"message\": \"Too many requests\", \"type\": \"rate_limited\", \"statusCode\": 429, \"retryAfter\": 30}"}"#;
        assert_eq!(
            decode(&[line]),
            Err("Too many requests (retry after 30s)".into())
        );
    }

    #[test]
    fn a_stream_error_without_text_still_ends_the_stream() {
        assert_eq!(
            decode(&[r#"data: {"type":"error"}"#]),
            Err("the agent stream failed".into())
        );
    }

    /// Feeds stream lines through the decoder the stream loop uses, then ends the stream.
    fn decode(lines: &[&str]) -> Result<Vec<StreamEvent>, String> {
        let mut decoder = Decoder::default();
        let mut events = Vec::new();
        for line in lines {
            events.extend(decoder.line(line)?);
        }
        events.extend(decoder.finish());
        Ok(events)
    }

    fn text_of(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter_map(|ev| match ev {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reasoning_never_ends_the_stream_even_when_it_mentions_an_error() {
        let lines = [
            r#"data: {"type":"reasoning-start","id":"m1"}"#,
            r#"data: {"type":"reasoning-delta","id":"m1","delta":"Maybe the error is in the index."}"#,
            r#"data: {"type":"reasoning-end","id":"m1"}"#,
        ];
        assert_eq!(decode(&lines), Ok(vec![]));
        assert!(lines.iter().all(|l| recognised_but_ignored(l)));
    }

    #[test]
    fn text_parts_of_separate_steps_read_as_separate_paragraphs() {
        let events = decode(&[
            r#"data: {"type":"start-step"}"#,
            r#"data: {"type":"text-start","id":"m1"}"#,
            r#"data: {"type":"text-delta","id":"m1","delta":"Let me look."}"#,
            r#"data: {"type":"text-end","id":"m1"}"#,
            r#"data: {"type":"tool-input-start","toolCallId":"c1","toolName":"search"}"#,
            r#"data: {"type":"tool-input-available","toolCallId":"c1","toolName":"search","input":{}}"#,
            r#"data: {"type":"tool-output-available","toolCallId":"c1","output":{}}"#,
            r#"data: {"type":"finish-step"}"#,
            r#"data: {"type":"start-step"}"#,
            r#"data: {"type":"text-start","id":"m1"}"#,
            r#"data: {"type":"text-delta","id":"m1","delta":"Found "}"#,
            r#"data: {"type":"text-delta","id":"m1","delta":"it."}"#,
            r#"data: {"type":"text-end","id":"m1"}"#,
        ])
        .unwrap();
        assert_eq!(text_of(&events), "Let me look.\n\nFound it.");
    }

    #[test]
    fn a_text_part_with_no_text_adds_no_break() {
        let events = decode(&[
            r#"data: {"type":"text-start","id":"m1"}"#,
            r#"data: {"type":"text-end","id":"m1"}"#,
            r#"data: {"type":"text-start","id":"m1"}"#,
            r#"data: {"type":"text-delta","id":"m1","delta":"Hi."}"#,
        ])
        .unwrap();
        assert_eq!(text_of(&events), "Hi.");
    }

    #[test]
    fn a_guardrail_block_shows_its_fallback_response() {
        let events = decode(&[
            r#"data: {"type":"start","messageId":"m1"}"#,
            r#"data: {"type":"data-guardrail-violation","data":{"category":"prompt_injection","guardrailType":"input","fallbackResponse":"I can't help with that."}}"#,
            r#"data: {"type":"finish"}"#,
        ])
        .unwrap();
        assert_eq!(text_of(&events), "I can't help with that.");
    }

    #[test]
    fn a_guardrail_block_without_a_fallback_says_why_the_reply_is_empty() {
        let events = decode(&[
            r#"data: {"type":"data-guardrail-violation","data":{"category":"prompt_injection","guardrailType":"input","fallbackResponse":null}}"#,
        ])
        .unwrap();
        let text = text_of(&events);
        assert!(text.contains("blocked"), "{text}");
        assert!(text.contains("prompt_injection"), "{text}");
    }

    #[test]
    fn a_broken_guardrail_provider_does_not_end_the_stream() {
        let line = r#"data: {"type":"data-guardrail-error","data":{"errorMessage":"provider error","guardrailType":"input"}}"#;
        assert_eq!(decode(&[line]), Ok(vec![]));
    }

    #[test]
    fn a_tool_awaiting_approval_shows_its_call_and_why_the_turn_stopped() {
        let events = decode(&[
            r#"data: {"type":"data-tool-approval","data":{"toolCallId":"c1","toolName":"delete_records","args":{"ids":[1]},"description":"Deletes records","argsHash":"h"}}"#,
        ])
        .unwrap();
        let Some(StreamEvent::ToolUsed(call)) = events.first() else {
            panic!("the call shows first: {events:?}");
        };
        assert_eq!(call.name, "delete_records");
        assert_eq!(
            call.input.as_deref(),
            Some("{\n  \"ids\": [\n    1\n  ]\n}")
        );
        let text = text_of(&events);
        assert!(text.contains("delete_records"), "{text}");
        assert!(text.contains("approval"), "{text}");
    }

    #[test]
    fn a_tool_input_error_keeps_the_input_and_fails_the_call_once() {
        let events = decode(&[
            r#"data: {"type":"tool-input-start","toolCallId":"c1","toolName":"search"}"#,
            r#"data: {"type":"tool-input-delta","toolCallId":"c1","inputTextDelta":"{\"query\": "}"#,
            r#"data: {"type":"tool-input-error","toolCallId":"c1","toolName":"search","input":"{\"query\": ","errorText":"Invalid JSON"}"#,
            r#"data: {"type":"tool-output-error","toolCallId":"c1","errorText":"Invalid JSON"}"#,
        ])
        .unwrap();
        assert_eq!(
            events,
            vec![
                StreamEvent::ToolStarted {
                    call_id: "c1".into(),
                    name: "search".into(),
                },
                StreamEvent::ToolUsed(ToolCall {
                    name: "search".into(),
                    input: Some("{\"query\": ".into()),
                    request: None,
                    error: None,
                    open: false,
                }),
                StreamEvent::ToolFailed {
                    call_id: "c1".into(),
                    name: "search".into(),
                    error: "Invalid JSON".into(),
                },
            ],
            "the buffered fragments aren't finalized again, and the paired output error is dropped"
        );
    }

    #[test]
    fn a_call_buffered_from_deltas_alone_is_finalized_when_the_stream_ends() {
        let events = decode(&[
            r#"data: {"type":"tool-input-start","toolCallId":"c1","toolName":"search"}"#,
            r#"data: {"type":"tool-input-delta","toolCallId":"c1","inputTextDelta":"{\"q\": 1}"}"#,
        ])
        .unwrap();
        assert!(
            matches!(events.last(), Some(StreamEvent::ToolUsed(call)) if call.input.as_deref() == Some("{\n  \"q\": 1\n}"))
        );
    }

    #[test]
    fn the_done_marker_and_keep_alives_are_quiet() {
        assert_eq!(decode(&["data: [DONE]", ": keep-alive", ""]), Ok(vec![]));
    }

    #[test]
    fn a_failed_call_takes_the_name_its_start_announced() {
        let mut inputs = ToolInputs::default();
        inputs.note_call("c1", "algolia_search_for_facet_values");
        let failed = StreamEvent::ToolFailed {
            call_id: "c1".into(),
            name: String::new(),
            error: "no".into(),
        };
        assert_eq!(
            inputs.named(failed),
            Some(StreamEvent::ToolFailed {
                call_id: "c1".into(),
                name: "algolia_search_for_facet_values".into(),
                error: "no".into(),
            })
        );
        let ghost = StreamEvent::ToolFailed {
            call_id: "ghost".into(),
            name: String::new(),
            error: "no".into(),
        };
        assert_eq!(inputs.named(ghost), None);
    }

    #[test]
    fn a_failed_tool_stream_fails_one_named_call_and_the_stream_goes_on() {
        let lines = [
            r#"data: {"type": "tool-input-start", "toolCallId": "call_facet_1", "toolName": "algolia_search_for_facet_values"}"#,
            r#"data: {"type": "tool-input-available", "toolCallId": "call_facet_1", "toolName": "algolia_search_for_facet_values", "input": {"facetName": "brand"}}"#,
            RECORDED_TOOL_ERROR,
            r#"data: {"type": "text-delta", "id": "t1", "delta": "Let me try another way."}"#,
        ];
        let events = replay(&lines);
        assert!(
            matches!(&events[2], StreamEvent::ToolFailed { name, error, .. }
            if name == "algolia_search_for_facet_values" && error == CANNOT_SEARCH)
        );
        assert!(decode(&lines).is_ok(), "nothing here aborts the stream");
    }

    #[test]
    fn a_tool_stream_starts_inputs_and_finishes_one_named_call() {
        let events = replay(&RECORDED_TOOL_STREAM);
        assert!(
            matches!(&events[0], StreamEvent::ToolStarted { name, .. } if name == "algolia_search_index_products_markdown")
        );
        assert!(matches!(&events[1], StreamEvent::ToolUsed(call) if call.input.is_some()));
        let StreamEvent::ToolFinished {
            call_id,
            name,
            output,
        } = &events[2]
        else {
            panic!("the output finishes the call: {events:?}");
        };
        assert_eq!(call_id, "call_search_1");
        assert_eq!(name, "algolia_search_index_products_markdown");
        assert!(output.as_deref().unwrap().contains("Quick Start Guide"));
        assert_eq!(events.len(), 3, "the metadata line is not a tool event");
    }

    #[test]
    fn a_finished_call_is_not_finalized_again_at_the_end_of_the_stream() {
        let mut inputs = ToolInputs::default();
        for line in RECORDED_TOOL_STREAM {
            if let Some(ev) = parse_sse_line(line) {
                inputs.track(ev, line);
            }
        }
        assert!(inputs.pending().is_empty());
    }

    #[test]
    fn a_harmless_line_mentioning_an_error_does_not_end_the_stream() {
        let metadata =
            "data: {\"type\": \"data-tool-output-metadata\", \"data\": {\"note\": \"no error\"}}";
        assert_eq!(decode(&[metadata]), Ok(vec![]));
        assert_eq!(
            decode(&["data: {\"type\": \"finish-step\", \"error\": null}"]),
            Ok(vec![])
        );
        assert_eq!(decode(&["data: an error happened"]), Ok(vec![]));
    }

    #[test]
    fn lifecycle_and_data_events_are_recognised_but_ignored() {
        for line in [
            "data: {\"type\":\"start-step\"}",
            "data: {\"type\":\"finish-step\"}",
            "data: {\"type\":\"text-start\",\"id\":\"t1\"}",
            "data: {\"type\":\"text-end\",\"id\":\"t1\"}",
            "data: {\"type\":\"finish\"}",
            "data: {\"type\":\"data-suggestions\",\"data\":{\"suggestions\":[]}}",
            "data: {\"type\":\"data-tool-output-metadata\",\"data\":{},\"transient\":true}",
        ] {
            assert_eq!(parse_sse_line(line), None, "{line}");
            assert!(
                recognised_but_ignored(line),
                "{line} logs at debug, not warn"
            );
        }
    }

    #[test]
    fn unknown_events_and_garbage_are_not_recognised() {
        assert!(!recognised_but_ignored("data: {\"type\":\"warp-drive\"}"));
        assert!(!recognised_but_ignored("data: not json"));
        assert!(!recognised_but_ignored(": keep-alive"));
    }

    #[test]
    fn a_tool_event_without_a_name_is_dropped() {
        for line in [
            "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\"}",
            "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\",\"toolName\":\"\"}",
            "data: {\"type\":\"tool-input-available\",\"toolCallId\":\"c1\",\"input\":{}}",
            "data: {\"type\":\"tool-input-available\",\"toolCallId\":\"c1\",\"toolName\":\"\",\"input\":{}}",
        ] {
            assert_eq!(parse_sse_line(line), None, "{line}");
            assert!(
                recognised_but_ignored(line),
                "an unnamed tool event logs at debug, not warn: {line}"
            );
        }
    }

    #[test]
    fn tool_input_deltas_accumulate_into_the_announced_call() {
        let mut inputs = ToolInputs::default();
        inputs.note_call("call_1", "algolia_search");
        for fragment in ["{\"qu", "ery\": \"ru", "stic season\"}"] {
            inputs.add_delta("call_1", fragment);
        }
        assert_eq!(
            inputs.pending(),
            vec![ToolCall {
                name: "algolia_search".into(),
                input: Some("{\n  \"query\": \"rustic season\"\n}".into()),
                open: false,
                request: None,
                error: None,
            }]
        );
    }

    #[test]
    fn a_call_with_its_complete_input_is_not_finalized_twice() {
        let mut inputs = ToolInputs::default();
        inputs.note_call("call_1", "search");
        inputs.add_delta("call_1", "{\"q\": 1}");
        inputs.finish("call_1");
        assert!(inputs.pending().is_empty());
    }

    #[test]
    fn a_call_without_deltas_has_nothing_to_finalize() {
        let mut inputs = ToolInputs::default();
        inputs.note_call("call_1", "search");
        inputs.note_call("call_2", "other");
        inputs.add_delta("call_2", "{}");
        inputs.finish("call_2");
        assert!(inputs.pending().is_empty());
    }

    #[test]
    fn delta_lines_yield_their_call_and_fragment() {
        assert_eq!(
            tool_input_delta(
                "data: {\"type\":\"tool-input-delta\",\"toolCallId\":\"c1\",\"inputTextDelta\":\"e\"}"
            ),
            Some(("c1".into(), "e".into()))
        );
        assert_eq!(
            tool_input_delta("data: {\"type\":\"text-delta\",\"delta\":\"e\"}"),
            None
        );
        assert_eq!(
            tool_input_delta(
                "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\",\"toolName\":\"t\"}"
            ),
            None
        );
        assert_eq!(
            tool_call_id(
                "data: {\"type\":\"tool-input-start\",\"toolCallId\":\"c1\",\"toolName\":\"t\"}"
            ),
            Some("c1".into())
        );
        assert_eq!(
            tool_call_id("data: {\"type\":\"text-delta\",\"delta\":\"e\"}"),
            None
        );
    }

    #[test]
    fn a_tool_input_pretty_prints_its_json() {
        let ev = parse_sse_line(
            "data: {\"type\":\"tool-input-available\",\"toolCallId\":\"c1\",\"toolName\":\"schedule_task_tool\",\"input\":{\"name\":\"inbox-check\"}}",
        );
        assert_eq!(
            ev,
            Some(StreamEvent::ToolUsed(ToolCall {
                name: "schedule_task_tool".into(),
                input: Some("{\n  \"name\": \"inbox-check\"\n}".into()),
                open: false,
                request: None,
                error: None,
            }))
        );
    }

    #[test]
    fn unknown_parts_and_blanks_are_tolerated() {
        assert_eq!(parse_sse_line("data: {\"type\":\"start-step\"}"), None);
        assert_eq!(parse_sse_line(": keep-alive"), None);
        assert_eq!(parse_sse_line(""), None);
    }
}
