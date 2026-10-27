use crate::agent_settings;
use crate::continuation::{self, Target};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{ConversationRow, TURN_HAS_ENDED, find_owned, parse_enum};
use chrono::{DateTime, Utc};
use mycelium_proto::api::ModelChoice;
use mycelium_proto::conversation::{AgentId, UsageWindow};
use mycelium_proto::routing::Routing;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use sqlx::types::Json;
use uuid::Uuid;

const FIVE_HOUR: &str = "five_hour";
const SEVEN_DAY: &str = "seven_day";
const FULL: f64 = 100.0;
const UNKNOWN: f64 = -1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub limited: bool,
    pub five_left: Option<f64>,
    pub week_left: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub node_id: Uuid,
    pub account: String,
    pub usage: Usage,
}

struct Move {
    target: Target,
    reason: &'static str,
    detail: Value,
}

pub fn usage_of(windows: &[UsageWindow], now: DateTime<Utc>) -> Usage {
    let left = |name: &str| {
        windows.iter().find(|window| window.name == name).map(|window| if window.resets_at.is_some_and(|reset| reset <= now) { FULL } else { (FULL - window.used_percent).clamp(0.0, FULL) })
    };
    let five_left = left(FIVE_HOUR);
    let week_left = left(SEVEN_DAY);

    Usage { limited: [five_left, week_left].into_iter().flatten().any(|remaining| remaining <= 0.0), five_left, week_left }
}

fn usage_with_fallback(windows: &[UsageWindow], usage_left: Option<i16>, now: DateTime<Utc>) -> Usage {
    match usage_left.filter(|_| windows.is_empty()) {
        Some(left) => Usage { limited: left <= 0, five_left: Some(f64::from(left)), week_left: None },
        None => usage_of(windows, now),
    }
}

fn standing(candidate: &Candidate) -> (bool, f64, f64) {
    (candidate.usage.five_left.is_some(), candidate.usage.five_left.unwrap_or(UNKNOWN), candidate.usage.week_left.unwrap_or(UNKNOWN))
}

fn most_left<'a>(candidates: impl Iterator<Item = &'a Candidate>) -> Option<&'a Candidate> {
    candidates.max_by(|left, right| standing(left).partial_cmp(&standing(right)).unwrap_or(std::cmp::Ordering::Equal).then_with(|| right.account.cmp(&left.account)))
}

pub fn best(candidates: &[Candidate]) -> Option<&Candidate> {
    most_left(candidates.iter().filter(|candidate| !candidate.usage.limited))
}

async fn candidates(state: &AppState, owner: Uuid, agent: AgentId, node_id: Option<Uuid>) -> ApiResult<Vec<Candidate>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Json<Vec<UsageWindow>>, Option<i16>)>(
        "select l.node_id, l.account, l.usage_windows, l.usage_left from logins l join nodes n on n.id = l.node_id
         where n.created_by = $1 and n.revoked_at is null and l.agent = $2 and l.state = 'signed_in' and l.account is not null and ($3::uuid is null or l.node_id = $3)
         order by l.account",
    )
    .bind(owner)
    .bind(agent.as_str())
    .bind(node_id)
    .fetch_all(&state.pool)
    .await?;
    let now = Utc::now();

    Ok(rows.into_iter().filter(|(node_id, _, _, _)| state.hub.is_online(*node_id)).map(|(node_id, account, windows, left)| Candidate { node_id, account, usage: usage_with_fallback(&windows.0, left, now) }).collect())
}

fn any_account(agent: AgentId) -> ModelChoice {
    ModelChoice { agent: Some(agent), provider_id: None, model: None, account: None, any_model: false }
}

async fn entries_of(state: &AppState, owner: Uuid, routing: Routing) -> ApiResult<Vec<ModelChoice>> {
    match routing {
        Routing::ClaudeAny => Ok(vec![any_account(AgentId::Claude)]),
        Routing::CodexAny => Ok(vec![any_account(AgentId::Codex)]),
        Routing::AnyModel => Ok(agent_settings::read(state, owner).await?.any_model),
    }
}

pub async fn expand_any_model(state: &AppState, owner: Uuid, models: Vec<ModelChoice>) -> ApiResult<Vec<ModelChoice>> {
    if !models.iter().any(|choice| choice.any_model) {
        return Ok(models);
    }

    let listed = agent_settings::read(state, owner).await?.any_model;

    Ok(models.into_iter().flat_map(|choice| if choice.any_model { listed.clone() } else { vec![choice] }).collect())
}

