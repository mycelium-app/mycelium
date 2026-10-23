use crate::account_usage;
use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::prompts::pump;
use crate::state::AppState;
use crate::store::{enum_text, find_owned, parse_enum, publish_queue};
use axum::Json;
use chrono::{DateTime, Utc};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Goal, GoalState, PatchGoalRequest, SetGoalRequest};
use mycelium_proto::conversation::SideKind;
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

const LAST_MESSAGE_LIMIT: usize = 4000;
const PAUSED_BY_STOP: &str = "Paused because you stopped the turn. Resume to let the agent carry on.";
const CHECK_UNREADABLE: &str = "The check did not answer in the form it was asked to, so the goal was stopped. Resume to check again.";
const CHECK_COULD_NOT_RUN: &str = "The check could not run, so the goal was stopped. Resume to try again.";
const AGENT_STOPPED: &str = "The agent stopped before the turn ended, so the goal was stopped.";
const FIVE_HOUR_WINDOW: &str = "five_hour";
const WEEKLY_WINDOW: &str = "seven_day";
const OVER_BUDGET: &str = "The goal is not met and the token budget is used up. Set the goal again with a larger budget to carry on.";
const CHECK_INSTRUCTIONS: &str = "You are checking whether a goal has been met. Do not change any files or run anything that changes state. Look at the work in this folder if you need to.\n\nAnswer with one JSON object and nothing else, in this form: {\"verdict\": \"met\" | \"continue\" | \"waiting\" | \"stuck\", \"reason\": \"one sentence\"}.\nmet: the goal is fully achieved.\ncontinue: more work remains and the agent can do it now.\nwaiting: the agent has started work that will resume it on its own, such as background commands, subagents, a workflow or a pipeline it is watching, and has nothing to do until that reports back.\nstuck: no further progress is possible without the person's help.";

enum Verdict {
    Met,
    Continue,
    Waiting,
    Stuck,
}

fn parse_verdict(answer: &str) -> Option<(Verdict, String)> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let parsed: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let reason = parsed.get("reason").and_then(Value::as_str).unwrap_or_default().trim().to_string();

    match parsed.get("verdict").and_then(Value::as_str)?.trim().to_lowercase().as_str() {
        "met" => Some((Verdict::Met, reason)),
        "continue" => Some((Verdict::Continue, reason)),
        "waiting" => Some((Verdict::Waiting, reason)),
        "stuck" => Some((Verdict::Stuck, reason)),
        _ => None,
    }
}

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

type GoalRow = (String, Option<i64>, Option<i64>, Option<i16>, Option<i16>, String, Option<String>, DateTime<Utc>, Uuid, String, Option<String>);

