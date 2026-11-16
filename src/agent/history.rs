use super::*;

/// One page of `GET /agent-studio/1/agents` (`PaginatedAgentsResponse`).
/// Returns the agents and the page count still to fetch after this page.
pub fn parse_agents_page(json: &str) -> Result<(Vec<AgentInfo>, usize), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad agents page: {e}"))?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("agents page has no data array")?;
    let pagination = v.get("pagination").ok_or("agents page has no pagination")?;
    let total_pages = pagination
        .get("totalPages")
        .and_then(|t| t.as_u64())
        .unwrap_or(1) as usize;
    let page = pagination.get("page").and_then(|p| p.as_u64()).unwrap_or(1) as usize;
    let mut agents = Vec::new();
    for a in data {
        let status = match a.get("status").and_then(|s| s.as_str()) {
            Some("published") => AgentStatus::Published,
            Some("draft") => AgentStatus::Draft,
            other => return Err(format!("unknown agent status {other:?}")),
        };
        agents.push(AgentInfo {
            agent_id: a
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or("agent has no id")?
                .into(),
            name: a
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or("agent has no name")?
                .into(),
            description: a
                .get("description")
                .and_then(|d| d.as_str())
                .map(Into::into),
            status,
            model: a.get("model").and_then(|m| m.as_str()).map(Into::into),
        });
    }
    Ok((agents, total_pages.saturating_sub(page)))
}

/// One page of `GET /agent-studio/1/agents/{agentId}/conversations`
/// (`PaginatedConversationsResponse`). Returns the conversations and the page count still
/// to fetch after this page.
pub fn parse_conversations_page(json: &str) -> Result<(Vec<SessionInfo>, usize), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad conversations page: {e}"))?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("conversations page has no data array")?;
    let pagination = v
        .get("pagination")
        .ok_or("conversations page has no pagination")?;
    let total_pages = pagination
        .get("totalPages")
        .and_then(|t| t.as_u64())
        .unwrap_or(1) as usize;
    let page = pagination.get("page").and_then(|p| p.as_u64()).unwrap_or(1) as usize;
    let mut sessions = Vec::new();
    for c in data {
        sessions.push(SessionInfo {
            id: c
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or("conversation has no id")?
                .into(),
            title: c.get("title").and_then(|t| t.as_str()).map(Into::into),
        });
    }
    Ok((sessions, total_pages.saturating_sub(page)))
}

/// One agent (`GET /agent-studio/1/agents/{agentId}`). Tools with a `type` other than a
/// client tool read as names the server runs, so an unknown kind is never an error.
pub fn parse_agent_config(json: &str) -> Result<AgentConfig, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("bad agent: {e}"))?;
    let text = |field: &str| v.get(field).and_then(|t| t.as_str()).map(String::from);
    let status = match text("status").as_deref() {
        Some("published") => AgentStatus::Published,
        Some("draft") => AgentStatus::Draft,
        other => return Err(format!("unknown agent status {other:?}")),
    };
    let mut client_tools = Vec::new();
    let mut other_tools = Vec::new();
    let tools = v
        .get("config")
        .and_then(|c| c.get("tools"))
        .or_else(|| v.get("tools"))
        .and_then(|t| t.as_array());
    for tool in tools.into_iter().flatten() {
        let field = |name: &str| tool.get(name).and_then(|t| t.as_str()).map(String::from);
        match tool.get("type").and_then(|t| t.as_str()) {
            Some("client_side") => {
                let Some(name) = field("name").filter(|n| !n.is_empty()) else {
                    continue;
                };
                client_tools.push(ClientToolDecl {
                    name,
                    description: field("description"),
                    input_schema: tool.get("inputSchema").filter(|s| !s.is_null()).cloned(),
                    is_terminal: tool
                        .get("isTerminal")
                        .and_then(|t| t.as_bool())
                        .unwrap_or(false),
                });
            }
            other => {
                if let Some(name) = field("name").or_else(|| other.map(String::from)) {
                    other_tools.push(name);
                }
            }
        }
    }
    Ok(AgentConfig {
        name: text("name").ok_or("agent has no name")?,
        description: text("description"),
        status,
        model: text("model"),
        client_tools,
        other_tools,
    })
}

