use crate::chat_tool_servers::{OWN_SERVER, ServerRow, bearer, list_owned, refreshable_after_rejection, renew, slug};
use crate::chats::{chat_node, record};
use crate::error::ApiResult;
use crate::integration_policy::Source as PolicySource;
use crate::integration_review;
use crate::mcp;
use crate::mcp_client::{McpConnection, McpError};
use crate::model_providers::{ToolCall, ToolDefinition, ToolResult};
use crate::state::AppState;
use crate::store::{CONVERSATION_GROUP_SQL, append_event, set_status};
use mycelium_proto::api::ToolKind;
use mycelium_proto::conversation::{ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub const TOOL_SEARCH: &str = "tool_search";
pub const TOOLS_INSTRUCTIONS: &str = "You can use tools. Only tool_search and the tools it has returned are loaded: call tool_search with a few keywords for what you need, then call the tool it returns by its name. Each tool call waits for the person to approve it, so call a tool only when it helps.";
const SEARCH_RESULTS: usize = 5;
const LISTED_TOOLS_PER_SERVER: usize = 40;
const MAX_TOOL_NAME: usize = 64;
const MAX_RESULT_CHARS: usize = 50_000;
pub const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";
const DECLINE: &str = "reject_once";
pub const DECLINED_TEXT: &str = "The person declined this tool call.";

#[derive(Clone)]
enum Source {
    Own,
    Remote { server: Box<ServerRow> },
    BuiltIn { server: Box<ServerRow> },
}

#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub server: String,
    original: String,
    description: String,
    schema: Value,
    source: Source,
}

impl Tool {
    fn server_row(&self) -> Option<&ServerRow> {
        match &self.source {
            Source::Own => None,
            Source::Remote { server } | Source::BuiltIn { server } => Some(server),
        }
    }
}

pub struct Toolbox {
    tools: Vec<Tool>,
    loaded: BTreeSet<String>,
    always_allowed: BTreeSet<String>,
    connections: HashMap<Uuid, McpConnection>,
    chat_node: Uuid,
    provider: Option<String>,
}

fn model_name(server: &str, tool: &str) -> String {
    let safe: String = tool.chars().map(|character| if character.is_ascii_alphanumeric() || character == '_' || character == '-' { character } else { '_' }).collect();

    format!("mcp__{}__{safe}", slug(server)).chars().take(MAX_TOOL_NAME).collect()
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_string).collect()
}

pub struct Candidate<T> {
    pub value: T,
    pub sort_key: String,
    pub server: String,
    pub tool: String,
    pub description: String,
}

pub fn best_matches<T>(query: &str, candidates: Vec<Candidate<T>>) -> Vec<T> {
    let wanted = words(query);
    let mut scored: Vec<(usize, Candidate<T>)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let name = words(&format!("{} {}", candidate.server, candidate.tool));
            let description = words(&candidate.description);
            let score: usize = wanted.iter().map(|word| if name.iter().any(|candidate| candidate.contains(word.as_str())) { 3 } else if description.iter().any(|candidate| candidate.contains(word.as_str())) { 1 } else { 0 }).sum();

            (score > 0).then_some((score, candidate))
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.sort_key.cmp(&right.1.sort_key)));
    scored.truncate(SEARCH_RESULTS);

    scored.into_iter().map(|(_, candidate)| candidate.value).collect()
}

fn event_text(update: Value) -> Value {
    json!({"update": update})
}

fn text_content(text: &str) -> Value {
    json!([{"type": "content", "content": {"type": "text", "text": text}}])
}

