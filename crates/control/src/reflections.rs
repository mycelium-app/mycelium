use crate::auth::AuthUser;
use crate::assistant_actions;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::wellness;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use mycelium_proto::api::{PatchReflectionRequest, RedraftReflection, Reflection};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

const REFLECTION_COLUMNS: &str = "id, week, done, learned, waiting, next_week, edited, drafted_at, conversation_id";

type ReflectionRow = (Uuid, NaiveDate, String, String, String, String, Vec<String>, Option<chrono::DateTime<Utc>>, Option<Uuid>);

fn to_proto(row: ReflectionRow) -> Reflection {
    Reflection { id: row.0, week: row.1.to_string(), done: row.2, learned: row.3, waiting: row.4, next_week: row.5, edited: row.6, drafted_at: row.7, conversation_id: row.8 }
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

async fn this_week(state: &AppState, owner: Uuid) -> ApiResult<NaiveDate> {
    let zone = wellness::zone_of(state, owner, None).await?;

    Ok(monday_of(Utc::now().with_timezone(&zone).date_naive()))
}

pub async fn save(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Reflection, String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let assistant_id = sqlx::query_scalar::<_, Option<Uuid>>("select assistant_id from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    if !assistant_actions::is_wellness_conversation(state, conversation_id).await? {
        return Ok(Err("Nothing was saved. Only the Wellness assistant can write a reflection.".to_string()));
    }

    let week = match arguments.get("week").and_then(Value::as_str).map(str::trim).filter(|week| !week.is_empty()) {
        Some(week) => match NaiveDate::parse_from_str(week, "%Y-%m-%d") {
            Ok(date) => monday_of(date),
            Err(_) => return Ok(Err("Nothing was saved. Give the week as a date like 2026-10-26, any day in that week.".to_string())),
        },
        None => this_week(state, owner).await?,
    };
    let text = |field: &str| arguments.get(field).and_then(Value::as_str).map(|text| text.trim().to_string());
    let row = sqlx::query_as::<_, ReflectionRow>(sqlx::AssertSqlSafe(format!(
        "insert into reflections (owner, week, done, learned, waiting, next_week, drafted_at, conversation_id, assistant_id)
         values ($1, $2, coalesce($3, ''), coalesce($4, ''), coalesce($5, ''), coalesce($6, ''), now(), $7, $8)
         on conflict (owner, week) do update set
             done = case when 'done' = any(reflections.edited) or $3 is null then reflections.done else $3 end,
             learned = case when 'learned' = any(reflections.edited) or $4 is null then reflections.learned else $4 end,
             waiting = case when 'waiting' = any(reflections.edited) or $5 is null then reflections.waiting else $5 end,
             next_week = case when 'next_week' = any(reflections.edited) or $6 is null then reflections.next_week else $6 end,
             drafted_at = now(), conversation_id = $7, assistant_id = $8
         returning {REFLECTION_COLUMNS}",
    )))
    .bind(owner)
    .bind(week)
    .bind(text("done"))
    .bind(text("learned"))
    .bind(text("waiting"))
    .bind(text("next_week"))
    .bind(conversation_id)
    .bind(assistant_id)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(owner), StreamMessage::WellnessChanged);

    Ok(Ok(to_proto(row)))
}

pub async fn redraft(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<RedraftReflection>)> {
    user.require_full_access()?;

    let week = sqlx::query_scalar::<_, NaiveDate>("select week from reflections where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That reflection"))?;
    let (assistant_id, action_id) = assistant_actions::action_of_key(&state, user.id, assistant_actions::WEEKLY_REFLECTION).await?;
    let zone = wellness::zone_of(&state, user.id, None).await?;
    let inputs = BTreeMap::from([("week".to_string(), week.to_string())]);
    let conversation = assistant_actions::run(&state, &user, assistant_id, action_id, &inputs, zone).await?;

    sqlx::query("update reflections set conversation_id = $2, assistant_id = $3 where id = $1").bind(id).bind(conversation.id).bind(assistant_id).execute(&state.pool).await?;

    Ok((StatusCode::ACCEPTED, Json(RedraftReflection { conversation_id: conversation.id })))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Reflection>>> {
    let rows = sqlx::query_as::<_, ReflectionRow>(sqlx::AssertSqlSafe(format!("select {REFLECTION_COLUMNS} from reflections where owner = $1 order by week desc"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(to_proto).collect()))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchReflectionRequest>) -> ApiResult<Json<Reflection>> {
    user.require_full_access()?;

    let given = [request.done, request.learned, request.waiting, request.next_week];
    let row = sqlx::query_as::<_, ReflectionRow>(sqlx::AssertSqlSafe(format!(
        "update reflections set
             done = coalesce($3::text, done), learned = coalesce($4::text, learned), waiting = coalesce($5::text, waiting), next_week = coalesce($6::text, next_week),
             edited = (select coalesce(array_agg(distinct name), '{{}}') from unnest(edited || array_remove(array[
                 case when $3 is distinct from done and $3 is not null then 'done' end,
                 case when $4 is distinct from learned and $4 is not null then 'learned' end,
                 case when $5 is distinct from waiting and $5 is not null then 'waiting' end,
                 case when $6 is distinct from next_week and $6 is not null then 'next_week' end
             ], null)) as name)
         where id = $1 and owner = $2 returning {REFLECTION_COLUMNS}",
    )))
    .bind(id)
    .bind(user.id)
    .bind(given[0].as_deref().map(str::trim))
    .bind(given[1].as_deref().map(str::trim))
    .bind(given[2].as_deref().map(str::trim))
    .bind(given[3].as_deref().map(str::trim))
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That reflection"))?;

    Ok(Json(to_proto(row)))
}
