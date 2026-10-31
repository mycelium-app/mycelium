use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, ProcessAnswer};
use crate::store::{node_summaries, publish_node};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{DrainRequest, LoginInputRequest, NodeResources, NodeSummary, NODE_COLOURS, NODE_ICONS, StartLoginRequest, UpdateNodeRequest, StartLoginResponse};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::link::{ControlToNode, GitCommand};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const WATCH_LEASE_SECONDS: u32 = 30;
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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

pub async fn update(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<UpdateNodeRequest>) -> ApiResult<StatusCode> {
    let name = request.name.as_deref().map(str::trim);

    if name == Some("") {
        return Err(ApiError::bad_request("name_required", "Enter a name for this node."));
    }

    if let Some(icon) = request.icon.as_deref().filter(|icon| !NODE_ICONS.contains(icon)) {
        return Err(ApiError::bad_request("unknown_icon", format!("{icon} is not a node icon. Choose one of {}.", NODE_ICONS.join(", "))));
    }

    if let Some(colour) = request.colour.as_deref().filter(|colour| !NODE_COLOURS.contains(colour)) {
        return Err(ApiError::bad_request("unknown_colour", format!("{colour} is not a node colour. Choose one of {}.", NODE_COLOURS.join(", "))));
    }

    let updated = sqlx::query("update nodes set name = coalesce($2, name), icon = coalesce($3, icon), colour = coalesce($4, colour) where id = $1 and revoked_at is null")
        .bind(id)
        .bind(name)
        .bind(&request.icon)
        .bind(&request.colour)
        .execute(&state.pool)
        .await;

    match updated {
        Ok(done) if done.rows_affected() == 0 => return Err(ApiError::not_found("That node")),
        Ok(_) => {}
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => {
            return Err(ApiError::conflict("name_taken", format!("Another node is already called {}. Choose a different name.", name.unwrap_or_default())));
        }
        Err(cause) => return Err(cause.into()),
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
    state.finish_logins_of_node(id);
    state.finish_terminals_of_node(id, &HashSet::new());
    state.publish(None, StreamMessage::NodeRemoved { node_id: id });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn watch(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<NodeResources>> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    if !state.hub.is_online(id) || !state.hub.send(id, ControlToNode::Watch { seconds: WATCH_LEASE_SECONDS }) {
        return Err(node_offline());
    }

    Ok(Json(state.hub.resources_of(id)))
}

pub async fn stop_process(State(state): State<AppState>, _user: AuthUser, Path((id, pid)): Path<(Uuid, u32)>) -> ApiResult<StatusCode> {
    let process = state.hub.resources_of(id).latest.and_then(|sample| sample.processes.into_iter().find(|process| process.pid == pid));
    let Some(process) = process else {
        return Err(ApiError::not_found("That process"));
    };

    if !process.stoppable {
        return Err(ApiError::conflict("process_not_stoppable", format!("{} does not belong to a Mycelium conversation or to the node user, so Mycelium cannot stop it.", process.name)));
    }


    if !state.hub.is_online(id) {
        return Err(node_offline());
    }

    let asked = state.ask_node::<ProcessAnswer>(id, STOP_TIMEOUT, |request_id| ControlToNode::StopProcess { request_id, pid, name: process.name }).await;

    let Some(answer) = asked else {
        return Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "stop_timeout", "The node did not answer in time. Try again in a moment."));
    };

    if !answer.ok {
        return Err(ApiError::conflict("stop_failed", answer.error.unwrap_or_else(|| "The node could not stop that process.".into())));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_login(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<StartLoginRequest>) -> ApiResult<Json<StartLoginResponse>> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    let login_id = Uuid::new_v4();
    state.hub.track_login(login_id, id, user.id);

    let sent = state.hub.send(id, ControlToNode::StartLogin { login_id, agent: request.agent, method: request.method, account: request.account.map(|account| account.trim().to_string()).filter(|account| !account.is_empty()) });

    if !sent {
        state.hub.finish_login(login_id);

        return Err(node_offline());
    }

    Ok(Json(StartLoginResponse { login_id }))
}

pub async fn login_input(State(state): State<AppState>, user: AuthUser, Path(login_id): Path<Uuid>, Json(request): Json<LoginInputRequest>) -> ApiResult<StatusCode> {
    let owner = state.hub.login_owner(login_id).filter(|owner| owner.user_id == user.id).ok_or_else(|| ApiError::not_found("That sign-in"))?;

    if !state.hub.send(owner.node_id, ControlToNode::LoginInput { login_id, text: request.text }) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop_login(State(state): State<AppState>, user: AuthUser, Path(login_id): Path<Uuid>) -> ApiResult<StatusCode> {
    let owner = state.hub.login_owner(login_id).filter(|owner| owner.user_id == user.id).ok_or_else(|| ApiError::not_found("That sign-in"))?;

    state.hub.send(owner.node_id, ControlToNode::StopLogin { login_id });
    state.hub.finish_login(login_id);
    state.publish(Some(owner.user_id), StreamMessage::LoginFinished { login_id, ok: false });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_login(State(state): State<AppState>, user: AuthUser, Path((id, agent, account)): Path<(Uuid, String, String)>) -> ApiResult<StatusCode> {
    let parsed: AgentId = crate::store::parse_enum(&agent).map_err(|_| ApiError::not_found("That login"))?;
    let managed = sqlx::query_scalar::<_, bool>(
        "select l.managed from logins l join nodes n on n.id = l.node_id where l.node_id = $1 and n.created_by = $2 and n.revoked_at is null and l.agent = $3 and l.account = $4",
    )
    .bind(id)
    .bind(user.id)
    .bind(&agent)
    .bind(&account)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That login"))?;

    if !managed {
        return Err(ApiError::conflict("login_not_managed", "This is the agent's own sign-in on the node, so Mycelium cannot remove it. Sign it out on the node itself."));
    }

    if !state.hub.send(id, ControlToNode::RemoveLogin { agent: parsed, account: account.clone() }) {
        return Err(node_offline());
    }

    sqlx::query("delete from logins where node_id = $1 and agent = $2 and account = $3").bind(id).bind(&agent).bind(&account).execute(&state.pool).await?;
    publish_node(&state, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct DirectoriesQuery {
    #[serde(default)]
    path: String,
}

pub async fn directories(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<DirectoriesQuery>) -> ApiResult<Json<Value>> {
    let version = sqlx::query_scalar::<_, String>("select version from nodes where id = $1 and revoked_at is null").bind(id).fetch_optional(&state.pool).await?;
    let Some(version) = version else {
        return Err(ApiError::not_found("That node"));
    };

    if !state.hub.is_online(id) {
        return Err(node_offline());
    }

    if !node_has(&version, HEAD_AND_DIRECTORIES_SINCE) {
        let name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(id).fetch_one(&state.pool).await?;

        return Err(ApiError::conflict("node_outdated", format!("Update {name} to browse its folders, or enter the path.")));
    }

    let answer = crate::git::relay(&state, id, query.path, GitCommand::Directories).await?;

    Ok(Json(answer))
}

pub fn node_has(node_version: &str, since: (u64, u64, u64)) -> bool {
    semver::Version::parse(node_version.trim_start_matches('v')).is_ok_and(|version| version >= semver::Version::new(since.0, since.1, since.2))
}

pub const HEAD_AND_DIRECTORIES_SINCE: (u64, u64, u64) = (0, 10, 0);

pub const STEERING_SINCE: (u64, u64, u64) = (0, 10, 1);

pub fn node_offline() -> ApiError {
    ApiError::conflict("node_offline", "This node is offline. Start it, then try again.")
}
