use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::forges::{self, ForgeApi};
use crate::projects;
use crate::state::AppState;
use crate::worktrees;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::future::join_all;
use mycelium_proto::api::{CreateConversationRequest, ForgeFailure, PullRequestDetail, PullRequestDiff, PullRequestItem, PullRequests, StartWorkspaceRequest};
use mycelium_proto::conversation::Conversation;
use serde::Deserialize;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct PullRequestQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize)]
pub struct RepositoryParameter {
    repository: String,
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<PullRequestQuery>) -> ApiResult<Json<PullRequests>> {
    let repositories: BTreeSet<(Uuid, String)> = sqlx::query_as::<_, (Uuid, String)>("select distinct forge_id, repository from projects where owner = $1 and forge_id is not null and repository is not null")
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .collect();
    let needle = query.q.trim().to_lowercase();

    let fetched = join_all(repositories.iter().map(|(forge_id, repository)| async {
        let forge = forges::find_owned(&state, *forge_id, user.id).await?;
        let api = ForgeApi::from_row(&forge)?.cached(&state.forge_cache).refreshing(query.refresh);
        let found = api.pull_requests(&state.http, repository).await.map_err(|error| error.into_api(api.kind, &api.base_url));

        Ok::<_, ApiError>((*forge_id, found))
    }))
    .await;

    let mut result = PullRequests { pull_requests: Vec::new(), failures: Vec::new() };

    for outcome in fetched {
        let (forge_id, found) = outcome?;

        match found {
            Ok(items) => result.pull_requests.extend(items.into_iter().filter(|item| needle.is_empty() || item.title.to_lowercase().contains(&needle) || item.number.to_string() == needle.trim_start_matches('#'))),
            Err(error) => result.failures.push(ForgeFailure { forge_id, message: error.message().to_string() }),
        }
    }

    result.pull_requests.sort_by(|first, second| second.updated_at.cmp(&first.updated_at));

    Ok(Json(result))
}

async fn forge_of(state: &AppState, owner: Uuid, forge_id: Uuid) -> ApiResult<ForgeApi> {
    let forge = forges::find_owned(state, forge_id, owner).await?;

    Ok(ForgeApi::from_row(&forge)?.cached(&state.forge_cache))
}

async fn found(api: &ForgeApi, state: &AppState, repository: &str, number: i64) -> ApiResult<PullRequestItem> {
    let pull_request = api.pull_request(&state.http, repository, number).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;

    pull_request.ok_or_else(|| ApiError::not_found("That pull request"))
}

pub async fn detail(State(state): State<AppState>, user: AuthUser, Path((forge_id, number)): Path<(Uuid, i64)>, Query(query): Query<RepositoryParameter>) -> ApiResult<Json<PullRequestDetail>> {
    let api = forge_of(&state, user.id, forge_id).await?;
    let pull_request = found(&api, &state, &query.repository, number).await?;
    let timeline = api.pull_request_timeline(&state.http, &query.repository, number).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;

    Ok(Json(PullRequestDetail { pull_request, timeline }))
}

pub async fn diff(State(state): State<AppState>, user: AuthUser, Path((forge_id, number)): Path<(Uuid, i64)>, Query(query): Query<RepositoryParameter>) -> ApiResult<Json<PullRequestDiff>> {
    let api = forge_of(&state, user.id, forge_id).await?;
    let diff = api.pull_request_diff(&state.http, &query.repository, number).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;

    Ok(Json(PullRequestDiff { diff }))
}

pub async fn start_workspace(State(state): State<AppState>, user: AuthUser, Path((project_id, number)): Path<(Uuid, i64)>, Json(request): Json<StartWorkspaceRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let project = projects::find_owned(&state, project_id, user.id).await?;
    let (Some(forge_id), Some(repository)) = (project.forge_id, project.repository.as_deref()) else {
        return Err(ApiError::conflict("project_without_forge", "This project did not come from a forge, so it has no pull requests. Choose a project cloned from a forge."));
    };

    let api = forge_of(&state, user.id, forge_id).await?;
    let pull_request = found(&api, &state, repository, number).await?;
    let existing = sqlx::query_scalar::<_, String>("select path from worktrees where project_id = $1 and branch = $2").bind(project.id).bind(&pull_request.source_branch).fetch_optional(&state.pool).await?;
    let cwd = match existing {
        Some(path) => path,
        None => worktrees::create_from_remote_branch(&state, &project, &pull_request.source_branch).await?.path,
    };

    let body = pull_request.body.trim();
    let details = if body.is_empty() { String::new() } else { format!("\n\n{body}") };
    let prompt = format!("Work on {}: {}\n{}{details}", pull_request.label, pull_request.title, pull_request.url);
    let start = CreateConversationRequest { account: None, node_id: project.node_id, agent: request.agent, cwd, project_id: None, branch: None, title: Some(format!("{} {}", pull_request.label, pull_request.title)), prompt: Some(prompt), parent_id: None, side_kind: None, model: None, routing: None };
    let conversation = conversations::start(&state, user.id, start).await?;

    Ok((StatusCode::CREATED, Json(conversation)))
}