pub async fn best_account(state: &AppState, owner: Uuid, node_id: Uuid, agent: AgentId) -> ApiResult<Option<String>> {
    let found = candidates(state, owner, agent, Some(node_id)).await?;

    Ok(best(&found).map(|candidate| candidate.account.clone()))
}

pub struct Start {
    pub agent: AgentId,
    pub account: String,
    pub model: Option<String>,
}

pub async fn start_pick(state: &AppState, owner: Uuid, node_id: Uuid, routing: Routing) -> ApiResult<Start> {
    let entries = entries_of(state, owner, routing).await?;
    let mut exhausted: Option<Start> = None;

    for entry in &entries {
        let Some(agent) = entry.agent else {
            continue;
        };
        let mut found = candidates(state, owner, agent, Some(node_id)).await?;

        if let Some(account) = &entry.account {
            found.retain(|candidate| &candidate.account == account);
        }

        if let Some(chosen) = best(&found) {
            return Ok(Start { agent, account: chosen.account.clone(), model: entry.model.clone() });
        }

        if exhausted.is_none() && let Some(chosen) = most_left(found.iter()) {
            exhausted = Some(Start { agent, account: chosen.account.clone(), model: entry.model.clone() });
        }
    }

    exhausted.ok_or_else(|| match routing {
        Routing::ClaudeAny => ApiError::conflict("no_routing_choice", "No Claude Code account is signed in on this node. Sign in to one in Settings, Providers, or choose another node."),
        Routing::CodexAny => ApiError::conflict("no_routing_choice", "No Codex account is signed in on this node. Sign in to one in Settings, Providers, or choose another node."),
        Routing::AnyModel => ApiError::conflict("no_routing_choice", "Nothing in your Any model list can start on this node. Sign in to an account it lists, or edit the list in Settings, Agents."),
    })
}

fn matches_current(entry: &ModelChoice, row: &ConversationRow) -> bool {
    match (entry.agent, entry.provider_id) {
        (Some(agent), _) => agent.as_str() == row.agent && entry.account.as_ref().is_none_or(|account| row.account.as_ref() == Some(account)),
        (None, Some(provider_id)) => row.provider_id == Some(provider_id) && entry.model.as_ref().is_none_or(|model| row.model.as_ref() == Some(model)),
        _ => false,
    }
}

async fn folder_exists(state: &AppState, owner: Uuid, node_id: Uuid, cwd: &str) -> ApiResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>("select exists (select 1 from projects where owner = $1 and node_id = $2 and path = $3)").bind(owner).bind(node_id).bind(cwd).fetch_one(&state.pool).await?)
}

async fn target_for(state: &AppState, owner: Uuid, entry: &ModelChoice, row: &ConversationRow, leaving_current: bool) -> ApiResult<Option<Target>> {
    if let Some(agent) = entry.agent {
        let here = state.hub.is_online(row.node_id).then_some(row.node_id);
        let mut found = candidates(state, owner, agent, here).await?;

        found.retain(|candidate| !(leaving_current && agent.as_str() == row.agent && row.account.as_ref() == Some(&candidate.account) && candidate.node_id == row.node_id));

        if let Some(account) = &entry.account {
            found.retain(|candidate| &candidate.account == account);
        }

        if here.is_none() {
            let mut reachable = Vec::new();

            for candidate in found {
                if folder_exists(state, owner, candidate.node_id, &row.cwd).await? {
                    reachable.push(candidate);
                }
            }

            found = reachable;
        }

        return Ok(best(&found).map(|chosen| Target { agent: agent.as_str().to_string(), account: Some(chosen.account.clone()), provider_id: None, model: entry.model.clone(), node_id: Some(chosen.node_id), cwd: None }));
    }

    let Some(provider_id) = entry.provider_id else {
        return Ok(None);
    };
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from model_providers where id = $1 and owner = $2)").bind(provider_id).bind(owner).fetch_one(&state.pool).await?;

    Ok(exists.then(|| Target { agent: crate::chats::CHAT_AGENT.to_string(), account: None, provider_id: Some(provider_id), model: entry.model.clone(), node_id: None, cwd: None }))
}

fn limited_window(usage: Usage) -> &'static str {
    if usage.five_left.is_some_and(|left| left <= 0.0) { FIVE_HOUR } else { SEVEN_DAY }
}

