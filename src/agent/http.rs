use std::io::BufRead;
use std::sync::Arc;

use serde_json::json;
use tracing::{debug, warn};

use super::*;

/// Lists an application's agents (`settings` ACL) and an agent's conversation history
/// (`logs` ACL).
pub trait AgentDirectory: Send + Sync {
    fn list(&self) -> Result<Vec<AgentInfo>, String>;
    fn sessions(&self, agent_id: &str) -> Result<Vec<SessionInfo>, String>;
    fn conversation(
        &self,
        agent_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<ChatMessage>, String>;
}

pub trait AgentClient: Send + Sync {
    /// Human-readable client name for debug logs.
    fn name(&self) -> &'static str;

    /// Sends 👍/👎 for a persisted assistant message (`search` ACL); one vote per message.
    fn send_feedback(&self, agent_id: &str, message_id: &str, vote: i8) -> Result<(), String>;

    /// Streams one assistant turn for `history` into `emit`, continuing the conversation
    /// `id` names when given. The caller sends `Finished` on `Ok` and turns `Err` into a
    /// failure; `emit` must not block the app.
    fn stream(
        &self,
        history: &[ChatMessage],
        conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String>;
}

/// Runs one turn on its own thread: every event goes to `send`, then `Finished` or `Failed`,
/// then the UI is woken. The sender is the caller's, so each can stamp or route events its
/// own way.
pub fn spawn_stream(
    client: Arc<dyn AgentClient>,
    history: Vec<ChatMessage>,
    conversation: Option<String>,
    send: impl Fn(StreamEvent) + Send + 'static,
    waker: Option<rune::Waker>,
) {
    std::thread::spawn(move || {
        let result = client.stream(&history, conversation.as_deref(), &mut |event| send(event));
        send(match result {
            Ok(()) => StreamEvent::Finished,
            Err(error) => StreamEvent::Failed(error),
        });
        if let Some(wake) = waker {
            wake();
        }
    });
}

/// The completions request body: the exchange's messages, and the conversation `id` when
/// the turn continues one — the server then appends to it instead of starting a new one.
/// Only a trailing assistant message goes out as its parts: that is a turn continuing after
/// client tools ran. Every other message is its text.
fn request_body(history: &[ChatMessage], conversation: Option<&str>) -> serde_json::Value {
    let last = history.len().saturating_sub(1);
    let messages: Vec<_> = history
        .iter()
        .enumerate()
        .map(|(index, m)| {
            let replay = index == last && m.role == Role::Assistant && !m.parts.is_empty();
            let parts: Vec<_> = if replay {
                m.parts.iter().filter_map(part_json).collect()
            } else {
                vec![json!({ "type": "text", "text": m.text })]
            };
            json!({
                "id": m.id,
                "role": if m.role == Role::User { "user" } else { "assistant" },
                "parts": parts,
            })
        })
        .collect();
    match conversation {
        Some(id) => json!({ "id": id, "messages": messages }),
        None => json!({ "messages": messages }),
    }
}

fn part_json(part: &Part) -> Option<serde_json::Value> {
    match part {
        Part::StepStart => Some(json!({ "type": "step-start" })),
        Part::Text(text) if text.is_empty() => None,
        Part::Text(text) => Some(json!({ "type": "text", "text": text })),
        Part::Tool(tool) => Some(tool_part_json(tool)),
    }
}

fn tool_part_json(tool: &ToolPart) -> serde_json::Value {
    let mut part = json!({
        "type": format!("tool-{}", tool.name),
        "toolCallId": tool.call_id,
        "input": tool.input,
    });
    match tool.state {
        ToolState::InputAvailable => part["state"] = json!("input-available"),
        ToolState::OutputAvailable => {
            part["state"] = json!("output-available");
            part["output"] = non_empty_output(tool.output.clone().unwrap_or_default());
        }
        ToolState::OutputError => {
            part["state"] = json!("output-error");
            part["errorText"] = json!(tool.error.as_deref().unwrap_or("The tool call failed."));
        }
    }
    if let Some(metadata) = &tool.provider_metadata {
        part["callProviderMetadata"] = metadata.clone();
    }
    part
}

pub struct AlgoliaClient {
    pub app_id: String,
    pub api_key: String,
    pub agent_id: String,
}

/// Percent-encodes a value for one URL path segment, so an id can't add segments, a query or a
/// fragment.
pub fn path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The HTTP client every Algolia call uses. An error status comes back as a response rather
/// than an error, so its body (the API's reason, such as a 422's validation message) can be
/// read and shown.
pub(super) fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

fn agent_studio_url(app_id: &str, path: &str) -> String {
    format!("https://{app_id}.algolia.net/agent-studio/1/{path}")
}

pub(super) fn authed<B>(
    request: ureq::RequestBuilder<B>,
    app_id: &str,
    api_key: &str,
) -> ureq::RequestBuilder<B> {
    request
        .header("x-algolia-application-id", app_id)
        .header("x-algolia-api-key", api_key)
}

const ERROR_DETAIL_CHARS: usize = 200;

fn status_error(status: impl std::fmt::Display, detail: &str) -> String {
    let detail: String = detail.chars().take(ERROR_DETAIL_CHARS).collect();
    format!("agent studio returned HTTP {status} — {detail}")
}

/// The response when it succeeded; otherwise the error carrying the API's reason, which a
/// validation failure (422) puts in the body.
fn check_status(
    response: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let detail = response.into_body().read_to_string().unwrap_or_default();
    let error = status_error(status, &detail);
    warn!("{error}");
    Err(error)
}

/// A parsed page: its items and how many pages are left.
type Page<T> = Result<(Vec<T>, usize), String>;

/// Every page of a paged listing: `fetch` gets a page number (from 1) and returns its body,
/// `parse` splits that into the page's items and how many pages are left.
fn paginate<T>(
    what: &str,
    mut fetch: impl FnMut(usize) -> Result<String, String>,
    parse: fn(&str) -> Page<T>,
) -> Result<Vec<T>, String> {
    let mut all = Vec::new();
    let mut remaining = 1;
    let mut page = 1;
    while remaining > 0 {
        let (items, pages_left) = parse(&fetch(page)?)?;
        debug!(
            "page {page}: {} {what}, {pages_left} pages left",
            items.len()
        );
        all.extend(items);
        remaining = pages_left;
        page += 1;
    }
    debug!("listed {} {what} in total", all.len());
    Ok(all)
}

impl AlgoliaClient {
    fn url(&self) -> String {
        agent_studio_url(
            &self.app_id,
            &format!(
                "agents/{}/completions?stream=true&compatibilityMode=ai-sdk-5",
                path_segment(&self.agent_id)
            ),
        )
    }