impl Toolbox {
    pub async fn open(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<Toolbox> {
        let mut tools: Vec<Tool> = mcp::tool_definitions()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|definition| {
                let original = definition["name"].as_str()?;

                if mcp::WORKSPACE_ONLY_TOOLS.contains(&original) {
                    return None;
                }

                Some(Tool { name: model_name(OWN_SERVER, original), server: OWN_SERVER.to_string(), original: original.to_string(), description: definition["description"].as_str()?.to_string(), schema: definition["inputSchema"].clone(), source: Source::Own })
            })
            .collect();
        let mut loaded: BTreeSet<String> = tools.iter().map(|tool| tool.name.clone()).collect();

        let group = sqlx::query_scalar::<_, Option<Uuid>>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_GROUP_SQL} from conversations where id = $1"))).bind(conversation_id).fetch_one(&state.pool).await?;
        let provider = sqlx::query_scalar::<_, String>("select p.name from conversations c join model_providers p on p.id = c.provider_id where c.id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

        for server in list_owned(state, owner).await?.into_iter().filter(|server| server.enabled && server.in_group(group)) {
            for stored in server.tool_list() {
                let source = match server.integration() {
                    Some(_) => Source::BuiltIn { server: Box::new(server.clone()) },
                    None => Source::Remote { server: Box::new(server.clone()) },
                };

                tools.push(Tool { name: model_name(&server.name, &stored.name), server: server.name.clone(), original: stored.name, description: stored.description, schema: stored.input_schema, source });
            }
        }

        let searched = sqlx::query_scalar::<_, String>(
            "select distinct jsonb_array_elements_text(payload->'update'->'rawOutput'->'loaded') from events
             where conversation_id = $1 and kind = 'acp_update' and jsonb_typeof(payload->'update'->'rawOutput'->'loaded') = 'array'",
        )
        .bind(conversation_id)
        .fetch_all(&state.pool)
        .await?;
        loaded.extend(searched.into_iter().filter(|name| tools.iter().any(|tool| &tool.name == name)));

        let always_allowed = sqlx::query_scalar::<_, String>(
            "select distinct r.payload->'toolCall'->>'title' from events r join events s on s.conversation_id = r.conversation_id and s.kind = 'permission_resolved' and s.payload->>'request_id' = r.payload->>'request_id'
             where r.conversation_id = $1 and r.kind = 'permission_request' and s.payload->>'option_id' = $2",
        )
        .bind(conversation_id)
        .bind(ALLOW_ALWAYS)
        .fetch_all(&state.pool)
        .await?;

        Ok(Toolbox { tools, loaded, always_allowed: always_allowed.into_iter().collect(), connections: HashMap::new(), chat_node: chat_node(state, owner).await?, provider })
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let search = ToolDefinition {
            name: TOOL_SEARCH.to_string(),
            description: "Find tools by keyword. Returns the names, descriptions and input schemas of the best matching tools from every enabled tool source, and loads them so you can call them by name.".to_string(),
            schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "A few keywords for what the tool should do."}}, "required": ["query"]}),
        };
        let loaded = self.tools.iter().filter(|tool| self.loaded.contains(&tool.name)).map(|tool| ToolDefinition { name: tool.name.clone(), description: tool.description.clone(), schema: tool.schema.clone() });

        std::iter::once(search).chain(loaded).collect()
    }

    pub fn instructions(&self) -> String {
        let mut servers: Vec<(&str, Vec<&str>)> = Vec::new();

        for tool in self.tools.iter().filter(|tool| !matches!(tool.source, Source::Own)) {
            match servers.iter_mut().find(|(name, _)| *name == tool.server) {
                Some((_, names)) => names.push(&tool.original),
                None => servers.push((&tool.server, vec![&tool.original])),
            }
        }

        if servers.is_empty() {
            return TOOLS_INSTRUCTIONS.to_string();
        }

        let lines: Vec<String> = servers
            .iter()
            .map(|(server, names)| {
                let listed = names.iter().take(LISTED_TOOLS_PER_SERVER).copied().collect::<Vec<_>>().join(", ");
                let hidden = names.len().saturating_sub(LISTED_TOOLS_PER_SERVER);

                match hidden {
                    0 => format!("- {server}: {listed}"),
                    _ => format!("- {server}: {listed}, and {hidden} more"),
                }
            })
            .collect();

        format!("{TOOLS_INSTRUCTIONS}\n\nConnected tool servers, loaded with tool_search when needed:\n{}", lines.join("\n"))
    }

    fn search(&mut self, query: &str) -> (String, Vec<String>) {
        let wanted = words(query);

        if wanted.is_empty() {
            return ("Nothing was searched. Give a few keywords for what the tool should do.".to_string(), Vec::new());
        }

        let candidates = self.tools.iter().map(|tool| Candidate { value: tool, sort_key: tool.name.clone(), server: tool.server.clone(), tool: tool.original.clone(), description: tool.description.clone() }).collect();
        let scored = best_matches(query, candidates);

        if scored.is_empty() {
            return ("No tools match those keywords. Try other words, for example a verb and a noun.".to_string(), Vec::new());
        }

        let names: Vec<String> = scored.iter().map(|tool| tool.name.clone()).collect();
        let entries: Vec<String> = scored.iter().map(|tool| format!("{} (server: {})\n{}\nInput schema: {}", tool.name, tool.server, tool.description, tool.schema)).collect();
        self.loaded.extend(names.iter().cloned());

        (format!("These tools are now loaded. Call them by name.\n\n{}", entries.join("\n\n")), names)
    }

    fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    fn needs_approval(&self, tool: &Tool) -> bool {
        let Some(server) = tool.server_row() else {
            return !self.always_allowed.contains(&tool.name);
        };
        let effective = server.effective(&tool.original);

        effective.ask_source == PolicySource::Locked || (effective.ask && !self.always_allowed.contains(&tool.name))
    }

    #[allow(clippy::too_many_arguments)]
    async fn record(&self, state: &AppState, owner: Uuid, conversation_id: Uuid, tool: &Tool, decision: &str, is_error: bool, review: Option<&str>) {
        if let Some(server) = tool.server_row() {
            record_call(state, owner, server, &tool.original, Some(conversation_id), "chat", self.provider.as_deref(), decision, is_error, review).await;
        }
    }

    async fn call(&mut self, state: &AppState, owner: Uuid, conversation_id: Uuid, tool_call: &str, tool: &Tool, input: &Value) -> ApiResult<(String, bool)> {
        match &tool.source {
            Source::Own if tool.original == "walkthrough" => Ok(("A walkthrough is about the changes in a workspace, and this chat has no workspace. Open a conversation on a node to write one.".to_string(), true)),
            Source::Own => mcp::call_in_process(state, self.chat_node, conversation_id, Some(tool_call), &tool.original, input).await,
            Source::BuiltIn { server } => {
                let Some(integration) = server.integration() else {
                    return Ok((format!("{} is not a built-in integration Mycelium knows. Remove it and add it again.", tool.server), true));
                };
                let output = integration.call(state, owner, server, &tool.original, input).await;

                Ok((output.text, output.is_error))
            }
            Source::Remote { server } => {
                let token = match bearer(state, server).await {
                    Ok(token) => token,
                    Err(error) => return Ok((error.message().to_string(), true)),
                };
                let connection = self.connections.entry(server.id).or_insert_with(|| McpConnection::new(&state.http, &server.url, token.as_deref(), server.key_header()));
                connection.set_token(token.as_deref());

                let mut called = connection.call_tool(&tool.original, input).await;

                if matches!(called, Err(McpError::Rejected)) && refreshable_after_rejection(server) {
                    match renew(state, server).await {
                        Ok(token) => {
                            connection.set_token(token.as_deref());
                            called = connection.call_tool(&tool.original, input).await;
                        }
                        Err(error) => return Ok((error.message().to_string(), true)),
                    }
                }

                Ok(match called {
                    Ok(output) => (output.text, output.is_error),
                    Err(error) => (error.describe(&tool.server), true),
                })
            }
        }
    }
}

