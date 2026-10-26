use crate::assistants;
use crate::attachments::hex_digest;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::{self, ForgeApi, ForgeError};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use mycelium_proto::api::{Assistant, AssistantBackup, SetAssistantBackupRequest};
use sqlx::types::Json as Jsonb;
use uuid::Uuid;

const GITIGNORE_PATH: &str = ".gitignore";

async fn stored_backup(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Option<AssistantBackup>> {
    let backup = sqlx::query_scalar::<_, Option<Jsonb<AssistantBackup>>>("select backup from assistants where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?;

    match backup {
        Some(backup) => Ok(backup.map(|backup| backup.0)),
        None => Err(ApiError::not_found("That assistant")),
    }
}

async fn save_backup(state: &AppState, id: Uuid, backup: Option<&AssistantBackup>) -> ApiResult<()> {
    sqlx::query("update assistants set backup = $2, updated_at = now() where id = $1").bind(id).bind(backup.map(Jsonb)).execute(&state.pool).await?;

    Ok(())
}

async fn ignored_by(state: &AppState, id: Uuid) -> ApiResult<Gitignore> {
    let content = sqlx::query_scalar::<_, Vec<u8>>("select content from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(GITIGNORE_PATH).fetch_optional(&state.pool).await?;
    let mut builder = GitignoreBuilder::new("");

    for line in String::from_utf8_lossy(&content.unwrap_or_default()).lines() {
        builder.add_line(None, line).ok();
    }

    Ok(builder.build().unwrap_or_else(|_| Gitignore::empty()))
}

fn backed_up(ignored: &Gitignore, path: &str) -> bool {
    !ignored.matched_path_or_any_parents(path, false).is_ignore()
}

async fn api_of(state: &AppState, owner: Uuid, backup: &AssistantBackup) -> ApiResult<ForgeApi> {
    let forge = forges::find_owned(state, backup.forge_id, owner).await?;

    ForgeApi::from_row(&forge)
}

fn explain(api: &ForgeApi, error: ForgeError) -> String {
    error.into_api(api.kind, &api.base_url).message().to_string()
}

async fn branch_of(state: &AppState, api: &ForgeApi, backup: &AssistantBackup) -> Result<String, ForgeError> {
    match &backup.branch {
        Some(branch) => Ok(branch.clone()),
        None => api.default_branch(&state.http, &backup.repository).await,
    }
}

pub async fn set(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SetAssistantBackupRequest>) -> ApiResult<Json<Assistant>> {
    stored_backup(&state, user.id, id).await?;

    let repository = request.repository.trim().to_string();

    if repository.is_empty() {
        return Err(ApiError::bad_request("repository_required", "Choose the repository to back this assistant up to."));
    }

    let backup = AssistantBackup { forge_id: request.forge_id, repository, branch: request.branch.map(|branch| branch.trim().to_string()).filter(|branch| !branch.is_empty()), last_at: None, last_commit: None, error: None };
    let api = api_of(&state, user.id, &backup).await?;
    let branch = branch_of(&state, &api, &backup).await.map_err(|error| explain_as_api(&api, error))?;

    api.tree(&state.http, &backup.repository, &branch).await.map_err(|error| explain_as_api(&api, error))?;
    save_backup(&state, id, Some(&backup)).await?;
    trigger(&state, user.id, id);

    Ok(Json(assistants::publish(&state, user.id, id).await?))
}

fn explain_as_api(api: &ForgeApi, error: ForgeError) -> ApiError {
    error.into_api(api.kind, &api.base_url)
}

pub async fn clear(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    stored_backup(&state, user.id, id).await?;
    save_backup(&state, id, None).await?;
    sqlx::query("update assistant_files set backed_up_sha256 = null where assistant_id = $1").bind(id).execute(&state.pool).await?;
    assistants::publish(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub fn trigger(state: &AppState, owner: Uuid, id: Uuid) {
    let state = state.clone();

    tokio::spawn(async move {
        if let Err(error) = back_up(&state, owner, id).await {
            tracing::warn!("could not back up assistant {id}: {}", error.message());
        }
    });
}

async fn back_up(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let Some(mut backup) = stored_backup(state, owner, id).await? else {
        return Ok(());
    };

    if !state.hub.begin_backup(id) {
        return Ok(());
    }

    let outcome = push(state, owner, id, &backup).await;

    state.hub.end_backup(id);

    match outcome {
        Ok(Some(commit)) => {
            backup.last_at = Some(chrono::Utc::now());
            backup.last_commit = Some(commit);
            backup.error = None;
        }
        Ok(None) => backup.error = None,
        Err(message) => backup.error = Some(message),
    }

    if stored_backup(state, owner, id).await?.is_some() {
        save_backup(state, id, Some(&backup)).await?;
        assistants::publish(state, owner, id).await?;
    }

    Ok(())
}

async fn push(state: &AppState, owner: Uuid, id: Uuid, backup: &AssistantBackup) -> Result<Option<String>, String> {
    let api = api_of(state, owner, backup).await.map_err(|error| error.message().to_string())?;
    let failed = |error: ForgeError| explain(&api, error);
    let branch = branch_of(state, &api, backup).await.map_err(failed)?;
    let remote = api.tree(&state.http, &backup.repository, &branch).await.map_err(failed)?;
    let ignored = ignored_by(state, id).await.map_err(|error| error.message().to_string())?;
    let database = |error: sqlx::Error| error.to_string();
    let files = sqlx::query_as::<_, (String, Vec<u8>, String, Option<String>)>("select path, content, sha256, backed_up_sha256 from assistant_files where assistant_id = $1 order by path").bind(id).fetch_all(&state.pool).await.map_err(database)?;
    let mut last = None;

    for (path, content, digest, done) in &files {
        if done.as_deref() == Some(digest) || !backed_up(&ignored, path) {
            continue;
        }

        let existing = remote.iter().find(|entry| entry.path == *path).map(|entry| entry.blob.as_str());
        let commit = api.commit_blob(&state.http, &backup.repository, &branch, path, content, &format!("chore: update {path}"), existing).await.map_err(failed)?;

        sqlx::query("update assistant_files set backed_up_sha256 = $3 where assistant_id = $1 and path = $2").bind(id).bind(path).bind(digest).execute(&state.pool).await.map_err(database)?;
        last = Some(commit);
    }

    for entry in remote.iter().filter(|entry| backed_up(&ignored, &entry.path) && !files.iter().any(|(path, ..)| *path == entry.path)) {
        api.remove_blob(&state.http, &backup.repository, &branch, &entry.path, &format!("chore: remove {}", entry.path), &entry.blob).await.map_err(failed)?;
        last = Some(branch.clone());
    }

    Ok(last)
}

pub async fn pull(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let Some(backup) = stored_backup(&state, user.id, id).await? else {
        return Err(ApiError::conflict("backup_not_set", "This assistant has no backup repository. Choose one first."));
    };
    let api = api_of(&state, user.id, &backup).await?;
    let branch = branch_of(&state, &api, &backup).await.map_err(|error| explain_as_api(&api, error))?;
    let remote = api.tree(&state.http, &backup.repository, &branch).await.map_err(|error| explain_as_api(&api, error))?;
    let ignored = ignored_by(&state, id).await?;

    for entry in remote.iter().filter(|entry| backed_up(&ignored, &entry.path) && assistants::valid_path(&entry.path).is_ok()) {
        let content = api.blob(&state.http, &backup.repository, &branch, &entry.path).await.map_err(|error| explain_as_api(&api, error))?;

        assistants::store_file(&state, id, &entry.path, &content).await?;
        sqlx::query("update assistant_files set backed_up_sha256 = $3 where assistant_id = $1 and path = $2").bind(id).bind(&entry.path).bind(hex_digest(&content)).execute(&state.pool).await?;
    }

    let stored = sqlx::query_scalar::<_, String>("select path from assistant_files where assistant_id = $1").bind(id).fetch_all(&state.pool).await?;

    for path in stored.iter().filter(|path| backed_up(&ignored, path) && !remote.iter().any(|entry| entry.path == **path)) {
        sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(path).execute(&state.pool).await?;
    }

    Ok(Json(assistants::publish(&state, user.id, id).await?))
}
