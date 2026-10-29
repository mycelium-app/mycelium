use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ResizeTerminalRequest, StartTerminalRequest, StartTerminalResponse, TerminalInputRequest};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<StartTerminalRequest>) -> ApiResult<Json<StartTerminalResponse>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if request.cols == 0 || request.rows == 0 {
        return Err(ApiError::bad_request("size_required", "The terminal needs at least one column and one row."));
    }

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let terminal_id = Uuid::new_v4();
    state.hub.track_terminal(terminal_id, row.node_id, user.id, id);

    let sent = state.hub.send(row.node_id, ControlToNode::StartTerminal { terminal_id, conversation_id: id, cwd: row.cwd, cols: request.cols, rows: request.rows });

    if !sent {
        state.hub.finish_terminal(terminal_id);

        return Err(node_offline());
    }

    Ok(Json(StartTerminalResponse { terminal_id }))
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