/// One conversation's messages (`ConversationFullResponse`): text parts join into the
/// message's text, tool calls become the UI's tool group, and a tool result fills the call
/// it answers. Parts the UI doesn't show (reasoning, lifecycle steps) are skipped.
pub fn parse_conversation(json: &str) -> Result<Vec<ChatMessage>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad conversation: {e}"))?;
    let messages = v
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or("conversation has no messages")?;
    let mut out: Vec<ChatMessage> = Vec::new();
    for m in messages {
        let role = match m.get("role").and_then(|r| r.as_str()) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            other => return Err(format!("unknown message role {other:?}")),
        };
        let id = m
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string();
        let mut text = String::new();
        let mut tools: Vec<ToolCall> = Vec::new();
        let mut replay: Vec<Part> = Vec::new();
        // toolCallId → indices into `tools` and `replay`, so a later tool result can fill its call.
        let mut calls: Vec<(String, usize, usize)> = Vec::new();
        if let Some(parts) = m.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                match part.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                text.push_str("\n\n");
                            }
                            text.push_str(t);
                            replay.push(Part::Text(t.to_string()));
                        }
                    }
                    Some("step-start") => replay.push(Part::StepStart),
                    Some("tool-call") => {
                        let Some(name) = part
                            .get("toolName")
                            .and_then(|t| t.as_str())
                            .filter(|n| !n.is_empty())
                        else {
                            continue;
                        };
                        let call_id = part
                            .get("toolCallId")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let input = part
                            .get("args")
                            .and_then(|a| serde_json::to_string_pretty(a).ok());
                        replay.push(Part::Tool(ToolPart {
                            call_id: call_id.clone(),
                            name: name.into(),
                            input: part.get("args").cloned().unwrap_or_default(),
                            state: ToolState::InputAvailable,
                            output: None,
                            error: None,
                            provider_metadata: None,
                        }));
                        calls.push((call_id.clone(), tools.len(), replay.len() - 1));
                        tools.push(ToolCall {
                            call_id,
                            provider_metadata: None,
                            name: name.into(),
                            request: input.clone(),
                            input,
                            open: false,
                            error: None,
                        });
                    }
                    Some("tool-result") => {
                        let call_id = part
                            .get("toolCallId")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default();
                        let Some(&(_, i, part_index)) =
                            calls.iter().rev().find(|(id, _, _)| id == call_id)
                        else {
                            continue;
                        };
                        let Part::Tool(tool_part) = &mut replay[part_index] else {
                            continue;
                        };
                        let output = part.get("output");
                        let Some(value) = output.and_then(|o| o.get("value")) else {
                            continue;
                        };
                        match output.and_then(|o| o.get("type")?.as_str()) {
                            Some("error-text" | "error-json") => {
                                let raw = match value {
                                    serde_json::Value::String(text) => text.clone(),
                                    other => other.to_string(),
                                };
                                let error = tool_error_message(&raw);
                                tool_part.state = ToolState::OutputError;
                                tool_part.error = Some(error.clone());
                                tools[i].error = Some(error);
                            }
                            _ => {
                                tool_part.state = ToolState::OutputAvailable;
                                tool_part.output = Some(value.clone());
                                if let Ok(pretty) = serde_json::to_string_pretty(value) {
                                    tools[i].input = Some(pretty);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if text.is_empty() && tools.is_empty() {
            continue;
        }
        let mut message = match role {
            Role::User => ChatMessage::user(id.clone(), text),
            Role::Assistant => {
                let mut assistant = ChatMessage::assistant(id.clone());
                assistant.text = text;
                assistant
            }
        };
        message.streaming = false;
        message.server_id = Some(id);
        message.tools = tools;
        if role == Role::Assistant {
            message.parts = replay;
        }
        out.push(message);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &str = r#"{
        "id": "a1", "name": "Shop", "description": "Sells things", "status": "published",
        "model": "gpt-5", "instructions": "Be kind",
        "config": { "tools": [
            { "type": "client_side", "name": "export_report", "description": "Exports",
              "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } } },
              "isTerminal": true },
            { "type": "algolia_search_index", "name": "products" },
            { "type": "mcp", "name": "docs" },
            { "type": "future_kind" }
        ] }
    }"#;

    #[test]
    fn an_agent_config_lists_client_tools_and_the_others_by_name() {
        let config = parse_agent_config(AGENT).unwrap();
        assert_eq!(config.name, "Shop");
        assert_eq!(config.status, AgentStatus::Published);
        assert_eq!(config.model.as_deref(), Some("gpt-5"));
        assert_eq!(config.client_tools.len(), 1);
        let tool = &config.client_tools[0];
        assert_eq!(tool.name, "export_report");
        assert_eq!(tool.description.as_deref(), Some("Exports"));
        assert!(tool.is_terminal);
        assert_eq!(tool.input_schema.as_ref().unwrap()["type"], "object");
        assert_eq!(config.other_tools, ["products", "docs", "future_kind"]);
    }

    #[test]
    fn an_agent_config_with_missing_optional_fields_still_parses() {
        let config = parse_agent_config(r#"{ "name": "Bare", "status": "draft" }"#).unwrap();
        assert_eq!(config.status, AgentStatus::Draft);
        assert_eq!(config.description, None);
        assert_eq!(config.model, None);
        assert!(config.client_tools.is_empty() && config.other_tools.is_empty());
    }

    #[test]
    fn an_agent_config_without_a_name_is_an_error() {
        assert!(parse_agent_config(r#"{ "status": "draft" }"#).is_err());
    }

    #[test]
    fn a_client_tool_without_a_schema_or_terminal_flag_takes_the_defaults() {
        let config = parse_agent_config(
            r#"{ "name": "A", "status": "published",
                 "config": { "tools": [ { "type": "client_side", "name": "ping" } ] } }"#,
        )
        .unwrap();
        assert_eq!(config.client_tools[0].input_schema, None);
        assert!(!config.client_tools[0].is_terminal);
    }

    #[test]
    fn a_conversations_page_parses_ids_titles_and_pages_left() {
        let json = r#"{
            "data": [
                { "id": "c1", "agentId": "a1", "createdAt": "t", "updatedAt": "t",
                  "title": "Quarterly numbers", "messageCount": 6 },
                { "id": "c2", "agentId": "a1", "createdAt": "t", "updatedAt": "t", "title": null }
            ],
            "pagination": { "page": 2, "limit": 100, "totalCount": 130, "totalPages": 2 }
        }"#;
        let (sessions, left) = parse_conversations_page(json).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "c1");
        assert_eq!(sessions[0].title.as_deref(), Some("Quarterly numbers"));
        assert_eq!(sessions[1].title, None, "a null title stays optional");
        assert_eq!(left, 0, "the last page leaves nothing to fetch");
    }

    #[test]
    fn a_conversation_parses_into_ui_messages_with_tools() {
        let json = r#"{
            "id": "c1", "agentId": "a1", "createdAt": "t", "updatedAt": "t",
            "messages": [
                { "id": "m1", "conversationId": "c1", "role": "user",
                  "createdAt": "t", "updatedAt": "t",
                  "parts": [{ "type": "text", "text": "What's wrong with the index?" }] },
                { "id": "m2", "conversationId": "c1", "role": "assistant",
                  "createdAt": "t", "updatedAt": "t",
                  "parts": [
                      { "type": "start" },
                      { "type": "tool-call", "toolCallId": "t1", "toolName": "search_tool",
                        "args": { "query": "index health" } },
                      { "type": "tool-result", "toolCallId": "t1", "toolName": "search_tool",
                        "output": { "type": "json", "value": { "hits": 3 } } },
                      { "type": "reasoning", "text": "thinking" },
                      { "type": "text", "text": "The index looks healthy." },
                      { "type": "text", "text": "Nothing to reindex." }
                  ] }
            ]
        }"#;
        let messages = parse_conversation(json).unwrap();
        assert_eq!(messages.len(), 2, "lifecycle and reasoning parts drop out");
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].text, "What's wrong with the index?");
        assert_eq!(messages[1].role, Role::Assistant);
        assert!(!messages[1].streaming, "history messages arrive settled");
        assert_eq!(
            messages[1].text, "The index looks healthy.\n\nNothing to reindex.",
            "text parts read as paragraphs, as they do live"
        );
        assert_eq!(messages[1].tools.len(), 1);
        assert_eq!(messages[1].tools[0].name, "search_tool");
        assert_eq!(
            messages[1].tools[0].input.as_deref(),
            Some("{\n  \"hits\": 3\n}"),
            "the tool result fills the call it answers"
        );
        assert_eq!(
            messages[1].tools[0].search_request().map(|q| q.query),
            Some("index health".into()),
            "the call's own arguments outlive its result"
        );
    }

    #[test]
    fn an_error_result_in_history_fails_its_call_and_keeps_the_arguments() {
        let json = r#"{ "id": "c1", "messages": [
            { "id": "m1", "role": "assistant", "parts": [
                { "type": "tool-call", "toolCallId": "t1", "toolName": "search_tool",
                  "args": { "query": "x" } },
                { "type": "tool-result", "toolCallId": "t1", "toolName": "search_tool",
                  "output": { "type": "error-text",
                              "value": "{\"error\": \"Error: Index does not exist\"}" } },
                { "type": "tool-call", "toolCallId": "t2", "toolName": "facets",
                  "args": {} },
                { "type": "tool-result", "toolCallId": "t2", "toolName": "facets",
                  "output": { "type": "error-json", "value": { "error": "Timed out" } } }
            ] }
        ] }"#;
        let messages = parse_conversation(json).unwrap();
        let tools = &messages[0].tools;
        assert_eq!(tools[0].error.as_deref(), Some("Index does not exist"));
        assert_eq!(tools[0].input.as_deref(), Some("{\n  \"query\": \"x\"\n}"));
        assert_eq!(tools[1].error.as_deref(), Some("Timed out"));
    }

    #[test]
    fn a_conversation_builds_its_parts_in_order_with_each_calls_outcome() {
        let json = r#"{ "id": "c1", "messages": [
            { "id": "m1", "role": "assistant", "parts": [
                { "type": "step-start" },
                { "type": "text", "text": "Checking." },
                { "type": "tool-call", "toolCallId": "t1", "toolName": "search_tool",
                  "args": { "q": "x" } },
                { "type": "tool-result", "toolCallId": "t1", "toolName": "search_tool",
                  "output": { "type": "json", "value": { "hits": 3 } } },
                { "type": "tool-call", "toolCallId": "t2", "toolName": "facets", "args": {} },
                { "type": "tool-result", "toolCallId": "t2", "toolName": "facets",
                  "output": { "type": "error-text", "value": "Error: no such facet" } },
                { "type": "data-suggestions", "data": { "suggestions": ["More"] } },
                { "type": "text", "text": "Done." }
            ] }
        ] }"#;
        let parts = &parse_conversation(json).unwrap()[0].parts;
        assert_eq!(parts.len(), 5, "data parts are not kept");
        assert_eq!(parts[0], Part::StepStart);
        assert_eq!(parts[1], Part::Text("Checking.".into()));
        let Part::Tool(found) = &parts[2] else {
            panic!("{parts:?}");
        };
        assert_eq!(found.state, ToolState::OutputAvailable);
        assert_eq!(found.input, serde_json::json!({ "q": "x" }));
        assert_eq!(found.output, Some(serde_json::json!({ "hits": 3 })));
        let Part::Tool(failed) = &parts[3] else {
            panic!("{parts:?}");
        };
        assert_eq!(failed.state, ToolState::OutputError);
        assert_eq!(failed.error.as_deref(), Some("no such facet"));
        assert_eq!(parts[4], Part::Text("Done.".into()));
    }

    #[test]
    fn an_unknown_message_role_is_an_error() {
        let json = r#"{ "id": "c1", "messages": [
            { "id": "m1", "role": "system", "parts": [] }
        ] }"#;
        assert!(parse_conversation(json).is_err());
    }

    #[test]
    fn an_empty_assistant_message_is_skipped() {
        let json = r#"{ "id": "c1", "messages": [
            { "id": "m1", "role": "assistant", "parts": [{ "type": "start" }] }
        ] }"#;
        assert_eq!(parse_conversation(json).unwrap().len(), 0);
    }

    #[test]
    fn an_agents_page_parses_into_agent_info() {
        let json = r#"{
            "data": [
                {
                    "id": "alg_agt_1", "name": "Shopping assistant",
                    "description": "Answers questions about the catalog",
                    "status": "published", "model": "gpt-5", "instructions": "", "config": {},
                    "createdAt": "2026-01-01", "updatedAt": null, "lastUsedAt": null, "providerId": "p1", "tools": []
                },
                {
                    "id": "alg_agt_2", "name": "Draft helper",
                    "description": null,
                    "status": "draft", "model": null, "instructions": "", "config": {},
                    "createdAt": "2026-01-02", "updatedAt": null, "lastUsedAt": null, "providerId": "p1", "tools": []
                }
            ],
            "pagination": { "page": 1, "limit": 10, "totalCount": 15, "totalPages": 2 }
        }"#;
        let (agents, remaining_pages) = parse_agents_page(json).unwrap();
        assert_eq!(remaining_pages, 1);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "Shopping assistant");
        assert_eq!(agents[0].status, AgentStatus::Published);
        assert_eq!(
            agents[0].description.as_deref(),
            Some("Answers questions about the catalog")
        );
        assert_eq!(agents[0].model.as_deref(), Some("gpt-5"));
        assert_eq!(agents[1].status, AgentStatus::Draft);
        assert_eq!(agents[1].description, None);
    }

    #[test]
    fn a_single_page_has_nothing_left_to_fetch() {
        let json = r#"{ "data": [], "pagination": { "page": 1, "limit": 10, "totalCount": 0, "totalPages": 1 } }"#;
        let (agents, remaining) = parse_agents_page(json).unwrap();
        assert!(agents.is_empty());
        assert_eq!(remaining, 0);
    }

    #[test]
    fn a_malformed_agents_page_is_an_error_not_a_panic() {
        assert!(parse_agents_page("not json").is_err());
        assert!(parse_agents_page("{ \"unexpected\": true }").is_err());
    }
}
