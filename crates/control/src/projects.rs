use crate::attachments;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges;
use crate::groups;
use crate::git::relay;
use crate::nodes::node_offline;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow};
use axum::Json;
use axum::extract::{Path, State};
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use mycelium_proto::api::{CreateProjectRequest, PatchProjectRequest, Project};
use mycelium_proto::link::GitCommand;
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

pub const PROJECT_COLUMNS: &str = "id, node_id, name, path, forge_id, repository, group_id, icon, avatar_sha256, created_at";
pub const MAX_AVATAR_BYTES: usize = 256 * 1024;
const MAX_ICON_CHARACTERS: usize = 40;
const AVATAR_MEDIA_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];
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
    pub icon: Option<String>,
    pub avatar_sha256: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ProjectRow {
    pub fn to_proto(self) -> Project {
        Project { id: self.id, node_id: self.node_id, name: self.name, path: self.path, forge_id: self.forge_id, repository: self.repository, group_id: self.group_id, icon: self.icon, avatar_sha256: self.avatar_sha256, created_at: self.created_at }
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

fn remote_location(url: &str) -> Option<(String, String)> {
    let (authority, path) = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').unwrap_or((rest, "")),
        None => url.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?.split(':').next()?.to_lowercase();
    let path = path.trim_matches('/').trim_end_matches(".git").to_string();

    (!host.is_empty()).then_some((host, path))
}

fn check_checkout(answer: &Value, registered: &str, base_url: &str, repository: &str) -> ApiResult<()> {
    let choose = format!("Choose the folder that holds {repository}.");

    if answer["repository"] != true {
        return Err(ApiError::bad_request("folder_not_checkout", format!("The folder at {registered} is not a git checkout. {choose}")));
    }

    let forge_host = remote_location(base_url).map(|(host, _)| host).unwrap_or_default();
    let remotes: Vec<(String, String)> = answer["remotes"].as_array().into_iter().flatten().filter_map(|remote| remote.as_str().and_then(remote_location)).collect();

    if remotes.iter().any(|(host, path)| *host == forge_host && path.eq_ignore_ascii_case(repository)) {
        return Ok(());
    }

    match remotes.iter().find(|(host, _)| *host == forge_host) {
        Some((_, other)) => Err(ApiError::bad_request("folder_other_repository", format!("The folder at {registered} is a checkout of {other}, not {repository}. {choose}"))),
        None => Err(ApiError::bad_request("folder_other_repository", format!("The folder at {registered} has no remote on {forge_host}, so Mycelium cannot tell it holds {repository}. {choose}"))),
    }
}

async fn link_conversations(state: &AppState, owner: Uuid, project: &ProjectRow) -> ApiResult<()> {
    let linked = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set project_id = $4 where owner = $1 and node_id = $2 and project_id is null and (cwd = $3 or left(cwd, length($3) + 1) = $3 || '/') returning {CONVERSATION_COLUMNS}",
    )))
    .bind(owner)
    .bind(project.node_id)
    .bind(&project.path)
    .bind(project.id)
    .fetch_all(&state.pool)
    .await?;

    for conversation in linked {
        state.publish(Some(owner), StreamMessage::Conversation(conversation.to_proto()?));
    }

    Ok(())
}

fn valid_icon(icon: &str) -> bool {
    icon.len() <= MAX_ICON_CHARACTERS && icon.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
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
        (Some(forge_id), Some(repository), Some(path)) if !path.is_empty() && !request.new_repository => {
            if !valid_repository(repository) {
                return Err(ApiError::bad_request("repository_invalid", "Enter the repository as owner/name, for example acme-org/acme-backups."));
            }

            let forge = forges::find_owned(&state, forge_id, user.id).await?;
            let answer = relay(&state, request.node_id, path.to_string(), GitCommand::Register).await?;
            let registered = answer["path"].as_str().unwrap_or_default().to_string();

            check_checkout(&answer, &registered, &forge.base_url, repository)?;

            let name = repository.rsplit('/').next().unwrap_or(repository).to_string();

            (name, registered, Some(forge_id), Some(repository.to_string()))
        }
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
        Ok(row) => {
            link_conversations(&state, user.id, &row).await?;

            Ok((StatusCode::CREATED, Json(row.to_proto())))
        }
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(ApiError::conflict("project_exists", "This folder is already a project on this node.")),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchProjectRequest>) -> ApiResult<Json<Project>> {
    let changes_icon = request.icon.is_some() || request.clear_icon;

    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let icon = request.icon.as_deref().map(str::trim).filter(|icon| !request.clear_icon && !icon.is_empty());

    if icon.is_some_and(|icon| !valid_icon(icon)) {
        return Err(ApiError::bad_request("icon_invalid", "Choose an icon from the list."));
    }

    let row = if changes_icon {
        sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set icon = $3, avatar_sha256 = null where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
            .bind(id)
            .bind(user.id)
            .bind(icon)
            .fetch_optional(&state.pool)
            .await?
    } else {
        sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set group_id = $3 where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
            .bind(id)
            .bind(user.id)
            .bind(request.group_id)
            .fetch_optional(&state.pool)
            .await?
    };

    let Some(row) = row else {
        return Err(ApiError::not_found("That project"));
    };

    publish_conversations(&state, user.id, id).await?;

    Ok(Json(row.to_proto()))
}

pub async fn upload_avatar(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, headers: HeaderMap, body: Bytes) -> ApiResult<Json<Project>> {
    find_owned(&state, id, user.id).await?;

    let media_type = attachments::media_type_of(&headers);

    if !AVATAR_MEDIA_TYPES.contains(&media_type.as_str()) {
        return Err(ApiError::bad_request("avatar_not_image", "The logo must be a PNG, JPEG or WebP image. Choose another file."));
    }

    if body.is_empty() {
        return Err(ApiError::bad_request("avatar_empty", "The file is empty. Choose an image that has content."));
    }

    if body.len() > MAX_AVATAR_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "avatar_too_large", "The logo is larger than 256 KB. Choose a smaller image."));
    }

    let (sha256, _, _) = attachments::store(&state, user.id, &body, &media_type).await?;
    let row = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set avatar_sha256 = $3, icon = null where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
        .bind(id)
        .bind(user.id)
        .bind(&sha256)
        .fetch_one(&state.pool)
        .await?;

    publish_conversations(&state, user.id, id).await?;

    Ok(Json(row.to_proto()))
}

async fn publish_conversations(state: &AppState, owner: Uuid, project_id: Uuid) -> ApiResult<()> {
    let conversations = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where project_id = $1 and owner = $2")))
        .bind(project_id)
        .bind(owner)
        .fetch_all(&state.pool)
        .await?;

    for conversation in conversations {
        state.publish(Some(owner), StreamMessage::Conversation(conversation.to_proto()?));
    }

    Ok(())
}
