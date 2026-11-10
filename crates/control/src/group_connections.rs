use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::CONVERSATION_GROUP_SQL;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateGroupConnectionRequest, GroupConnection};
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const COLUMNS: &str = "c.id, c.source_group_id, c.target_group_id, s.name as source_name, t.name as target_name, c.both_ways, c.created_at, c.revoked_at";
const JOINS: &str = "group_connections c join groups s on s.id = c.source_group_id join groups t on t.id = c.target_group_id";

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: Uuid,
    source_group_id: Uuid,
    target_group_id: Uuid,
    source_name: String,
    target_name: String,
    both_ways: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ConnectionRow {
    fn to_proto(self) -> GroupConnection {
        GroupConnection {
            id: self.id,
            source_group_id: self.source_group_id,
            target_group_id: self.target_group_id,
            source_name: self.source_name,
            target_name: self.target_name,
            both_ways: self.both_ways,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
        }
    }
}

pub async fn active(state: &AppState, owner: Uuid, from_group: Uuid, to_group: Uuid) -> ApiResult<bool> {
    let found = sqlx::query_scalar("select exists(select 1 from group_connections where owner = $1 and revoked_at is null and ((source_group_id = $2 and target_group_id = $3) or (both_ways and source_group_id = $3 and target_group_id = $2)))")
        .bind(owner).bind(from_group).bind(to_group).fetch_one(&state.pool).await?;

    Ok(found)
}

pub async fn reverse_only(state: &AppState, owner: Uuid, from_group: Uuid, to_group: Uuid) -> ApiResult<Option<String>> {
    if active(state, owner, from_group, to_group).await? || !active(state, owner, to_group, from_group).await? {
        return Ok(None);
    }

    let from_name = sqlx::query_scalar::<_, String>("select name from groups where id = $1").bind(from_group).fetch_one(&state.pool).await?;
    let to_name = sqlx::query_scalar::<_, String>("select name from groups where id = $1").bind(to_group).fetch_one(&state.pool).await?;

    Ok(Some(format!("{from_name} is not allowed to reach {to_name}. Ask the person to connect it that way in Settings, Agent connections.")))
}

pub async fn group_of(state: &AppState, conversation: Uuid) -> ApiResult<Option<Uuid>> {
    let group = sqlx::query_scalar::<_, Option<Uuid>>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_GROUP_SQL} from conversations where id = $1")))
        .bind(conversation)
        .fetch_optional(&state.pool)
        .await?;

    Ok(group.flatten())
}

pub async fn connected(state: &AppState, owner: Uuid, from: Uuid, to: Uuid) -> ApiResult<bool> {
    let (Some(source), Some(target)) = (group_of(state, from).await?, group_of(state, to).await?) else {
        return Ok(false);
    };

    if source == target {
        return Ok(true);
    }

    active(state, owner, source, target).await
}

async fn publish(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<GroupConnection> {
    let row = sqlx::query_as::<_, ConnectionRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from {JOINS} where c.id = $1 and c.owner = $2")))
        .bind(id)
        .bind(owner)
        .fetch_one(&state.pool)
        .await?;
    let connection = row.to_proto();
    state.publish(Some(owner), StreamMessage::GroupConnection(connection.clone()));

    Ok(connection)
}

pub async fn connect(state: &AppState, owner: Uuid, source: Uuid, target: Uuid, both_ways: bool) -> ApiResult<GroupConnection> {
    if source == target {
        return Err(ApiError::bad_request("connection_self", "Choose two different groups."));
    }

    crate::groups::find_owned(state, source, owner).await?;
    crate::groups::find_owned(state, target, owner).await?;

    let id = sqlx::query_scalar::<_, Uuid>("insert into group_connections (owner, source_group_id, target_group_id, both_ways) values ($1, $2, $3, $4) on conflict (owner, least(source_group_id, target_group_id), greatest(source_group_id, target_group_id)) do update set source_group_id = excluded.source_group_id, target_group_id = excluded.target_group_id, both_ways = excluded.both_ways, revoked_at = null returning id")
        .bind(owner).bind(source).bind(target).bind(both_ways).fetch_one(&state.pool).await?;

    publish(state, owner, id).await
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<GroupConnection>>> {
    let rows = sqlx::query_as::<_, ConnectionRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from {JOINS} where c.owner = $1 and c.revoked_at is null order by c.created_at desc"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(ConnectionRow::to_proto).collect()))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(request): Json<CreateGroupConnectionRequest>,
) -> ApiResult<(StatusCode, Json<GroupConnection>)> {
    user.require_full_access()?;
    let connection = connect(&state, user.id, request.source_group_id, request.target_group_id, request.both_ways).await?;

    Ok((StatusCode::CREATED, Json(connection)))
}

pub async fn revoke(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<GroupConnection>> {
    user.require_full_access()?;
    let changed = sqlx::query("update group_connections set revoked_at = now() where id = $1 and owner = $2 and revoked_at is null").bind(id).bind(user.id).execute(&state.pool).await?;

    if changed.rows_affected() == 0 {
        return Err(ApiError::not_found("That connection"));
    }

    Ok(Json(publish(&state, user.id, id).await?))
}
