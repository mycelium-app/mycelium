use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges;
use crate::groups;
use crate::git::relay;
use crate::nodes::node_offline;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateProjectRequest, PatchProjectRequest, Project};
use mycelium_proto::link::GitCommand;
use uuid::Uuid;

pub const PROJECT_COLUMNS: &str = "id, node_id, name, path, forge_id, repository, group_id, created_at";
const PROJECTS_FOLDER: &str = "projects";

#[derive(Clone, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub node_id: Uuid,
    pub name: String,
    pub path: String,
    pub forge_id: Option<Uuid>,
    pub repository: Option<String>,
    pub group_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ProjectRow {
    pub fn to_proto(self) -> Project {
        Project { id: self.id, node_id: self.node_id, name: self.name, path: self.path, forge_id: self.forge_id, repository: self.repository, group_id: self.group_id, created_at: self.created_at }
    }
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProjectRow> {
    let row = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("select {PROJECT_COLUMNS} from projects where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That project"))
}

pub async fn clone_repository(state: &AppState, forge: &forges::ForgeRow, repository: &str, node_id: Uuid) -> ApiResult<String> {
    let credentials = forge.git_credentials()?;
    let url = format!("{}/{repository}.git", forge.base_url);
    let answer = relay(state, node_id, format!("{PROJECTS_FOLDER}/{repository}"), GitCommand::Clone { url, credentials: Some(credentials) }).await?;

    Ok(answer["path"].as_str().unwrap_or_default().to_string())
}

pub fn valid_repository(repository: &str) -> bool {
    let segments: Vec<&str> = repository.split('/').collect();

    segments.len() >= 2 && segments.iter().all(|segment| !segment.is_empty() && *segment != "." && *segment != ".." && segment.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Project>>> {
    let rows = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("select {PROJECT_COLUMNS} from projects where owner = $1 order by created_at, id"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(ProjectRow::to_proto).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateProjectRequest>) -> ApiResult<(StatusCode, Json<Project>)> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(request.node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    if !state.hub.is_online(request.node_id) {
        return Err(node_offline());
    }

    let (name, path, forge_id, repository) = match (request.forge_id, request.repository.as_deref().map(str::trim), request.path.as_deref().map(str::trim)) {
        (Some(forge_id), Some(repository), None) => {
            if !valid_repository(repository) {
                return Err(ApiError::bad_request("repository_invalid", "Enter the repository as owner/name, for example acme-org/acme-backups."));
            }

            let forge = forges::find_owned(&state, forge_id, user.id).await?;

            if request.new_repository {
                let api = forges::ForgeApi::from_row(&forge)?.cached(&state.forge_cache);

                api.create_repository(&state.http, &forge.account, repository).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;
            }

            let path = clone_repository(&state, &forge, repository, request.node_id).await?;
            let name = repository.rsplit('/').next().unwrap_or(repository).to_string();

            (name, path, Some(forge_id), Some(repository.to_string()))
        }
        (None, None, Some(path)) if !path.is_empty() => {
            let answer = relay(&state, request.node_id, path.to_string(), GitCommand::Register).await?;
            let registered = answer["path"].as_str().unwrap_or_default().to_string();
            let name = registered.rsplit('/').next().unwrap_or(&registered).to_string();

            (name, registered, None, None)
        }
        _ => return Err(ApiError::bad_request("project_source_required", "Choose a forge repository or enter the folder on the node.")),
    };

    let inserted = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!(
        "insert into projects (owner, node_id, name, path, forge_id, repository) values ($1, $2, $3, $4, $5, $6) returning {PROJECT_COLUMNS}",
    )))
    .bind(user.id)
    .bind(request.node_id)
    .bind(&name)
    .bind(&path)
    .bind(forge_id)
    .bind(&repository)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok(row) => Ok((StatusCode::CREATED, Json(row.to_proto()))),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(ApiError::conflict("project_exists", "This folder is already a project on this node.")),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchProjectRequest>) -> ApiResult<Json<Project>> {
    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let row = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set group_id = $3 where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
        .bind(id)
        .bind(user.id)
        .bind(request.group_id)
        .fetch_optional(&state.pool)
        .await?;

    row.map(|row| Json(row.to_proto())).ok_or_else(|| ApiError::not_found("That project"))
}