    fn authed<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        authed(request, &self.app_id, &self.api_key)
    }

    /// An authenticated GET's body, with the error handling every directory call shares.
    fn get(&self, path: &str) -> Result<String, String> {
        let url = agent_studio_url(&self.app_id, path);
        debug!("GET {url}");
        let response = self.authed(http().get(&url)).call().map_err(|e| {
            warn!("request failed: {e}");
            format!("request failed: {e}")
        })?;
        check_status(response)?
            .into_body()
            .read_to_string()
            .map_err(|e| {
                warn!("reading the response failed: {e}");
                format!("reading the response failed: {e}")
            })
    }
}

impl AgentClient for AlgoliaClient {
    fn name(&self) -> &'static str {
        "agent-studio"
    }

    fn send_feedback(&self, agent_id: &str, message_id: &str, vote: i8) -> Result<(), String> {
        let url = agent_studio_url(&self.app_id, "feedback");
        debug!("POST {url} (message {message_id}, vote {vote})");
        let response = self
            .authed(http().post(&url))
            .send_json(json!({ "messageId": message_id, "agentId": agent_id, "vote": vote }))
            .map_err(|e| {
                warn!("feedback failed: {e}");
                format!("feedback failed: {e}")
            })?;
        check_status(response).map(drop)
    }

