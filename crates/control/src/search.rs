use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use mycelium_proto::api::{Indexing, NodeIndexing, SearchHit, SearchQuery};
use uuid::Uuid;

pub async fn indexing(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Indexing>> {
    let conversations = sqlx::query_scalar::<_, i64>("select count(*) from conversations where owner = $1").bind(user.id).fetch_one(&state.pool).await?;
    let entries = sqlx::query_scalar::<_, i64>("select count(*) from search_entries e join conversations c on c.id = e.conversation_id where c.owner = $1").bind(user.id).fetch_one(&state.pool).await?;
    let rows = sqlx::query_as::<_, (Uuid, i64, i64, i64)>(
        "select n.id,
                (select count(*) from transcripts t where t.node_id = n.id),
                (select count(*) from conversations c where c.node_id = n.id and c.imported_from is not null),
                (select count(*) from conversations c where c.node_id = n.id)
         from nodes n where n.revoked_at is null order by n.name",
    )
    .fetch_all(&state.pool)
    .await?;

    let nodes = rows.into_iter().map(|(node_id, transcripts, imported, conversations)| NodeIndexing { node_id, transcripts, imported, conversations }).collect();

    Ok(Json(Indexing { conversations, entries, nodes }))
}

pub async fn search(State(state): State<AppState>, user: AuthUser, Query(query): Query<SearchQuery>) -> ApiResult<Json<Vec<SearchHit>>> {
    let text = query.q.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("query_required", "Enter something to search for."));
    }

    let rows = sqlx::query_as::<_, (Uuid, String, Uuid, chrono::DateTime<chrono::Utc>, String)>(
        "select c.id, c.title, c.node_id, c.updated_at,
                ts_headline('english', best.text, websearch_to_tsquery('english', $2), 'StartSel=**, StopSel=**, MaxWords=24')
         from conversations c
         join lateral (
             select e.text, ts_rank(e.tsv, websearch_to_tsquery('english', $2)) as rank
             from search_entries e
             where e.conversation_id = c.id and e.tsv @@ websearch_to_tsquery('english', $2)
             order by rank desc, e.seq limit 1
         ) best on true
         where c.owner = $1
         order by best.rank desc, c.updated_at desc
         limit $3",
    )
    .bind(user.id)
    .bind(text)
    .bind(query.limit.clamp(1, 100))
    .fetch_all(&state.pool)
    .await?;

    let hits = rows.into_iter().map(|(conversation_id, title, node_id, updated_at, snippet)| SearchHit { conversation_id, title, node_id, updated_at, snippet }).collect();

    Ok(Json(hits))
}
