use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use crate::wellness;
use chrono::{DateTime, Utc};
use mycelium_proto::api::{Group, GroupFolder, GroupFolderRequest, GroupOpening, GroupRequest, Hours, OpenGroupRequest};
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const NAME_LIMIT: usize = 40;

#[derive(sqlx::FromRow)]
pub struct GroupRow {
    pub id: Uuid,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub always_open: bool,
}

#[derive(sqlx::FromRow)]
struct FolderRow {
    id: Uuid,
    group_id: Uuid,
    node_id: Option<Uuid>,
    prefix: String,
}

impl GroupRow {
    async fn to_proto(self, state: &AppState, owner: Uuid, folders: &[FolderRow]) -> ApiResult<Group> {
        let folders = folders.iter().filter(|folder| folder.group_id == self.id).map(|folder| GroupFolder { id: folder.id, node_id: folder.node_id, prefix: folder.prefix.clone() }).collect();
        let hours = wellness::load_hours(state, owner, Some(self.id), self.always_open).await?;
        let app = wellness::app_hours(state, owner).await?;
        let zone = wellness::zone_of(state, owner, None).await?;
        let opened_until = wellness::group_opened_until(state, owner, self.id).await?;
        let status = wellness::status_of(&app, Some(&hours), opened_until, zone, Utc::now());

        Ok(Group { id: self.id, name: self.name, folders, created_at: self.created_at, hours, open_now: status.open_now, closes_at: status.closes_at, opens_at: status.opens_at, opened_until: status.opened_until })
    }
}

async fn folders_of(state: &AppState, owner: Uuid) -> ApiResult<Vec<FolderRow>> {
    Ok(sqlx::query_as::<_, FolderRow>("select f.id, f.group_id, f.node_id, f.prefix from group_folders f join groups g on g.id = f.group_id where g.owner = $1 order by f.prefix, f.created_at").bind(owner).fetch_all(&state.pool).await?)
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<GroupRow> {
    let row = sqlx::query_as::<_, GroupRow>("select id, name, created_at, always_open from groups where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That group"))
}

fn valid_name(request: &GroupRequest) -> ApiResult<String> {
    let name = request.name.trim();

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for the group."));
    }

    if name.len() > NAME_LIMIT || !name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')) {
        return Err(ApiError::bad_request("name_invalid", "Use up to 40 letters, numbers, dots, dashes or underscores in the group name, with no spaces."));
    }

    Ok(name.to_string())
}

