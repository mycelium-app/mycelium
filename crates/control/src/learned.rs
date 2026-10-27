use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use mycelium_proto::api::LearnedItem;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

const MOST_LEARNED: i64 = 50;
const LONGEST_NOTE: usize = 300;

#[derive(Deserialize)]
pub struct LearnedQuery {
    limit: Option<i64>,
}

pub async fn active_source(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<&'static str>> {
    let teaching = sqlx::query_scalar::<_, bool>("select teach_me from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?.unwrap_or(false);

    if teaching {
        return Ok(Some("teach_me"));
    }

    let reviewing = sqlx::query_scalar::<_, bool>("select exists (select 1 from exercises where conversation_id = $1 and reviewing)").bind(conversation_id).fetch_one(&state.pool).await?;

    if reviewing {
        return Ok(Some("exercise_review"));
    }

    let checking = sqlx::query_scalar::<_, bool>("select exists (select 1 from walkthroughs where conversation_id = $1)").bind(conversation_id).fetch_one(&state.pool).await?;

    Ok(checking.then_some("walkthrough_check"))
}

pub async fn note_from_agent(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<(), String>> {
    let text = arguments.get("text").and_then(Value::as_str).map(str::trim).unwrap_or_default();

    if text.is_empty() || text.chars().count() > LONGEST_NOTE {
        return Ok(Err(format!("Nothing was saved. Give text, one sentence of up to {LONGEST_NOTE} characters, naming what the person learned.")));
    }

    let Some(source) = active_source(state, conversation_id).await? else {
        return Ok(Err("Nothing was saved. note_learned works only while Teach me is on, during a walkthrough check or during an exercise review.".to_string()));
    };

    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let turn = sqlx::query_scalar::<_, Option<i64>>("select max(seq) from events where conversation_id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    sqlx::query("insert into learned_items (owner, text, source_kind, conversation_id, message_id) values ($1, $2, $3, $4, $5)").bind(owner).bind(text).bind(source).bind(conversation_id).bind(turn).execute(&state.pool).await?;

    Ok(Ok(()))
}

#[derive(sqlx::FromRow)]
struct LearnedRow {
    id: Uuid,
    text: String,
    source_kind: String,
    conversation_id: Uuid,
    conversation_title: String,
    message_id: Option<i64>,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<LearnedQuery>) -> ApiResult<Json<Vec<LearnedItem>>> {
    let limit = query.limit.unwrap_or(5).clamp(1, MOST_LEARNED);
    let rows = sqlx::query_as::<_, LearnedRow>(
        "select l.id, l.text, l.source_kind, l.conversation_id, c.title as conversation_title, l.message_id, l.created_at from learned_items l join conversations c on c.id = l.conversation_id where l.owner = $1 order by l.created_at desc limit $2",
    )
    .bind(user.id)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(|row| LearnedItem { id: row.id, text: row.text, source_kind: row.source_kind, conversation_id: row.conversation_id, conversation_title: row.conversation_title, message_id: row.message_id, created_at: row.created_at }).collect()))
}
