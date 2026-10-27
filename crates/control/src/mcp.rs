mod api_access;
mod artifacts;
mod assistants;
mod call;
mod conversations;
mod decide;
mod earlier_turns;
mod goal_and_usage;
mod instructions;
mod integration_tools;
mod learning;
mod memory;
mod outline;
mod project_icon;
mod server_tools;
mod ssh_access;
mod tasks;
mod workflows;

use crate::auth::AuthNode;
use crate::error::ApiResult;
use crate::learned;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use call::{Caller, ToolCall, ToolReply, node_owner};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const TOOLS_CHANGED: &str = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;

pub const WORKSPACE_ONLY_TOOLS: [&str; 2] = ["find_integration_tools", "call_integration_tool"];

#[derive(Clone, Copy)]
enum Area {
    IntegrationTools,
    ServerTools,
    Workflows,
    Memory,
    Tasks,
    EarlierTurns,
    Assistants,
    Learning,
    Instructions,
    GoalAndUsage,
    ProjectIcon,
    Outline,
    Conversations,
    ApiAccess,
    Decide,
    SshAccess,
    Artifacts,
}

fn area_of(tool: &str) -> Option<Area> {
    match tool {
        "find_integration_tools" => Some(Area::IntegrationTools),
        "start_workflow" | "start_subagent" | "list_places" | "get_workflow" => Some(Area::Workflows),
        "recall" | "remember" => Some(Area::Memory),
        "propose_task" | "ask_question" | "propose_schedule" => Some(Area::Tasks),
        "read_earlier_turns" => Some(Area::EarlierTurns),
        "assistant_list_files" | "assistant_read_file" | "assistant_write_file" | "assistant_delete_file" | "assistant_set_secret" | "assistant_request_secret" | "assistant_propose_action" | "assistant_list" | "assistant_create" | "assistant_update" | "assistant_use_workspace" => Some(Area::Assistants),
        "walkthrough" | "save_reflection" | "propose_exercise" | "note_learned" | "save_exercise_review" => Some(Area::Learning),
        "get_instructions" | "propose_instructions_edit" => Some(Area::Instructions),
        "get_goal" | "get_account_usage" => Some(Area::GoalAndUsage),
        "set_project_icon" => Some(Area::ProjectIcon),
        "open_outline" => Some(Area::Outline),
        "find_conversations" | "start_conversation" | "send_to_conversation" => Some(Area::Conversations),
        "request_api_access" => Some(Area::ApiAccess),
        "decide" => Some(Area::Decide),
        "request_ssh_access" => Some(Area::SshAccess),
        "find_artifacts" | "request_artifact_access" | "create_artifact" | "update_artifact" | "design_set_artboard" | "design_remove_item" | "read_artifact" | "read_artifact_feedback" => Some(Area::Artifacts),
        name if name.contains("__") => Some(Area::ServerTools),
        _ => None,
    }
}

async fn call_area(area: Area, call: ToolCall<'_>) -> ApiResult<ToolReply> {
    match area {
        Area::IntegrationTools => integration_tools::call(call).await,
        Area::ServerTools => server_tools::call(call).await,
        Area::Workflows => workflows::call(call).await,
        Area::Memory => memory::call(call).await,
        Area::Tasks => tasks::call(call).await,
        Area::EarlierTurns => earlier_turns::call(call).await,
        Area::Assistants => assistants::call(call).await,
        Area::Learning => learning::call(call).await,
        Area::Instructions => instructions::call(call).await,
        Area::GoalAndUsage => goal_and_usage::call(call).await,
        Area::ProjectIcon => project_icon::call(call).await,
        Area::Outline => outline::call(call).await,
        Area::Conversations => conversations::call(call).await,
        Area::ApiAccess => api_access::call(call).await,
        Area::Decide => decide::call(call).await,
        Area::SshAccess => ssh_access::call(call).await,
        Area::Artifacts => artifacts::call(call).await,
    }
}

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
    let areas = [
        integration_tools::definitions(),
        workflows::definitions(),
        memory::definitions(),
        tasks::definitions(),
        earlier_turns::definitions(),
        assistants::definitions(),
        learning::definitions(),
        instructions::definitions(),
        goal_and_usage::definitions(),
        project_icon::definitions(),
        outline::definitions(),
        conversations::definitions(),
        api_access::definitions(),
        decide::definitions(),
        ssh_access::definitions(),
        artifacts::definitions(),
    ];

    Value::Array(areas.into_iter().flat_map(|definitions| definitions.as_array().cloned().unwrap_or_default()).collect())
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

pub async fn handle(State(state): State<AppState>, node: AuthNode, headers: HeaderMap, Json(request): Json<Value>) -> ApiResult<Response> {
    respond(&state, node.id, &headers, request).await
}

