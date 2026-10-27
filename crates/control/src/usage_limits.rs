use crate::account_usage::windows_of;
use crate::auth::AuthUser;
use crate::boot::run_while_ready;
use crate::chats::CHAT_AGENT;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::prompts::{continue_unless_queued, retry_failed};
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, find_owned, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use mycelium_proto::api::{OnUsageLimit, ResumeRequest, UsageLimitSetting};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const RESUME_DELAY_SECONDS: i64 = 60;
const AUTOMATIC_RESUME_CAP: i32 = 3;
const DUE_BATCH: i64 = 20;
const LIMIT_REPLY_LENGTH: usize = 400;
const CONTINUE_TEXT: &str = "The usage limit has reset. Continue from where you stopped.";

#[derive(Debug, Clone, PartialEq)]
pub struct UsageLimit {
    pub window: String,
    pub resets_at: Option<DateTime<Utc>>,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

pub fn detect(message: &str, now: DateTime<Utc>, fallback_zone: Tz) -> Option<UsageLimit> {
    let lower = message.to_lowercase();
    let window = window_of(&lower)?;
    let resets_at = reset_of(&lower, now, fallback_zone);

    Some(UsageLimit { window: window.into(), resets_at })
}

fn window_of(lower: &str) -> Option<&'static str> {
    if lower.contains("session limit") || lower.contains("5-hour limit") || lower.contains("five-hour limit") {
        return Some("five_hour");
    }

    if lower.contains("opus limit") {
        return Some("seven_day_opus");
    }

    if lower.contains("weekly limit") || lower.contains("7-day limit") || lower.contains("seven-day limit") {
        return Some("seven_day");
    }

    if lower.contains("hit your usage limit") || lower.contains("usage limit reached") || lower.contains("hit your limit") {
        return Some("usage");
    }

    None
}

fn reset_of(lower: &str, now: DateTime<Utc>, fallback_zone: Tz) -> Option<DateTime<Utc>> {
    if let Some(epoch) = lower.split_once("usage limit reached|").and_then(|(_, rest)| rest.trim().parse::<i64>().ok()) {
        return DateTime::from_timestamp(epoch, 0);
    }

    let after = lower.split_once("resets").map(|(_, rest)| rest)?;
    let zone = zone_in(after).unwrap_or(fallback_zone);
    let words: Vec<&str> = after.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '·' || c == '∙').filter(|word| !word.is_empty() && *word != "at" && *word != "on").collect();
    let time_index = words.iter().position(|word| time_of(word).is_some())?;
    let time = time_of(words[time_index])?;
    let date = time_index.checked_sub(2).and_then(|start| date_of(words[start], words[start + 1]));
    let today = now.with_timezone(&zone).date_naive();

    match date {
        Some((month, day)) => (0..=1).find_map(|years| zoned(zone, NaiveDate::from_ymd_opt(today.year() + years, month, day)?, time).filter(|candidate| *candidate > now)),
        None => (0..=1).find_map(|days| zoned(zone, today + Duration::days(days), time).filter(|candidate| *candidate > now)),
    }
}

fn zone_in(text: &str) -> Option<Tz> {
    let inside = text.split_once('(')?.1.split_once(')')?.0.trim();

    inside.split('/').map(|part| part.split('_').map(capitalised).collect::<Vec<_>>().join("_")).collect::<Vec<_>>().join("/").parse().ok().or_else(|| inside.to_uppercase().parse().ok())
}

