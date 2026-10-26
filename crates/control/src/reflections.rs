use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::schedules;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use crate::wellness;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use mycelium_proto::api::{CreateConversationRequest, CreateScheduleRequest, EachRun, PatchReflectionRequest, PatchScheduleRequest, RedraftReflection, Reflection, ReflectionSchedule, ReflectionScheduleRequest};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

const SCHEDULE_PRESET: &str = "fridays at 16:00";
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

fn drafting_prompt(week: Option<NaiveDate>) -> String {
    let which = match week {
        Some(week) => format!("the week starting Monday {week}"),
        None => "the week that is ending (Monday to Sunday)".to_string(),
    };

    format!(
        "Draft my weekly reflection for {which}. Use find_conversations and recall to look over that week, then call save_reflection once with four short sections in plain Markdown, written in the first person in British English: done (what was finished), learned (what I found out or got better at), waiting (what is blocked or waiting on someone) and next_week (what to pick up next). Leave a section empty rather than padding it. Do not change any files."
    )
}

pub async fn set_schedule(State(state): State<AppState>, user: AuthUser, Json(request): Json<ReflectionScheduleRequest>) -> ApiResult<Json<ReflectionSchedule>> {
    user.require_full_access()?;

    let current = sqlx::query_scalar::<_, Option<Uuid>>("select reflection_schedule_id from user_settings where owner = $1").bind(user.id).fetch_optional(&state.pool).await?.flatten();
    let same_place = match current {
        Some(id) => {
            let place = sqlx::query_as::<_, (Uuid, String, String)>("select node_id, agent, cwd from schedules where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?;

            place.is_some_and(|(node_id, agent, cwd)| node_id == request.node_id && agent == enum_text(&request.agent) && cwd == request.cwd.trim())
        }
        None => false,
    };

    let schedule = match (current, same_place) {
        (Some(id), true) => schedules::patch(State(state.clone()), user.clone(), Path(id), Json(PatchScheduleRequest { paused: Some(!request.enabled), prompt: None, cron: None, each_run: None, time_zone: None })).await?.0,
        _ => {
            if let Some(id) = current {
                schedules::delete(State(state.clone()), user.clone(), Path(id)).await.ok();
            }

            let cron = schedules::preset_cron(SCHEDULE_PRESET).unwrap_or_default();
            let created = schedules::create(State(state.clone()), user.clone(), Json(CreateScheduleRequest { node_id: request.node_id, agent: request.agent, cwd: request.cwd.clone(), project_id: None, prompt: drafting_prompt(None), cron, each_run: EachRun::New, time_zone: None })).await?.1.0;

            match request.enabled {
                true => created,
                false => schedules::patch(State(state.clone()), user.clone(), Path(created.id), Json(PatchScheduleRequest { paused: Some(true), prompt: None, cron: None, each_run: None, time_zone: None })).await?.0,
            }
        }
    };

    sqlx::query("insert into user_settings (owner, reflection_schedule_id) values ($1, $2) on conflict (owner) do update set reflection_schedule_id = excluded.reflection_schedule_id").bind(user.id).bind(schedule.id).execute(&state.pool).await?;

    Ok(Json(ReflectionSchedule { schedule_id: schedule.id, node_id: schedule.node_id, agent: schedule.agent, cwd: schedule.cwd, enabled: !schedule.paused, cron: schedule.cron }))
}

pub async fn save(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Reflection, String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let drafting = sqlx::query_scalar::<_, bool>(
        "select exists (select 1 from reflections where owner = $1 and conversation_id = $2)
             or exists (select 1 from schedule_runs r join user_settings u on u.reflection_schedule_id = r.schedule_id where u.owner = $1 and r.conversation_id = $2)",
    )
    .bind(owner)
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    if !drafting {
        return Ok(Err("Nothing was saved. Only the weekly reflection run, or a redraft the person asked for, can write a reflection.".to_string()));
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
        "insert into reflections (owner, week, done, learned, waiting, next_week, drafted_at, conversation_id)
         values ($1, $2, coalesce($3, ''), coalesce($4, ''), coalesce($5, ''), coalesce($6, ''), now(), $7)
         on conflict (owner, week) do update set
             done = case when 'done' = any(reflections.edited) or $3 is null then reflections.done else $3 end,
             learned = case when 'learned' = any(reflections.edited) or $4 is null then reflections.learned else $4 end,
             waiting = case when 'waiting' = any(reflections.edited) or $5 is null then reflections.waiting else $5 end,
             next_week = case when 'next_week' = any(reflections.edited) or $6 is null then reflections.next_week else $6 end,
             drafted_at = now(), conversation_id = $7
         returning {REFLECTION_COLUMNS}",
    )))
    .bind(owner)
    .bind(week)
    .bind(text("done"))
    .bind(text("learned"))
    .bind(text("waiting"))
    .bind(text("next_week"))
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(owner), StreamMessage::WellnessChanged);

    Ok(Ok(to_proto(row)))
}

pub async fn redraft(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<RedraftReflection>)> {
    user.require_full_access()?;

    let week = sqlx::query_scalar::<_, NaiveDate>("select week from reflections where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That reflection"))?;
    let place = sqlx::query_as::<_, (Uuid, String, String)>("select s.node_id, s.agent, s.cwd from user_settings u join schedules s on s.id = u.reflection_schedule_id where u.owner = $1").bind(user.id).fetch_optional(&state.pool).await?;
    let Some((node_id, agent, cwd)) = place else {
        return Err(ApiError::conflict("reflection_not_set_up", "Choose a node and an agent for the weekly reflection before redrafting."));
    };
    let request = CreateConversationRequest { account: None, node_id, agent: parse_enum(&agent)?, cwd, project_id: None, branch: None, title: Some(format!("Reflection for the week of {week}")), prompt: Some(drafting_prompt(Some(week))), parent_id: None, side_kind: None };
    let conversation = conversations::start(&state, &user, request).await?;

    sqlx::query("update reflections set conversation_id = $2 where id = $1").bind(id).bind(conversation.id).execute(&state.pool).await?;

    Ok((StatusCode::ACCEPTED, Json(RedraftReflection { conversation_id: conversation.id })))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Reflection>>> {
    let rows = sqlx::query_as::<_, ReflectionRow>(sqlx::AssertSqlSafe(format!("select {REFLECTION_COLUMNS} from reflections where owner = $1 order by week desc"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(to_proto).collect()))
}

pub async fn get_schedule(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Option<ReflectionSchedule>>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, String, String, bool, String)>("select s.id, s.node_id, s.agent, s.cwd, s.paused, s.cron from user_settings u join schedules s on s.id = u.reflection_schedule_id where u.owner = $1").bind(user.id).fetch_optional(&state.pool).await?;

    Ok(Json(row.map(|(schedule_id, node_id, agent, cwd, paused, cron)| parse_enum(&agent).map(|agent| ReflectionSchedule { schedule_id, node_id, agent, cwd, enabled: !paused, cron })).transpose()?))
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
