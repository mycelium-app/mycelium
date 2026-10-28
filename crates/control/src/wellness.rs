use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use mycelium_proto::api::{ActivityBreak, ActivityDay, ActivityHours, ActivityHoursDay, ActivitySpan, Break, BreakNoteRequest, HeldWeek, HoldRequest, Hours, HoursDay, OutOfHoursOpen, PuzzleBucket, PuzzleDay, PuzzleResult, PuzzleStats, PuzzleSummary, StartBreakRequest, WellnessDay, WellnessHeartbeat, WellnessOverview, WellnessSettings};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use uuid::Uuid;

pub const PUZZLES: [&str; 5] = ["wordle", "connections", "daily-orbs", "catfishing", "enclose-horse"];
const PUZZLE_STATS_DAYS: i64 = 30;
const HEARTBEAT_CAP_SECONDS: i32 = 90;
const HEARTBEAT_FUTURE_MINUTES: i64 = 5;
const HEARTBEAT_PAST_HOURS: i64 = 48;
const SPAN_JOIN_SECONDS: i64 = 90;
const ACTIVITY_KINDS: [&str; 4] = ["conversation", "chat", "assistant", "elsewhere"];
const ACTIVITY_HOURS_DAYS_LIMIT: i64 = 31;
const OVERVIEW_DAYS_LIMIT: i64 = 31;

type SettingsRow = (bool, i32, i32, i32, bool, bool, bool, bool, Option<String>);

pub const OPENING_MINUTES: i64 = 15;
const DAY_MINUTES: u32 = 24 * 60;
const FLIP_SEARCH_DAYS: i64 = 8;

const RECENT_BREAKS: i64 = 20;
const BREAK_COLUMNS: &str = "id, started_at, ends_at, ended_early_at, planned_minutes, case when ended_early_at is not null or ends_at <= now() then coalesce(ended_early_at, ends_at) end, ended_early_at is not null, note, noted_at";

type BreakRow = (Uuid, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>, i32, Option<DateTime<Utc>>, bool, Option<String>, Option<DateTime<Utc>>);

fn to_break(row: BreakRow) -> Break {
    Break { id: row.0, started_at: row.1, ends_at: row.2, ended_early_at: row.3, planned_minutes: row.4, ended_at: row.5, ended_early: row.6, note: row.7, noted_at: row.8 }
}

const SETTINGS_COLUMNS: &str = "break_reminders, break_after_minutes, snooze_minutes, break_minutes, app_always_open, quiet_hold_notifications, quiet_hold_schedules, quiet_hold_pings, time_zone";

#[derive(Deserialize)]
pub struct OverviewQuery {
    days: Option<i64>,
}

pub fn default_hours() -> Hours {
    Hours { always_open: true, days: (0..7u8).map(|weekday| HoursDay { weekday, closed: weekday >= 5, opens: "09:00".into(), closes: "18:00".into() }).collect() }
}

fn time_invalid() -> ApiError {
    ApiError::bad_request("hours_time_invalid", "Enter a time like 09:00.")
}

fn parse_minutes(text: &str) -> Option<u32> {
    let (hours, minutes) = text.split_once(':')?;

    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }

    let total = hours.parse::<u32>().ok()? * 60 + minutes.parse::<u32>().ok()?;

    (minutes.parse::<u32>().ok()? < 60 && total <= DAY_MINUTES).then_some(total)
}

pub fn validate_hours(hours: &Hours) -> ApiResult<Vec<(i16, bool, String, String)>> {
    let mut days = Vec::new();

    for day in &hours.days {
        if day.weekday > 6 {
            return Err(ApiError::bad_request("weekday_invalid", "Hours cover weekdays 0 to 6, Monday to Sunday."));
        }

        let opens = parse_minutes(&day.opens).filter(|minutes| *minutes < DAY_MINUTES).ok_or_else(time_invalid)?;
        let closes = parse_minutes(&day.closes).ok_or_else(time_invalid)?;

        days.push((day.weekday as i16, day.closed, format_minutes(opens), format_minutes(closes)));
    }

    Ok(days)
}

