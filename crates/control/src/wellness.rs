use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use mycelium_proto::api::{Break, QuietDay, StartBreakRequest, WellnessDay, WellnessHeartbeat, WellnessOverview, WellnessSettings};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

const PUZZLES: [&str; 4] = ["wordle", "connections", "daily-orbs", "catfishing"];
const HEARTBEAT_CAP_SECONDS: i32 = 90;
const OVERVIEW_DAYS_LIMIT: i64 = 31;

type SettingsRow = (bool, i32, i32, i32, bool, bool, bool, bool, bool, Option<String>);

const SETTINGS_COLUMNS: &str = "break_reminders, break_after_minutes, snooze_minutes, break_minutes, pomodoro, quiet_hours, quiet_hold_notifications, quiet_hold_schedules, quiet_hold_pings, time_zone";

#[derive(Deserialize)]
pub struct OverviewQuery {
    days: Option<i64>,
}

fn default_quiet_days() -> Vec<QuietDay> {
    (0..7u8).map(|weekday| QuietDay { weekday, enabled: true, all_day: weekday >= 5, starts: "22:00".into(), ends: "08:00".into() }).collect()
}

fn parse_time(text: &str) -> ApiResult<NaiveTime> {
    NaiveTime::parse_from_str(text, "%H:%M").map_err(|_| ApiError::bad_request("quiet_time_invalid", "Enter quiet hours as a time like 22:00."))
}

fn require_range(name: &'static str, message: &'static str, value: i32, low: i32, high: i32) -> ApiResult<()> {
    if value < low || value > high {
        return Err(ApiError::bad_request(name, message));
    }

    Ok(())
}

pub fn is_quiet(days: &[QuietDay], at: NaiveDateTime) -> bool {
    let today = at.weekday().num_days_from_monday() as u8;
    let yesterday = (today + 6) % 7;
    let time = at.time();
    let in_today = days.iter().filter(|day| day.enabled && day.weekday == today).any(|day| {
        if day.all_day {
            return true;
        }

        let (Ok(starts), Ok(ends)) = (parse_time(&day.starts), parse_time(&day.ends)) else { return false };

        if ends > starts { time >= starts && time < ends } else { time >= starts }
    });
    let in_yesterdays_overnight = days.iter().filter(|day| day.enabled && !day.all_day && day.weekday == yesterday).any(|day| {
        let (Ok(starts), Ok(ends)) = (parse_time(&day.starts), parse_time(&day.ends)) else { return false };

        ends <= starts && time < ends
    });

    in_today || in_yesterdays_overnight
}

async fn zone_of(state: &AppState, owner: Uuid, saved: Option<&str>) -> ApiResult<Tz> {
    if let Some(zone) = saved.and_then(|name| name.parse::<Tz>().ok()) {
        return Ok(zone);
    }

    let stored = sqlx::query_scalar::<_, Option<String>>("select time_zone from users where id = $1").bind(owner).fetch_one(&state.pool).await?;

    Ok(stored.and_then(|name| name.parse::<Tz>().ok()).unwrap_or(Tz::UTC))
}

