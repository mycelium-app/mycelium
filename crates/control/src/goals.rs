use crate::account_usage;
use crate::auth::AuthUser;
use crate::boot::run_while_ready;
use crate::error::{ApiError, ApiResult};
use crate::prompts::pump;
use crate::state::AppState;
use crate::store::{append_event, enum_text, find_owned, parse_enum, publish_queue};
use axum::Json;
use chrono::{DateTime, Utc};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ContinueGoalRequest, Goal, GoalDelivery, GoalState, GoalWait, PatchGoalRequest, SetGoalRequest};
use mycelium_proto::conversation::EventKind;
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

const PAUSED_BY_STOP: &str = "Paused because you stopped the turn. Resume to let the agent carry on.";
const AGENT_STOPPED: &str = "The agent stopped before the turn ended, so the goal was stopped.";
const FIVE_HOUR_WINDOW: &str = "five_hour";
const WEEKLY_WINDOW: &str = "seven_day";
pub const GOAL_ORIGIN: &str = "goal";
const SEND_AGAIN_UNNEEDED: &str = "The goal reached the agent, so there is nothing to send again.";
const OVER_BUDGET: &str = "The goal is not met and the token budget is used up. Set the goal again with a larger budget to carry on.";
const DEFAULT_REPEAT_LIMIT: i64 = 3;
const MINIMUM_REPEAT_LIMIT: i64 = 2;
const BACKOFF_SECONDS: i64 = 120;
const BACKGROUND_WAIT_SECONDS: i64 = 600;
const NUDGE_TICK: Duration = Duration::from_secs(10);
const SUMMARY_LIMIT: usize = 120;
const WHY_SET: &str = "you set the goal";
const WHY_SENT_AGAIN: &str = "you chose to send the goal again";
const WHY_NO_REPORT: &str = "the turn ended without a report, and the budget and time limit have room";
const WHY_NUDGED_NOW: &str = "you chose to nudge now";
const WHY_RESUMED: &str = "you resumed the goal";
const WHY_CONTINUED: &str = "you chose to continue the goal";
const NOT_WAITING: &str = "Mycelium is not waiting to nudge this goal.";
const NOT_IDLE: &str = "The agent is still working on a turn. Mycelium nudges when it ends.";
const NOT_BLOCKED: &str = "The agent has not reported this goal blocked, so there is nothing to continue.";

async fn latest_agent_text(state: &AppState, conversation_id: Uuid) -> ApiResult<String> {
    let text = sqlx::query_scalar::<_, String>(
        "select coalesce(string_agg(payload->'update'->'content'->>'text', '' order by seq), '') from events
         where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
           and seq > coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message'), 0)",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(text)
}

async fn changed_files(state: &AppState, conversation_id: Uuid) -> ApiResult<String> {
    let files = sqlx::query_scalar::<_, String>(
        "select coalesce(string_agg(distinct path, ',' order by path), '') from (
            select payload->'update'->'locations'->0->>'path' as path from events
            where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'kind' in ('edit', 'delete', 'move')
              and seq > coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message'), 0)
         ) changed where path is not null",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(files)
}

fn summary_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("no reply");

    line.chars().take(SUMMARY_LIMIT).collect()
}

#[derive(sqlx::FromRow)]
struct GoalRow {
    text: String,
    token_budget: Option<i64>,
    time_limit_seconds: Option<i64>,
    five_hour_limit: Option<i16>,
    weekly_limit: Option<i16>,
    state: String,
    reason: Option<String>,
    started_at: DateTime<Utc>,
    delivery_state: String,
    delivered_at: Option<DateTime<Utc>>,
    delivery_reason: Option<String>,
    held_since: Option<DateTime<Utc>>,
    held_seconds: i64,
    resume_at: Option<DateTime<Utc>>,
    node_id: Uuid,
    agent: String,
    account: Option<String>,
    nudges: i32,
    repeats: i32,
    repeat_limit: Option<i32>,
    next_nudge_at: Option<DateTime<Utc>>,
    wait_reason: Option<String>,
    reported_at: Option<DateTime<Utc>>,
}