pub async fn respond(state: &AppState, node_id: Uuid, headers: &HeaderMap, request: Value) -> ApiResult<Response> {
    let Some(id) = request.get("id").cloned() else {
        return Ok(StatusCode::ACCEPTED.into_response());
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let caller = Caller { state, node_id, headers };

    let response = match request.get("method").and_then(Value::as_str).unwrap_or_default() {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL_VERSION);

            let mut response = reply(&id, json!({"protocolVersion": version, "capabilities": {"tools": {"listChanged": true}}, "serverInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")}}));
            response.headers_mut().insert("mcp-session-id", Uuid::new_v4().to_string().parse().map_err(crate::error::ApiError::internal)?);

            response
        }
        "ping" => reply(&id, json!({})),
        "tools/list" => {
            let mut tools = tool_definitions().as_array().cloned().unwrap_or_default();
            let learning = match caller.conversation().await? {
                Some((conversation_id, _)) => learned::active_source(state, conversation_id).await?.is_some(),
                None => false,
            };

            if !learning {
                tools.retain(|tool| tool["name"].as_str() != Some("note_learned"));
            }

            tools.extend(integration_tools::listed(&caller).await?);

            reply(&id, json!({"tools": tools}))
        }
        "tools/call" => {
            let mut arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let mut called = params.get("name").and_then(Value::as_str).map(String::from);

            if called.as_deref() == Some("call_integration_tool") {
                let inner = arguments.get("name").and_then(Value::as_str).filter(|name| name.contains("__") && !WORKSPACE_ONLY_TOOLS.contains(name)).map(String::from);
                let Some(inner) = inner else {
                    return Ok(reply(&id, tool_text("Give the name of an integration tool as find_integration_tools returned it, such as server__tool.", true)));
                };
                let inner_arguments = arguments.get("arguments").cloned().unwrap_or_else(|| json!({}));
                called = Some(inner);
                arguments = inner_arguments;
            }

            let area = called.as_deref().and_then(area_of);

            match (called.as_deref(), area) {
                (Some(tool), Some(area)) => {
                    let outcome = call_area(area, ToolCall { caller, tool, params: &params, arguments }).await?;

                    match outcome {
                        ToolReply::Text { text, is_error } => reply(&id, tool_text(&text, is_error)),
                        ToolReply::Invalid(message) => failure(&id, INVALID_PARAMS, &message),
                    }
                }
                _ => failure(&id, INVALID_PARAMS, "Unknown tool. Use list_places, assistant_list, assistant_create, assistant_update, assistant_propose_action, recall, remember, propose_task, ask_question, propose_schedule, walkthrough, save_reflection, propose_exercise, note_learned, save_exercise_review, read_earlier_turns, get_instructions, propose_instructions_edit, find_conversations, send_to_conversation, request_api_access, decide, request_ssh_access, create_artifact, update_artifact, design_set_artboard, design_remove_item, read_artifact or read_artifact_feedback."),
            }
        }
        _ => failure(&id, METHOD_NOT_FOUND, "Method not found."),
    };

    Ok(response)
}

pub async fn stream(State(state): State<AppState>, node: AuthNode) -> ApiResult<Response> {
    let owner = node_owner(&state, node.id).await?;
    let receiver = state.hub.subscribe_tools(owner);
    let notifications = futures_util::stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Err(RecvError::Closed) => None,
            _ => Some((Ok::<Event, Infallible>(Event::default().data(TOOLS_CHANGED)), receiver)),
        }
    });

    Ok(Sse::new(notifications).keep_alive(KeepAlive::default()).into_response())
}

pub async fn call_in_process(state: &AppState, node_id: Uuid, conversation_id: Uuid, tool_call: Option<&str>, name: &str, arguments: &Value) -> ApiResult<(String, bool)> {
    let mut headers = HeaderMap::new();

    if let Some(tool_call) = tool_call.and_then(|tool_call| tool_call.parse().ok()) {
        headers.insert("x-mycelium-tool-call", tool_call);
    }

    headers.insert("x-mycelium-conversation", conversation_id.to_string().parse().map_err(crate::error::ApiError::internal)?);
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
    let response = respond(state, node_id, &headers, request).await?;
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.map_err(crate::error::ApiError::internal)?;
    let answer: Value = serde_json::from_slice(&bytes).map_err(crate::error::ApiError::internal)?;

    if let Some(message) = answer["error"]["message"].as_str() {
        return Ok((message.to_string(), true));
    }

    let text: Vec<&str> = answer["result"]["content"].as_array().into_iter().flatten().filter_map(|part| part["text"].as_str()).collect();

    Ok((text.join("\n"), answer["result"]["isError"].as_bool().unwrap_or(false)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_defined_tool_has_an_area() {
        let definitions = tool_definitions();
        let unrouted: Vec<&str> = definitions.as_array().into_iter().flatten().filter_map(|tool| tool["name"].as_str()).filter(|name| *name != "call_integration_tool" && area_of(name).is_none()).collect();

        assert!(unrouted.is_empty(), "no area for {unrouted:?}");
    }
}