    fn stream(
        &self,
        history: &[ChatMessage],
        conversation: Option<&str>,
        emit: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), String> {
        debug!("POST {} ({} messages)", self.url(), history.len());
        let response = self
            .authed(http().post(self.url()))
            .send_json(request_body(history, conversation))
            .map_err(|e| {
                warn!("request failed: {e}");
                format!("request failed: {e}")
            })?;
        let response = check_status(response)?;
        if let Some(id) = response
            .headers()
            .get("x-conversation-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
        {
            debug!("conversation id {}", id);
            emit(StreamEvent::ConversationStarted(id.to_string()));
        }
        let reader = response.into_body().into_reader();
        let mut decoder = Decoder::default();
        for line in std::io::BufReader::new(reader).lines() {
            let line = line.map_err(|e| {
                warn!("stream read failed after {} deltas: {e}", decoder.deltas);
                format!("stream read failed: {e}")
            })?;
            decoder.line(&line)?.into_iter().for_each(&mut *emit);
        }
        decoder.finish().into_iter().for_each(emit);
        Ok(())
    }
}

impl AgentDirectory for AlgoliaClient {
    fn list(&self) -> Result<Vec<AgentInfo>, String> {
        paginate(
            "agents",
            |page| self.get(&format!("agents?page={page}&limit=100")),
            parse_agents_page,
        )
    }

    fn sessions(&self, agent_id: &str) -> Result<Vec<SessionInfo>, String> {
        paginate(
            "conversations",
            |page| {
                self.get(&format!(
                    "agents/{}/conversations?page={page}&limit=100",
                    path_segment(agent_id)
                ))
            },
            parse_conversations_page,
        )
    }

    fn conversation(
        &self,
        agent_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<ChatMessage>, String> {
        let path = format!(
            "agents/{}/conversations/{}",
            path_segment(agent_id),
            path_segment(conversation_id)
        );
        parse_conversation(&self.get(&path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paged_listing_fetches_pages_until_none_are_left() {
        let mut fetched = Vec::new();
        let items = paginate(
            "numbers",
            |page| {
                fetched.push(page);
                Ok(format!("{page}"))
            },
            |body| {
                let page: usize = body.parse().unwrap();
                Ok((vec![page * 10], 3 - page))
            },
        )
        .unwrap();
        assert_eq!(fetched, [1, 2, 3]);
        assert_eq!(items, [10, 20, 30]);
    }

    #[test]
    fn a_paged_listing_stops_at_the_first_failed_page() {
        let result = paginate::<u8>(
            "numbers",
            |page| {
                if page == 2 {
                    Err("down".into())
                } else {
                    Ok(String::new())
                }
            },
            |_| Ok((vec![1], 5)),
        );
        assert_eq!(result, Err("down".into()));
    }

    #[test]
    fn an_error_status_carries_the_status_and_at_most_200_characters_of_the_reason() {
        let long = "x".repeat(500);
        let error = status_error(422, &long);
        assert_eq!(
            error,
            format!("agent studio returned HTTP 422 — {}", "x".repeat(200))
        );
    }

    #[test]
    fn agent_studio_urls_share_one_host_and_prefix() {
        assert_eq!(
            agent_studio_url("APPID12345", "agents?page=1&limit=100"),
            "https://APPID12345.algolia.net/agent-studio/1/agents?page=1&limit=100"
        );
    }

    #[test]
    fn a_spawned_stream_sends_its_events_then_finishes_and_wakes_the_ui() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;
        let (sender, receiver) = mpsc::channel();
        let woken = Arc::new(AtomicUsize::new(0));
        let waker: rune::Waker = {
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::SeqCst);
            })
        };
        let client: Arc<dyn AgentClient> = Arc::new(MockAgent {
            response: "Hi",
            chunk_chars: 8,
            tools: Vec::new(),
            script: None,
        });
        spawn_stream(
            client,
            vec![ChatMessage::user("u1", "hello")],
            None,
            move |event| {
                let _ = sender.send(event);
            },
            Some(waker),
        );
        let events: Vec<StreamEvent> = receiver.iter().collect();
        assert!(matches!(events.last(), Some(StreamEvent::Finished)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta(_)))
        );
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_spawned_stream_reports_a_failed_request_as_an_event() {
        struct Failing;
        impl AgentClient for Failing {
            fn name(&self) -> &'static str {
                "failing"
            }
            fn send_feedback(&self, _: &str, _: &str, _: i8) -> Result<(), String> {
                Ok(())
            }
            fn stream(
                &self,
                _: &[ChatMessage],
                _: Option<&str>,
                _: &mut dyn FnMut(StreamEvent),
            ) -> Result<(), String> {
                Err("boom".into())
            }
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        spawn_stream(
            Arc::new(Failing),
            Vec::new(),
            None,
            move |event| {
                let _ = sender.send(event);
            },
            None,
        );
        let events: Vec<StreamEvent> = receiver.iter().collect();
        assert!(matches!(events.as_slice(), [StreamEvent::Failed(e)] if e == "boom"));
    }