fn exists() -> ApiError {
    ApiError::conflict("group_exists", "You already have a group with this name. Choose another name.")
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Group>>> {
    let rows = sqlx::query_as::<_, GroupRow>("select id, name, created_at, always_open from groups where owner = $1 order by lower(name), id").bind(user.id).fetch_all(&state.pool).await?;

    let folders = folders_of(&state, user.id).await?;

    let mut groups = Vec::new();

    for row in rows {
        groups.push(row.to_proto(&state, user.id, &folders).await?);
    }

    Ok(Json(groups))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<GroupRequest>) -> ApiResult<(StatusCode, Json<Group>)> {
    let name = valid_name(&request)?;
    let inserted = sqlx::query_as::<_, GroupRow>("insert into groups (owner, name) values ($1, $2) returning id, name, created_at, always_open").bind(user.id).bind(&name).fetch_one(&state.pool).await;

    match inserted {
        Ok(row) => Ok((StatusCode::CREATED, Json(row.to_proto(&state, user.id, &[]).await?))),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(exists()),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn rename(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<GroupRequest>) -> ApiResult<Json<Group>> {
    let name = valid_name(&request)?;
    let updated = sqlx::query_as::<_, GroupRow>("update groups set name = $3 where id = $1 and owner = $2 returning id, name, created_at, always_open").bind(id).bind(user.id).bind(&name).fetch_optional(&state.pool).await;

    match updated {
        Ok(Some(row)) => Ok(Json(row.to_proto(&state, user.id, &folders_of(&state, user.id).await?).await?)),
        Ok(None) => Err(ApiError::not_found("That group")),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(exists()),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn save_hours(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<Hours>) -> ApiResult<Json<Group>> {
    find_owned(&state, id, user.id).await?;

    let mut tx = state.pool.begin().await?;

    wellness::save_hours(&mut tx, user.id, Some(id), &request).await?;
    sqlx::query("update groups set always_open = $2 where id = $1").bind(id).bind(request.always_open).execute(&mut *tx).await?;
    tx.commit().await?;
    wellness::release_holds(&state, user.id).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    let row = find_owned(&state, id, user.id).await?;

    Ok(Json(row.to_proto(&state, user.id, &folders_of(&state, user.id).await?).await?))
}

pub async fn open(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<OpenGroupRequest>) -> ApiResult<(StatusCode, Json<GroupOpening>)> {
    user.require_full_access()?;

    let row = find_owned(&state, id, user.id).await?;
    let reason = request.reason.trim();

    if reason.is_empty() {
        return Err(ApiError::bad_request("reason_required", format!("Enter why you need to open {}", row.name)));
    }

    if wellness::closure(&state, user.id, Some(id)).await?.is_none() {
        return Err(ApiError::conflict("group_open", "That group is already open."));
    }

    let until = sqlx::query_scalar::<_, DateTime<Utc>>("insert into out_of_hours_opens (owner, group_id, reason, until) values ($1, $2, $3, now() + make_interval(mins => $4)) returning until")
        .bind(user.id)
        .bind(id)
        .bind(reason)
        .bind(wellness::OPENING_MINUTES as i32)
        .fetch_one(&state.pool)
        .await?;

    wellness::release_holds(&state, user.id).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok((StatusCode::CREATED, Json(GroupOpening { opened_until: until })))
}

pub async fn close_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    user.require_full_access()?;
    find_owned(&state, id, user.id).await?;
    sqlx::query("update out_of_hours_opens set closed_at = now() where owner = $1 and group_id = $2 and closed_at is null and until > now()").bind(user.id).bind(id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from groups where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That group"));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_folder(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<GroupFolderRequest>) -> ApiResult<(StatusCode, Json<GroupFolder>)> {
    find_owned(&state, id, user.id).await?;

    let prefix = request.prefix.trim().trim_end_matches('/');

    if !prefix.starts_with('/') || prefix.split('/').skip(1).any(|segment| segment.is_empty() || segment == "." || segment == "..") {
        return Err(ApiError::bad_request("folder_invalid", "Enter the folder as a full path below the root, for example /home/claude/projects."));
    }

    if let Some(node_id) = request.node_id {
        let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(node_id).fetch_one(&state.pool).await?;

        if !exists {
            return Err(ApiError::not_found("That node"));
        }
    }

    let claimed = sqlx::query_scalar::<_, bool>(
        "select exists (select 1 from group_folders f join groups g on g.id = f.group_id
         where g.owner = $1 and f.prefix = $2 and f.node_id is not distinct from $3)",
    )
    .bind(user.id)
    .bind(prefix)
    .bind(request.node_id)
    .fetch_one(&state.pool)
    .await?;

    if claimed {
        return Err(ApiError::conflict("folder_claimed", "A group already claims this folder on this node. Remove it from that group first."));
    }

    let row = sqlx::query_as::<_, FolderRow>("insert into group_folders (group_id, node_id, prefix) values ($1, $2, $3) returning id, group_id, node_id, prefix").bind(id).bind(request.node_id).bind(prefix).fetch_one(&state.pool).await?;

    Ok((StatusCode::CREATED, Json(GroupFolder { id: row.id, node_id: row.node_id, prefix: row.prefix })))
}

pub async fn remove_folder(State(state): State<AppState>, user: AuthUser, Path((id, folder_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    find_owned(&state, id, user.id).await?;

    let deleted = sqlx::query("delete from group_folders where id = $1 and group_id = $2").bind(folder_id).bind(id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That folder"));
    }

    Ok(StatusCode::NO_CONTENT)
}