async fn load(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<Goal>> {
    let row = sqlx::query_as::<_, GoalRow>(
        "select g.text, g.token_budget, g.time_limit_seconds, g.five_hour_limit, g.weekly_limit, g.state, g.reason, g.started_at, c.node_id, c.agent, c.account
         from goals g join conversations c on c.id = g.conversation_id where g.conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((text, token_budget, time_limit_seconds, five_hour_limit, weekly_limit, goal_state, reason, started_at, node_id, agent, account)) = row else {
        return Ok(None);
    };

    let (tokens_used, turns) = sqlx::query_as::<_, (i64, i64)>(
        "select coalesce(sum(u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens), 0)::bigint, (count(*) filter (where u.conversation_id = g.conversation_id))::bigint
         from goals g
         join usage_turns u on u.ended_at >= g.started_at
          and (u.conversation_id = g.conversation_id or u.conversation_id in (select id from conversations where parent_id = g.conversation_id and side_kind = 'check'))
         where g.conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    let windows = account_usage::windows_of(state, node_id, parse_enum(&agent)?, account.as_deref()).await?;
    let used = |name: &str| windows.iter().find(|window| window.name == name).map(|window| window.used_percent);

    Ok(Some(Goal {
        conversation_id,
        text,
        token_budget,
        time_limit_seconds,
        five_hour_limit: five_hour_limit.and_then(|limit| u8::try_from(limit).ok()),
        weekly_limit: weekly_limit.and_then(|limit| u8::try_from(limit).ok()),
        tokens_used,
        turns,
        started_at,
        five_hour_used: used(FIVE_HOUR_WINDOW),
        weekly_used: used(WEEKLY_WINDOW),
        state: parse_enum(&goal_state)?,
        reason,
    }))
}

fn reached_limit(goal: &Goal) -> Option<(GoalState, String)> {
    if goal.token_budget.is_some_and(|budget| goal.tokens_used >= budget) {
        return Some((GoalState::OverBudget, OVER_BUDGET.to_string()));
    }

    if let Some(limit) = goal.time_limit_seconds {
        let elapsed = (Utc::now() - goal.started_at).num_seconds();

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
    match load(state, conversation_id).await? {
        Some(goal) => state.publish(Some(owner), StreamMessage::Goal(goal)),
        None => state.publish(Some(owner), StreamMessage::GoalCleared { conversation_id }),
    }

    Ok(())
}

async fn end(state: &AppState, owner: Uuid, conversation_id: Uuid, goal_state: GoalState, reason: &str) -> ApiResult<()> {
    sqlx::query("update goals set state = $2, reason = $3, check_id = null where conversation_id = $1").bind(conversation_id).bind(enum_text(&goal_state)).bind(reason).execute(&state.pool).await?;

    announce(state, owner, conversation_id).await
}

async fn enqueue(state: &AppState, owner: Uuid, conversation_id: Uuid, text: &str) -> ApiResult<()> {
    sqlx::query("insert into queued_prompts (conversation_id, position, text) select $1, coalesce(max(position), 0) + 1, $2 from queued_prompts where conversation_id = $1").bind(conversation_id).bind(text).execute(&state.pool).await?;

    pump(state, conversation_id, false).await?;
    publish_queue(state, owner, conversation_id).await
}

async fn start_check(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let row = find_owned(&state.pool, conversation_id, owner).await?;
    let text = sqlx::query_scalar::<_, String>("select text from goals where conversation_id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let last: String = latest_agent_text(state, conversation_id).await?.chars().take(LAST_MESSAGE_LIMIT).collect();
    let prompt = format!("{CHECK_INSTRUCTIONS}\n\nGoal: {text}\n\nThe agent's latest message:\n{last}");
    let user = AuthUser { id: owner, email: String::new(), name: String::new(), session: None, scopes: None, expires_at: None };
    let request = mycelium_proto::api::CreateConversationRequest {
        node_id: row.node_id,
        agent: parse_enum(&row.agent)?,
        account: row.account.clone(),
        cwd: row.cwd.clone(),
        project_id: None,
        branch: None,
        title: Some("Goal check".to_string()),
        prompt: Some(prompt),
        parent_id: Some(conversation_id),
        side_kind: Some(SideKind::Check),
    };

    match conversations::start(state, &user, request).await {
        Ok(check) => {
            sqlx::query("update goals set check_id = $2 where conversation_id = $1").bind(conversation_id).bind(check.id).execute(&state.pool).await?;

            Ok(())
        }
        Err(_) => end(state, owner, conversation_id, GoalState::Stuck, CHECK_COULD_NOT_RUN).await,
    }
}

async fn finish_check(state: &AppState, owner: Uuid, goal_conversation_id: Uuid, check_id: Uuid) -> ApiResult<()> {
    let answer = latest_agent_text(state, check_id).await?;
    sqlx::query("update conversations set archived_at = coalesce(archived_at, now()) where id = $1").bind(check_id).execute(&state.pool).await?;

    let pending = sqlx::query_scalar::<_, Uuid>("update goals set check_id = null where conversation_id = $1 and check_id = $2 and state = 'active' returning conversation_id").bind(goal_conversation_id).bind(check_id).fetch_optional(&state.pool).await?;

    if pending.is_none() {
        return Ok(());
    }

    let row = find_owned(&state.pool, goal_conversation_id, owner).await?;

    if !row.is_idle() {
        return Ok(());
    }

    match parse_verdict(&answer) {
        Some((Verdict::Met, reason)) => end(state, owner, goal_conversation_id, GoalState::Met, &reason).await,
        Some((Verdict::Stuck, reason)) => end(state, owner, goal_conversation_id, GoalState::Stuck, &reason).await,
        None => end(state, owner, goal_conversation_id, GoalState::Stuck, CHECK_UNREADABLE).await,
        Some((Verdict::Waiting, reason)) => {
            sqlx::query("update goals set reason = $2 where conversation_id = $1").bind(goal_conversation_id).bind(&reason).execute(&state.pool).await?;
            announce(state, owner, goal_conversation_id).await
        }
        Some((Verdict::Continue, reason)) => {
            let goal = load(state, goal_conversation_id).await?;

            let Some(goal) = goal else {
                return Ok(());
            };

            if let Some((limit_state, limit_reason)) = reached_limit(&goal) {
                return end(state, owner, goal_conversation_id, limit_state, &limit_reason).await;
            }

            sqlx::query("update goals set reason = $2 where conversation_id = $1").bind(goal_conversation_id).bind(&reason).execute(&state.pool).await?;
            announce(state, owner, goal_conversation_id).await?;
            enqueue(state, owner, goal_conversation_id, &format!("Continue towards the goal: {}\n\n{}", goal.text, reason)).await
        }
    }
}

pub async fn turn_ended(state: &AppState, conversation_id: Uuid, stop_reason: &str) -> ApiResult<()> {
    let (owner, parent_id, side_kind) = sqlx::query_as::<_, (Uuid, Option<Uuid>, Option<String>)>("select owner, parent_id, side_kind from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    if let (Some(parent_id), Some("check")) = (parent_id, side_kind.as_deref()) {
        return finish_check(state, owner, parent_id, conversation_id).await;
    }

    let Some(goal) = load(state, conversation_id).await? else {
        return Ok(());
    };

    announce(state, owner, conversation_id).await?;

    if goal.state != GoalState::Active {
        return Ok(());
    }

    if stop_reason == "cancelled" {
        return end(state, owner, conversation_id, GoalState::Paused, PAUSED_BY_STOP).await;
    }

    let waiting_on_check = sqlx::query_scalar::<_, bool>("select check_id is not null from goals where conversation_id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    if waiting_on_check {
        return Ok(());
    }

    start_check(state, owner, conversation_id).await
}

pub async fn session_failed(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let row = sqlx::query_as::<_, (Uuid, Option<Uuid>, Option<String>)>("select owner, parent_id, side_kind from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

    let Some((owner, parent_id, side_kind)) = row else {
        return Ok(());
    };

    if let (Some(parent_id), Some("check")) = (parent_id, side_kind.as_deref()) {
        sqlx::query("update conversations set archived_at = coalesce(archived_at, now()) where id = $1").bind(conversation_id).execute(&state.pool).await?;
        let pending = sqlx::query_scalar::<_, Uuid>("select conversation_id from goals where conversation_id = $1 and check_id = $2 and state = 'active'").bind(parent_id).bind(conversation_id).fetch_optional(&state.pool).await?;

        if pending.is_some() {
            end(state, owner, parent_id, GoalState::Stuck, CHECK_COULD_NOT_RUN).await?;
        }

        return Ok(());
    }

    let active = sqlx::query_scalar::<_, Uuid>("select conversation_id from goals where conversation_id = $1 and state = 'active'").bind(conversation_id).fetch_optional(&state.pool).await?;

    if active.is_some() {
        end(state, owner, conversation_id, GoalState::Stuck, AGENT_STOPPED).await?;
    }

    Ok(())
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<Goal>>> {
    find_owned(&state.pool, id, user.id).await?;

    Ok(Json(load(&state, id).await?))
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

    if row.side_kind.is_some() {
        return Err(ApiError::bad_request("goal_not_allowed", "Set a goal on the conversation itself, not on a side chat."));
    }

    sqlx::query(
        "insert into goals (conversation_id, text, token_budget, time_limit_seconds, five_hour_limit, weekly_limit) values ($1, $2, $3, $4, $5, $6)
         on conflict (conversation_id) do update set text = $2, token_budget = $3, time_limit_seconds = $4, five_hour_limit = $5, weekly_limit = $6, state = 'active', reason = null, check_id = null, started_at = now()",
    )
    .bind(id)
    .bind(text)
    .bind(request.token_budget)
    .bind(request.time_limit_seconds)
    .bind(request.five_hour_limit.map(|percent| percent as i16))
    .bind(request.weekly_limit.map(|percent| percent as i16))
    .execute(&state.pool)
    .await?;

    announce(&state, user.id, id).await?;

    if row.is_idle() {
        enqueue(&state, user.id, id, &format!("Work towards this goal: {text}")).await?;
    }

    let goal = load(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchGoalRequest>) -> ApiResult<Json<Goal>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let goal = load(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    if !matches!(goal.state, GoalState::Active | GoalState::Paused) {
        return Err(ApiError::conflict("goal_ended", "This goal has ended. Clear it and set a new one."));
    }

    if request.paused && goal.state == GoalState::Active {
        sqlx::query("update goals set state = 'paused', reason = null where conversation_id = $1").bind(id).execute(&state.pool).await?;
    }

    if !request.paused && goal.state == GoalState::Paused {
        sqlx::query("update goals set state = 'active', reason = null where conversation_id = $1").bind(id).execute(&state.pool).await?;
        announce(&state, user.id, id).await?;

        if row.is_idle() {
            start_check(&state, user.id, id).await?;
        }
    }

    announce(&state, user.id, id).await?;
    let goal = load(&state, id).await?.ok_or_else(|| ApiError::not_found("That goal"))?;

    Ok(Json(goal))
}

pub async fn clear(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;
    sqlx::query("delete from goals where conversation_id = $1").bind(id).execute(&state.pool).await?;
    announce(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
