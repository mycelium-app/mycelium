use crate::auth::AuthNode;
use crate::error::ApiResult;
use crate::memories::{omitted_groups, search};
use crate::schedules::{self, ProposedSchedule, preset_cron};
use crate::state::AppState;
use crate::tasks;
use crate::walkthroughs;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{EachRun, TaskStatus};
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
            "description": "Search the shared memory for what earlier conversations learned. Use it before starting work that may have been done or decided before. It searches the global memories and this conversation's group, and says how many matches it left out in other groups. Pass groups to search other groups too.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to search for."},
                    "limit": {"type": "number", "description": "How many memories to return. Defaults to 5."},
                    "groups": {"description": "Other groups to search as well as this conversation's own: a list of group names, or \"all\" for every group.", "oneOf": [{"type": "array", "items": {"type": "string"}}, {"type": "string", "enum": ["all"]}]},
                },
                "required": ["query"],
            },
        },
        {
            "name": "remember",
            "description": "Store a short, confirmed finding in the shared memory so later conversations on any node can recall it. It is kept in this conversation's group, or globally when the conversation has no group.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "What to remember."},
                    "scope": {"type": "string", "enum": ["global", "group"], "description": "group (default when the conversation is in a group) is recalled only by conversations in the same group. global is recalled in every conversation: use it for what is true of the tools, the host or the agents themselves."},
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
                    "depends_on": {"type": "array", "items": {"type": "string"}, "description": "Ids of tasks that must be done before this one can be started."},
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
            "name": "propose_schedule",
            "description": "Set up a recurring run of a prompt when the person asks you to schedule something, run it every day, every weekday, every hour and so on. Always use this for Mycelium work, never crontab, systemd timers or sleep loops. It waits for the person to confirm in a card in this conversation and never runs until they do. By default each run is queued in this conversation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "The prompt to run on each occurrence. Write it so it makes sense without the earlier conversation."},
                    "preset": {"type": "string", "description": "When to run, as \"weekdays at 20:00\", \"daily at 09:00\", \"every 15 minutes\" or \"every 2 hours\". Give this or cron."},
                    "cron": {"type": "string", "description": "When to run, as five cron fields, for example 0 20 * * 1-5. Evaluated in time_zone. Used when preset is not given."},
                    "each_run": {"type": "string", "enum": ["same", "new"], "description": "same (default) queues each run in this conversation. new starts a new conversation for each run."},
                    "time_zone": {"type": "string", "description": "An IANA time zone such as Europe/London. Defaults to the person's own."},
                },
                "required": ["prompt"],
            },
        },
        {
            "name": "walkthrough",
            "description": "Write a diff walkthrough for this conversation when the person asks for one: the change read in order, as numbered steps. Group the steps into named sections in reading order, each with an icon, and mark exactly one step as the key change. The person reads it in a Walkthrough tab beside the conversation. Calling it again replaces the previous walkthrough.",
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
                                "key": {"type": "boolean", "description": "True for the step that carries the change. Mark exactly one."},
                                "section": {"type": "string", "description": "The name of the section this step belongs to, such as Retry logic or Tests. Give consecutive steps of one section the same name."},
                                "icon": {"type": "string", "enum": ["logic", "tests", "docs", "config", "ui", "data", "other"], "description": "The icon for the step's section. Defaults to other."},
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

pub fn tools_instructions() -> String {
    let lines: Vec<String> = tool_definitions()
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| Some(format!("- mcp__mycelium__{}: {}", tool["name"].as_str()?, tool["description"].as_str()?.replace('\n', " "))))
        .collect();

    format!(
        "You are running inside Mycelium, and its MCP server, mycelium, gives you these tools. They are MCP tools named mcp__mycelium__<name>, not skills. Your own memory features are off on purpose: use remember and recall instead.\n{}",
        lines.join("\n")
    )
}

async fn claimed_conversation(state: &AppState, headers: &HeaderMap, node_id: Uuid) -> ApiResult<Option<(Uuid, String)>> {
    let claimed = headers.get("x-mycelium-conversation").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<Uuid>().ok());

    match claimed {
        Some(conversation_id) => Ok(sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = $1 and node_id = $2").bind(conversation_id).bind(node_id).fetch_optional(&state.pool).await?),
        None => Ok(None),
    }
}

async fn group_of(state: &AppState, headers: &HeaderMap, node_id: Uuid) -> ApiResult<Option<(Uuid, String)>> {
    let Some((conversation_id, _)) = claimed_conversation(state, headers, node_id).await? else {
        return Ok(None);
    };

    Ok(sqlx::query_as::<_, (Uuid, String)>("select g.id, g.name from conversations c join groups g on g.id = conversation_group(c.cwd, c.node_id, c.project_id) where c.id = $1").bind(conversation_id).fetch_optional(&state.pool).await?)
}

