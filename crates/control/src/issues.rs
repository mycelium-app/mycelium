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
use mycelium_proto::api::{CreateConversationRequest, ForgeFailure, Issues, StartWorkspaceRequest};
use mycelium_proto::conversation::Conversation;
use serde::Deserialize;
use std::collections::BTreeSet;
use uuid::Uuid;

const SLUG_LIMIT: usize = 40;

#[derive(Deserialize)]
pub struct IssueQuery {
    #[serde(default)]
    q: String,
}

fn slug(title: &str) -> String {
    let words: Vec<String> = title.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect();
    let joined = words.join("-");

    joined.chars().take(SLUG_LIMIT).collect::<String>().trim_end_matches('-').to_string()
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<IssueQuery>) -> ApiResult<Json<Issues>> {
    let repositories: BTreeSet<(Uuid, String)> = sqlx::query_as::<_, (Uuid, String)>("select distinct forge_id, repository from projects where owner = $1 and forge_id is not null and repository is not null")
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .collect();

    let mut result = Issues { issues: Vec::new(), failures: Vec::new() };
    let fetched = join_all(repositories.iter().map(|(forge_id, repository)| async {
        let forge = forges::find_owned(&state, *forge_id, user.id).await?;
        let api = ForgeApi::from_row(&forge)?;
        let issues = api.issues(&state.http, repository, &query.q).await.map_err(|error| error.into_api(api.kind, &api.base_url));

        Ok::<_, ApiError>((*forge_id, issues))
    }))
    .await;

    for outcome in fetched {
        let (forge_id, issues) = outcome?;

        match issues {
            Ok(found) => result.issues.extend(found),
            Err(error) => result.failures.push(ForgeFailure { forge_id, message: error.message().to_string() }),
        }
    }

    Ok(Json(result))
}

pub async fn start_workspace(State(state): State<AppState>, user: AuthUser, Path((project_id, number)): Path<(Uuid, i64)>, Json(request): Json<StartWorkspaceRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let project = projects::find_owned(&state, project_id, user.id).await?;
    let (Some(forge_id), Some(repository)) = (project.forge_id, project.repository.as_deref()) else {
        return Err(ApiError::conflict("project_without_forge", "This project did not come from a forge, so it has no issues. Choose a project cloned from a forge."));
    };

    let forge = forges::find_owned(&state, forge_id, user.id).await?;
    let api = ForgeApi::from_row(&forge)?;
    let found = api.issue(&state.http, repository, number).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;
    let Some(issue) = found else {
        return Err(ApiError::not_found("That issue"));
    };

    let branch = format!("issue/{number}-{}", slug(&issue.title));
    let existing = sqlx::query_scalar::<_, String>("select path from worktrees where project_id = $1 and issue_number = $2").bind(project.id).bind(number).fetch_optional(&state.pool).await?;
    let cwd = match existing {
        Some(path) => path,
        None => worktrees::create(&state, &project, &branch, Some(number)).await?.path,
    };

    let body = issue.body.trim();
    let details = if body.is_empty() { String::new() } else { format!("\n\n{body}") };
    let prompt = format!("Work on issue #{number}: {}\n{}{details}", issue.title, issue.url);
    let start = CreateConversationRequest { node_id: project.node_id, agent: request.agent, cwd, project_id: None, branch: None, title: Some(format!("#{number} {}", issue.title)), prompt: Some(prompt) };
    let conversation = conversations::start(&state, &user, start).await?;

    Ok((StatusCode::CREATED, Json(conversation)))
}
