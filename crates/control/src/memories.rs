use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Memory, MemorySource, UpdateMemoryRequest};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

pub const MEMORY_COLUMNS: &str = "id, text, scopes, created_by, conversation_id, model, agent, node_id, node_name, account, project_id, project_name, message_seq, tool_call_id, created_at, updated_at";

#[derive(Deserialize)]
pub struct ListQuery {
    query: Option<String>,
}

#[derive(sqlx::FromRow)]
pub struct MemoryRow {
    pub id: Uuid,
    pub text: String,
    pub scopes: Vec<String>,
    pub created_by: String,
    pub conversation_id: Option<Uuid>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub node_id: Option<Uuid>,
    pub node_name: Option<String>,
    pub account: Option<String>,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub message_seq: Option<i64>,
    pub tool_call_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl MemoryRow {
    pub fn to_proto(self) -> Memory {
        Memory {
            id: self.id,
            text: self.text,
            scopes: self.scopes,
            created_by: self.created_by,
            conversation_id: self.conversation_id,
            source: MemorySource {
                model: self.model,
                agent: self.agent,
                node_id: self.node_id,
                node_name: self.node_name,
                account: self.account,
                project_id: self.project_id,
                project_name: self.project_name,
                conversation_title: None,
                message_seq: self.message_seq,
                tool_call_id: self.tool_call_id,
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn search(pool: &PgPool, query: Option<&str>, visible_scopes: Option<&[String]>, limit: i64) -> Result<Vec<MemoryRow>, sqlx::Error> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());

    sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!(
        "select {MEMORY_COLUMNS} from memories
         where ($1::text is null or to_tsvector('english', text) @@ websearch_to_tsquery('english', $1))
           and ($2::text[] is null or scopes && $2)
         order by case when $1::text is null then 0 else ts_rank(to_tsvector('english', text), websearch_to_tsquery('english', $1)) end desc, created_at desc
         limit $3"
    )))
    .bind(query)
    .bind(visible_scopes)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn omitted_groups(pool: &PgPool, query: &str, visible_scopes: &[String]) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as::<_, (String, i64)>(
        "select g.name, count(distinct m.id) from memories m
         join groups g on 'group:' || g.id = any(m.scopes)
         where to_tsvector('english', m.text) @@ websearch_to_tsquery('english', $1) and not (m.scopes && $2)
         group by g.name order by g.name",
    )
    .bind(query.trim())
    .bind(visible_scopes)
    .fetch_all(pool)
    .await
}

pub async fn list(State(state): State<AppState>, _user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Memory>>> {
    let rows = search(&state.pool, query.query.as_deref(), None, i64::MAX).await?;
    let titles: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = any($1)")
        .bind(rows.iter().filter_map(|row| row.conversation_id).collect::<Vec<Uuid>>())
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .collect();
    let memories = rows
        .into_iter()
        .map(|row| {
            let title = row.conversation_id.and_then(|conversation_id| titles.get(&conversation_id)).filter(|title| !title.trim().is_empty()).cloned();
            let mut memory = row.to_proto();
            memory.source.conversation_title = title;

            memory
        })
        .collect();

    Ok(Json(memories))
}

pub async fn update(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<UpdateMemoryRequest>) -> ApiResult<Json<Memory>> {
    let text = request.text.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter what to remember."));
    }

    let row = sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!("update memories set text = $2, updated_at = now() where id = $1 returning {MEMORY_COLUMNS}")))
        .bind(id)
        .bind(text)
        .fetch_optional(&state.pool)
        .await?;

    let memory = row.ok_or_else(|| ApiError::not_found("That memory"))?.to_proto();
    state.publish(None, StreamMessage::Memory(memory.clone()));

    Ok(Json(memory))
}

pub async fn delete(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from memories where id = $1").bind(id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That memory"));
    }

    state.publish(None, StreamMessage::MemoryRemoved { memory_id: id });

    Ok(StatusCode::NO_CONTENT)
}