pub async fn run_calls(state: &AppState, owner: Uuid, id: Uuid, toolbox: &mut Toolbox, calls: &[ToolCall]) -> ApiResult<Vec<ToolResult>> {
    for call in calls {
        let update = json!({"sessionUpdate": "tool_call", "toolCallId": call.id, "title": call.name, "kind": "other", "status": "pending", "rawInput": call.input});

        record(state, owner, id, EventKind::AcpUpdate, &event_text(update)).await?;
    }

    let mut results = Vec::new();

    for call in calls {
        let (content, is_error, loaded) = outcome_of(state, owner, id, toolbox, call).await?;
        let content = match content.chars().count() > MAX_RESULT_CHARS {
            true => format!("{}\n[Cut: the result was longer than {MAX_RESULT_CHARS} characters.]", content.chars().take(MAX_RESULT_CHARS).collect::<String>()),
            false => content,
        };
        let mut update = json!({"sessionUpdate": "tool_call_update", "toolCallId": call.id, "status": if is_error { "failed" } else { "completed" }, "content": text_content(&content)});

        if let Some(loaded) = loaded {
            update["rawOutput"] = json!({"loaded": loaded});
        }

        record(state, owner, id, EventKind::AcpUpdate, &event_text(update)).await?;
        results.push(ToolResult { call_id: call.id.clone(), content, is_error });
    }

    Ok(results)
}

