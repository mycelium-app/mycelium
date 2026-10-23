use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::link::{CommitOptions, GitCommand};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_LOG_LIMIT: u32 = 50;
const DEFAULT_GRAPH_LIMIT: u32 = 200;
const MOST_LOG_ENTRIES: u32 = 200;

#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct GraphQuery {
    limit: Option<u32>,
    skip: Option<u32>,
}

#[derive(Deserialize)]
pub struct ShowQuery {
    hash: String,
}

#[derive(Deserialize)]
pub struct SwitchRequest {
    branch: String,
    #[serde(default)]
    create: bool,
    track: Option<String>,
}

#[derive(Deserialize)]
pub struct DiffQuery {
    path: String,
    #[serde(default)]
    staged: bool,
}

#[derive(Deserialize)]
pub struct PathsRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct CommitRequest {
    message: String,
    #[serde(default)]
    amend: bool,
    #[serde(default)]
    signoff: bool,
    #[serde(default)]
    no_verify: bool,
}

pub async fn relay(state: &AppState, node_id: Uuid, cwd: String, command: GitCommand) -> ApiResult<Value> {
    let timeout = match command {
        GitCommand::Fetch | GitCommand::Snapshot { .. } | GitCommand::Apply { .. } | GitCommand::Undo { .. } => FETCH_TIMEOUT,
        GitCommand::Clone { .. } => CLONE_TIMEOUT,
        _ => GIT_TIMEOUT,
    };
    let Some((request_id, answered)) = state.hub.request_git(node_id, cwd, command) else {
        return Err(node_offline());
    };

    let answer = tokio::time::timeout(timeout, answered).await.ok().and_then(Result::ok);
    state.hub.forget_git_request(request_id);

    let Some(answer) = answer else {
        return Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "git_timeout", "The node did not answer in time. Try again in a moment."));
    };

    if !answer.ok {
        return Err(ApiError::conflict("git_failed", answer.error.unwrap_or_else(|| "Git failed without saying why.".into())));
    }

    Ok(answer.output)
}

async fn run(state: &AppState, user: &AuthUser, id: Uuid, command: GitCommand) -> ApiResult<Json<Value>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    Ok(Json(relay(state, row.node_id, row.cwd, command).await?))
}

pub async fn status(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Status).await
}

pub async fn log(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<LogQuery>) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, MOST_LOG_ENTRIES);

    run(&state, &user, id, GitCommand::Log { limit }).await
}

pub async fn graph(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<GraphQuery>) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(DEFAULT_GRAPH_LIMIT).clamp(1, MOST_LOG_ENTRIES);

    run(&state, &user, id, GitCommand::Graph { limit, skip: query.skip.unwrap_or(0) }).await
}

pub async fn show(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<ShowQuery>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Show { hash: query.hash }).await
}

pub async fn branches(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Branches).await
}

pub async fn switch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SwitchRequest>) -> ApiResult<Json<Value>> {
    let branch = request.branch.trim().to_string();

    if branch.is_empty() {
        return Err(ApiError::bad_request("branch_required", "Enter a branch name."));
    }

    run(&state, &user, id, GitCommand::Switch { branch, create: request.create, track: request.track }).await
}

pub async fn diff(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<DiffQuery>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Diff { path: query.path, staged: query.staged }).await
}

pub async fn stage(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PathsRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Stage { paths: request.paths }).await
}

pub async fn unstage(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PathsRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Unstage { paths: request.paths }).await
}

pub async fn stage_all(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::StageAll).await
}

pub async fn commit(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<CommitRequest>) -> ApiResult<Json<Value>> {
    let message = request.message.trim().to_string();

    if message.is_empty() && !request.amend {
        return Err(ApiError::bad_request("message_required", "Enter a commit message."));
    }

    let options = CommitOptions { amend: request.amend, signoff: request.signoff, no_verify: request.no_verify };

    run(&state, &user, id, GitCommand::Commit { message, options }).await
}

pub async fn fetch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Fetch).await
}

pub async fn undo(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::UndoLastCommit).await
}