async fn plan(state: &AppState, owner: Uuid, row: &ConversationRow, entries: &[ModelChoice], limit: Option<&str>, repick: bool) -> ApiResult<Option<Move>> {
    let current_agent = parse_enum::<AgentId>(&row.agent).ok();
    let on_current_node = match current_agent {
        Some(agent) => candidates(state, owner, agent, Some(row.node_id)).await?,
        None => Vec::new(),
    };
    let current_usage = on_current_node.iter().find(|candidate| row.account.as_ref() == Some(&candidate.account)).map(|candidate| candidate.usage);
    let (cause, window): (Option<&'static str>, Option<String>) = match (current_agent, limit) {
        (Some(_), Some(window)) => (Some("usage_limit"), Some(window.to_string())),
        (Some(_), None) if !state.hub.is_online(row.node_id) => (Some("offline"), None),
        (Some(_), None) if row.account.is_some() => match current_usage {
            None => (Some("signed_out"), None),
            Some(usage) if usage.limited => (Some("usage_limit"), Some(limited_window(usage).to_string())),
            Some(_) => (None, None),
        },
        _ => (None, None),
    };

    for entry in entries {
        if cause.is_none() && !repick && matches_current(entry, row) {
            return Ok(None);
        }

        let Some(target) = target_for(state, owner, entry, row, cause.is_some()).await? else {
            continue;
        };

        if cause.is_none() && target.agent == row.agent && target.account == row.account && target.node_id.is_none_or(|node_id| node_id == row.node_id) {
            return Ok(None);
        }

        let others_left = on_current_node.iter().any(|candidate| row.account.as_ref() != Some(&candidate.account) && !candidate.usage.limited);
        let every_account = cause.is_some() && !others_left && target.agent != row.agent;

        return Ok(Some(Move { reason: cause.unwrap_or("usage_returned"), detail: json!({"window": window, "every_account": every_account}), target }));
    }

    Ok(None)
}

pub async fn rebalance(state: &AppState, owner: Uuid, id: Uuid, limit: Option<&str>) -> ApiResult<bool> {
    let row = find_owned(&state.pool, id, owner).await?;
    let Some(routing) = row.routing.as_deref().and_then(Routing::parse) else {
        return Ok(false);
    };

    if row.side_kind.as_deref() == Some("subagent") {
        return Ok(false);
    }

    let entries = entries_of(state, owner, routing).await?;
    let Some(chosen) = plan(state, owner, &row, &entries, limit, false).await? else {
        return Ok(false);
    };

    continuation::continue_with(state, owner, id, chosen.target, Some(chosen.reason), chosen.detail).await?;

    Ok(true)
}

pub async fn before_turn(state: &AppState, conversation_id: Uuid) {
    let due = sqlx::query_scalar::<_, Uuid>(sqlx::AssertSqlSafe(format!(
        "select owner from conversations where id = $1 and routing is not null and (status in ('review', 'done') or (status = 'working' and {TURN_HAS_ENDED}))
           and exists (select 1 from queued_prompts where conversation_id = $1)",
    )))
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await;

    let owner = match due {
        Ok(Some(owner)) => owner,
        Ok(None) => return,
        Err(cause) => {
            tracing::warn!("could not check whether {conversation_id} should change account before its turn: {cause}");

            return;
        }
    };

    if let Err(error) = rebalance(state, owner, conversation_id, None).await {
        tracing::warn!("could not re-pick the account of {conversation_id} before its turn: {}", error.message());
    }
}

