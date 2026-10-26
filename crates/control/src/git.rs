use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::{self, ForgeApi};
use crate::nodes::node_offline;
use crate::projects;
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::link::{CommitOptions, GitCommand, RowRange};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const TOWN_TIMEOUT: Duration = Duration::from_secs(300);
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_LOG_LIMIT: u32 = 50;
const DEFAULT_GRAPH_LIMIT: u32 = 200;
const MOST_LOG_ENTRIES: u32 = 200;
const MOST_ROW_RANGES: usize = 64;

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
pub struct RowsRequest {
    ranges: Vec<RowRange>,
}

#[derive(Deserialize)]
pub struct PathsRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct HunkRequest {
    path: String,
    row: u32,
    fingerprint: String,
    action: String,
}

#[derive(Deserialize)]
pub struct FileRequest {
    path: String,
    content: Option<String>,
    expected: Option<String>,
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

#[derive(Deserialize)]
pub struct PullRequest {
    #[serde(default)]
    rebase: bool,
}

#[derive(Deserialize)]
pub struct PushRequest {
    #[serde(default)]
    force: bool,
}

pub async fn relay(state: &AppState, node_id: Uuid, cwd: String, command: GitCommand) -> ApiResult<Value> {
    let timeout = match command {
        GitCommand::Remote { .. } | GitCommand::Fetch | GitCommand::Pull { .. } | GitCommand::Push { .. } | GitCommand::Snapshot { .. } | GitCommand::Apply { .. } | GitCommand::Undo { .. } => FETCH_TIMEOUT,
        GitCommand::TownSync | GitCommand::TownPropose => TOWN_TIMEOUT,
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

pub async fn stack(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let mut answer = relay(&state, row.node_id, row.cwd, GitCommand::Stack).await?;

    attach_pull_requests(&state, &user, row.project_id, &mut answer).await;

    Ok(Json(answer))
}

async fn attach_pull_requests(state: &AppState, user: &AuthUser, project_id: Option<Uuid>, answer: &mut Value) {
    let found = pull_requests_of(state, user.id, project_id).await;
    let available = found.is_ok();
    let requests = found.as_ref().ok();

    for row in answer["branches"].as_array_mut().into_iter().flatten() {
        let request = requests.and_then(|requests| row["name"].as_str().and_then(|name| requests.get(name)));

        row["pull_request"] = request.map(|request| json!(request)).unwrap_or(Value::Null);
    }

    answer["pull_requests_available"] = json!(available);

    if let Err(Some(problem)) = found {
        answer["pull_requests_problem"] = json!(problem);
    }
}

pub(crate) async fn pull_requests_of(state: &AppState, owner: Uuid, project_id: Option<Uuid>) -> Result<std::collections::HashMap<String, forges::BranchPullRequest>, Option<String>> {
    let project_id = project_id.ok_or(None)?;
    let project = projects::find_owned(state, project_id, owner).await.map_err(|_| None)?;
    let (Some(forge_id), Some(repository)) = (project.forge_id, project.repository.as_deref()) else {
        return Err(None);
    };
    let forge = forges::find_owned(state, forge_id, owner).await.map_err(|_| None)?;
    let api = ForgeApi::from_row(&forge).map_err(|_| None)?.cached(&state.forge_cache);

    api.branch_pull_requests(&state.http, repository).await.map_err(|error| Some(error.into_api(api.kind, &api.base_url).message().to_string()))
}

pub async fn switch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SwitchRequest>) -> ApiResult<Json<Value>> {
    let branch = request.branch.trim().to_string();

    if branch.is_empty() {
        return Err(ApiError::bad_request("branch_required", "Enter a branch name."));
    }

    let switched = run(&state, &user, id, GitCommand::Switch { branch, create: request.create, track: request.track }).await?;

    crate::conversations::spawn_branch_refresh(&state, id);

    Ok(switched)
}

pub async fn diff(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<DiffQuery>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Diff { path: query.path, staged: query.staged }).await
}

pub async fn changes_layout(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::ChangesLayout).await
}

pub async fn changes_rows(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<RowsRequest>) -> ApiResult<Json<Value>> {
    if request.ranges.len() > MOST_ROW_RANGES {
        return Err(ApiError::bad_request("too_many_ranges", "Ask for fewer ranges at a time."));
    }

    run(&state, &user, id, GitCommand::ChangesRows { ranges: request.ranges }).await
}

pub async fn stage(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PathsRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Stage { paths: request.paths }).await
}

pub async fn unstage(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PathsRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Unstage { paths: request.paths }).await
}

pub async fn hunk(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<HunkRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Hunk { path: request.path, row: request.row, fingerprint: request.fingerprint, action: request.action }).await
}

pub async fn file(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<FileRequest>) -> ApiResult<Json<Value>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let saved = request.content.clone();
    let before = request.expected.clone().unwrap_or_default();
    let path = request.path.clone();
    let answer = relay(&state, row.node_id, row.cwd.clone(), GitCommand::File { path: request.path, content: request.content, expected: request.expected }).await?;

    if let Some(saved) = saved {
        if let Err(cause) = crate::skills::count_own(&state.pool, user.id, &row.cwd, &path, &saved, &before).await {
            tracing::warn!("counting skill lines for {id} failed: {cause:?}");
        }
    }

    Ok(Json(answer))
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

async fn run_remote(state: &AppState, user: &AuthUser, id: Uuid, action: &str, force: bool) -> ApiResult<Json<Value>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let command = if let Some(project_id) = row.project_id {
        let project = projects::find_owned(state, project_id, user.id).await?;

        if let (Some(forge_id), Some(repository)) = (project.forge_id, project.repository.as_deref()) {
            let forge = forges::find_owned(state, forge_id, user.id).await?;
            let credentials = forge.git_credentials()?;
            let url = format!("{}/{repository}.git", forge.base_url);

            let mut allowed_urls = ForgeApi::from_row(&forge)?.clone_urls(&state.http, repository).await;
            allowed_urls.push(url.clone());

            GitCommand::Remote { action: action.into(), force, url, allowed_urls, credentials }
        } else if action == "fetch" {
            GitCommand::Fetch
        } else {
            GitCommand::Push { force }
        }
    } else if action == "fetch" {
        GitCommand::Fetch
    } else {
        GitCommand::Push { force }
    };

    Ok(Json(relay(state, row.node_id, row.cwd, command).await?))
}

pub async fn fetch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run_remote(&state, &user, id, "fetch", false).await
}

pub async fn pull(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PullRequest>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::Pull { rebase: request.rebase }).await
}

pub async fn push(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PushRequest>) -> ApiResult<Json<Value>> {
    run_remote(&state, &user, id, "push", request.force).await
}

pub async fn undo(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::UndoLastCommit).await
}

pub async fn town_sync(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::TownSync).await
}

pub async fn town_propose(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    run(&state, &user, id, GitCommand::TownPropose).await
}
