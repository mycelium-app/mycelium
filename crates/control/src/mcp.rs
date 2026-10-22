use crate::auth::AuthNode;
use crate::error::ApiResult;
use crate::memories::search;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
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
                "properties": {"text": {"type": "string", "description": "What to remember."}},
                "required": ["text"],
            },
        },
    ])
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
                    let rows = search(&state.pool, Some(query), Some(&["global".to_string()]), limit).await?;

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
                    let claimed = headers.get("x-mycelium-conversation").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<Uuid>().ok());
                    let conversation = match claimed {
                        Some(conversation_id) => {
                            sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = $1 and node_id = $2").bind(conversation_id).bind(node.id).fetch_optional(&state.pool).await?
                        }
                        None => None,
                    };
                    let source = match &conversation {
                        Some((conversation_id, title)) if title.trim().is_empty() => conversation_id.to_string(),
                        Some((_, title)) => title.clone(),
                        None => "unknown conversation".to_string(),
                    };
                    let memory_id = sqlx::query_scalar::<_, Uuid>("insert into memories (text, created_by, conversation_id) values ($1, $2, $3) returning id")
                        .bind(text)
                        .bind(format!("{node_name}/{source}"))
                        .bind(conversation.map(|(conversation_id, _)| conversation_id))
                        .fetch_one(&state.pool)
                        .await?;

                    reply(&id, tool_text(&format!("Remembered as {memory_id}."), false))
                }
                _ => failure(&id, INVALID_PARAMS, "Unknown tool. Use recall or remember."),
            }
        }
        _ => failure(&id, METHOD_NOT_FOUND, "Method not found."),
    };

    Ok(response)
}
