use crate::chat_tool_servers::{OWN_SERVER, list_owned, slug};
use crate::chats::{chat_node, record};
use crate::error::ApiResult;
use crate::mcp;
use crate::mcp_client::McpConnection;
use crate::model_providers::{ToolCall, ToolDefinition, ToolResult};
use crate::state::AppState;
use crate::store::{append_event, set_status};
use mycelium_proto::conversation::{ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub const TOOL_SEARCH: &str = "tool_search";
pub const TOOLS_INSTRUCTIONS: &str = "You can use tools. Only tool_search and the tools it has returned are loaded: call tool_search with a few keywords for what you need, then call the tool it returns by its name. Each tool call waits for the person to approve it, so call a tool only when it helps.";
const SEARCH_RESULTS: usize = 5;
const MAX_TOOL_NAME: usize = 64;
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";
const DECLINE: &str = "reject_once";
const DECLINED_TEXT: &str = "The person declined this tool call.";

#[derive(Clone)]
enum Source {
    Own,
    Remote { url: String, token: Option<String>, always_allow: bool, server_id: Uuid },
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

pub struct Toolbox {
    tools: Vec<Tool>,
    loaded: BTreeSet<String>,
    always_allowed: BTreeSet<String>,
    connections: HashMap<Uuid, McpConnection>,
    chat_node: Uuid,
}

fn model_name(server: &str, tool: &str) -> String {
    let safe: String = tool.chars().map(|character| if character.is_ascii_alphanumeric() || character == '_' || character == '-' { character } else { '_' }).collect();

    format!("mcp__{}__{safe}", slug(server)).chars().take(MAX_TOOL_NAME).collect()
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_string).collect()
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

                Some(Tool { name: model_name(OWN_SERVER, original), server: OWN_SERVER.to_string(), original: original.to_string(), description: definition["description"].as_str()?.to_string(), schema: definition["inputSchema"].clone(), source: Source::Own })
            })
            .collect();
        let mut loaded: BTreeSet<String> = tools.iter().map(|tool| tool.name.clone()).collect();

        for server in list_owned(state, owner).await?.into_iter().filter(|server| server.enabled) {
            for stored in &server.tools.0 {
                tools.push(Tool {
                    name: model_name(&server.name, &stored.name),
                    server: server.name.clone(),
                    original: stored.name.clone(),
                    description: stored.description.clone(),
                    schema: stored.input_schema.clone(),
                    source: Source::Remote { url: server.url.clone(), token: server.bearer_token.clone(), always_allow: server.always_allow, server_id: server.id },
                });
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

        Ok(Toolbox { tools, loaded, always_allowed: always_allowed.into_iter().collect(), connections: HashMap::new(), chat_node: chat_node(state, owner).await? })
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

    fn search(&mut self, query: &str) -> (String, Vec<String>) {
        let wanted = words(query);

        if wanted.is_empty() {
            return ("Nothing was searched. Give a few keywords for what the tool should do.".to_string(), Vec::new());
        }

        let mut scored: Vec<(usize, &Tool)> = self
            .tools
            .iter()
            .filter_map(|tool| {
                let name = words(&format!("{} {}", tool.server, tool.original));
                let description = words(&tool.description);
                let score: usize = wanted.iter().map(|word| if name.iter().any(|candidate| candidate.contains(word.as_str())) { 3 } else if description.iter().any(|candidate| candidate.contains(word.as_str())) { 1 } else { 0 }).sum();

                (score > 0).then_some((score, tool))
            })
            .collect();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.name.cmp(&right.1.name)));
        scored.truncate(SEARCH_RESULTS);

        if scored.is_empty() {
            return ("No tools match those keywords. Try other words, for example a verb and a noun.".to_string(), Vec::new());
        }

        let names: Vec<String> = scored.iter().map(|(_, tool)| tool.name.clone()).collect();
        let entries: Vec<String> = scored.iter().map(|(_, tool)| format!("{} (server: {})\n{}\nInput schema: {}", tool.name, tool.server, tool.description, tool.schema)).collect();
        self.loaded.extend(names.iter().cloned());

        (format!("These tools are now loaded. Call them by name.\n\n{}", entries.join("\n\n")), names)
    }

    fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    fn needs_approval(&self, tool: &Tool) -> bool {
        let server_trusted = matches!(tool.source, Source::Remote { always_allow: true, .. });

        !server_trusted && !self.always_allowed.contains(&tool.name)
    }

    async fn call(&mut self, state: &AppState, conversation_id: Uuid, tool: &Tool, input: &Value) -> ApiResult<(String, bool)> {
        match &tool.source {
            Source::Own if tool.original == "walkthrough" => Ok(("A walkthrough is about the changes in a workspace, and this chat has no workspace. Open a conversation on a node to write one.".to_string(), true)),
            Source::Own => mcp::call_in_process(state, self.chat_node, conversation_id, &tool.original, input).await,
            Source::Remote { url, token, server_id, .. } => {
                let connection = self.connections.entry(*server_id).or_insert_with(|| McpConnection::new(&state.http, url, token.as_deref()));

                Ok(match connection.call_tool(&tool.original, input).await {
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

    if toolbox.needs_approval(&tool) {
        let answer = ask(state, owner, id, call, &tool).await?;

        if answer == ALLOW_ALWAYS {
            toolbox.always_allowed.insert(tool.name.clone());
        }

        if answer != ALLOW_ONCE && answer != ALLOW_ALWAYS {
            return Ok((DECLINED_TEXT.to_string(), true, None));
        }
    }

    let (text, is_error) = toolbox.call(state, id, &tool, &call.input).await?;

    Ok((text, is_error, None))
}

async fn ask(state: &AppState, owner: Uuid, id: Uuid, call: &ToolCall, tool: &Tool) -> ApiResult<String> {
    let answer = state.hub.await_chat_approval(id, &call.id);
    let request = json!({
        "request_id": call.id,
        "toolCall": {"toolCallId": call.id, "title": call.name, "rawInput": call.input, "server": tool.server},
        "options": [
            {"optionId": ALLOW_ONCE, "name": "Allow once", "kind": ALLOW_ONCE},
            {"optionId": ALLOW_ALWAYS, "name": "Always allow this tool", "kind": ALLOW_ALWAYS},
            {"optionId": DECLINE, "name": "Deny", "kind": DECLINE},
        ],
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
    let event = append_event(&mut tx, id, EventKind::PermissionResolved, &json!({"request_id": call.id, "option_id": option_id}), None).await?;
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