pub async fn before_chat_turn(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<bool> {
    let row = find_owned(&state.pool, id, owner).await?;

    if row.routing.is_none() || row.status == "working" {
        return Ok(false);
    }

    let moved = match rebalance(state, owner, id, None).await {
        Ok(moved) => moved,
        Err(error) => {
            tracing::warn!("could not re-pick the model of {id} before its turn: {}", error.message());

            false
        }
    };

    if !moved {
        return Ok(false);
    }

    let after = find_owned(&state.pool, id, owner).await?;

    Ok(!crate::chats::is_chat(&after))
}

async fn record_limit(state: &AppState, owner: Uuid, id: Uuid, window: &str) -> ApiResult<()> {
    let row = find_owned(&state.pool, id, owner).await?;

    sqlx::query(
        "update logins set usage_left = 0,
             usage_windows = coalesce((select jsonb_agg(case when w->>'name' = $4 then jsonb_set(w, '{used_percent}', '100') else w end) from jsonb_array_elements(usage_windows) w), '[]')
         where node_id = $1 and agent = $2 and account = $3",
    )
    .bind(row.node_id)
    .bind(&row.agent)
    .bind(&row.account)
    .bind(window)
    .execute(&state.pool)
    .await?;

    Ok(())
}

pub async fn switch_after_limit(state: &AppState, owner: Uuid, conversation_id: Uuid, window: &str) -> bool {
    if let Err(error) = record_limit(state, owner, conversation_id, window).await {
        tracing::warn!("could not record the usage limit of {conversation_id}: {}", error.message());
    }

    match rebalance(state, owner, conversation_id, Some(window)).await {
        Ok(moved) => moved,
        Err(error) => {
            tracing::warn!("could not move {conversation_id} on after its usage limit: {}", error.message());

            false
        }
    }
}

pub async fn carry_on(state: &AppState, owner: Uuid, id: Uuid, text: &str) -> ApiResult<()> {
    let row = find_owned(&state.pool, id, owner).await?;

    if crate::chats::is_chat(&row) {
        crate::chats::send(state, owner, id, text, Vec::new()).await?;

        return Ok(());
    }

    crate::prompts::retry_failed(state, owner, id).await?;

    if crate::goals::resumed(state, owner, id).await? {
        return Ok(());
    }

    crate::prompts::continue_unless_queued(state, owner, id, text).await
}

pub async fn choose(state: &AppState, owner: Uuid, id: Uuid, routing: Routing) -> ApiResult<ConversationRow> {
    let before = find_owned(&state.pool, id, owner).await?;

    if matches!(before.status.as_str(), "working" | "input") {
        return Err(ApiError::conflict("conversation_busy", "This conversation is still working. Wait for the turn to finish or stop it, then continue elsewhere."));
    }

    let entries = entries_of(state, owner, routing).await?;

    if entries.is_empty() {
        return Err(ApiError::conflict("any_model_empty", "Your Any model list is empty. Add models to it in Settings, Agents."));
    }

    sqlx::query("update conversations set routing = $2 where id = $1").bind(id).bind(routing.as_str()).execute(&state.pool).await?;

    let routed = find_owned(&state.pool, id, owner).await?;

    match plan(state, owner, &routed, &entries, None, true).await? {
        Some(chosen) => continuation::continue_with(state, owner, id, chosen.target, Some("chosen"), chosen.detail).await,
        None => {
            state.publish(Some(owner), StreamMessage::Conversation(routed.to_proto()?));

            Ok(routed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn window(name: &str, used: f64, resets: Option<&str>) -> UsageWindow {
        UsageWindow { name: name.into(), used_percent: used, resets_at: resets.map(at), rate_per_hour: None, runs_out_at: None }
    }

    fn candidate(account: &str, five_used: Option<f64>, week_used: Option<f64>) -> Candidate {
        let now = at("2026-10-27T12:00:00Z");
        let windows: Vec<UsageWindow> = [five_used.map(|used| window(FIVE_HOUR, used, None)), week_used.map(|used| window(SEVEN_DAY, used, None))].into_iter().flatten().collect();

        Candidate { node_id: Uuid::nil(), account: account.into(), usage: usage_of(&windows, now) }
    }

    #[test]
    fn the_account_with_the_most_five_hour_usage_left_wins_then_the_weekly_window_breaks_a_tie() {
        let accounts = [candidate("a", Some(60.0), Some(10.0)), candidate("b", Some(20.0), Some(90.0)), candidate("c", Some(20.0), Some(40.0))];

        assert_eq!(best(&accounts).map(|chosen| chosen.account.as_str()), Some("c"));
    }

    #[test]
    fn an_account_at_a_limit_is_never_chosen_while_another_has_usage() {
        let accounts = [candidate("full", Some(100.0), Some(10.0)), candidate("weekly", Some(5.0), Some(100.0)), candidate("ok", Some(95.0), Some(95.0))];

        assert_eq!(best(&accounts).map(|chosen| chosen.account.as_str()), Some("ok"));
        assert_eq!(best(&accounts[..2]), None);
    }

    #[test]
    fn an_account_with_unknown_usage_ranks_after_one_with_a_reading() {
        let accounts = [candidate("unknown", None, None), candidate("known", Some(90.0), Some(90.0))];

        assert_eq!(best(&accounts).map(|chosen| chosen.account.as_str()), Some("known"));
        assert_eq!(best(&accounts[..1]).map(|chosen| chosen.account.as_str()), Some("unknown"));
    }

    #[test]
    fn a_window_that_has_reset_counts_as_unused() {
        let now = at("2026-10-27T12:00:00Z");
        let usage = usage_of(&[window(FIVE_HOUR, 100.0, Some("2026-10-27T11:00:00Z"))], now);

        assert!(!usage.limited);
        assert_eq!(usage.five_left, Some(100.0));
    }

    #[test]
    fn equal_standing_is_settled_by_account_name_so_the_choice_is_stable() {
        let accounts = [candidate("b", Some(10.0), Some(10.0)), candidate("a", Some(10.0), Some(10.0))];

        assert_eq!(best(&accounts).map(|chosen| chosen.account.as_str()), Some("a"));
    }
}
