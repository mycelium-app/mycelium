use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Group, GroupRequest};
use uuid::Uuid;

const NAME_LIMIT: usize = 40;

#[derive(sqlx::FromRow)]
pub struct GroupRow {
    pub id: Uuid,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl GroupRow {
    fn to_proto(self) -> Group {
        Group { id: self.id, name: self.name, created_at: self.created_at }
    }
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<GroupRow> {
    let row = sqlx::query_as::<_, GroupRow>("select id, name, created_at from groups where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?;

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
    let rows = sqlx::query_as::<_, GroupRow>("select id, name, created_at from groups where owner = $1 order by lower(name), id").bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(GroupRow::to_proto).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<GroupRequest>) -> ApiResult<(StatusCode, Json<Group>)> {
    let name = valid_name(&request)?;
    let inserted = sqlx::query_as::<_, GroupRow>("insert into groups (owner, name) values ($1, $2) returning id, name, created_at").bind(user.id).bind(&name).fetch_one(&state.pool).await;

    match inserted {
        Ok(row) => Ok((StatusCode::CREATED, Json(row.to_proto()))),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(exists()),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn rename(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<GroupRequest>) -> ApiResult<Json<Group>> {
    let name = valid_name(&request)?;
    let updated = sqlx::query_as::<_, GroupRow>("update groups set name = $3 where id = $1 and owner = $2 returning id, name, created_at").bind(id).bind(user.id).bind(&name).fetch_optional(&state.pool).await;

    match updated {
        Ok(Some(row)) => Ok(Json(row.to_proto())),
        Ok(None) => Err(ApiError::not_found("That group")),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(exists()),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from groups where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That group"));
    }

    Ok(StatusCode::NO_CONTENT)
}
