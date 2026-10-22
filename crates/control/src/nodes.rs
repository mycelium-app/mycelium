use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{node_summaries, publish_node};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{DrainRequest, LoginInputRequest, NodeSummary, StartLoginRequest, StartLoginResponse};
use mycelium_proto::link::ControlToNode;
use uuid::Uuid;

pub async fn list(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<NodeSummary>>> {
    Ok(Json(node_summaries(&state, None).await?))
}

pub async fn drain(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<DrainRequest>) -> ApiResult<StatusCode> {
    let updated = sqlx::query("update nodes set draining = $2 where id = $1 and revoked_at is null").bind(id).bind(request.draining).execute(&state.pool).await?;

    if updated.rows_affected() == 0 {
        return Err(ApiError::not_found("That node"));
    }

    publish_node(&state, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let updated = sqlx::query("update nodes set revoked_at = now() where id = $1 and revoked_at is null").bind(id).execute(&state.pool).await?;

    if updated.rows_affected() == 0 {
        return Err(ApiError::not_found("That node"));
    }

    state.hub.send(id, ControlToNode::Revoked);
    state.hub.disconnect(id);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_login(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<StartLoginRequest>) -> ApiResult<Json<StartLoginResponse>> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    let login_id = Uuid::new_v4();
    state.hub.track_login(login_id, id);

    let sent = state.hub.send(id, ControlToNode::StartLogin { login_id, agent: request.agent, method: request.method });

    if !sent {
        state.hub.finish_login(login_id);

        return Err(node_offline());
    }

    Ok(Json(StartLoginResponse { login_id }))
}

pub async fn login_input(State(state): State<AppState>, _user: AuthUser, Path(login_id): Path<Uuid>, Json(request): Json<LoginInputRequest>) -> ApiResult<StatusCode> {
    let node_id = state.hub.login_node(login_id).ok_or_else(|| ApiError::not_found("That sign-in"))?;

    if !state.hub.send(node_id, ControlToNode::LoginInput { login_id, text: request.text }) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop_login(State(state): State<AppState>, _user: AuthUser, Path(login_id): Path<Uuid>) -> ApiResult<StatusCode> {
    let node_id = state.hub.login_node(login_id).ok_or_else(|| ApiError::not_found("That sign-in"))?;

    state.hub.send(node_id, ControlToNode::StopLogin { login_id });
    state.hub.finish_login(login_id);

    Ok(StatusCode::NO_CONTENT)
}

pub fn node_offline() -> ApiError {
    ApiError::conflict("node_offline", "This node is offline. Start it, then try again.")
}