async fn outcome_of(state: &AppState, owner: Uuid, id: Uuid, toolbox: &mut Toolbox, call: &ToolCall) -> ApiResult<(String, bool, Option<Vec<String>>)> {
    if call.name == TOOL_SEARCH {
        let (text, names) = toolbox.search(call.input["query"].as_str().unwrap_or_default());

        return Ok((text, false, Some(names)));
    }

    let Some(tool) = toolbox.find(&call.name).cloned() else {
        return Ok((format!("There is no tool called {}. Use {TOOL_SEARCH} to find the tools you can call.", call.name), true, None));
    };

    let mut decision = "auto";

    if toolbox.needs_approval(&tool) {
        let answer = ask(state, owner, id, call, &tool).await?;

        if answer == ALLOW_ALWAYS {
            toolbox.always_allowed.insert(tool.name.clone());
        }

        if answer != ALLOW_ONCE && answer != ALLOW_ALWAYS {
            toolbox.record(state, owner, id, &tool, "declined", false, None).await;

            return Ok((DECLINED_TEXT.to_string(), true, None));
        }

        decision = "approved";
    }

    let (mut text, is_error) = match toolbox.call(state, owner, id, &call.id, &tool, &call.input).await {
        Ok(result) => result,
        Err(error) => (format!("The {} tool failed: {} The person did not decline this call. Do not report the action as completed or declined; check whether it was recorded before retrying.", tool.original, error.message()), true),
    };

    let mut review = None;

    if !is_error && tool.server_row().is_some_and(|server| server.effective(&tool.original).review) {
        let reviewed = integration_review::hold(state, owner, id, &call.id, &tool.server, &tool.original, text).await?;
        text = reviewed.text;
        review = Some(reviewed.review);
    }

    toolbox.record(state, owner, id, &tool, decision, is_error, review).await;

    Ok((text, is_error, None))
}

#[allow(clippy::too_many_arguments)]
pub async fn record_call(state: &AppState, owner: Uuid, server: &ServerRow, tool: &str, conversation_id: Option<Uuid>, surface: &str, provider: Option<&str>, decision: &str, is_error: bool, review: Option<&str>) {
    let kind = match server.tool_kind(tool) {
        ToolKind::Read => "read",
        ToolKind::Write => "write",
    };
    let recorded = sqlx::query("insert into integration_calls (owner, server_id, server_name, tool, kind, conversation_id, surface, provider, decision, is_error, review) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)")
        .bind(owner)
        .bind(server.id)
        .bind(&server.name)
        .bind(tool)
        .bind(kind)
        .bind(conversation_id)
        .bind(surface)
        .bind(provider)
        .bind(decision)
        .bind(is_error)
        .bind(review)
        .execute(&state.pool)
        .await;

    if decision != "declined" {
        let error = is_error.then_some("The last call failed.");
        let touched = sqlx::query("update chat_tool_servers set last_used_at = now(), last_error = $2 where id = $1").bind(server.id).bind(error).execute(&state.pool).await;

        if let Err(cause) = touched {
            tracing::warn!("could not note the use of {}: {cause}", server.name);
        }
    }

    if let Err(cause) = recorded {
        tracing::warn!("could not record a call to {}: {cause}", server.name);
    }
}

