use crate::auth::AuthNode;
use crate::error::ApiResult;
use crate::memories::search;
use crate::state::AppState;
use crate::tasks;
use crate::walkthroughs;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::TaskStatus;
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const DEFAULT_RECALL_LIMIT: i64 = 5;
const MAX_RECALL_LIMIT: i64 = 20;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn reply(id: &Value, result: Value) -> Response {
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

fn failure(id: &Value, code: i64, message: &str) -> Response {
    Json(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})).into_response()
}

fn tool_text(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "recall",
            "description": "Search the shared memory for what earlier conversations learned. Use it before starting work that may have been done or decided before.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to search for."},
                    "limit": {"type": "number", "description": "How many memories to return. Defaults to 5."},
                },
                "required": ["query"],
            },
        },
        {
            "name": "remember",
            "description": "Store a short, confirmed finding in the shared memory so later conversations on any node can recall it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "What to remember."},
                    "scope": {"type": "string", "enum": ["global", "group"], "description": "global (default) is recalled in every conversation. group is recalled only in conversations whose project is in the same group."},
                },
                "required": ["text"],
            },
        },
        {
            "name": "propose_task",
            "description": "Propose a task for the person to approve. It waits in the Proposed column of their Tasks board until they approve or decline it. Use it for follow-up work you found but are not doing now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "A short title for the task."},
                    "body": {"type": "string", "description": "What the task involves. It becomes the prompt of the conversation that works on it."},
                    "project": {"type": "string", "description": "The name of the project the task belongs to. Defaults to this conversation's project."},
                },
                "required": ["title"],
            },
        },
        {
            "name": "ask_question",
            "description": "Ask the person a question that you need answered before you can go on. It appears in the Questions column of their Tasks board. End your turn after asking: the answer arrives in this conversation as a new message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "The question."},
                    "options": {"type": "array", "items": {"type": "string"}, "description": "Suggested answers the person can pick."},
                },
                "required": ["question"],
            },
        },
        {
            "name": "walkthrough",
            "description": "Write a diff walkthrough for this conversation when the person asks for one: the change read in order, as numbered steps, with the key step marked. The person reads it in a Walkthrough tab beside the conversation. Calling it again replaces the previous walkthrough.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "description": "The steps in reading order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": {"type": "string", "description": "A short title for the step."},
                                "key": {"type": "boolean", "description": "True for the step that carries the change. Mark one."},
                                "explanation": {"type": "string", "description": "What this step does and why."},
                                "files": {
                                    "type": "array",
                                    "description": "The files this step changes.",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "path": {"type": "string", "description": "The file's path."},
                                            "diff": {"type": "string", "description": "The unified diff for this file in this step, one line per diff line with + and - prefixes."},
                                            "hunks": {"type": "array", "items": {"type": "string"}, "description": "The diff as hunks, used when diff is not given."},
                                        },
                                        "required": ["path"],
                                    },
                                },
                            },
                            "required": ["title"],
                        },
                    },
                },
                "required": ["steps"],
            },
        },
    ])
}

async fn claimed_conversation(state: &AppState, headers: &HeaderMap, node_id: Uuid) -> ApiResult<Option<(Uuid, String)>> {
    let claimed = headers.get("x-mycelium-conversation").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<Uuid>().ok());

    match claimed {
        Some(conversation_id) => Ok(sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = $1 and node_id = $2").bind(conversation_id).bind(node_id).fetch_optional(&state.pool).await?),
        None => Ok(None),
    }
}

async fn group_scope_of(state: &AppState, headers: &HeaderMap, node_id: Uuid) -> ApiResult<Option<String>> {
    let Some((conversation_id, _)) = claimed_conversation(state, headers, node_id).await? else {
        return Ok(None);
    };
    let group = sqlx::query_scalar::<_, Option<Uuid>>("select p.group_id from conversations c join projects p on p.id = c.project_id where c.id = $1").bind(conversation_id).fetch_optional(&state.pool).await?.flatten();

    Ok(group.map(|group_id| format!("group:{group_id}")))
}