fn format_minutes(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub fn is_open(hours: &Hours, at: NaiveDateTime) -> bool {
    if hours.always_open {
        return true;
    }

    let today = at.weekday().num_days_from_monday() as u8;
    let yesterday = (today + 6) % 7;
    let now = at.time().num_seconds_from_midnight() / 60;
    let in_today = hours.days.iter().filter(|day| !day.closed && day.weekday == today).any(|day| {
        let (Some(opens), Some(closes)) = (parse_minutes(&day.opens), parse_minutes(&day.closes)) else { return false };

        if closes > opens { now >= opens && now < closes } else { now >= opens }
    });
    let in_yesterdays_overnight = hours.days.iter().filter(|day| !day.closed && day.weekday == yesterday).any(|day| {
        let (Some(opens), Some(closes)) = (parse_minutes(&day.opens), parse_minutes(&day.closes)) else { return false };

        closes <= opens && now < closes
    });

    in_today || in_yesterdays_overnight
}

fn all_open(hours: &[&Hours], at: NaiveDateTime) -> bool {
    hours.iter().all(|each| is_open(each, at))
}

fn next_change(hours: &[&Hours], at: NaiveDateTime) -> Option<NaiveDateTime> {
    let before = all_open(hours, at);
    let mut candidates = Vec::new();

    for offset in 0..=FLIP_SEARCH_DAYS {
        let date = at.date() + Duration::days(offset);
        let weekday = date.weekday().num_days_from_monday() as u8;

        for each in hours.iter().filter(|each| !each.always_open) {
            for day in each.days.iter().filter(|day| !day.closed && day.weekday == weekday) {
                let (Some(opens), Some(closes)) = (parse_minutes(&day.opens), parse_minutes(&day.closes)) else { continue };
                let closing_date = if closes <= opens { date + Duration::days(1) } else { date };

                candidates.push(date.and_hms_opt(0, 0, 0)? + Duration::minutes(opens as i64));
                candidates.push(closing_date.and_hms_opt(0, 0, 0)? + Duration::minutes(closes as i64));
            }
        }
    }

    candidates.sort();
    candidates.into_iter().filter(|candidate| *candidate > at).find(|candidate| all_open(hours, *candidate) != before)
}

pub struct GroupStatus {
    pub open_now: bool,
    pub closes_at: Option<DateTime<Utc>>,
    pub opens_at: Option<DateTime<Utc>>,
    pub opened_until: Option<DateTime<Utc>>,
}

pub fn status_of(app: &Hours, group: Option<&Hours>, opened_until: Option<DateTime<Utc>>, zone: Tz, now: DateTime<Utc>) -> GroupStatus {
    let opened_until = opened_until.filter(|until| *until > now);
    let local = now.with_timezone(&zone).naive_local();
    let hours: Vec<&Hours> = std::iter::once(app).chain(group).collect();
    let by_hours = all_open(&hours, local);
    let to_utc = |naive: NaiveDateTime| zone.from_local_datetime(&naive).earliest().map(|moment| moment.with_timezone(&Utc));
    let change = next_change(&hours, local).and_then(to_utc);

    if opened_until.is_some() {
        return GroupStatus { open_now: true, closes_at: opened_until, opens_at: None, opened_until };
    }

    GroupStatus { open_now: by_hours, closes_at: change.filter(|_| by_hours), opens_at: change.filter(|_| !by_hours), opened_until: None }
}

fn require_range(name: &'static str, message: &'static str, value: i32, low: i32, high: i32) -> ApiResult<()> {
    if value < low || value > high {
        return Err(ApiError::bad_request(name, message));
    }

    Ok(())
}

pub async fn zone_of(state: &AppState, owner: Uuid, saved: Option<&str>) -> ApiResult<Tz> {
    if let Some(zone) = saved.and_then(|name| name.parse::<Tz>().ok()) {
        return Ok(zone);
    }

    let stored = sqlx::query_scalar::<_, Option<String>>("select time_zone from users where id = $1").bind(owner).fetch_one(&state.pool).await?;

    Ok(stored.and_then(|name| name.parse::<Tz>().ok()).unwrap_or(Tz::UTC))
}

pub async fn load_hours(state: &AppState, owner: Uuid, group_id: Option<Uuid>, always_open: bool) -> ApiResult<Hours> {
    let stored = sqlx::query_as::<_, (i16, bool, String, String)>("select weekday, closed, to_char(opens, 'HH24:MI'), to_char(closes, 'HH24:MI') from hours where owner = $1 and group_id is not distinct from $2 order by weekday")
        .bind(owner)
        .bind(group_id)
        .fetch_all(&state.pool)
        .await?;
    let mut hours = default_hours();

    hours.always_open = always_open;

    for (weekday, closed, opens, closes) in stored {
        hours.days[weekday as usize] = HoursDay { weekday: weekday as u8, closed, opens, closes };
    }

    Ok(hours)
}

pub async fn save_hours(connection: &mut sqlx::PgConnection, owner: Uuid, group_id: Option<Uuid>, hours: &Hours) -> ApiResult<()> {
    for (weekday, closed, opens, closes) in validate_hours(hours)? {
        sqlx::query(
            "insert into hours (owner, group_id, weekday, closed, opens, closes) values ($1, $2, $3, $4, $5::text::time, $6::text::time)
             on conflict (owner, group_id, weekday) do update set closed = excluded.closed, opens = excluded.opens, closes = excluded.closes",
        )
        .bind(owner)
        .bind(group_id)
        .bind(weekday)
        .bind(closed)
        .bind(opens)
        .bind(closes)
        .execute(&mut *connection)
        .await?;
    }

    Ok(())
}

async fn load_settings(state: &AppState, owner: Uuid) -> ApiResult<WellnessSettings> {
    let row = sqlx::query_as::<_, SettingsRow>(sqlx::AssertSqlSafe(format!("select {SETTINGS_COLUMNS} from user_settings where owner = $1"))).bind(owner).fetch_optional(&state.pool).await?;
    let row = row.unwrap_or((true, 50, 10, 5, true, true, true, true, None));
    let hours = load_hours(state, owner, None, row.4).await?;

    Ok(WellnessSettings {
        break_reminders: row.0,
        break_after_minutes: row.1,
        snooze_minutes: row.2,
        break_minutes: row.3,
        hours,
        quiet_hold_notifications: row.5,
        quiet_hold_schedules: row.6,
        quiet_hold_pings: row.7,
        time_zone: row.8,
    })
}

pub async fn get_settings(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<WellnessSettings>> {
    Ok(Json(load_settings(&state, user.id).await?))
}

pub async fn save_settings(State(state): State<AppState>, user: AuthUser, Json(request): Json<WellnessSettings>) -> ApiResult<Json<WellnessSettings>> {
    user.require_full_access()?;
    require_range("break_after_invalid", "Choose a break reminder between 5 and 480 minutes.", request.break_after_minutes, 5, 480)?;
    require_range("snooze_invalid", "Choose a snooze between 1 and 120 minutes.", request.snooze_minutes, 1, 120)?;
    require_range("break_length_invalid", "Choose a break between 1 and 120 minutes.", request.break_minutes, 1, 120)?;

    if let Some(name) = request.time_zone.as_deref() {
        name.parse::<Tz>().map_err(|_| ApiError::bad_request("time_zone_invalid", "Enter a time zone from the IANA list, for example Europe/London."))?;
    }

    let mut tx = state.pool.begin().await?;

    sqlx::query(
        "insert into user_settings (owner, break_reminders, break_after_minutes, snooze_minutes, break_minutes, app_always_open, quiet_hold_notifications, quiet_hold_schedules, quiet_hold_pings, time_zone)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (owner) do update set break_reminders = excluded.break_reminders, break_after_minutes = excluded.break_after_minutes, snooze_minutes = excluded.snooze_minutes, break_minutes = excluded.break_minutes,
            app_always_open = excluded.app_always_open, quiet_hold_notifications = excluded.quiet_hold_notifications, quiet_hold_schedules = excluded.quiet_hold_schedules,
            quiet_hold_pings = excluded.quiet_hold_pings, time_zone = excluded.time_zone",
    )
    .bind(user.id)
    .bind(request.break_reminders)
    .bind(request.break_after_minutes)
    .bind(request.snooze_minutes)
    .bind(request.break_minutes)
    .bind(request.hours.always_open)
    .bind(request.quiet_hold_notifications)
    .bind(request.quiet_hold_schedules)
    .bind(request.quiet_hold_pings)
    .bind(&request.time_zone)
    .execute(&mut *tx)
    .await?;

    save_hours(&mut tx, user.id, None, &request.hours).await?;

    tx.commit().await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(load_settings(&state, user.id).await?))
}

pub async fn app_hours(state: &AppState, owner: Uuid) -> ApiResult<Hours> {
    Ok(load_settings(state, owner).await?.hours)
}

pub async fn group_opened_until(state: &AppState, owner: Uuid, group_id: Uuid) -> ApiResult<Option<DateTime<Utc>>> {
    Ok(sqlx::query_scalar::<_, Option<DateTime<Utc>>>("select max(until) from out_of_hours_opens where owner = $1 and group_id = $2 and closed_at is null and until > now()").bind(owner).bind(group_id).fetch_one(&state.pool).await?)
}

pub async fn closure(state: &AppState, owner: Uuid, group_id: Option<Uuid>) -> ApiResult<Option<WellnessSettings>> {
    let settings = load_settings(state, owner).await?;
    let zone = zone_of(state, owner, settings.time_zone.as_deref()).await?;
    let group_hours = match group_id {
        Some(id) => {
            let always_open = sqlx::query_scalar::<_, bool>("select always_open from groups where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?;

            match always_open {
                Some(always_open) => Some(load_hours(state, owner, Some(id), always_open).await?),
                None => None,
            }
        }
        None => None,
    };
    let opened_until = match group_id {
        Some(id) => group_opened_until(state, owner, id).await?,
        None => None,
    };
    let status = status_of(&settings.hours, group_hours.as_ref(), opened_until, zone, Utc::now());

    Ok((!status.open_now).then_some(settings))
}

const CONVERSATION_GROUP: &str = "coalesce((select p.group_id from projects p where p.id = c.project_id), (select a.group_id from assistants a where a.id = c.assistant_id))";
const SCHEDULE_GROUP: &str = "coalesce((select p.group_id from projects p where p.id = s.project_id), (select a.group_id from assistants a where a.id = s.assistant_id), (select p.group_id from conversations c join projects p on p.id = c.project_id where c.id = s.conversation_id))";

pub async fn schedule_closure(state: &AppState, owner: Uuid, schedule_id: Uuid) -> ApiResult<Option<WellnessSettings>> {
    let group_id = sqlx::query_scalar::<_, Option<Uuid>>(sqlx::AssertSqlSafe(format!("select {SCHEDULE_GROUP} from schedules s where s.id = $1"))).bind(schedule_id).fetch_optional(&state.pool).await?.flatten();

    closure(state, owner, group_id).await
}

pub async fn heartbeat(State(state): State<AppState>, user: AuthUser, Json(request): Json<WellnessHeartbeat>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    if let Some(kind) = request.kind.as_deref() {
        if !ACTIVITY_KINDS.contains(&kind) {
            return Err(ApiError::bad_request("activity_kind_invalid", "Choose conversation, chat, assistant or elsewhere."));
        }
    }

    let seconds = request.seconds.clamp(0, HEARTBEAT_CAP_SECONDS);
    let settings = load_settings(&state, user.id).await?;
    let zone = zone_of(&state, user.id, settings.time_zone.as_deref()).await?;
    let now = Utc::now();
    let ended = request.at.filter(|at| *at <= now + Duration::minutes(HEARTBEAT_FUTURE_MINUTES) && *at >= now - Duration::hours(HEARTBEAT_PAST_HOURS)).unwrap_or(now);
    let day = ended.with_timezone(&zone).date_naive();
    let is_session = request.kind.as_deref().is_none_or(|kind| kind != "elsewhere");

    if is_session {
        sqlx::query("insert into viewing_time (owner, day, seconds) values ($1, $2, $3) on conflict (owner, day) do update set seconds = viewing_time.seconds + excluded.seconds")
            .bind(user.id)
            .bind(day)
            .bind(seconds)
            .execute(&state.pool)
            .await?;
    }

    let Some(kind) = request.kind.as_deref().filter(|_| seconds > 0) else { return Ok(StatusCode::NO_CONTENT) };
    let started = ended - Duration::seconds(seconds as i64);
    let route = request.route.clone().unwrap_or_default();
    let conversation_id = match request.conversation_id {
        Some(id) => sqlx::query_scalar::<_, Uuid>("select id from conversations where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let assistant_id = match request.assistant_id {
        Some(id) => sqlx::query_scalar::<_, Uuid>("select id from assistants where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let group_id = match request.group_id {
        Some(id) => sqlx::query_scalar::<_, Uuid>("select id from groups where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let last = sqlx::query_as::<_, (Uuid, String, String, Option<Uuid>, Option<Uuid>, DateTime<Utc>)>("select id, kind, route, conversation_id, assistant_id, ended_at from activity_spans where owner = $1 order by ended_at desc limit 1")
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;
    let continues = last.filter(|(_, last_kind, last_route, last_conversation, last_assistant, last_ended)| {
        last_kind == kind && *last_route == route && *last_conversation == conversation_id && *last_assistant == assistant_id && started - *last_ended <= Duration::seconds(SPAN_JOIN_SECONDS)
    });

    match continues {
        Some((id, ..)) => {
            sqlx::query("update activity_spans set ended_at = greatest(ended_at, $2), started_at = least(started_at, $3), title = coalesce($4, title) where id = $1").bind(id).bind(ended).bind(started).bind(&request.title).execute(&state.pool).await?;
        }
        None => {
            sqlx::query("insert into activity_spans (owner, started_at, ended_at, kind, route, title, group_id, conversation_id, assistant_id) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
                .bind(user.id)
                .bind(started)
                .bind(ended)
                .bind(kind)
                .bind(&route)
                .bind(&request.title)
                .bind(group_id)
                .bind(conversation_id)
                .bind(assistant_id)
                .execute(&state.pool)
                .await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_break(State(state): State<AppState>, user: AuthUser, Json(request): Json<StartBreakRequest>) -> ApiResult<(StatusCode, Json<Break>)> {
    user.require_full_access()?;

    let settings = load_settings(&state, user.id).await?;
    let minutes = request.minutes.unwrap_or(settings.break_minutes);

    require_range("break_length_invalid", "Choose a break between 1 and 120 minutes.", minutes, 1, 120)?;

    let started = sqlx::query_as::<_, BreakRow>(sqlx::AssertSqlSafe(format!(
        "insert into breaks (owner, ends_at, planned_minutes) values ($1, now() + make_interval(mins => $2), $2) returning {BREAK_COLUMNS}",
    )))
    .bind(user.id)
    .bind(minutes)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok((StatusCode::CREATED, Json(to_break(started))))
}

pub async fn end_break(State(state): State<AppState>, user: AuthUser) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    sqlx::query("update breaks set ended_early_at = now() where owner = $1 and ends_at > now() and ended_early_at is null").bind(user.id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn note_break(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<BreakNoteRequest>) -> ApiResult<Json<Break>> {
    user.require_full_access()?;

    let note = request.note.trim();

    if note.is_empty() {
        return Err(ApiError::bad_request("break_note_missing", "Enter what you did in your break"));
    }

    let noted = sqlx::query_as::<_, BreakRow>(sqlx::AssertSqlSafe(format!(
        "update breaks set note = $3, noted_at = now() where id = $1 and owner = $2 and (ended_early_at is not null or ends_at <= now()) returning {BREAK_COLUMNS}",
    )))
    .bind(id)
    .bind(user.id)
    .bind(note)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That break"))?;

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(to_break(noted)))
}

fn require_puzzle(puzzle: &str) -> ApiResult<()> {
    if !PUZZLES.contains(&puzzle) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "puzzle_not_found", "That puzzle is not on the list. Choose wordle, connections, daily-orbs, catfishing or enclose-horse."));
    }

    Ok(())
}

pub async fn today_of(state: &AppState, owner: Uuid) -> ApiResult<NaiveDate> {
    let settings = load_settings(state, owner).await?;
    let zone = zone_of(state, owner, settings.time_zone.as_deref()).await?;

    Ok(Utc::now().with_timezone(&zone).date_naive())
}

pub async fn save_puzzle_result(state: &AppState, owner: Uuid, puzzle: &str, day: NaiveDate, result: &PuzzleResult, source: &str) -> ApiResult<()> {
    sqlx::query(
        "insert into puzzle_plays (owner, puzzle, day, solved, score, seconds, source) values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (owner, puzzle, day) do update set solved = excluded.solved, score = excluded.score, seconds = excluded.seconds, source = excluded.source, recorded_at = now()",
    )
    .bind(owner)
    .bind(puzzle)
    .bind(day)
    .bind(result.solved)
    .bind(result.score)
    .bind(result.seconds)
    .bind(source)
    .execute(&state.pool)
    .await?;

    Ok(())
}

pub async fn mark_puzzle(State(state): State<AppState>, user: AuthUser, Path(puzzle): Path<String>, body: Bytes) -> ApiResult<StatusCode> {
    user.require_full_access()?;
    require_puzzle(&puzzle)?;

    let day = today_of(&state, user.id).await?;

    if body.is_empty() {
        sqlx::query("insert into puzzle_plays (owner, puzzle, day) values ($1, $2, $3) on conflict do nothing").bind(user.id).bind(&puzzle).bind(day).execute(&state.pool).await?;
    } else {
        let result = serde_json::from_slice::<PuzzleResult>(&body).map_err(|_| ApiError::bad_request("puzzle_result_invalid", "Send the result as solved, score, seconds and source."))?;
        let source = result.source.as_deref().unwrap_or("manual");

        if !["manual", "read"].contains(&source) {
            return Err(ApiError::bad_request("puzzle_source_invalid", "Record a puzzle result as manual or read."));
        }

        if result.score.is_some_and(|score| score < 0) || result.seconds.is_some_and(|seconds| seconds < 0) {
            return Err(ApiError::bad_request("puzzle_result_invalid", "Enter a score and a time of zero or more."));
        }

        save_puzzle_result(&state, user.id, &puzzle, day, &result, source).await?;
    }

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn unmark_puzzle(State(state): State<AppState>, user: AuthUser, Path(puzzle): Path<String>) -> ApiResult<StatusCode> {
    user.require_full_access()?;
    require_puzzle(&puzzle)?;

    let day = today_of(&state, user.id).await?;

    sqlx::query("delete from puzzle_plays where owner = $1 and puzzle = $2 and day = $3").bind(user.id).bind(&puzzle).bind(day).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PuzzlesQuery {
    days: Option<i64>,
}

type PuzzleRow = (String, NaiveDate, Option<bool>, Option<i32>, Option<i32>, String, DateTime<Utc>);

fn streaks(days: &BTreeSet<NaiveDate>, today: NaiveDate) -> (i64, i64) {
    let mut cursor = if days.contains(&today) { today } else { today - Duration::days(1) };
    let mut current = 0;

    while days.contains(&cursor) {
        current += 1;
        cursor -= Duration::days(1);
    }

    let mut best = 0;
    let mut run = 0;
    let mut previous: Option<NaiveDate> = None;

    for day in days {
        run = if previous.is_some_and(|before| *day - before == Duration::days(1)) { run + 1 } else { 1 };
        best = best.max(run);
        previous = Some(*day);
    }

    (current, best)
}

fn mean(values: &[i32]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().map(|value| f64::from(*value)).sum::<f64>() / values.len() as f64)
}

pub async fn puzzles(State(state): State<AppState>, user: AuthUser, Query(query): Query<PuzzlesQuery>) -> ApiResult<Json<Vec<PuzzleSummary>>> {
    let count = query.days.unwrap_or(30).clamp(1, 366);
    let today = today_of(&state, user.id).await?;
    let first = today - Duration::days(count - 1);
    let stats_first = today - Duration::days(PUZZLE_STATS_DAYS - 1);
    let rows = sqlx::query_as::<_, PuzzleRow>("select puzzle, day, solved, score, seconds, source, recorded_at from puzzle_plays where owner = $1 order by day desc").bind(user.id).fetch_all(&state.pool).await?;

    let summaries = PUZZLES
        .iter()
        .map(|name| {
            let own: Vec<&PuzzleRow> = rows.iter().filter(|row| row.0 == *name).collect();
            let recent: Vec<&PuzzleRow> = own.iter().copied().filter(|row| row.1 >= stats_first).collect();
            let result = |row: &PuzzleRow| PuzzleResult { solved: row.2, score: row.3, seconds: row.4, source: Some(row.5.clone()) };
            let scores: Vec<i32> = recent.iter().filter_map(|row| row.3).collect();
            let times: Vec<i32> = recent.iter().filter_map(|row| row.4).collect();
            let streak_days: BTreeSet<NaiveDate> = own.iter().filter(|row| row.2 != Some(false)).map(|row| row.1).collect();
            let (streak, best_streak) = streaks(&streak_days, today);
            let mut distribution: BTreeMap<i32, i64> = BTreeMap::new();

            for score in &scores {
                *distribution.entry(*score).or_default() += 1;
            }

            PuzzleSummary {
                puzzle: name.to_string(),
                day: today.to_string(),
                today: own.iter().find(|row| row.1 == today).map(|row| result(row)),
                history: own.iter().filter(|row| row.1 >= first).map(|row| PuzzleDay { day: row.1.to_string(), result: result(row), recorded_at: row.6 }).collect(),
                stats: PuzzleStats {
                    played: recent.len() as i64,
                    solved: recent.iter().filter(|row| row.2 == Some(true)).count() as i64,
                    average: mean(&scores),
                    average_seconds: mean(&times),
                    streak,
                    best_streak,
                    distribution: distribution.into_iter().map(|(value, count)| PuzzleBucket { value, count }).collect(),
                },
            }
        })
        .collect();

    Ok(Json(summaries))
}

pub async fn hold(State(state): State<AppState>, user: AuthUser, Json(request): Json<HoldRequest>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    let Some(settings) = closure(&state, user.id, request.group_id).await? else { return Ok(StatusCode::NO_CONTENT) };
    let wanted = match request.kind.as_str() {
        "notification" => settings.quiet_hold_notifications,
        "ping" => settings.quiet_hold_pings,
        _ => return Err(ApiError::bad_request("hold_kind_invalid", "Hold a notification or a ping.")),
    };

    if wanted {
        sqlx::query("insert into quiet_holds (owner, kind, subject) values ($1, $2, $3) on conflict do nothing").bind(user.id).bind(&request.kind).bind(&request.subject).execute(&state.pool).await?;
        state.publish(Some(user.id), StreamMessage::WellnessChanged);
    }

    Ok(StatusCode::NO_CONTENT)
}

const BOUNDARY_TICK_SECONDS: u64 = 30;

pub async fn announce_boundaries(state: AppState) {
    let seen: Mutex<HashMap<Uuid, BTreeSet<Uuid>>> = Mutex::new(HashMap::new());

    crate::boot::run_while_ready(&state, "announce groups opening and closing", state.config.scheduler_tick.min(std::time::Duration::from_secs(BOUNDARY_TICK_SECONDS)), || announce_changes(&state, &seen)).await;
}

async fn announce_changes(state: &AppState, seen: &Mutex<HashMap<Uuid, BTreeSet<Uuid>>>) -> ApiResult<()> {
    let owners = sqlx::query_scalar::<_, Uuid>("select distinct owner from hours").fetch_all(&state.pool).await?;

    for owner in owners {
        let groups = sqlx::query_scalar::<_, Uuid>("select id from groups where owner = $1").bind(owner).fetch_all(&state.pool).await?;
        let mut closed = BTreeSet::new();

        if closure(state, owner, None).await?.is_some() {
            closed.insert(Uuid::nil());
        }

        for group in groups {
            if closure(state, owner, Some(group)).await?.is_some() {
                closed.insert(group);
            }
        }

        let before = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(owner, closed.clone());

        if before.is_some_and(|before| before != closed) {
            release_holds(state, owner).await?;
            state.publish(Some(owner), StreamMessage::WellnessChanged);
        }
    }

    Ok(())
}

pub async fn release_holds(state: &AppState, owner: Uuid) -> ApiResult<()> {
    let open = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(sqlx::AssertSqlSafe(format!(
        "select h.id, (select {CONVERSATION_GROUP} from conversations c where c.id::text = h.subject) from quiet_holds h where h.owner = $1 and h.released_at is null",
    )))
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;
    let mut still_closed: BTreeMap<Option<Uuid>, bool> = BTreeMap::new();
    let mut released = Vec::new();

    for (hold, group_id) in open {
        if !still_closed.contains_key(&group_id) {
            let closed = closure(state, owner, group_id).await?.is_some();

            still_closed.insert(group_id, closed);
        }

        if !still_closed[&group_id] {
            released.push(hold);
        }
    }

    sqlx::query("update quiet_holds set released_at = now() where id = any($1)").bind(&released).execute(&state.pool).await?;

    Ok(())
}

fn union_minutes_per_day(spans: Vec<(DateTime<Utc>, DateTime<Utc>)>, zone: Tz) -> BTreeMap<NaiveDate, i64> {
    let mut spans = spans;
    spans.sort();

    let mut merged: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();

    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    let mut seconds: BTreeMap<NaiveDate, i64> = BTreeMap::new();

    for (start, end) in merged {
        let mut cursor = start;

        while cursor < end {
            let local = cursor.with_timezone(&zone);
            let next_midnight = (local.date_naive() + Duration::days(1)).and_hms_opt(0, 0, 0).and_then(|midnight| midnight.and_local_timezone(zone).earliest()).map(|midnight| midnight.with_timezone(&Utc)).unwrap_or(end);
            let slice_end = end.min(next_midnight.max(cursor + Duration::seconds(1)));

            *seconds.entry(local.date_naive()).or_default() += (slice_end - cursor).num_seconds();
            cursor = slice_end;
        }
    }

    seconds.into_iter().map(|(day, seconds)| (day, seconds / 60)).collect()
}

#[derive(sqlx::FromRow)]
struct OutOfHoursRow {
    group_id: Uuid,
    group_name: String,
    reason: String,
    opened_at: DateTime<Utc>,
    until: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

pub async fn overview(State(state): State<AppState>, user: AuthUser, Query(query): Query<OverviewQuery>) -> ApiResult<Json<WellnessOverview>> {
    let count = query.days.unwrap_or(7).clamp(1, OVERVIEW_DAYS_LIMIT);
    let settings = load_settings(&state, user.id).await?;
    let zone = zone_of(&state, user.id, settings.time_zone.as_deref()).await?;
    let today = Utc::now().with_timezone(&zone).date_naive();
    let first = today - Duration::days(count - 1);
    let since = first.and_hms_opt(0, 0, 0).and_then(|midnight| midnight.and_local_timezone(zone).earliest()).map(|midnight| midnight.with_timezone(&Utc)).unwrap_or_else(Utc::now) - Duration::hours(1);

    let viewing = sqlx::query_as::<_, (NaiveDate, i32)>("select day, seconds from viewing_time where owner = $1 and day >= $2").bind(user.id).bind(first).fetch_all(&state.pool).await?;
    let turns = sqlx::query_as::<_, (DateTime<Utc>, i64)>("select ended_at, coalesce(working_ms, 0)::bigint from usage_turns where owner = $1 and ended_at >= $2 and coalesce(working_ms, 0) > 0").bind(user.id).bind(since).fetch_all(&state.pool).await?;
    let breaks = sqlx::query_as::<_, (DateTime<Utc>, Option<DateTime<Utc>>, DateTime<Utc>)>("select started_at, ended_early_at, ends_at from breaks where owner = $1 and started_at >= $2").bind(user.id).bind(since).fetch_all(&state.pool).await?;
    let spans = turns.into_iter().map(|(ended, working_ms)| (ended - Duration::milliseconds(working_ms), ended)).collect();
    let agent_minutes = union_minutes_per_day(spans, zone);
    let current_break = sqlx::query_as::<_, BreakRow>(sqlx::AssertSqlSafe(format!("select {BREAK_COLUMNS} from breaks where owner = $1 and ends_at > now() and ended_early_at is null order by started_at desc limit 1")))
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?
        .map(to_break);
    let awaiting_note = sqlx::query_as::<_, BreakRow>(sqlx::AssertSqlSafe(format!(
        "select {BREAK_COLUMNS} from breaks where owner = $1 and (ended_early_at is not null or ends_at <= now()) and noted_at is null order by started_at desc limit 1",
    )))
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .map(to_break);
    let recent_breaks = sqlx::query_as::<_, BreakRow>(sqlx::AssertSqlSafe(format!("select {BREAK_COLUMNS} from breaks where owner = $1 and note is not null order by started_at desc limit {RECENT_BREAKS}")))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(to_break)
        .collect();
    let last_break_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>("select max(coalesce(ended_early_at, ends_at)) from breaks where owner = $1 and coalesce(ended_early_at, ends_at) <= now()").bind(user.id).fetch_one(&state.pool).await?;
    let quiet = !is_open(&settings.hours, Utc::now().with_timezone(&zone).naive_local());
    let due = sqlx::query_scalar::<_, Uuid>("select id from schedules where owner = $1 and not paused and not proposed and next_run_at <= now()").bind(user.id).fetch_all(&state.pool).await?;
    let mut held = 0;

    for schedule in due {
        if schedule_closure(&state, user.id, schedule).await?.is_some_and(|closed| closed.quiet_hold_schedules) {
            held += 1;
        }
    }

    let days = (0..count)
        .map(|offset| {
            let day = first + Duration::days(offset);

            WellnessDay {
                day: day.to_string(),
                viewing_minutes: viewing.iter().filter(|(d, _)| *d == day).map(|(_, seconds)| *seconds as i64 / 60).sum(),
                agent_minutes: agent_minutes.get(&day).copied().unwrap_or(0),
                breaks: breaks.iter().filter(|(started, _, _)| started.with_timezone(&zone).date_naive() == day).count() as i64,
            }
        })
        .collect();

    release_holds(&state, user.id).await?;

    let held_kind = |kind: &'static str| sqlx::query_scalar::<_, i64>("select count(*) from quiet_holds where owner = $1 and kind = $2 and held_at > now() - interval '7 days'").bind(user.id).bind(kind).fetch_one(&state.pool);
    let schedules = sqlx::query_scalar::<_, i64>("select count(*) from schedule_runs where outcome = 'held' and ran_at > now() - interval '7 days' and schedule_id in (select id from schedules where owner = $1)").bind(user.id).fetch_one(&state.pool).await?;
    let held_week = HeldWeek { notifications: held_kind("notification").await?, schedules, pings: held_kind("ping").await? };

    let opened_out_of_hours = sqlx::query_as::<_, OutOfHoursRow>(
        "select o.group_id, g.name as group_name, o.reason, o.opened_at, o.until, o.closed_at from out_of_hours_opens o join groups g on g.id = o.group_id where o.owner = $1 and o.opened_at > now() - interval '7 days' order by o.opened_at desc",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| OutOfHoursOpen { group_id: row.group_id, group_name: row.group_name, reason: row.reason, opened_at: row.opened_at, until: row.until, closed_at: row.closed_at })
    .collect();

    Ok(Json(WellnessOverview { days, last_break_at, current_break, awaiting_note, recent_breaks, quiet_now: quiet, held, held_week, opened_out_of_hours }))
}

#[derive(Deserialize)]
pub struct ActivityDayQuery {
    date: Option<String>,
}

#[derive(Deserialize)]
pub struct ActivityHoursQuery {
    days: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct ActivitySpanRow {
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    kind: String,
    route: String,
    title: Option<String>,
    conversation_title: Option<String>,
    assistant_name: Option<String>,
    project: Option<String>,
    group_name: Option<String>,
    conversation_id: Option<Uuid>,
    assistant_id: Option<Uuid>,
}

impl ActivitySpanRow {
    fn title(&self) -> String {
        let title = match self.kind.as_str() {
            "assistant" => self.assistant_name.clone().or_else(|| self.title.clone()),
            "elsewhere" => self.title.clone().or_else(|| Some(self.route.clone())),
            _ => self.conversation_title.clone().or_else(|| self.title.clone()),
        };

        title.filter(|title| !title.is_empty()).unwrap_or_else(|| "Untitled".to_string())
    }

    fn headline(&self) -> String {
        match (&self.project, self.kind.as_str()) {
            (Some(project), "conversation") => format!("{project} {}", self.title()),
            _ => self.title(),
        }
    }
}

fn local_midnight(zone: Tz, date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0).and_then(|midnight| midnight.and_local_timezone(zone).earliest()).map(|midnight| midnight.with_timezone(&Utc)).unwrap_or_else(Utc::now)
}

async fn spans_between(state: &AppState, owner: Uuid, from: DateTime<Utc>, to: DateTime<Utc>) -> ApiResult<Vec<ActivitySpanRow>> {
    let spans = sqlx::query_as::<_, ActivitySpanRow>(
        "select s.started_at, s.ended_at, s.kind, s.route, s.title, c.title as conversation_title, a.name as assistant_name, p.name as project, g.name as group_name, s.conversation_id, s.assistant_id \
         from activity_spans s \
         left join conversations c on c.id = s.conversation_id \
         left join projects p on p.id = c.project_id \
         left join assistants a on a.id = s.assistant_id \
         left join groups g on g.id = coalesce(s.group_id, p.group_id, a.group_id) \
         where s.owner = $1 and s.ended_at > $2 and s.started_at < $3 order by s.started_at",
    )
    .bind(owner)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;

    Ok(spans)
}

pub async fn activity_day(State(state): State<AppState>, user: AuthUser, Query(query): Query<ActivityDayQuery>) -> ApiResult<Json<ActivityDay>> {
    let settings = load_settings(&state, user.id).await?;
    let zone = zone_of(&state, user.id, settings.time_zone.as_deref()).await?;
    let date = match query.date.as_deref() {
        Some(text) => text.parse::<NaiveDate>().map_err(|_| ApiError::bad_request("date_invalid", "Enter a date like 2026-10-28."))?,
        None => Utc::now().with_timezone(&zone).date_naive(),
    };
    let from = local_midnight(zone, date);
    let to = local_midnight(zone, date + Duration::days(1));
    let rows = spans_between(&state, user.id, from, to).await?;
    let spans: Vec<ActivitySpan> = rows
        .iter()
        .map(|row| ActivitySpan {
            start: row.started_at.max(from),
            end: row.ended_at.min(to),
            kind: row.kind.clone(),
            title: row.title(),
            project: row.project.clone().filter(|_| row.kind == "conversation"),
            group: row.group_name.clone(),
            conversation_id: row.conversation_id,
            assistant_id: row.assistant_id,
        })
        .collect();
    let on_screen_seconds: i64 = spans.iter().map(|span| (span.end - span.start).num_seconds()).sum();
    let breaks = sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>)>("select started_at, least(coalesce(ended_early_at, ends_at), now()) from breaks where owner = $1 and started_at < $3 and coalesce(ended_early_at, ends_at) > $2 order by started_at")
        .bind(user.id)
        .bind(from)
        .bind(to)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .filter(|(started, ended)| ended > started)
        .map(|(started, ended)| ActivityBreak { start: started.max(from), end: ended.min(to) })
        .collect();
    let recorded_since = sqlx::query_scalar::<_, Option<DateTime<Utc>>>("select min(started_at) from activity_spans where owner = $1").bind(user.id).fetch_one(&state.pool).await?.map(|first| first.with_timezone(&zone).date_naive().to_string());

    Ok(Json(ActivityDay { date: date.to_string(), on_screen_minutes: (on_screen_seconds + 30) / 60, spans, breaks, recorded_since }))
}

pub async fn activity_hours(State(state): State<AppState>, user: AuthUser, Query(query): Query<ActivityHoursQuery>) -> ApiResult<Json<ActivityHours>> {
    let count = query.days.unwrap_or(7).clamp(1, ACTIVITY_HOURS_DAYS_LIMIT);
    let settings = load_settings(&state, user.id).await?;
    let zone = zone_of(&state, user.id, settings.time_zone.as_deref()).await?;
    let today = Utc::now().with_timezone(&zone).date_naive();
    let first = today - Duration::days(count - 1);
    let from = local_midnight(zone, first);
    let to = local_midnight(zone, today + Duration::days(1));
    let rows = spans_between(&state, user.id, from, to).await?;
    let mut seconds: BTreeMap<(NaiveDate, usize), HashMap<String, i64>> = BTreeMap::new();

    for row in &rows {
        let headline = row.headline();
        let end = row.ended_at.min(to);
        let mut cursor = row.started_at.max(from);

        while cursor < end {
            let local = cursor.with_timezone(&zone);
            let into_hour = (local.minute() * 60 + local.second()) as i64;
            let slice_end = end.min(cursor - Duration::seconds(into_hour) + Duration::hours(1)).max(cursor + Duration::seconds(1));

            *seconds.entry((local.date_naive(), local.hour() as usize)).or_default().entry(headline.clone()).or_default() += (slice_end - cursor).num_seconds();
            cursor = slice_end;
        }
    }

    let days = (0..count)
        .map(|offset| {
            let date = first + Duration::days(offset);
            let mut minutes = vec![0; 24];
            let mut top = vec![None; 24];

            for hour in 0..24 {
                let Some(titles) = seconds.get(&(date, hour)) else { continue };

                minutes[hour] = (titles.values().sum::<i64>() + 30) / 60;
                top[hour] = titles.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(title, _)| title.clone());
            }

            ActivityHoursDay { date: date.to_string(), minutes, top }
        })
        .collect();

    Ok(Json(ActivityHours { days }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    fn workdays() -> Hours {
        Hours { always_open: false, ..default_hours() }
    }

    fn night_shift() -> Hours {
        Hours { always_open: false, days: (0..7u8).map(|weekday| HoursDay { weekday, closed: false, opens: "22:00".into(), closes: "06:00".into() }).collect() }
    }

    #[test]
    fn always_open_hours_are_open_at_any_time() {
        assert!(is_open(&default_hours(), at("2026-10-31 03:00")));
    }

    #[test]
    fn weekday_hours_close_in_the_evening_and_stay_closed_at_the_weekend() {
        assert!(is_open(&workdays(), at("2026-10-27 09:00")));
        assert!(is_open(&workdays(), at("2026-10-27 17:59")));
        assert!(!is_open(&workdays(), at("2026-10-27 18:00")));
        assert!(!is_open(&workdays(), at("2026-10-27 08:59")));
        assert!(!is_open(&workdays(), at("2026-10-31 12:00")));
    }

    #[test]
    fn a_closing_time_before_the_opening_time_runs_past_midnight() {
        assert!(is_open(&night_shift(), at("2026-10-27 23:00")));
        assert!(is_open(&night_shift(), at("2026-10-28 05:59")));
        assert!(!is_open(&night_shift(), at("2026-10-28 06:00")));
    }

    #[test]
    fn closing_at_24_00_covers_the_last_minute_of_the_day() {
        let mut hours = workdays();

        hours.days[1].closes = "24:00".into();

        assert!(is_open(&hours, at("2026-10-27 23:59")));
        assert!(!is_open(&hours, at("2026-10-28 00:00")));
    }

    #[test]
    fn the_next_change_is_the_closing_time_while_open_and_the_opening_time_while_closed() {
        assert_eq!(next_change(&[&workdays()], at("2026-10-27 12:00")), Some(at("2026-10-27 18:00")));
        assert_eq!(next_change(&[&workdays()], at("2026-10-27 19:00")), Some(at("2026-10-28 09:00")));
        assert_eq!(next_change(&[&workdays()], at("2026-10-30 19:00")), Some(at("2026-11-02 09:00")));
        assert_eq!(next_change(&[&default_hours()], at("2026-10-27 12:00")), None);
    }

    #[test]
    fn a_group_is_closed_while_the_app_is_closed_unless_it_was_opened_out_of_hours() {
        let app = workdays();
        let group = default_hours();
        let evening = Utc.with_ymd_and_hms(2026, 10, 27, 19, 0, 0).unwrap();

        assert!(!status_of(&app, Some(&group), None, Tz::UTC, evening).open_now);
        assert!(status_of(&app, Some(&group), Some(evening + Duration::minutes(10)), Tz::UTC, evening).open_now);
        assert!(!status_of(&app, Some(&group), Some(evening - Duration::minutes(1)), Tz::UTC, evening).open_now);
    }
}