async fn ask(state: &AppState, owner: Uuid, id: Uuid, call: &ToolCall, tool: &Tool) -> ApiResult<String> {
    request_approval(state, owner, id, &call.id, &call.name, &call.input, &tool.server, true).await
}

#[allow(clippy::too_many_arguments)]
pub async fn request_approval(state: &AppState, owner: Uuid, id: Uuid, request_id: &str, title: &str, input: &Value, server: &str, offer_always: bool) -> ApiResult<String> {
    let answer = state.hub.await_chat_approval(id, request_id);
    let mut options = vec![json!({"optionId": ALLOW_ONCE, "name": "Allow once", "kind": ALLOW_ONCE})];

    if offer_always {
        options.push(json!({"optionId": ALLOW_ALWAYS, "name": "Always allow this tool", "kind": ALLOW_ALWAYS}));
    }

    options.push(json!({"optionId": DECLINE, "name": "Deny", "kind": DECLINE}));

    let request = json!({
        "request_id": request_id,
        "toolCall": {"toolCallId": request_id, "title": title, "rawInput": input, "server": server},
        "options": options,
    });
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, id, EventKind::PermissionRequest, &request, None).await?;
    let waiting = set_status(&mut tx, id, ConversationStatus::Input).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(waiting.to_proto()?));

    let option_id = answer.await.unwrap_or_else(|_| DECLINE.to_string());
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, id, EventKind::PermissionResolved, &json!({"request_id": request_id, "option_id": option_id}), None).await?;
    let working = set_status(&mut tx, id, ConversationStatus::Working).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(working.to_proto()?));

    Ok(option_id)
}

pub async fn close_open_calls(state: &AppState, owner: Uuid, id: Uuid, reason: &str) -> ApiResult<()> {
    let requests = sqlx::query_scalar::<_, String>(
        "select r.payload->>'request_id' from events r where r.conversation_id = $1 and r.kind = 'permission_request'
           and not exists (select 1 from events s where s.conversation_id = $1 and s.kind = 'permission_resolved' and s.payload->>'request_id' = r.payload->>'request_id')",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    for request_id in requests {
        record(state, owner, id, EventKind::PermissionResolved, &json!({"request_id": request_id, "option_id": "cancelled"})).await?;
    }

    let reviews = sqlx::query_scalar::<_, String>(
        "select r.payload->>'request_id' from events r where r.conversation_id = $1 and r.kind = 'review_request'
           and not exists (select 1 from events s where s.conversation_id = $1 and s.kind = 'review_resolved' and s.payload->>'request_id' = r.payload->>'request_id')",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    for request_id in reviews {
        record(state, owner, id, EventKind::ReviewResolved, &json!({"request_id": request_id, "decision": "withhold", "removed_count": 0})).await?;
    }

    let calls = sqlx::query_scalar::<_, String>(
        "select c.payload->'update'->>'toolCallId' from events c where c.conversation_id = $1 and c.kind = 'acp_update' and c.payload->'update'->>'sessionUpdate' = 'tool_call'
           and not exists (select 1 from events u where u.conversation_id = $1 and u.kind = 'acp_update' and u.payload->'update'->>'sessionUpdate' = 'tool_call_update' and u.payload->'update'->>'toolCallId' = c.payload->'update'->>'toolCallId')
         order by c.seq",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    for call_id in calls {
        let update = json!({"sessionUpdate": "tool_call_update", "toolCallId": call_id, "status": "failed", "content": text_content(reason)});

        record(state, owner, id, EventKind::AcpUpdate, &event_text(update)).await?;
    }

    Ok(())
}