async fn read(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<Goal>> {
    let row = sqlx::query_as::<_, GoalRow>(
        "select g.text, g.token_budget, g.time_limit_seconds, g.five_hour_limit, g.weekly_limit, g.state, g.reason, g.started_at,
                g.delivery_state, g.delivered_at, g.delivery_reason, g.held_since, g.held_seconds, c.resume_at, c.node_id, c.agent, c.account,
                g.nudges, g.repeats, g.repeat_limit, g.next_nudge_at, g.wait_reason, g.reported_at
         from goals g join conversations c on c.id = g.conversation_id where g.conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(GoalRow { text, token_budget, time_limit_seconds, five_hour_limit, weekly_limit, state: goal_state, reason, started_at, delivery_state, delivered_at, delivery_reason, held_since, held_seconds, resume_at, node_id, agent, account, nudges, repeats, repeat_limit, next_nudge_at, wait_reason, reported_at }) = row else {
        return Ok(None);
    };

    let (tokens_used, turns, tokens_known) = sqlx::query_as::<_, (i64, i64, bool)>(
        "with samples as (
            select conversation_id, recorded_at,
                   case when not cumulative then tokens
                        when lag(tokens) over w is null then last_tokens
                        when tokens < lag(tokens) over w then tokens
                        else tokens - lag(tokens) over w end as tokens
            from conversation_usage_samples
            window w as (partition by conversation_id, source order by position)
         ), scope as (
            select c.id, g.started_at, g.conversation_id as parent
            from conversations c join goals g on (c.id = g.conversation_id or (c.parent_id = g.conversation_id and c.side_kind = 'check'))
            where g.conversation_id = $1
         )
         select coalesce(sum(greatest(
                    (select coalesce(sum(u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens),0) from usage_turns u where u.conversation_id = s.id and u.ended_at >= s.started_at),
                    (select coalesce(sum(u.tokens),0) from samples u where u.conversation_id = s.id and u.recorded_at >= s.started_at))),0)::bigint,
                (select count(*)::bigint from usage_turns u join goals g on g.conversation_id = u.conversation_id where g.conversation_id = $1 and u.ended_at >= g.started_at),
                coalesce(bool_or(exists(select 1 from usage_turns u where u.conversation_id = s.id and u.ended_at >= s.started_at and u.reported)
                              or exists(select 1 from samples u where u.conversation_id = s.id and u.recorded_at >= s.started_at)), false)
         from scope s",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;
    let turn_active: bool = sqlx::query_scalar("select agent_active or exists(select 1 from events e where e.conversation_id = c.id and e.kind = 'user_message' and e.seq > coalesce((select max(seq) from events where conversation_id = c.id and kind = 'turn_end'),0)) from conversations c where c.id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    let windows = if agent == crate::chats::CHAT_AGENT { Vec::new() } else { account_usage::windows_of(state, node_id, parse_enum(&agent)?, account.as_deref()).await? };
    let used = |name: &str| windows.iter().find(|window| window.name == name).map(|window| window.used_percent);

    Ok(Some(Goal {
        conversation_id,
        text,
        token_budget,
        time_limit_seconds,
        five_hour_limit: five_hour_limit.and_then(|limit| u8::try_from(limit).ok()),
        weekly_limit: weekly_limit.and_then(|limit| u8::try_from(limit).ok()),
        tokens_used,
        tokens_known,
        turns,
        turn_active,
        started_at,
        five_hour_used: used(FIVE_HOUR_WINDOW),
        weekly_used: used(WEEKLY_WINDOW),
        state: parse_enum(&goal_state)?,
        reason,
        delivery: parse_enum(&delivery_state)?,
        delivered_at,
        delivery_reason,
        held_since,
        held_seconds,
        resumes_at: resume_at.filter(|_| held_since.is_some()),
        nudges: i64::from(nudges),
        repeats: i64::from(repeats),
        repeat_limit: repeat_limit.map(i64::from),
        next_nudge_at,
        wait_reason: wait_reason.as_deref().map(parse_enum::<GoalWait>).transpose()?,
        reported_at,
    }))
}

fn reached_limit(goal: &Goal) -> Option<(GoalState, String)> {
    if goal.token_budget.is_some_and(|budget| goal.tokens_used >= budget) {
        return Some((GoalState::OverBudget, OVER_BUDGET.to_string()));
    }

    if let Some(limit) = goal.time_limit_seconds {
        let now = Utc::now();
        let on_hold = goal.held_since.map_or(0, |since| (now - since).num_seconds());
        let elapsed = (now - goal.started_at).num_seconds() - goal.held_seconds - on_hold;

        if elapsed >= limit {
            return Some((GoalState::LimitReached, format!("Time limit reached after {}. Set the goal again with a longer limit to carry on.", duration_text(limit))));
        }
    }

    let windows = [("5-hour", goal.five_hour_limit, goal.five_hour_used), ("Weekly", goal.weekly_limit, goal.weekly_used)];

    for (name, limit, used) in windows {
        if let (Some(limit), Some(used)) = (limit, used)
            && used >= f64::from(limit)
        {
            return Some((GoalState::LimitReached, format!("{name} window at {}%, over your limit of {limit}%. Set the goal again to carry on.", used.round())));
        }
    }

    None
}

fn duration_text(seconds: i64) -> String {
    match (seconds / 3600, seconds % 3600 / 60) {
        (0, 0) => format!("{seconds}s"),
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

async fn announce(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    match read(state, conversation_id).await? {
        Some(goal) => state.publish(Some(owner), StreamMessage::Goal(goal)),
        None => state.publish(Some(owner), StreamMessage::GoalCleared { conversation_id }),
    }

    Ok(())
}

async fn end(state: &AppState, owner: Uuid, conversation_id: Uuid, goal_state: GoalState, reason: &str) -> ApiResult<()> {
    sqlx::query("update goals set state = $2, reason = $3, wait_reason = null, next_nudge_at = null where conversation_id = $1").bind(conversation_id).bind(enum_text(&goal_state)).bind(reason).execute(&state.pool).await?;

    announce(state, owner, conversation_id).await
}

async fn enqueue(state: &AppState, owner: Uuid, conversation_id: Uuid, text: &str) -> ApiResult<()> {
    sqlx::query("insert into queued_prompts (conversation_id, position, text, origin) select $1, coalesce(max(position), 0) + 1, $2, $3 from queued_prompts where conversation_id = $1").bind(conversation_id).bind(text).bind(GOAL_ORIGIN).execute(&state.pool).await?;

    pump(state, conversation_id, false).await?;
    publish_queue(state, owner, conversation_id).await
}

fn agent_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        other => other,
    }
}

async fn deliver(state: &AppState, owner: Uuid, conversation_id: Uuid, text: &str) -> ApiResult<()> {
    sqlx::query("delete from queued_prompts where conversation_id = $1 and origin = $2").bind(conversation_id).bind(GOAL_ORIGIN).execute(&state.pool).await?;
    let person = sqlx::query_scalar::<_, String>("select u.name from users u join conversations c on c.owner = u.id where c.id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let person = if person.trim().is_empty() { "the person".to_string() } else { person };
    enqueue(state, owner, conversation_id, &format!("Work towards this goal: {text}. When the goal is done, or you cannot go on without {person}, call report_goal and say why.")).await?;

    let (node_id, node_name, agent) = sqlx::query_as::<_, (Uuid, String, String)>("select c.node_id, n.name, c.agent from conversations c join nodes n on n.id = c.node_id where c.id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    if state.hub.is_online(node_id) {
        return Ok(());
    }

    sqlx::query("delete from queued_prompts where conversation_id = $1 and origin = $2").bind(conversation_id).bind(GOAL_ORIGIN).execute(&state.pool).await?;
    publish_queue(state, owner, conversation_id).await?;

    let reason = format!("Mycelium could not send the goal to {} because {node_name} did not answer. Check that {node_name} is online, then send the goal again.", agent_name(&agent));
    sqlx::query("update goals set delivery_state = 'failed', delivery_reason = $2 where conversation_id = $1").bind(conversation_id).bind(reason).execute(&state.pool).await?;

    announce(state, owner, conversation_id).await
}

pub async fn sending(state: &AppState, conversation_id: Uuid) -> ApiResult<(i32, Option<String>)> {
    let sending = sqlx::query_as::<_, (i32, Option<String>)>("select nudges, why from goals where conversation_id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

    Ok(sending.unwrap_or((0, None)))
}

pub async fn delivered(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let owner = sqlx::query_scalar::<_, Uuid>(
        "update goals g set delivery_state = 'sent', delivered_at = now(), delivery_reason = null from conversations c
         where c.id = g.conversation_id and g.conversation_id = $1 and g.delivery_state = 'pending' returning c.owner",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;

    match owner {
        Some(owner) => announce(state, owner, conversation_id).await,
        None => Ok(()),
    }
}

pub async fn hold(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let owner = sqlx::query_scalar::<_, Uuid>("update goals set held_since = coalesce(held_since, now()) where conversation_id = $1 and state = 'active' returning (select owner from conversations where id = $1)")
        .bind(conversation_id)
        .fetch_optional(&state.pool)
        .await?;

    match owner {
        Some(owner) => announce(state, owner, conversation_id).await,
        None => Ok(()),
    }
}

async fn release_hold(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<(String, String)>> {
    let released = sqlx::query_as::<_, (String, String)>("update goals set held_seconds = held_seconds + floor(extract(epoch from now() - held_since))::bigint, held_since = null where conversation_id = $1 and held_since is not null returning text, state")
        .bind(conversation_id)
        .fetch_optional(&state.pool)
        .await?;

    Ok(released)
}

pub async fn resumed(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<bool> {
    let Some((_, goal_state)) = release_hold(state, conversation_id).await? else {
        return Ok(false);
    };

    announce(state, owner, conversation_id).await?;

    if goal_state != "active" {
        return Ok(false);
    }

    let goal_waiting = sqlx::query_scalar::<_, bool>("select exists (select 1 from queued_prompts where conversation_id = $1 and origin = $2)").bind(conversation_id).bind(GOAL_ORIGIN).fetch_one(&state.pool).await?;

    if !goal_waiting {
        let agent = sqlx::query_scalar::<_, String>("select agent from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
        nudge(state, owner, conversation_id, &format!("{}'s 5-hour limit reset", agent_name(&agent))).await?;
    }

    Ok(true)
}

async fn nudge(state: &AppState, owner: Uuid, conversation_id: Uuid, why: &str) -> ApiResult<()> {
    let (text, note) = sqlx::query_as::<_, (String, Option<String>)>("select text, note from goals where conversation_id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    sqlx::query("update goals set nudges = nudges + 1, why = $2, note = null, wait_reason = null, next_nudge_at = null where conversation_id = $1").bind(conversation_id).bind(why).execute(&state.pool).await?;
    sqlx::query("delete from queued_prompts where conversation_id = $1 and origin = $2").bind(conversation_id).bind(GOAL_ORIGIN).execute(&state.pool).await?;

    let body = match note {
        Some(note) => format!("Continue towards the goal: {text}\n\n{note}"),
        None => format!("Continue towards the goal: {text}"),
    };
    announce(state, owner, conversation_id).await?;

    enqueue(state, owner, conversation_id, &body).await
}

async fn record_report(state: &AppState, owner: Uuid, conversation_id: Uuid, outcome: &str, reason: &str) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::GoalReport, &json!({"outcome": outcome, "reason": reason, "at": Utc::now()}), None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    Ok(())
}

pub async fn report(state: &AppState, owner: Uuid, conversation_id: Uuid, outcome: &str, reason: &str) -> ApiResult<bool> {
    let goal_state = if outcome == "done" { GoalState::Met } else { GoalState::Stuck };
    let active = sqlx::query_scalar::<_, Uuid>("select conversation_id from goals where conversation_id = $1 and state = 'active'").bind(conversation_id).fetch_optional(&state.pool).await?;

    if active.is_none() {
        return Ok(false);
    }

    release_hold(state, conversation_id).await?;
    sqlx::query("delete from queued_prompts where conversation_id = $1 and origin = $2").bind(conversation_id).bind(GOAL_ORIGIN).execute(&state.pool).await?;
    publish_queue(state, owner, conversation_id).await?;
    record_report(state, owner, conversation_id, outcome, reason).await?;
    sqlx::query("update goals set reported_at = now() where conversation_id = $1").bind(conversation_id).execute(&state.pool).await?;
    end(state, owner, conversation_id, goal_state, reason).await?;

    Ok(true)
}

pub async fn refresh(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let owner = sqlx::query_scalar("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let row = find_owned(&state.pool, conversation_id, owner).await?;
    let target = if row.side_kind.as_deref() == Some("check") { row.parent_id.unwrap_or(conversation_id) } else { conversation_id };
    if read(state, target).await?.is_some() {
        announce(state, row.owner, target).await?;
    }

    Ok(())
}

async fn wait_for(state: &AppState, owner: Uuid, conversation_id: Uuid, reason: &str, seconds: Option<i64>) -> ApiResult<()> {
    sqlx::query("update goals set wait_reason = $2, next_nudge_at = case when $3::bigint is null then null else now() + make_interval(secs => $3::bigint) end, held_since = case when $2 = 'question_waiting' then coalesce(held_since, now()) else held_since end where conversation_id = $1")
        .bind(conversation_id)
        .bind(reason)
        .bind(seconds)
        .execute(&state.pool)
        .await?;

    announce(state, owner, conversation_id).await
}

pub async fn turn_ended(state: &AppState, conversation_id: Uuid, stop_reason: &str) -> ApiResult<()> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    release_hold(state, conversation_id).await?;
    sqlx::query("update goals set wait_reason = null, next_nudge_at = null where conversation_id = $1").bind(conversation_id).execute(&state.pool).await?;

    let Some(goal) = read(state, conversation_id).await? else {
        return Ok(());
    };

    announce(state, owner, conversation_id).await?;

    if goal.state != GoalState::Active || goal.delivery == GoalDelivery::Pending {
        return Ok(());
    }

    if stop_reason == "cancelled" {
        record_report(state, owner, conversation_id, "stopped", PAUSED_BY_STOP).await?;

        return end(state, owner, conversation_id, GoalState::Paused, PAUSED_BY_STOP).await;
    }

    if let Some((limit_state, limit_reason)) = reached_limit(&goal) {
        return end(state, owner, conversation_id, limit_state, &limit_reason).await;
    }

    let question_open = sqlx::query_scalar::<_, bool>("select exists (select 1 from tasks where source_id = $1 and status = 'question')").bind(conversation_id).fetch_one(&state.pool).await?;

    if question_open {
        return wait_for(state, owner, conversation_id, "question_waiting", None).await;
    }

    if crate::link::has_agents_out(state, conversation_id).await? {
        return wait_for(state, owner, conversation_id, "background_work", Some(BACKGROUND_WAIT_SECONDS)).await;
    }

    let text = latest_agent_text(state, conversation_id).await?;
    let files = changed_files(state, conversation_id).await?;
    let outcome = sqlx::query_scalar::<_, String>("select md5($1)").bind(format!("{}\n{files}", text.trim())).fetch_one(&state.pool).await?;
    let repeats = sqlx::query_scalar::<_, i32>("update goals set repeats = case when last_outcome = $2 then repeats + 1 else 1 end, last_outcome = $2 where conversation_id = $1 returning repeats").bind(conversation_id).bind(&outcome).fetch_one(&state.pool).await?;
    let repeats = i64::from(repeats);
    let limit = goal.repeat_limit.unwrap_or(DEFAULT_REPEAT_LIMIT);

    if repeats >= limit {
        let reason = format!("Stopped after {repeats} turns ended the same way: {}", summary_of(&text).trim_end_matches('.'));

        return end(state, owner, conversation_id, GoalState::Stalled, &reason).await;
    }

    if repeats >= MINIMUM_REPEAT_LIMIT {
        return wait_for(state, owner, conversation_id, "repeated_outcome", Some(backoff_seconds(repeats))).await;
    }

    nudge(state, owner, conversation_id, WHY_NO_REPORT).await
}

fn backoff_seconds(repeats: i64) -> i64 {
    BACKOFF_SECONDS << (repeats - MINIMUM_REPEAT_LIMIT).clamp(0, 10)
}

pub async fn nudge_regularly(state: AppState) {
    run_while_ready(&state, "nudge the goals whose wait is over", NUDGE_TICK, || nudge_due(&state)).await;
}

async fn nudge_due(state: &AppState) -> ApiResult<()> {
    let due = sqlx::query_as::<_, (Uuid, Uuid, String, i32)>(
        "select g.conversation_id, c.owner, g.wait_reason, g.repeats from goals g join conversations c on c.id = g.conversation_id
         where g.state = 'active' and g.next_nudge_at <= now() and g.wait_reason in ('repeated_outcome', 'background_work')",
    )
    .fetch_all(&state.pool)
    .await?;

    for (conversation_id, owner, wait_reason, repeats) in due {
        let row = find_owned(&state.pool, conversation_id, owner).await?;

        if !row.is_idle() {
            continue;
        }

        let why = if wait_reason == "repeated_outcome" { format!("the {}-minute wait after a repeated outcome ended", backoff_seconds(i64::from(repeats)) / 60) } else { "the wait for background work ended".to_string() };
        nudge(state, owner, conversation_id, &why).await?;
    }

    Ok(())
}

pub async fn session_failed(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

    let Some(owner) = owner else {
        return Ok(());
    };

    let active = sqlx::query_scalar::<_, Uuid>("select conversation_id from goals where conversation_id = $1 and state = 'active'").bind(conversation_id).fetch_optional(&state.pool).await?;

    if active.is_some() {
        end(state, owner, conversation_id, GoalState::Stuck, AGENT_STOPPED).await?;
    }

    Ok(())
}

pub async fn load(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Option<Goal>> {
    find_owned(&state.pool, id, owner).await?;

    read(state, id).await
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<Goal>>> {
    Ok(Json(load(&state, user.id, id).await?))
}

pub async fn set(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SetGoalRequest>) -> ApiResult<Json<Goal>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let text = request.text.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter the goal the agent should work towards."));
    }

    if request.token_budget.is_some_and(|budget| budget <= 0) {
        return Err(ApiError::bad_request("budget_invalid", "Enter a token budget greater than zero."));
    }

    if request.time_limit_seconds.is_some_and(|seconds| seconds <= 0) {
        return Err(ApiError::bad_request("time_limit_invalid", "Enter a time limit greater than zero."));
    }

    if request.five_hour_limit.is_some_and(|percent| !(1..=100).contains(&percent)) {
        return Err(ApiError::bad_request("five_hour_limit_invalid", "Enter a 5-hour window limit between 1 and 100 percent."));
    }

    if request.weekly_limit.is_some_and(|percent| !(1..=100).contains(&percent)) {
        return Err(ApiError::bad_request("weekly_limit_invalid", "Enter a weekly window limit between 1 and 100 percent."));
    }

    if request.repeat_limit.is_some_and(|turns| turns < MINIMUM_REPEAT_LIMIT) {
        return Err(ApiError::bad_request("repeat_limit_invalid", "Enter how many turns, as a whole number like 3, or leave it empty."));
    }

    if row.side_kind.is_some() {
        return Err(ApiError::bad_request("goal_not_allowed", "Set a goal on the conversation itself, not on a side chat."));
    }

    sqlx::query(
        "insert into goals (conversation_id, text, token_budget, time_limit_seconds, five_hour_limit, weekly_limit, repeat_limit, why, delivery_state) values ($1, $2, $3, $4, $5, $6, $7, $8, 'pending')
         on conflict (conversation_id) do update set text = $2, token_budget = $3, time_limit_seconds = $4, five_hour_limit = $5, weekly_limit = $6, repeat_limit = $7, why = $8, state = 'active', reason = null, started_at = now(),
             delivery_state = 'pending', delivered_at = null, delivery_reason = null, held_since = null, held_seconds = 0,
             nudges = 0, repeats = 0, last_outcome = null, next_nudge_at = null, wait_reason = null, note = null, reported_at = null",
    )
    .bind(id)
    .bind(text)
    .bind(request.token_budget)
    .bind(request.time_limit_seconds)
    .bind(request.five_hour_limit.map(|percent| percent as i16))
    .bind(request.weekly_limit.map(|percent| percent as i16))
    .bind(request.repeat_limit.map(|turns| turns as i32))
    .bind(WHY_SET)
    .execute(&state.pool)
    .await?;

    announce(&state, user.id, id).await?;
    deliver(&state, user.id, id, text).await?;

    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchGoalRequest>) -> ApiResult<Json<Goal>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    if !matches!(goal.state, GoalState::Active | GoalState::Paused | GoalState::Stalled) {
        return Err(ApiError::conflict("goal_ended", "This goal has ended. Clear it and set a new one."));
    }

    if request.paused && goal.state == GoalState::Active {
        sqlx::query("update goals set state = 'paused', reason = null, wait_reason = null, next_nudge_at = null where conversation_id = $1").bind(id).execute(&state.pool).await?;
    }

    if !request.paused && matches!(goal.state, GoalState::Paused | GoalState::Stalled) {
        sqlx::query("update goals set state = 'active', reason = null, repeats = 0, last_outcome = null where conversation_id = $1").bind(id).execute(&state.pool).await?;
        announce(&state, user.id, id).await?;

        if row.is_idle() {
            nudge(&state, user.id, id, WHY_RESUMED).await?;
        }
    }

    announce(&state, user.id, id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn nudge_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Goal>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    if goal.state != GoalState::Active || !matches!(goal.wait_reason, Some(GoalWait::RepeatedOutcome | GoalWait::BackgroundWork)) {
        return Err(ApiError::conflict("goal_not_waiting", NOT_WAITING));
    }

    if !row.is_idle() {
        return Err(ApiError::conflict("turn_running", NOT_IDLE));
    }

    nudge(&state, user.id, id, WHY_NUDGED_NOW).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn continue_goal(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<ContinueGoalRequest>) -> ApiResult<Json<Goal>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    if goal.state != GoalState::Stuck {
        return Err(ApiError::conflict("goal_not_blocked", NOT_BLOCKED));
    }

    let note = request.note.as_deref().map(str::trim).filter(|note| !note.is_empty());
    sqlx::query("update goals set state = 'active', reason = null, repeats = 0, last_outcome = null, reported_at = null, note = $2 where conversation_id = $1").bind(id).bind(note).execute(&state.pool).await?;
    announce(&state, user.id, id).await?;

    if row.is_idle() {
        nudge(&state, user.id, id, WHY_CONTINUED).await?;
    }

    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn send_again(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Goal>> {
    find_owned(&state.pool, id, user.id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    if goal.delivery != GoalDelivery::Failed {
        return Err(ApiError::conflict("goal_delivered", SEND_AGAIN_UNNEEDED));
    }

    sqlx::query("update goals set delivery_state = 'pending', delivery_reason = null, why = $2 where conversation_id = $1").bind(id).bind(WHY_SENT_AGAIN).execute(&state.pool).await?;
    deliver(&state, user.id, id, &goal.text).await?;
    announce(&state, user.id, id).await?;
    let goal = read(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn clear(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;
    sqlx::query("delete from goals where conversation_id = $1").bind(id).execute(&state.pool).await?;
    announce(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