fn capitalised(word: &str) -> String {
    let mut characters = word.chars();

    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

fn time_of(word: &str) -> Option<NaiveTime> {
    let (digits, evening) = match (word.strip_suffix("am"), word.strip_suffix("pm")) {
        (Some(digits), _) => (digits, Some(false)),
        (_, Some(digits)) => (digits, Some(true)),
        _ => (word, None),
    };
    let (hour, minute) = digits.split_once(':').map_or((digits, "0"), |parts| parts);
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;

    if evening.is_none() && !digits.contains(':') {
        return None;
    }

    let hour = match evening {
        Some(evening) if (1..=12).contains(&hour) => hour % 12 + if evening { 12 } else { 0 },
        Some(_) => return None,
        None => hour,
    };

    NaiveTime::from_hms_opt(hour, minute, 0)
}

fn date_of(month: &str, day: &str) -> Option<(u32, u32)> {
    let month = MONTHS.iter().position(|name| month.starts_with(name))? as u32 + 1;
    let day = day.trim_end_matches(|c: char| c.is_alphabetic()).parse().ok()?;

    Some((month, day))
}

fn zoned(zone: Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    zone.from_local_datetime(&date.and_time(time)).earliest().map(|instant| instant.with_timezone(&Utc))
}

pub struct Detected {
    pub payload: Value,
    pub resume_at: Option<DateTime<Utc>>,
    pub switch_to: Option<String>,
    pub account: Option<String>,
}

pub async fn inspect(state: &AppState, node_id: Uuid, conversation_id: Uuid, message: &str) -> ApiResult<Option<Detected>> {
    let row = sqlx::query_as::<_, (String, Option<String>, i32, bool, Option<String>, String)>(
        "select c.agent, c.account, c.limit_resumes, c.parent_id is not null, u.time_zone, coalesce(s.on_usage_limit, 'resume')
         from conversations c join users u on u.id = c.owner left join user_settings s on s.owner = c.owner where c.id = $1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((agent, account, resumes, side_chat, time_zone, on_usage_limit)) = row else {
        return Ok(None);
    };
    let now = Utc::now();
    let zone = time_zone.and_then(|name| name.parse().ok()).unwrap_or(Tz::UTC);

    let Some(mut limit) = detect(message, now, zone) else {
        return Ok(None);
    };

    if limit.resets_at.is_none() {
        let windows = windows_of(state, node_id, parse_enum(&agent)?, account.as_deref()).await?;
        let wanted = windows.iter().filter(|window| window.name == limit.window).filter_map(|window| window.resets_at);
        let any = windows.iter().filter_map(|window| window.resets_at);
        limit.resets_at = wanted.filter(|at| *at > now).min().or_else(|| any.filter(|at| *at > now).min());
    }

    let automatic = matches!(on_usage_limit.as_str(), "resume" | "switch") && !side_chat && resumes < AUTOMATIC_RESUME_CAP;
    let switching = on_usage_limit == "switch" && !side_chat && agent != CHAT_AGENT;
    let switch_to = if switching {
        sqlx::query_scalar::<_, String>(
            "select account from logins where node_id = $1 and agent = $2 and state = 'signed_in' and account is not null and account is distinct from $3 and coalesce(usage_left, 0) > 0
             order by usage_left desc, account limit 1",
        )
        .bind(node_id)
        .bind(&agent)
        .bind(&account)
        .fetch_optional(&state.pool)
        .await?
    } else {
        None
    };
    let resume_at = limit.resets_at.filter(|_| automatic).map(|at| at + Duration::seconds(RESUME_DELAY_SECONDS));

    Ok(Some(Detected { payload: json!({"message": message, "usage_limit": {"window": limit.window, "resets_at": limit.resets_at}}), resume_at, switch_to, account }))
}

pub async fn inspect_reply(state: &AppState, node_id: Uuid, conversation_id: Uuid) -> ApiResult<Option<(String, Detected)>> {
    let reply = sqlx::query_scalar::<_, Option<String>>(
        "select payload->'update'->'content'->>'text' from events
         where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
           and seq > coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message'), 0)
         order by seq desc limit 1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?
    .flatten();
    let Some(message) = reply.map(|text| text.trim().to_string()).filter(|text| text.chars().count() <= LIMIT_REPLY_LENGTH) else {
        return Ok(None);
    };
    let limit = inspect(state, node_id, conversation_id, &message).await?;

    Ok(limit.map(|limit| (message, limit)))
}

pub async fn inspect_workflow_line(state: &AppState, node_id: Uuid, conversation_id: Uuid, line: &Value) -> ApiResult<Option<Value>> {
    if !matches!(line["type"].as_str(), Some("failed" | "result")) {
        return Ok(None);
    }

    let text = ["message", "reason", "result"].iter().find_map(|field| line[*field].as_str()).map(str::trim).filter(|text| text.chars().count() <= LIMIT_REPLY_LENGTH);
    let Some(text) = text else {
        return Ok(None);
    };
    let Some(limit) = inspect(state, node_id, conversation_id, text).await? else {
        return Ok(None);
    };

    if let Some(at) = limit.resume_at {
        schedule(state, conversation_id, at).await?;
    }

    Ok(Some(limit.payload["usage_limit"].clone()))
}

pub async fn schedule(state: &AppState, conversation_id: Uuid, at: DateTime<Utc>) -> ApiResult<()> {
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set resume_at = $2 where id = $1 and resume_at is distinct from $2 returning {CONVERSATION_COLUMNS}")))
        .bind(conversation_id)
        .bind(at)
        .fetch_optional(&state.pool)
        .await?;

    if let Some(row) = row {
        state.publish(Some(row.owner), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(())
}

pub async fn resume(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<ResumeRequest>>) -> ApiResult<StatusCode> {
    let row = find_owned(&state.pool, id, user.id).await?;

    let limit = latest_limit(&state, id).await?;

    if row.status != "failed" && limit.is_none() {
        return Err(ApiError::conflict("not_failed", "This conversation has not stopped, so there is nothing to resume."));
    }

    let chosen = request.and_then(|Json(request)| request.at);
    let at = match chosen {
        Some(at) => at,
        None => limit.flatten().map(|at| at + Duration::seconds(RESUME_DELAY_SECONDS)).ok_or_else(|| ApiError::conflict("no_reset_time", "Mycelium does not know when the limit resets. Choose a time to resume."))?,
    };

    schedule(&state, id, at.max(Utc::now())).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_resume(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set resume_at = null where id = $1 and resume_at is not null returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    if let Some(row) = row {
        state.publish(Some(row.owner), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn latest_limit(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<Option<DateTime<Utc>>>> {
    let payload = sqlx::query_scalar::<_, Value>("select payload from events where conversation_id = $1 and kind in ('error', 'workflow') and payload ? 'usage_limit' order by seq desc limit 1").bind(conversation_id).fetch_optional(&state.pool).await?;

    Ok(payload.map(|payload| payload["usage_limit"]["resets_at"].as_str().and_then(|at| at.parse().ok())))
}

pub async fn get_setting(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<UsageLimitSetting>> {
    let stored = sqlx::query_scalar::<_, String>("select on_usage_limit from user_settings where owner = $1").bind(user.id).fetch_optional(&state.pool).await?;
    let on_usage_limit = match stored {
        Some(text) => parse_enum(&text)?,
        None => OnUsageLimit::Resume,
    };

    Ok(Json(UsageLimitSetting { on_usage_limit }))
}

pub async fn save_setting(State(state): State<AppState>, user: AuthUser, Json(request): Json<UsageLimitSetting>) -> ApiResult<Json<UsageLimitSetting>> {
    sqlx::query("insert into user_settings (owner, on_usage_limit) values ($1, $2) on conflict (owner) do update set on_usage_limit = excluded.on_usage_limit")
        .bind(user.id)
        .bind(crate::store::enum_text(&request.on_usage_limit))
        .execute(&state.pool)
        .await?;

    Ok(Json(request))
}

pub async fn resume_regularly(state: AppState) {
    run_while_ready(&state, "resume the conversations that reached a usage limit", state.config.scheduler_tick, || resume_due(&state)).await;
}

async fn resume_due(state: &AppState) -> ApiResult<()> {
    let due = sqlx::query_as::<_, (Uuid, Uuid)>(sqlx::AssertSqlSafe(format!(
        "update conversations set resume_at = null, limit_resumes = limit_resumes + 1 where id in (
             select id from conversations where resume_at <= now() order by resume_at limit {DUE_BATCH} for update skip locked
         ) returning id, owner",
    )))
    .fetch_all(&state.pool)
    .await?;

    for (id, owner) in due {
        let resumed = resume_one(state, owner, id).await;

        if let Err(cause) = resumed {
            tracing::warn!("could not resume {id} after its usage limit, trying again in a minute: {}", cause.message());
            sqlx::query("update conversations set resume_at = now() + make_interval(secs => $2), limit_resumes = limit_resumes - 1 where id = $1 and status = 'failed'").bind(id).bind(RESUME_DELAY_SECONDS as f64).execute(&state.pool).await?;
        }
    }

    Ok(())
}

async fn resume_one(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let row = find_owned(&state.pool, id, owner).await?;

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    retry_failed(state, owner, id).await?;

    continue_unless_queued(state, owner, id, CONTINUE_TEXT).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn detected(message: &str, now: &str) -> Option<UsageLimit> {
        detect(message, at(now), Tz::UTC)
    }

    #[test]
    fn a_session_limit_without_a_date_resets_later_today_in_its_zone() {
        let limit = detected("Internal error: You've hit your session limit · resets 4:30pm (Europe/London)", "2026-10-23T12:00:00Z").unwrap();

        assert_eq!(limit, UsageLimit { window: "five_hour".into(), resets_at: Some(at("2026-10-23T15:30:00Z")) });
    }

    #[test]
    fn a_time_that_has_passed_today_means_tomorrow() {
        let limit = detected("You've hit your session limit · resets 4:30pm (Europe/London)", "2026-10-23T16:00:00Z").unwrap();

        assert_eq!(limit.resets_at, Some(at("2026-10-24T15:30:00Z")));
    }

    #[test]
    fn a_reset_just_after_midnight_is_found_across_midnight() {
        let limit = detected("You've hit your session limit · resets 12:15am (Europe/London)", "2026-10-23T22:50:00Z").unwrap();

        assert_eq!(limit.resets_at, Some(at("2026-10-23T23:15:00Z")));
    }

    #[test]
    fn other_zones_are_read_as_their_own_local_time() {
        let limit = detected("You've hit your session limit · resets 9am (America/New_York)", "2026-10-23T05:00:00Z").unwrap();

        assert_eq!(limit.resets_at, Some(at("2026-10-23T13:00:00Z")));
    }

    #[test]
    fn a_weekly_limit_with_a_date_resets_on_that_date() {
        let limit = detected("You've hit your weekly limit · resets Oct 28, 9am (Europe/London)", "2026-10-23T12:00:00Z").unwrap();

        assert_eq!(limit, UsageLimit { window: "seven_day".into(), resets_at: Some(at("2026-10-28T09:00:00Z")) });
    }

    #[test]
    fn a_date_that_has_passed_this_year_means_next_year() {
        let limit = detected("You've hit your weekly limit · resets Jan 2 at 9am (UTC)", "2026-12-30T12:00:00Z").unwrap();

        assert_eq!(limit.resets_at, Some(at("2027-01-02T09:00:00Z")));
    }

    #[test]
    fn an_opus_limit_is_its_own_window() {
        let limit = detected("You've hit your Opus limit · resets 3pm (UTC)", "2026-10-23T12:00:00Z").unwrap();

        assert_eq!(limit.window, "seven_day_opus");
    }

    #[test]
    fn a_message_without_a_zone_uses_the_fallback_zone() {
        let limit = detect("You've hit your session limit · resets 4:30pm", at("2026-10-23T12:00:00Z"), Tz::Europe__London).unwrap();

        assert_eq!(limit.resets_at, Some(at("2026-10-23T15:30:00Z")));
    }

    #[test]
    fn the_older_message_carries_the_reset_as_an_epoch() {
        let limit = detected("Claude AI usage limit reached|1792000000", "2026-10-23T12:00:00Z").unwrap();

        assert_eq!(limit.resets_at, DateTime::from_timestamp(1792000000, 0));
    }

    #[test]
    fn a_usage_limit_without_a_reset_time_is_still_a_usage_limit() {
        let limit = detected("You've hit your usage limit. Upgrade to Pro or try again later.", "2026-10-23T12:00:00Z").unwrap();

        assert_eq!(limit, UsageLimit { window: "usage".into(), resets_at: None });
    }

    #[test]
    fn other_failures_are_not_usage_limits() {
        assert_eq!(detected("Internal error: the keeper is unreachable", "2026-10-23T12:00:00Z"), None);
        assert_eq!(detected("The agent could not start: Method not found", "2026-10-23T12:00:00Z"), None);
    }
}
