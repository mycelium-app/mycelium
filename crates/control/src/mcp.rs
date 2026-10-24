use crate::api_access;
use crate::artifacts;
use crate::auth::AuthNode;
use crate::error::ApiResult;
use crate::instruction_proposals;
use crate::memories::{MEMORY_COLUMNS, MemoryRow, omitted_groups, search};
use crate::message_proposals;
use crate::schedules::{self, ProposedSchedule, preset_cron};
use crate::ssh_access;
use crate::state::AppState;
use crate::tasks;
use crate::walkthroughs;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{EachRun, TaskStatus};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const DEFAULT_RECALL_LIMIT: i64 = 5;
const MAX_RECALL_LIMIT: i64 = 20;
const DEFAULT_FIND_LIMIT: i64 = 10;
const MAX_FIND_LIMIT: i64 = 50;
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

pub fn tool_definitions() -> Value {
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
            "description": "Write a diff walkthrough for this conversation when the person asks for one: the change read in order, as numbered steps. Group the steps into named sections in reading order, each with an icon, and mark exactly one step as the key change. The person reads it in a Walkthrough tab beside the conversation. Every change in the scope must be covered by a step: give a file with only its path to cover all of its changes, or split a file across steps by hunk. The call is refused with the uncovered files and hunks until every change is in a step. Calling it again replaces the previous walkthrough.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "enum": ["branch", "uncommitted", "last_commit"], "description": "The changes the walkthrough is about: this branch against its base branch (default), the uncommitted changes in the working tree, or the last commit."},
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
                                            "diff": {"type": "string", "description": "Optional. The part of this file's diff this step is about, used to pick its hunks when a file is split across steps. Omit it to show every change in the file."},
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
        {
            "name": "get_instructions",
            "description": "Read the current text and revision of one layer of the person's agent instructions (the AGENTS.md files). Call it before propose_instructions_edit: the proposal must be made on that revision. Without a layer it reads this conversation's group layer, or the global layer when the conversation has no group.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "layer": {"type": "string", "description": "global, or the name of a group. Defaults to this conversation's group, or global."},
                },
            },
        },
        {
            "name": "propose_instructions_edit",
            "description": "Propose an edit to the person's agent instructions (AGENTS.md) for them to approve. Use it when the person corrects how you work and the correction is a lasting rule, not a one-off: quote their words in reason. Never edit instruction files directly. Read the layer with get_instructions first, then send the complete new text of that layer and the revision you read. It waits in Settings, Instructions until the person approves or declines it, and a decline arrives in this conversation as a message. Layer defaults to this conversation's group, or global without one; pass layer global for a rule that applies everywhere.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The complete new text of the layer, with your edit applied to the text you read."},
                    "base_revision": {"type": "string", "description": "The revision get_instructions returned for the text you edited. A proposal on an older revision is refused."},
                    "reason": {"type": "string", "description": "One paragraph saying why, quoting the person's own words."},
                    "layer": {"type": "string", "description": "global, or the name of a group. Defaults to this conversation's group, or global."},
                },
                "required": ["text", "base_revision", "reason"],
            },
        },
        {
            "name": "find_conversations",
            "description": "Look up the person's other conversations, most recent first, to find the one to message with send_to_conversation. Archived conversations are left out. It only reads and needs no approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words that must each appear in the conversation's title, project name or folder."},
                    "limit": {"type": "number", "description": "How many conversations to return. Defaults to 10."},
                },
            },
        },
        {
            "name": "send_to_conversation",
            "description": "Propose a message to send to another of the person's conversations. It never sends directly: it waits for the person to approve it in a card in this conversation, and only then is the message queued in the other conversation, prefixed with the title of this one. Find the target with find_conversations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "conversation": {"type": "string", "description": "The id of the target conversation, or its exact title."},
                    "message": {"type": "string", "description": "The message to send. Write it so it makes sense without this conversation."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["conversation", "message", "reason"],
            },
        },
        {
            "name": "request_api_access",
            "description": "Ask the person for temporary access to the Mycelium API, for work such as debugging that needs more than these tools. It never grants anything itself: it waits for the person to approve it in a card in this conversation, where they can untick scopes and see the duration. On approval you are told in a message the path of a file holding a token and the API address; read the token from that file when you need it and never copy it into a message, a file or a commit. It expires and the person can revoke it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scopes": {"type": "array", "items": {"type": "string", "enum": ["conversations:read", "conversations:write", "nodes:read", "settings:read"]}, "description": "The access you need. Ask for the least that works."},
                    "hours": {"type": "number", "description": "How long the access lasts, 1 to 24. Defaults to 4."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["scopes", "reason"],
            },
        },
        {
            "name": "request_ssh_access",
            "description": "Ask the person for temporary SSH access to one of their nodes, for work such as debugging a machine this conversation does not run on. It never grants anything itself: it waits for the person to approve it in a card in this conversation, where they can shorten the duration. On approval you are told in a message the exact ssh command to run. The connection goes through Mycelium, so the node needs no open port. Never copy the token file it names into a message, a file or a commit. Access expires, and the person can revoke it, which ends open sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": {"type": "string", "description": "The node to reach: its name or id."},
                    "minutes": {"type": "number", "description": "How long the access lasts, 5 to 240. Defaults to 30."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["node", "reason"],
            },
        },
        {
            "name": "create_artifact",
            "description": "Save something the person should look at and come back to as an artifact: a design (artboards on a canvas), a page (one HTML file, which may keep state in the artifact's data store), a markdown document, or any other file such as a PDF. Give the content as text, or the path of a file in this conversation's folder. A design without content starts with one empty page. The person opens it in Mycelium, comments on it and edits it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["design", "page", "markdown", "file"], "description": "design, page (HTML), markdown, or file (anything else)."},
                    "title": {"type": "string", "description": "A short title."},
                    "content": {"type": "string", "description": "The content as text. A design is JSON, {\"pages\": [{\"id\", \"name\", \"items\": [...]}]}."},
                    "path": {"type": "string", "description": "A file in this conversation's folder to store instead of content. Use it for binary files."},
                    "media_type": {"type": "string", "description": "The media type of a file. Defaults to one guessed from the path."},
                },
                "required": ["kind", "title"],
            },
        },
        {
            "name": "update_artifact",
            "description": "Save a new version of an artifact with the complete new content, as text or from a file in this conversation's folder. Earlier versions are kept. To change one artboard of a design use design_set_artboard instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                    "content": {"type": "string", "description": "The complete new content as text."},
                    "path": {"type": "string", "description": "A file in this conversation's folder to store instead of content."},
                    "note": {"type": "string", "description": "A short note saying what changed."},
                },
                "required": ["artifact"],
            },
        },
        {
            "name": "design_set_artboard",
            "description": "Replace or add one artboard in the latest version of a design, and save a new version. Everything else in the design stays as it is, including what the person drew. Give artboard to replace an existing one, or leave it out to add a new one beside the others. The html is a complete document, shown in a sandboxed frame.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The design's id or slug."},
                    "page": {"type": "string", "description": "The id of the page. Defaults to the first page."},
                    "artboard": {"type": "string", "description": "The id of the artboard to replace. A new artboard is added when it is left out."},
                    "name": {"type": "string", "description": "The artboard's name."},
                    "x": {"type": "number", "description": "Left edge on the canvas."},
                    "y": {"type": "number", "description": "Top edge on the canvas."},
                    "w": {"type": "number", "description": "Width. Defaults to 1440 for a new artboard."},
                    "h": {"type": "number", "description": "Height. Defaults to 900 for a new artboard."},
                    "html": {"type": "string", "description": "The artboard's complete HTML document."},
                },
                "required": ["artifact", "html"],
            },
        },
        {
            "name": "design_remove_item",
            "description": "Remove one item (an artboard, shape, text, note or drawing) from a design by its id, and save a new version.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The design's id or slug."},
                    "item": {"type": "string", "description": "The id of the item to remove. read_artifact lists them."},
                },
                "required": ["artifact", "item"],
            },
        },
        {
            "name": "read_artifact",
            "description": "Read the latest version of an artifact: the text of a page or document, a summary of a file, or for a design its pages and items with ids, names and sizes. Give artboard to read the full HTML of one artboard of a design.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                    "artboard": {"type": "string", "description": "The id or name of an artboard of a design."},
                },
                "required": ["artifact"],
            },
        },
        {
            "name": "read_artifact_feedback",
            "description": "Read what people left on an artifact: its unresolved comments with where they point, and the keys and values its data store holds. Use it to read back the choices a person made on an interactive page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string", "description": "The artifact's id or slug."},
                },
                "required": ["artifact"],
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
                    let memory = sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!("insert into memories (text, scopes, created_by, conversation_id) values ($1, $2, $3, $4) returning {MEMORY_COLUMNS}")))
                        .bind(crate::store::text_without_nul(&text))
                        .bind(&scopes)
                        .bind(format!("{node_name}/{source}"))
                        .bind(conversation.map(|(conversation_id, _)| conversation_id))
                        .fetch_one(&state.pool)
                        .await?
                        .to_proto();
                    let memory_id = memory.id;
                    state.publish(None, StreamMessage::Memory(memory));

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
                Some("get_instructions") => {
                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("The instructions belong to a person, and this call did not come from a conversation.", true)));
                    };

                    match instruction_proposals::layer_text(&state, conversation_id, arguments.get("layer").and_then(Value::as_str)).await? {
                        Ok(layer) => reply(&id, tool_text(&format!("Layer: {}\nRevision: {}\n\n{}", layer.name, layer.revision, layer.text), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("propose_instructions_edit") => {
                    let field = |name: &str| arguments.get(name).and_then(Value::as_str);
                    let (Some(text), Some(base_revision), Some(reason)) = (field("text"), field("base_revision"), field("reason").map(str::trim)) else {
                        return Ok(failure(&id, INVALID_PARAMS, "propose_instructions_edit needs text, base_revision and reason, strings."));
                    };

                    if text.trim().is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was proposed. Give the complete new text of the layer.", true)));
                    }

                    if reason.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was proposed. Give the reason, quoting the person's own words.", true)));
                    }

                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was proposed. A proposal belongs to a conversation, and this call did not come from one.", true)));
                    };

                    match instruction_proposals::propose(&state, conversation_id, field("layer"), base_revision, text, reason).await? {
                        Ok(proposal) => reply(&id, tool_text(&format!("Proposal {} made for the {} instructions. It waits for the person to approve it in Settings, Instructions. If they decline it, the reason arrives here as a message.", proposal.id, proposal.layer_name), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("find_conversations") => {
                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("The conversations belong to a person, and this call did not come from one of them.", true)));
                    };
                    let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(DEFAULT_FIND_LIMIT).clamp(1, MAX_FIND_LIMIT);
                    let words: Vec<String> = arguments.get("query").and_then(Value::as_str).unwrap_or_default().split_whitespace().map(String::from).collect();
                    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, String, String, String, chrono::DateTime<chrono::Utc>)>(
                        "select c.id, c.title, p.name, coalesce(n.name, ''), c.agent, c.status, c.updated_at
                         from conversations c left join projects p on p.id = c.project_id left join nodes n on n.id = c.node_id
                         where c.owner = (select owner from conversations where id = $1) and c.archived_at is null
                           and not exists (select 1 from unnest($2::text[]) as word where not (strpos(lower(c.title), lower(word)) > 0 or strpos(lower(coalesce(p.name, '')), lower(word)) > 0 or strpos(lower(c.cwd), lower(word)) > 0))
                         order by c.updated_at desc, c.id limit $3",
                    )
                    .bind(conversation_id)
                    .bind(&words)
                    .bind(limit)
                    .fetch_all(&state.pool)
                    .await?;

                    if rows.is_empty() {
                        reply(&id, tool_text("No conversations match that query.", false))
                    } else {
                        let lines: Vec<String> = rows
                            .iter()
                            .map(|(found, title, project, node_name, agent, status, updated_at)| {
                                format!("{found}: {} (project {}, node {node_name}, agent {agent}, status {status}, last activity {})", message_proposals::label(*found, title).replace('\n', " "), project.as_deref().unwrap_or("none"), updated_at.to_rfc3339())
                            })
                            .collect();

                        reply(&id, tool_text(&lines.join("\n"), false))
                    }
                }
                Some("send_to_conversation") => {
                    let field = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim);
                    let (Some(target), Some(message), Some(reason)) = (field("conversation"), field("message"), field("reason")) else {
                        return Ok(failure(&id, INVALID_PARAMS, "send_to_conversation needs conversation, message and reason, strings."));
                    };

                    if target.is_empty() || message.is_empty() || reason.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was proposed. Give the conversation, the message and the reason.", true)));
                    }

                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was proposed. A message proposal belongs to a conversation, and this call did not come from one.", true)));
                    };

                    match message_proposals::propose(&state, conversation_id, target, message, reason).await? {
                        Ok(proposal) => reply(&id, tool_text(&format!("Waiting for the person to approve sending this to {} (proposal {}).", proposal.target_title, proposal.id), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("request_api_access") => {
                    let scopes: Vec<String> = arguments.get("scopes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                    let reason = arguments.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();

                    if reason.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was requested. Give the reason.", true)));
                    }

                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was requested. API access belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let hours = arguments.get("hours").and_then(Value::as_i64);

                    match api_access::request(&state, conversation_id, &scopes, hours, reason).await? {
                        Ok(request) => reply(&id, tool_text(&format!("Waiting for the person to approve API access (request {}).", request.id), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some("request_ssh_access") => {
                    let node_name = arguments.get("node").and_then(Value::as_str).map(str::trim).unwrap_or_default();
                    let reason = arguments.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();

                    if reason.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was requested. Give the reason.", true)));
                    }

                    if node_name.is_empty() {
                        return Ok(reply(&id, tool_text("Nothing was requested. Give the node.", true)));
                    }

                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("Nothing was requested. SSH access belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let minutes = arguments.get("minutes").and_then(Value::as_i64);

                    match ssh_access::request(&state, conversation_id, node_name, minutes, reason).await? {
                        Ok(request) => reply(&id, tool_text(&format!("Waiting for the person to approve SSH access to {} (request {}).", request.node_name, request.id), false)),
                        Err(message) => reply(&id, tool_text(&message, true)),
                    }
                }
                Some(tool @ ("create_artifact" | "update_artifact" | "design_set_artboard" | "design_remove_item" | "read_artifact" | "read_artifact_feedback")) => {
                    let Some((conversation_id, _)) = claimed_conversation(&state, &headers, node.id).await? else {
                        return Ok(reply(&id, tool_text("An artifact belongs to a conversation, and this call did not come from one.", true)));
                    };
                    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

                    match artifacts::tool(&state, owner, conversation_id, tool, &arguments).await {
                        Ok(text) => reply(&id, tool_text(&text, false)),
                        Err(error) => reply(&id, tool_text(error.message(), true)),
                    }
                }
                _ => failure(&id, INVALID_PARAMS, "Unknown tool. Use recall, remember, propose_task, ask_question, propose_schedule, walkthrough, get_instructions, propose_instructions_edit, find_conversations, send_to_conversation, request_api_access, request_ssh_access, create_artifact, update_artifact, design_set_artboard, design_remove_item, read_artifact or read_artifact_feedback."),
            }
        }
        _ => failure(&id, METHOD_NOT_FOUND, "Method not found."),
    };

    Ok(response)
}

pub async fn call_in_process(state: &AppState, node_id: Uuid, conversation_id: Uuid, name: &str, arguments: &Value) -> ApiResult<(String, bool)> {
    let mut headers = HeaderMap::new();
    headers.insert("x-mycelium-conversation", conversation_id.to_string().parse().map_err(crate::error::ApiError::internal)?);
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
    let response = handle(State(state.clone()), AuthNode { id: node_id }, headers, Json(request)).await?;
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.map_err(crate::error::ApiError::internal)?;
    let answer: Value = serde_json::from_slice(&bytes).map_err(crate::error::ApiError::internal)?;

    if let Some(message) = answer["error"]["message"].as_str() {
        return Ok((message.to_string(), true));
    }

    let text: Vec<&str> = answer["result"]["content"].as_array().into_iter().flatten().filter_map(|part| part["text"].as_str()).collect();

    Ok((text.join("\n"), answer["result"]["isError"].as_bool().unwrap_or(false)))
}