async fn scopes_named(state: &AppState, node_id: Uuid, names: &[String]) -> ApiResult<Result<Vec<String>, String>> {
    let mut scopes = Vec::new();

    for name in names {
        let found = sqlx::query_scalar::<_, Uuid>("select g.id from groups g join nodes n on n.created_by = g.owner where n.id = $1 and lower(g.name) = lower($2)").bind(node_id).bind(name.trim()).fetch_optional(&state.pool).await?;

        match found {
            Some(group_id) => scopes.push(format!("group:{group_id}")),
            None => return Ok(Err(format!("No group is called {}. Use a group name, or groups: \"all\".", name.trim()))),
        }
    }

    Ok(Ok(scopes))
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
                    let group = group_of(&state, &headers, node.id).await?;
                    let mut visible: Vec<String> = ["global".to_string()].into_iter().chain(group.map(|(group_id, _)| format!("group:{group_id}"))).collect();
                    let everywhere = match arguments.get("groups") {
                        Some(Value::String(text)) if text == "all" => true,
                        Some(Value::String(name)) => {
                            match scopes_named(&state, node.id, std::slice::from_ref(name)).await? {
                                Ok(scopes) => visible.extend(scopes),
                                Err(message) => return Ok(reply(&id, tool_text(&message, true))),
                            }

                            false
                        }
                        Some(Value::Array(names)) => {
                            let names: Vec<String> = names.iter().filter_map(Value::as_str).map(String::from).collect();

                            match scopes_named(&state, node.id, &names).await? {
                                Ok(scopes) => visible.extend(scopes),
                                Err(message) => return Ok(reply(&id, tool_text(&message, true))),
                            }

                            false
                        }
                        _ => false,
                    };
                    let rows = search(&state.pool, Some(query), (!everywhere).then_some(visible.as_slice()), limit).await?;
                    let omitted = if everywhere { Vec::new() } else { omitted_groups(&state.pool, query, &visible).await? };
                    let note = if omitted.is_empty() {
                        String::new()
                    } else {
                        let count: i64 = omitted.iter().map(|(_, count)| count).sum();
                        let names: Vec<&str> = omitted.iter().map(|(name, _)| name.as_str()).collect();

                        format!("\n{count} more {} in other groups ({}) left out. Search them with groups: [\"name\"], or every group with groups: \"all\".", if count == 1 { "match" } else { "matches" }, names.join(", "))
                    };

                    if rows.is_empty() {
                        reply(&id, tool_text(&format!("No memories match that query.{note}"), false))
                    } else {
                        let lines: Vec<String> = rows.iter().map(|row| format!("{}: {}", row.id, row.text.replace('\n', " "))).collect();

                        reply(&id, tool_text(&format!("{}{note}", lines.join("\n")), false))
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
                    let group = group_of(&state, &headers, node.id).await?;
                    let (scopes, shared_with) = match (arguments.get("scope").and_then(Value::as_str), group) {
                        (Some("group"), None) => return Ok(reply(&id, tool_text("Nothing was stored. This conversation is not in a group, so remember it without a scope, or with scope global, to share it with every conversation.", true))),
                        (Some("global"), _) | (_, None) => (vec!["global".to_string()], "every conversation".to_string()),
                        (_, Some((group_id, name))) => (vec![format!("group:{group_id}")], format!("the {name} group")),
                    };
                    let memory_id = sqlx::query_scalar::<_, Uuid>("insert into memories (text, scopes, created_by, conversation_id) values ($1, $2, $3, $4) returning id")
                        .bind(text)
                        .bind(&scopes)
                        .bind(format!("{node_name}/{source}"))
                        .bind(conversation.map(|(conversation_id, _)| conversation_id))
                        .fetch_one(&state.pool)
                        .await?;

                    reply(&id, tool_text(&format!("Remembered as {memory_id}, shared with {shared_with}."), false))
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

                    let mut depends_on = Vec::new();

                    for value in arguments.get("depends_on").and_then(Value::as_array).into_iter().flatten() {
                        let Some(task_id) = value.as_str().and_then(|text| Uuid::parse_str(text.trim()).ok()) else {
                            return Ok(reply(&id, tool_text("Nothing was stored. depends_on must be a list of task ids.", true)));
                        };

                        depends_on.push(task_id);
                    }

                    match tasks::record(&state, conversation_id, status, title, body, &options, project, &depends_on).await? {
                        Ok(task) if status == TaskStatus::Question => reply(&id, tool_text(&format!("Question {} asked. End your turn: the answer arrives here as a new message.", task.id), false)),
                        Ok(task) => reply(&id, tool_text(&format!("Task {} proposed. The person can approve it from their Tasks board.", task.id), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("propose_schedule") => {
                    let prompt = arguments.get("prompt").and_then(Value::as_str).map(str::trim).unwrap_or_default();

                    if prompt.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was proposed. Give the prompt to run.", true)));
                    }

                    let preset = arguments.get("preset").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
                    let cron = match (preset, arguments.get("cron").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())) {
                        (Some(preset), _) => match preset_cron(preset) {
                            Some(cron) => cron,
                            None => return Ok(reply(&id, tool_text("Nothing was proposed. Use a preset such as \"weekdays at 20:00\", \"daily at 09:00\", \"every 15 minutes\" or \"every 2 hours\", or give cron.", true))),
                        },
                        (None, Some(cron)) => cron.to_string(),
                        (None, None) => return Ok(reply(&id, tool_text("Nothing was proposed. Give a preset or a cron expression.", true))),
                    };
                    let each_run = match arguments.get("each_run").and_then(Value::as_str) {
                        None | Some("same") => EachRun::Same,
                        Some("new") => EachRun::New,
                        Some(_) => return Ok(reply(&id, tool_text("Nothing was proposed. each_run is same or new.", true))),
                    };
                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was proposed. A schedule belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let time_zone = arguments.get("time_zone").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());

                    match schedules::propose(&state, conversation_id, ProposedSchedule { cron, prompt, each_run, time_zone }).await? {
                        Ok(schedule) => reply(&id, tool_text(&format!("Schedule {} proposed ({}, {}). It waits for the person to confirm and will not run until they do.", schedule.id, schedule.cron, schedule.time_zone), false)),
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
                _ => failure(&id, INVALID_PARAMS, "Unknown tool. Use recall, remember, propose_task, ask_question, propose_schedule or walkthrough."),
            }
        }
        _ => failure(&id, METHOD_NOT_FOUND, "Method not found."),
    };

    Ok(response)
}