async fn load_settings(state: &AppState, owner: Uuid) -> ApiResult<WellnessSettings> {
    let row = sqlx::query_as::<_, SettingsRow>(sqlx::AssertSqlSafe(format!("select {SETTINGS_COLUMNS} from user_settings where owner = $1"))).bind(owner).fetch_optional(&state.pool).await?;
    let row = row.unwrap_or((true, 50, 10, 5, false, false, true, true, true, None));
    let stored = sqlx::query_as::<_, (i16, bool, bool, NaiveTime, NaiveTime)>("select weekday, enabled, all_day, starts, ends from quiet_hours where owner = $1 order by weekday").bind(owner).fetch_all(&state.pool).await?;
    let mut quiet_days = default_quiet_days();

    for (weekday, enabled, all_day, starts, ends) in stored {
        quiet_days[weekday as usize] = QuietDay { weekday: weekday as u8, enabled, all_day, starts: starts.format("%H:%M").to_string(), ends: ends.format("%H:%M").to_string() };
    }

    Ok(WellnessSettings {
        break_reminders: row.0,
        break_after_minutes: row.1,
        snooze_minutes: row.2,
        break_minutes: row.3,
        pomodoro: row.4,
        quiet_hours: row.5,
        quiet_hold_notifications: row.6,
        quiet_hold_schedules: row.7,
        quiet_hold_pings: row.8,
        time_zone: row.9,
        quiet_days,
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
        "insert into user_settings (owner, break_reminders, break_after_minutes, snooze_minutes, break_minutes, pomodoro, quiet_hours, quiet_hold_notifications, quiet_hold_schedules, quiet_hold_pings, time_zone)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         on conflict (owner) do update set break_reminders = excluded.break_reminders, break_after_minutes = excluded.break_after_minutes, snooze_minutes = excluded.snooze_minutes, break_minutes = excluded.break_minutes,
            pomodoro = excluded.pomodoro, quiet_hours = excluded.quiet_hours, quiet_hold_notifications = excluded.quiet_hold_notifications, quiet_hold_schedules = excluded.quiet_hold_schedules,
            quiet_hold_pings = excluded.quiet_hold_pings, time_zone = excluded.time_zone",
    )
    .bind(user.id)
    .bind(request.break_reminders)
    .bind(request.break_after_minutes)
    .bind(request.snooze_minutes)
    .bind(request.break_minutes)
    .bind(request.pomodoro)
    .bind(request.quiet_hours)
    .bind(request.quiet_hold_notifications)
    .bind(request.quiet_hold_schedules)
    .bind(request.quiet_hold_pings)
    .bind(&request.time_zone)
    .execute(&mut *tx)
    .await?;

    for day in &request.quiet_days {
        if day.weekday > 6 {
            return Err(ApiError::bad_request("weekday_invalid", "Quiet hours cover weekdays 0 to 6, Monday to Sunday."));
        }

        sqlx::query(
            "insert into quiet_hours (owner, weekday, enabled, all_day, starts, ends) values ($1, $2, $3, $4, $5, $6)
             on conflict (owner, weekday) do update set enabled = excluded.enabled, all_day = excluded.all_day, starts = excluded.starts, ends = excluded.ends",
        )
        .bind(user.id)
        .bind(day.weekday as i16)
        .bind(day.enabled)
        .bind(day.all_day)
        .bind(parse_time(&day.starts)?)
        .bind(parse_time(&day.ends)?)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(load_settings(&state, user.id).await?))
}

pub async fn quiet_settings(state: &AppState, owner: Uuid) -> ApiResult<Option<WellnessSettings>> {
    let settings = load_settings(state, owner).await?;

    if !settings.quiet_hours {
        return Ok(None);
    }

    let zone = zone_of(state, owner, settings.time_zone.as_deref()).await?;
    let quiet = is_quiet(&settings.quiet_days, Utc::now().with_timezone(&zone).naive_local());

    Ok(quiet.then_some(settings))
}

pub async fn heartbeat(State(state): State<AppState>, user: AuthUser, Json(request): Json<WellnessHeartbeat>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    let seconds = request.seconds.clamp(0, HEARTBEAT_CAP_SECONDS);
    let settings = load_settings(&state, user.id).await?;
    let zone = zone_of(&state, user.id, settings.time_zone.as_deref()).await?;
    let day = Utc::now().with_timezone(&zone).date_naive();

    sqlx::query("insert into viewing_time (owner, day, seconds) values ($1, $2, $3) on conflict (owner, day) do update set seconds = viewing_time.seconds + excluded.seconds")
        .bind(user.id)
        .bind(day)
        .bind(seconds)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_break(State(state): State<AppState>, user: AuthUser, Json(request): Json<StartBreakRequest>) -> ApiResult<(StatusCode, Json<Break>)> {
    user.require_full_access()?;

    let settings = load_settings(&state, user.id).await?;
    let minutes = request.minutes.unwrap_or(settings.break_minutes);

    require_range("break_length_invalid", "Choose a break between 1 and 120 minutes.", minutes, 1, 120)?;

    let started = sqlx::query_as::<_, (Uuid, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>(
        "insert into breaks (owner, ends_at) values ($1, now() + make_interval(mins => $2)) returning id, started_at, ends_at, ended_early_at",
    )
    .bind(user.id)
    .bind(minutes)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok((StatusCode::CREATED, Json(Break { id: started.0, started_at: started.1, ends_at: started.2, ended_early_at: started.3 })))
}

pub async fn end_break(State(state): State<AppState>, user: AuthUser) -> ApiResult<StatusCode> {
    user.require_full_access()?;

    sqlx::query("update breaks set ended_early_at = now() where owner = $1 and ends_at > now() and ended_early_at is null").bind(user.id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(StatusCode::NO_CONTENT)
}

fn require_puzzle(puzzle: &str) -> ApiResult<()> {
    if !PUZZLES.contains(&puzzle) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "puzzle_not_found", "That puzzle is not on the list. Choose wordle, connections, daily-orbs or catfishing."));
    }

    Ok(())
}

async fn today_of(state: &AppState, owner: Uuid) -> ApiResult<NaiveDate> {
    let settings = load_settings(state, owner).await?;
    let zone = zone_of(state, owner, settings.time_zone.as_deref()).await?;

    Ok(Utc::now().with_timezone(&zone).date_naive())
}

pub async fn mark_puzzle(State(state): State<AppState>, user: AuthUser, Path(puzzle): Path<String>) -> ApiResult<StatusCode> {
    user.require_full_access()?;
    require_puzzle(&puzzle)?;

    let day = today_of(&state, user.id).await?;

    sqlx::query("insert into puzzle_plays (owner, puzzle, day) values ($1, $2, $3) on conflict do nothing").bind(user.id).bind(&puzzle).bind(day).execute(&state.pool).await?;
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
    let puzzles = sqlx::query_as::<_, (NaiveDate, String)>("select day, puzzle from puzzle_plays where owner = $1 and day >= $2 order by puzzle").bind(user.id).bind(first).fetch_all(&state.pool).await?;
    let spans = turns.into_iter().map(|(ended, working_ms)| (ended - Duration::milliseconds(working_ms), ended)).collect();
    let agent_minutes = union_minutes_per_day(spans, zone);
    let current_break = sqlx::query_as::<_, (Uuid, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>("select id, started_at, ends_at, ended_early_at from breaks where owner = $1 and ends_at > now() and ended_early_at is null order by started_at desc limit 1")
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?
        .map(|row| Break { id: row.0, started_at: row.1, ends_at: row.2, ended_early_at: row.3 });
    let last_break_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>("select max(coalesce(ended_early_at, ends_at)) from breaks where owner = $1 and coalesce(ended_early_at, ends_at) <= now()").bind(user.id).fetch_one(&state.pool).await?;
    let quiet = settings.quiet_hours && is_quiet(&settings.quiet_days, Utc::now().with_timezone(&zone).naive_local());
    let held = if quiet && settings.quiet_hold_schedules {
        sqlx::query_scalar::<_, i64>("select count(*) from schedules where owner = $1 and not paused and not proposed and next_run_at <= now()").bind(user.id).fetch_one(&state.pool).await?
    } else {
        0
    };

    let days = (0..count)
        .map(|offset| {
            let day = first + Duration::days(offset);

            WellnessDay {
                day: day.to_string(),
                viewing_minutes: viewing.iter().filter(|(d, _)| *d == day).map(|(_, seconds)| *seconds as i64 / 60).sum(),
                agent_minutes: agent_minutes.get(&day).copied().unwrap_or(0),
                breaks: breaks.iter().filter(|(started, _, _)| started.with_timezone(&zone).date_naive() == day).count() as i64,
                puzzles: puzzles.iter().filter(|(d, _)| *d == day).map(|(_, puzzle)| puzzle.clone()).collect(),
            }
        })
        .collect();

    Ok(Json(WellnessOverview { days, last_break_at, current_break, quiet_now: quiet, held }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn quiet_window_spanning_midnight_ends_next_morning() {
        let days = default_quiet_days();

        assert!(is_quiet(&days, at("2026-10-26 23:00")));
        assert!(is_quiet(&days, at("2026-10-27 07:59")));
        assert!(!is_quiet(&days, at("2026-10-27 08:00")));
        assert!(!is_quiet(&days, at("2026-10-27 12:00")));
    }

    #[test]
    fn friday_night_runs_into_all_day_saturday_and_sunday_night_into_monday_morning() {
        let days = default_quiet_days();

        assert!(is_quiet(&days, at("2026-10-31 15:00")));
        assert!(is_quiet(&days, at("2026-11-01 15:00")));
        assert!(!is_quiet(&days, at("2026-11-02 08:00")));
    }
}
