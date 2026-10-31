use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::state::{AppState, TerminalBufferAnswer};
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use futures_util::future::join_all;
use mycelium_proto::api::{OpenTerminal, ResizeTerminalRequest, StartTerminalRequest, StartTerminalResponse, TerminalInputRequest};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const BUFFER_TIMEOUT: Duration = Duration::from_secs(3);
const LAYOUT_LIMIT_BYTES: usize = 64 * 1024;

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<StartTerminalRequest>) -> ApiResult<Json<StartTerminalResponse>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if request.cols == 0 || request.rows == 0 {
        return Err(ApiError::bad_request("size_required", "The terminal needs at least one column and one row."));
    }

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let terminal_id = Uuid::new_v4();
    state.hub.track_terminal(terminal_id, row.node_id, user.id, id, 0);

    let sent = state.hub.send(row.node_id, ControlToNode::StartTerminal { terminal_id, conversation_id: id, cwd: row.cwd, cols: request.cols, rows: request.rows });

    if !sent {
        state.hub.finish_terminal(terminal_id);

        return Err(node_offline());
    }

    Ok(Json(StartTerminalResponse { terminal_id }))
}

pub async fn open(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<OpenTerminal>>> {
    find_owned(&state.pool, id, user.id).await?;

    let state = &state;
    let reads = state.hub.terminals_of_conversation(id).into_iter().map(|terminal_id| async move {
        let buffer = read_buffer(state, terminal_id).await?;

        Some(OpenTerminal { terminal_id, scrollback: buffer.data, offset: buffer.total })
    });
    let open = join_all(reads).await.into_iter().flatten().collect();

    Ok(Json(open))
}

pub async fn read_buffer(state: &AppState, terminal_id: Uuid) -> Option<TerminalBufferAnswer> {
    let owner = state.hub.terminal_owner(terminal_id)?;
    let buffer = state.ask_node::<TerminalBufferAnswer>(owner.node_id, BUFFER_TIMEOUT, |request_id| ControlToNode::ReadTerminal { request_id, terminal_id, from: 0 }).await?;

    if !buffer.found {
        state.hub.finish_terminal(terminal_id);
        state.publish(Some(owner.user_id), StreamMessage::TerminalExited { terminal_id });

        return None;
    }

    Some(buffer)
}

pub async fn input(State(state): State<AppState>, user: AuthUser, Path(terminal_id): Path<Uuid>, Json(request): Json<TerminalInputRequest>) -> ApiResult<StatusCode> {
    let owner = state.hub.terminal_owner(terminal_id).filter(|owner| owner.user_id == user.id).ok_or_else(|| ApiError::not_found("That terminal"))?;

    if !state.hub.send(owner.node_id, ControlToNode::TerminalInput { terminal_id, data: request.data }) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn resize(State(state): State<AppState>, user: AuthUser, Path(terminal_id): Path<Uuid>, Json(request): Json<ResizeTerminalRequest>) -> ApiResult<StatusCode> {
    let owner = state.hub.terminal_owner(terminal_id).filter(|owner| owner.user_id == user.id).ok_or_else(|| ApiError::not_found("That terminal"))?;

    if request.cols == 0 || request.rows == 0 {
        return Err(ApiError::bad_request("size_required", "The terminal needs at least one column and one row."));
    }

    if !state.hub.send(owner.node_id, ControlToNode::ResizeTerminal { terminal_id, cols: request.cols, rows: request.rows }) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop(State(state): State<AppState>, user: AuthUser, Path(terminal_id): Path<Uuid>) -> ApiResult<StatusCode> {
    let owner = state.hub.terminal_owner(terminal_id).filter(|owner| owner.user_id == user.id).ok_or_else(|| ApiError::not_found("That terminal"))?;

    state.hub.send(owner.node_id, ControlToNode::StopTerminal { terminal_id });
    state.hub.finish_terminal(terminal_id);
    state.publish(Some(owner.user_id), StreamMessage::TerminalExited { terminal_id });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn layout(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    find_owned(&state.pool, id, user.id).await?;

    let stored = sqlx::query_scalar::<_, Value>("select layout from terminal_layouts where conversation_id = $1").bind(id).fetch_optional(&state.pool).await?;

    Ok(Json(stored.unwrap_or_else(|| json!({}))))
}

pub async fn save_layout(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(layout): Json<Value>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    if !layout.is_object() || layout.to_string().len() > LAYOUT_LIMIT_BYTES {
        return Err(ApiError::bad_request("layout_invalid", "The terminal layout must be a JSON object of at most 64 KiB."));
    }

    sqlx::query("insert into terminal_layouts (conversation_id, layout) values ($1, $2) on conflict (conversation_id) do update set layout = excluded.layout, updated_at = now()").bind(id).bind(layout).execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}