    #[test]
    fn a_path_segment_keeps_unreserved_characters_and_escapes_the_rest() {
        assert_eq!(path_segment("my-index_v1.2~x"), "my-index_v1.2~x");
        assert_eq!(path_segment("a/../b?c#d e'f"), "a%2F..%2Fb%3Fc%23d%20e%27f");
        assert_eq!(path_segment("é"), "%C3%A9");
    }

    #[test]
    fn a_completions_url_cannot_leave_its_agent_path() {
        let client = AlgoliaClient {
            app_id: "APPID12345".into(),
            api_key: "k".into(),
            agent_id: "a/../b?x#y".into(),
        };
        assert_eq!(
            client.url(),
            "https://APPID12345.algolia.net/agent-studio/1/agents/a%2F..%2Fb%3Fx%23y/completions?stream=true&compatibilityMode=ai-sdk-5"
        );
    }

    #[test]
    fn the_request_body_carries_the_conversation_it_continues() {
        let history = vec![
            ChatMessage::user("m1", "Hello"),
            ChatMessage::assistant("m2"),
        ];
        let fresh = request_body(&history, None);
        assert!(
            fresh.get("id").is_none(),
            "a new turn starts no conversation"
        );
        assert_eq!(fresh["messages"].as_array().unwrap().len(), 2);
        assert_eq!(fresh["messages"][0]["parts"][0]["text"], "Hello");

        let continuing = request_body(&history, Some("alg_cnv_1"));
        assert_eq!(
            continuing["id"], "alg_cnv_1",
            "the turn continues the conversation"
        );
    }

    fn tool(call_id: &str, name: &str, state: ToolState) -> ToolPart {
        ToolPart {
            call_id: call_id.into(),
            name: name.into(),
            input: json!({ "q": call_id }),
            state,
            output: None,
            error: None,
            provider_metadata: None,
        }
    }

    fn continued(parts: Vec<Part>) -> serde_json::Value {
        let mut reply = ChatMessage::assistant("m2");
        reply.text = "ignored: the parts are what goes out".into();
        reply.parts = parts;
        request_body(&[ChatMessage::user("m1", "Hello"), reply], Some("c1"))
    }

    #[test]
    fn the_trailing_message_replays_its_parts_in_order() {
        let mut call = tool("c1", "throw_confetti", ToolState::OutputAvailable);
        call.output = Some(json!({ "ok": true }));
        call.provider_metadata = Some(json!({ "google": { "thoughtSignature": "sig" } }));
        let body = continued(vec![
            Part::StepStart,
            Part::Text("Let's celebrate.".into()),
            Part::Tool(call),
        ]);
        assert_eq!(
            body["messages"][1],
            json!({
                "id": "m2",
                "role": "assistant",
                "parts": [
                    { "type": "step-start" },
                    { "type": "text", "text": "Let's celebrate." },
                    {
                        "type": "tool-throw_confetti",
                        "toolCallId": "c1",
                        "state": "output-available",
                        "input": { "q": "c1" },
                        "output": { "ok": true },
                        "callProviderMetadata": { "google": { "thoughtSignature": "sig" } },
                    },
                ],
            })
        );
        assert_eq!(
            body["messages"][0]["parts"],
            json!([{ "type": "text", "text": "Hello" }]),
            "earlier messages stay text-only"
        );
    }