pub async fn handle(State(state): State<AppState>, node: AuthNode, headers: HeaderMap, Json(request): Json<Value>) -> ApiResult<Response> {
    let Some(id) = request.get("id").cloned() else {
        return Ok(StatusCode::ACCEPTED.into_response());
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    let response = match request.get("method").and_then(Value::as_str).unwrap_or_default() {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL_VERSION);

            reply(&id, json!({"protocolVersion": version, "capabilities": {"tools": {}}, "serverInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")}}))
        }
        "ping" => reply(&id, json!({})),
        "tools/list" => reply(&id, json!({"tools": tool_definitions()})),
        "tools/call" => {
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

            match params.get("name").and_then(Value::as_str) {
                Some("recall") => {
                    let Some(query) = arguments.get("query").and_then(Value::as_str) else {
                        return Ok(failure(&id, INVALID_PARAMS, "recall needs a query, a string."));
                    };
                    let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(DEFAULT_RECALL_LIMIT).clamp(1, MAX_RECALL_LIMIT);
                    let group_scope = group_scope_of(&state, &headers, node.id).await?;
                    let visible: Vec<String> = ["global".to_string()].into_iter().chain(group_scope).collect();
                    let rows = search(&state.pool, Some(query), Some(&visible), limit).await?;

                    if rows.is_empty() {
                        reply(&id, tool_text("No memories match that query.", false))
                    } else {
                        let lines: Vec<String> = rows.iter().map(|row| format!("{}: {}", row.id, row.text.replace('\n', " "))).collect();

                        reply(&id, tool_text(&lines.join("\n"), false))
                    }
                }
                Some("remember") => {
                    let Some(text) = arguments.get("text").and_then(Value::as_str) else {
                        return Ok(failure(&id, INVALID_PARAMS, "remember needs text, a string."));
                    };
                    let text = text.trim();

                    if text.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was stored. Give the text to remember.", true)));
                    }

                    let node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(node.id).fetch_one(&state.pool).await?;
                    let conversation = claimed_conversation(&state, &headers, node.id).await?;
                    let source = match &conversation {
                        Some((conversation_id, title)) if title.trim().is_empty() => conversation_id.to_string(),
                        Some((_, title)) => title.clone(),
                        None => "unknown conversation".to_string(),
                    };
                    let scopes = match arguments.get("scope").and_then(Value::as_str) {
                        Some("group") => match group_scope_of(&state, &headers, node.id).await? {
                            Some(scope) => vec![scope],
                            None => return Ok(reply(&id, tool_text("Nothing was stored. This conversation's project is not in a group, so remember it without a scope to share it with every conversation.", true))),
                        },
                        _ => vec!["global".to_string()],
                    };
                    let memory_id = sqlx::query_scalar::<_, Uuid>("insert into memories (text, scopes, created_by, conversation_id) values ($1, $2, $3, $4) returning id")
                        .bind(text)
                        .bind(&scopes)
                        .bind(format!("{node_name}/{source}"))
                        .bind(conversation.map(|(conversation_id, _)| conversation_id))
                        .fetch_one(&state.pool)
                        .await?;

                    reply(&id, tool_text(&format!("Remembered as {memory_id}."), false))
                }
                Some(tool @ ("propose_task" | "ask_question")) => {
                    let (field, status) = if tool == "propose_task" { ("title", TaskStatus::Proposed) } else { ("question", TaskStatus::Question) };
                    let Some(title) = arguments.get(field).and_then(Value::as_str).map(str::trim) else {
                        return Ok(failure(&id, INVALID_PARAMS, &format!("{tool} needs {field}, a string.")));
                    };

                    if title.is_empty() {
                        return Ok(reply(&id, tool_text(&format!("Nothing was stored. Give the {field}."), true)));
                    }

                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was stored. A task belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let body = arguments.get("body").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
                    let options: Vec<String> = arguments.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|option| !option.is_empty()).map(String::from).collect();
                    let project = arguments.get("project").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());

                    match tasks::record(&state, conversation_id, status, title, body, &options, project).await? {
                        Ok(task) if status == TaskStatus::Question => reply(&id, tool_text(&format!("Question {} asked. End your turn: the answer arrives here as a new message.", task.id), false)),
                        Ok(task) => reply(&id, tool_text(&format!("Task {} proposed. The person can approve it from their Tasks board.", task.id), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("walkthrough") => {
                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was stored. A walkthrough belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

                    match walkthroughs::record(&state, owner, conversation_id, &arguments).await? {
                        Ok(walkthrough) => reply(&id, tool_text(&format!("Walkthrough stored with {} steps. The person can read it in the Walkthrough tab.", walkthrough.steps.len()), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                _ => failure(&id, INVALID_PARAMS, "Unknown tool. Use recall, remember, propose_task, ask_question or walkthrough."),
            }
        }
        _ => failure(&id, METHOD_NOT_FOUND, "Method not found."),
    };

    Ok(response)
}
