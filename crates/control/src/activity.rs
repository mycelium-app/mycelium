use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::Tx;
use axum::Json;
use axum::extract::{Query, State};
use chrono::NaiveDate;
use mycelium_proto::api::{Activity, ActivityRow};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

const BREAKDOWN_LIMIT: i64 = 50;
const PROJECT_OF_CONVERSATION: &str = "(select p.id from projects p where p.node_id = c.node_id and (c.cwd = p.path or starts_with(c.cwd, p.path || '/') or starts_with(c.cwd, p.path || '-worktrees/')) order by length(p.path) desc limit 1)";

#[derive(Deserialize)]
pub struct ActivityQuery {
    range: Option<String>,
    node_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct TotalsRow {
    key: String,
    name: String,
    turns: i64,
    reported_turns: i64,
    working_ms: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_usd: Option<f64>,
}

impl TotalsRow {
    fn to_proto(self) -> ActivityRow {
        ActivityRow {
            key: self.key,
            name: self.name,
            turns: self.turns,
            reported_turns: self.reported_turns,
            working_ms: self.working_ms,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            cost_usd: self.cost_usd,
        }
    }
}

fn token_count(usage: &Value, field: &str) -> i64 {
    usage.get(field).and_then(Value::as_i64).unwrap_or(0).max(0)
}

pub async fn record_turn(state: &AppState, conversation_id: Uuid, usage: Option<&Value>) -> ApiResult<()> {
    let latest_update = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "select payload->'update'->'cost'->>'amount', payload->'update'->'_meta'->>'_claude/model' from events
         where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'usage_update'
         order by seq desc limit 1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;
    let (cost_total, reported_model) = match latest_update {
        Some((cost, model)) => (cost.and_then(|cost| cost.parse::<f64>().ok()), model.filter(|model| !model.is_empty())),
        None => (None, None),
    };
    let previous_total = sqlx::query_scalar::<_, f64>("select cost_total from usage_turns where conversation_id = $1 and cost_total is not null order by id desc limit 1").bind(conversation_id).fetch_optional(&state.pool).await?;
    let cost = cost_total.map(|total| match previous_total {
        Some(previous) if total >= previous => total - previous,
        _ => total,
    });

    let empty = Value::Null;
    let usage = usage.unwrap_or(&empty);
    let reported = usage.is_object() || cost.is_some();

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "insert into usage_turns (conversation_id, owner, node_id, agent, project_id, model, day, working_ms, reported, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd, cost_total)
         select c.id, c.owner, c.node_id, c.agent, {PROJECT_OF_CONVERSATION}, coalesce($2, c.model), (now() at time zone 'utc')::date,
                (select (extract(epoch from now() - max(e.created_at)) * 1000)::bigint from events e where e.conversation_id = c.id and e.kind = 'user_message'),
                $3, $4, $5, $6, $7, $8, $9
         from conversations c where c.id = $1"
    )))
    .bind(conversation_id)
    .bind(reported_model)
    .bind(reported)
    .bind(token_count(usage, "inputTokens"))
    .bind(token_count(usage, "outputTokens"))
    .bind(token_count(usage, "cachedReadTokens"))
    .bind(token_count(usage, "cachedWriteTokens"))
    .bind(cost)
    .bind(cost_total)
    .execute(&state.pool)
    .await?;

    Ok(())
}

pub async fn record_imported_turns(tx: &mut Tx<'_>, conversation_id: Uuid) -> ApiResult<()> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "insert into usage_turns (conversation_id, owner, node_id, agent, project_id, model, ended_at, day)
         select c.id, c.owner, c.node_id, c.agent, {PROJECT_OF_CONVERSATION}, c.model, c.updated_at, (c.updated_at at time zone 'utc')::date
         from events e join conversations c on c.id = e.conversation_id where e.conversation_id = $1 and e.kind = 'turn_end'"
    )))
    .bind(conversation_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn grouped(state: &AppState, owner: Uuid, days: i32, node_id: Option<Uuid>, key: &str, name: &str, order: &str) -> ApiResult<Vec<ActivityRow>> {
    let rows = sqlx::query_as::<_, TotalsRow>(sqlx::AssertSqlSafe(format!(
        "select {key} as key, {name} as name, count(*) as turns, count(*) filter (where u.reported) as reported_turns,
                coalesce(sum(u.working_ms), 0)::bigint as working_ms, coalesce(sum(u.input_tokens), 0)::bigint as input_tokens,
                coalesce(sum(u.output_tokens), 0)::bigint as output_tokens, coalesce(sum(u.cache_read_tokens), 0)::bigint as cache_read_tokens,
                coalesce(sum(u.cache_write_tokens), 0)::bigint as cache_write_tokens, sum(u.cost_usd) as cost_usd
         from usage_turns u
         join conversations c on c.id = u.conversation_id
         join nodes n on n.id = u.node_id
         left join projects p on p.id = u.project_id
         where u.owner = $1 and u.day > (now() at time zone 'utc')::date - $2 and ($3::uuid is null or u.node_id = $3)
         group by 1, 2 order by {order} limit {BREAKDOWN_LIMIT}"
    )))
    .bind(owner)
    .bind(days)
    .bind(node_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows.into_iter().map(TotalsRow::to_proto).collect())
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Query(query): Query<ActivityQuery>) -> ApiResult<Json<Activity>> {
    let (range, days) = match query.range.as_deref().unwrap_or("7d") {
        "today" => ("today", 1),
        "7d" => ("7d", 7),
        "30d" => ("30d", 30),
        _ => return Err(ApiError::bad_request("range_invalid", "Choose today, 7d or 30d as the range.")),
    };

    let by_tokens = "sum(u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens) desc, count(*) desc";
    let totals = grouped(&state, user.id, days, query.node_id, "''", "''", "1").await?.into_iter().next().unwrap_or_default();
    let recorded_days = grouped(&state, user.id, days, query.node_id, "u.day::text", "u.day::text", "1").await?;
    let conversations = grouped(&state, user.id, days, query.node_id, "c.id::text", "c.title", by_tokens).await?;
    let nodes = grouped(&state, user.id, days, query.node_id, "n.id::text", "n.name", by_tokens).await?;
    let agents = grouped(&state, user.id, days, query.node_id, "u.agent", "u.agent", by_tokens).await?;
    let projects = grouped(&state, user.id, days, query.node_id, "coalesce(p.id::text, '')", "coalesce(p.name, 'No project')", by_tokens).await?;
    let models = grouped(&state, user.id, days, query.node_id, "coalesce(u.model, '')", "coalesce(u.model, 'Unknown')", by_tokens).await?;

    let today = sqlx::query_scalar::<_, NaiveDate>("select (now() at time zone 'utc')::date").fetch_one(&state.pool).await?;
    let series = (0..days).rev().map(|ago| (today - chrono::Duration::days(ago as i64)).to_string()).map(|date| {
        recorded_days.iter().find(|row| row.key == date).cloned().unwrap_or(ActivityRow { key: date.clone(), name: date, ..ActivityRow::default() })
    });
    let unreported_agents = agents.iter().filter(|row| row.turns > 0 && row.reported_turns == 0).map(|row| row.key.clone()).collect();

    Ok(Json(Activity { range: range.to_string(), totals, days: series.collect(), conversations, nodes, agents, projects, models, unreported_agents }))
}
