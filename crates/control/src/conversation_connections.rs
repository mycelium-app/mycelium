use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ConversationConnection, CreateConversationConnectionRequest};
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const COLUMNS: &str = "c.id, c.source_id, c.target_id, s.title as source_title, t.title as target_title, c.created_at, c.revoked_at";
const JOINS: &str = "conversation_connections c join conversations s on s.id = c.source_id join conversations t on t.id = c.target_id";

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
    source_title: String,
    target_title: String,
    created_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ConnectionRow {
    fn to_proto(self) -> ConversationConnection {
        ConversationConnection {
            id: self.id,
            source_id: self.source_id,
            target_id: self.target_id,
            source_title: crate::message_proposals::label(self.source_id, &self.source_title),
            target_title: crate::message_proposals::label(self.target_id, &self.target_title),
            created_at: self.created_at,
            revoked_at: self.revoked_at,
        }
    }
}

pub async fn active(state: &AppState, owner: Uuid, source: Uuid, target: Uuid) -> ApiResult<bool> {
    let found = sqlx::query_scalar("select exists(select 1 from conversation_connections where owner = $1 and source_id = least($2, $3) and target_id = greatest($2, $3) and revoked_at is null)")
        .bind(owner).bind(source).bind(target).fetch_one(&state.pool).await?;

    Ok(found)
}

pub async fn connect(
    state: &AppState,
    user: &AuthUser,
    source: Uuid,
    target: Uuid,
) -> ApiResult<ConversationConnection> {
    user.require_full_access()?;

    if source == target {
        return Err(ApiError::bad_request(
            "connection_self",
            "Choose two different conversations.",
        ));
    }

    for id in [source, target] {
        let archived = sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "select archived_at from conversations where id = $1 and owner = $2",
        )
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;

        match archived {
            None => return Err(ApiError::not_found("That conversation")),
            Some(Some(_)) => {
                return Err(ApiError::conflict(
                    "connection_archived",
                    "Restore the conversation before connecting it.",
                ));
            }
            Some(None) => {}
        }
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into conversation_connections (owner, source_id, target_id) values ($1, least($2, $3), greatest($2, $3)) on conflict (owner, source_id, target_id) do update set revoked_at = null returning id")
        .bind(user.id).bind(source).bind(target).fetch_one(&state.pool).await?;
    let row = sqlx::query_as::<_, ConnectionRow>(sqlx::AssertSqlSafe(format!(
        "select {COLUMNS} from {JOINS} where c.id = $1 and c.owner = $2"
    )))
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.pool)
    .await?;
    let connection = row.to_proto();
    state.publish(
        Some(user.id),
        StreamMessage::ConversationConnection(connection.clone()),
    );

    Ok(connection)
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ConversationConnection>>> {
    let rows = sqlx::query_as::<_, ConnectionRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from {JOINS} where c.owner = $1 and c.revoked_at is null order by c.created_at desc"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(
        rows.into_iter().map(ConnectionRow::to_proto).collect(),
    ))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(request): Json<CreateConversationConnectionRequest>,
) -> ApiResult<(StatusCode, Json<ConversationConnection>)> {
    let connection = connect(&state, &user, request.source_id, request.target_id).await?;

    Ok((StatusCode::CREATED, Json(connection)))
}

pub async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ConversationConnection>> {
    user.require_full_access()?;
    let changed = sqlx::query("update conversation_connections set revoked_at = now() where id = $1 and owner = $2 and revoked_at is null").bind(id).bind(user.id).execute(&state.pool).await?;

    if changed.rows_affected() == 0 {
        return Err(ApiError::not_found("That connection"));
    }

    let row = sqlx::query_as::<_, ConnectionRow>(sqlx::AssertSqlSafe(format!(
        "select {COLUMNS} from {JOINS} where c.id = $1 and c.owner = $2"
    )))
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.pool)
    .await?;
    let connection = row.to_proto();
    state.publish(
        Some(user.id),
        StreamMessage::ConversationConnection(connection.clone()),
    );

    Ok(Json(connection))
}