    #[test]
    fn a_mixed_step_sends_both_calls_with_their_results() {
        let mut server = tool("s1", "search", ToolState::OutputAvailable);
        server.output = Some(json!({ "hits": [1] }));
        let mut client = tool("c1", "throw_confetti", ToolState::OutputAvailable);
        client.output = Some(json!("done"));
        let body = continued(vec![
            Part::StepStart,
            Part::Tool(server),
            Part::Tool(client),
        ]);
        let parts = body["messages"][1]["parts"].as_array().unwrap();
        assert_eq!(parts[1]["output"], json!({ "hits": [1] }));
        assert_eq!(parts[2]["output"], json!("done"));
    }

    #[test]
    fn an_empty_output_is_sent_as_ok_true() {
        let mut empty = tool("c1", "t", ToolState::OutputAvailable);
        empty.output = Some(json!({}));
        let missing = tool("c2", "t", ToolState::OutputAvailable);
        let body = continued(vec![Part::Tool(empty), Part::Tool(missing)]);
        let parts = body["messages"][1]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["output"], json!({ "ok": true }));
        assert_eq!(parts[1]["output"], json!({ "ok": true }));
    }

    #[test]
    fn a_failure_is_sent_as_an_output_error_with_its_text() {
        let mut failed = tool("c1", "t", ToolState::OutputError);
        failed.error = Some("Timed out".into());
        let body = continued(vec![Part::Tool(failed)]);
        let part = &body["messages"][1]["parts"][0];
        assert_eq!(part["state"], "output-error");
        assert_eq!(part["errorText"], "Timed out");
        assert!(part.get("output").is_none());
    }

    #[test]
    fn a_call_still_waiting_is_sent_as_input_available() {
        let body = continued(vec![Part::Tool(tool("c1", "t", ToolState::InputAvailable))]);
        let part = &body["messages"][1]["parts"][0];
        assert_eq!(part["state"], "input-available");
        assert!(part.get("output").is_none());
    }

    #[test]
    fn data_parts_loaded_from_history_never_go_out() {
        let history = parse_conversation(
            r#"{ "id": "c1", "messages": [
                { "id": "m1", "role": "user", "parts": [{ "type": "text", "text": "Hi" }] },
                { "id": "m2", "role": "assistant", "parts": [
                    { "type": "data-suggestions", "data": { "suggestions": ["More"] } },
                    { "type": "text", "text": "Hello" },
                    { "type": "data-tool-output-metadata", "data": {} }
                ] }
            ] }"#,
        )
        .unwrap();
        let body = request_body(&history, Some("c1"));
        assert_eq!(
            body["messages"][1]["parts"],
            json!([{ "type": "text", "text": "Hello" }])
        );
    }

    #[test]
    fn a_loaded_conversation_that_ends_on_a_resolved_tool_replays_it() {
        let history = parse_conversation(
            r#"{ "id": "c1", "messages": [
                { "id": "m1", "role": "user", "parts": [{ "type": "text", "text": "Party" }] },
                { "id": "m2", "role": "assistant", "parts": [
                    { "type": "text", "text": "Sure." },
                    { "type": "tool-call", "toolCallId": "t1", "toolName": "throw_confetti",
                      "args": { "count": 5 } },
                    { "type": "tool-result", "toolCallId": "t1", "toolName": "throw_confetti",
                      "output": { "type": "json", "value": { "ok": true } } }
                ] }
            ] }"#,
        )
        .unwrap();
        let body = request_body(&history, Some("c1"));
        assert_eq!(
            body["messages"][1]["parts"],
            json!([
                { "type": "text", "text": "Sure." },
                {
                    "type": "tool-throw_confetti",
                    "toolCallId": "t1",
                    "state": "output-available",
                    "input": { "count": 5 },
                    "output": { "ok": true },
                },
            ])
        );
    }

    #[test]
    fn a_trailing_user_message_is_text_only() {
        let body = request_body(&[ChatMessage::user("m1", "Hello")], None);
        assert_eq!(
            body["messages"][0]["parts"],
            json!([{ "type": "text", "text": "Hello" }])
        );
    }
}
