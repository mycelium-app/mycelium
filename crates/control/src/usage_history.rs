use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use mycelium_proto::conversation::{AgentId, UsageWindow};
use mycelium_proto::usage_history::{ImportOrchestratorUsage, ImportedUsage, OrchestratorReading, TokensPerPoint, UsageHistory, UsageHistoryAccount, UsageHistoryWindow, UsageReading};
use serde::Deserialize;
use sqlx::types::Json as Sql;
use uuid::Uuid;

const FIVE_HOUR: &str = "five_hour";
const SEVEN_DAY: &str = "seven_day";
const SAME_CYCLE_MINUTES: i64 = 30;
const FALL_THAT_STARTS_A_CYCLE: f64 = 0.5;
const SMALLEST_RISE: f64 = 5.0;
const READINGS_PER_WINDOW_LIMIT: usize = 1500;
const PLAN_PRICES_AS_OF: &str = "2026-10-27";

const PLAN_PRICES: &[(AgentId, &str, f64)] = &[
    (AgentId::Claude, "pro", 20.0),
    (AgentId::Claude, "max_5x", 100.0),
    (AgentId::Claude, "max_20x", 200.0),
    (AgentId::Codex, "plus", 20.0),
    (AgentId::Codex, "pro", 200.0),
];

#[derive(Deserialize)]
pub struct HistoryQuery {
    range: Option<String>,
    account: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub at: DateTime<Utc>,
    pub percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub at: DateTime<Utc>,
    pub tokens: i64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    pub started_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub first_at: DateTime<Utc>,
    pub first_percent: f64,
    pub last_at: DateTime<Utc>,
    pub peak_percent: f64,
}

pub fn plan_price(agent: AgentId, plan: &str) -> Option<f64> {
    PLAN_PRICES.iter().find(|(known, name, _)| *known == agent && *name == plan).map(|(_, _, price)| *price)
}

pub fn cycles(readings: &[Reading], length: Duration) -> Vec<Cycle> {
    let mut found: Vec<Cycle> = Vec::new();
    let mut resets: Option<DateTime<Utc>> = None;

    for reading in readings {
        let same = found.last().is_some_and(|cycle| match (resets, reading.resets_at) {
            (Some(known), Some(incoming)) => (incoming - known).num_minutes().abs() <= SAME_CYCLE_MINUTES,
            _ => reading.percent >= cycle.peak_percent - FALL_THAT_STARTS_A_CYCLE,
        });

        if same && let Some(cycle) = found.last_mut() {
            cycle.last_at = reading.at;
            cycle.peak_percent = cycle.peak_percent.max(reading.percent);

            continue;
        }

        resets = reading.resets_at;
        found.push(Cycle { started_at: reading.resets_at.map_or(reading.at, |ends| ends - length), ends_at: reading.resets_at, first_at: reading.at, first_percent: reading.percent, last_at: reading.at, peak_percent: reading.percent });
    }

    found
}

pub fn tokens_between(turns: &[Turn], from: DateTime<Utc>, until: DateTime<Utc>) -> i64 {
    turns.iter().filter(|turn| turn.at >= from && turn.at < until).map(|turn| turn.tokens).sum()
}

pub fn cost_between(turns: &[Turn], from: DateTime<Utc>, until: DateTime<Utc>) -> Option<f64> {
    turns.iter().filter(|turn| turn.at >= from && turn.at < until).filter_map(|turn| turn.cost_usd).reduce(|total, cost| total + cost)
}

pub fn tokens_per_point(cycle: &Cycle, turns: &[Turn]) -> Option<f64> {
    let rise = cycle.peak_percent - cycle.first_percent;

    if rise < SMALLEST_RISE {
        return None;
    }

    let tokens = tokens_between(turns, cycle.first_at, cycle.last_at + Duration::seconds(1));

    (tokens > 0).then(|| tokens as f64 / rise)
}

pub fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;

    match values.len() {
        0 => None,
        length if length % 2 == 1 => Some(values[middle]),
        _ => Some((values[middle - 1] + values[middle]) / 2.0),
    }
}

fn thinned<T: Clone>(items: Vec<T>) -> Vec<T> {
    if items.len() <= READINGS_PER_WINDOW_LIMIT {
        return items;
    }

    let stride = items.len().div_ceil(READINGS_PER_WINDOW_LIMIT);

    items.into_iter().step_by(stride).collect()
}

fn range_start(range: &str, now: DateTime<Utc>) -> ApiResult<Option<DateTime<Utc>>> {
    match range {
        "today" => Ok(now.date_naive().and_hms_opt(0, 0, 0).map(|midnight| Utc.from_utc_datetime(&midnight))),
        "7d" => Ok(Some(now - Duration::days(7))),
        "30d" => Ok(Some(now - Duration::days(30))),
        "all" => Ok(None),
        _ => Err(ApiError::bad_request("range_invalid", "Choose today, 7d, 30d or all as the range.")),
    }
}

fn month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0).single().unwrap_or(now)
}

fn readings_of(rows: &[(DateTime<Utc>, Vec<UsageWindow>)], window: &str) -> Vec<Reading> {
    rows.iter().filter_map(|(at, windows)| windows.iter().find(|candidate| candidate.name == window).map(|found| Reading { at: *at, percent: found.used_percent, resets_at: found.resets_at })).collect()
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Query(query): Query<HistoryQuery>) -> ApiResult<Json<UsageHistory>> {
    let now = Utc::now();
    let since = range_start(query.range.as_deref().unwrap_or("7d"), now)?;
    let wanted = query.account.as_deref().and_then(|text| text.split_once(':')).map(|(agent, account)| (agent.to_string(), account.to_string()));
    let known = sqlx::query_as::<_, (String, String)>(
        "select h.agent, h.account from login_usage_history h join nodes n on n.id = h.node_id where n.revoked_at is null
         union select l.agent, coalesce(l.account, '') from logins l join nodes n on n.id = l.node_id where n.revoked_at is null and l.state = 'signed_in'
         order by 1, 2",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut accounts = Vec::new();

    for (agent, account) in known.into_iter().filter(|candidate| wanted.as_ref().is_none_or(|wanted| wanted == candidate)) {
        accounts.push(account_history(&state, user.id, parse_enum(&agent)?, account, since, now).await?);
    }

    Ok(Json(UsageHistory { accounts }))
}

async fn account_history(state: &AppState, owner: Uuid, agent: AgentId, account: String, since: Option<DateTime<Utc>>, now: DateTime<Utc>) -> ApiResult<UsageHistoryAccount> {
    let rows = sqlx::query_as::<_, (DateTime<Utc>, Sql<Vec<UsageWindow>>)>(
        "select h.read_at, h.windows from login_usage_history h join nodes n on n.id = h.node_id where n.revoked_at is null and h.agent = $1 and h.account = $2 order by h.read_at",
    )
    .bind(enum_text(&agent))
    .bind(&account)
    .fetch_all(&state.pool)
    .await?;
    let rows: Vec<(DateTime<Utc>, Vec<UsageWindow>)> = rows.into_iter().map(|(at, windows)| (at, windows.0)).collect();
    let (readings_since, orchestrator_until) = sqlx::query_as::<_, (Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
        "select min(h.read_at), max(h.read_at) filter (where h.source = 'orchestrator') from login_usage_history h join nodes n on n.id = h.node_id where n.revoked_at is null and h.agent = $1 and h.account = $2",
    )
    .bind(enum_text(&agent))
    .bind(&account)
    .fetch_one(&state.pool)
    .await?;
    let plan = sqlx::query_scalar::<_, Option<String>>("select l.plan from logins l join nodes n on n.id = l.node_id where n.revoked_at is null and l.agent = $1 and coalesce(l.account, '') = $2 and l.plan is not null order by l.usage_read_at desc nulls last limit 1")
        .bind(enum_text(&agent))
        .bind(&account)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let turns = sqlx::query_as::<_, (DateTime<Utc>, i64, Option<f64>)>(
        "select u.ended_at, (u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens)::bigint, u.cost_usd
         from usage_turns u join conversations c on c.id = u.conversation_id where u.owner = $1 and u.agent = $2 and coalesce(c.account, '') = $3 order by u.ended_at",
    )
    .bind(owner)
    .bind(enum_text(&agent))
    .bind(&account)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|(at, tokens, cost_usd)| Turn { at, tokens, cost_usd })
    .collect::<Vec<_>>();

    let five_hour = cycles(&readings_of(&rows, FIVE_HOUR), Duration::hours(5));
    let seven_day = cycles(&readings_of(&rows, SEVEN_DAY), Duration::days(7));
    let month = month_start(now);
    let in_range = |at: DateTime<Utc>| since.is_none_or(|start| at >= start);
    let window_end = |cycle: &Cycle| cycle.ends_at.unwrap_or(cycle.last_at);
    let windows = five_hour
        .iter()
        .filter(|cycle| in_range(cycle.started_at))
        .map(|cycle| UsageHistoryWindow {
            window: FIVE_HOUR.into(),
            started_at: cycle.started_at,
            peak_percent: cycle.peak_percent,
            tokens: tokens_between(&turns, cycle.started_at, window_end(cycle)),
            tokens_per_point: tokens_per_point(cycle, &turns),
            api_equivalent_usd: cost_between(&turns, cycle.started_at, window_end(cycle)),
        })
        .collect();
    let five_hour_values: Vec<f64> = five_hour.iter().filter_map(|cycle| tokens_per_point(cycle, &turns)).collect();
    let seven_day_values: Vec<f64> = seven_day.iter().filter_map(|cycle| tokens_per_point(cycle, &turns)).collect();
    let month_windows: Vec<f64> = five_hour.iter().filter(|cycle| cycle.started_at >= month).filter_map(|cycle| cost_between(&turns, cycle.started_at, window_end(cycle))).collect();
    let api_equivalent_month_usd = cost_between(&turns, month, now + Duration::seconds(1));
    let api_equivalent_per_window_usd = (!month_windows.is_empty()).then(|| month_windows.iter().sum::<f64>() / month_windows.len() as f64);
    let price = plan.as_deref().and_then(|plan| plan_price(agent, plan));
    let readings = [FIVE_HOUR, SEVEN_DAY]
        .iter()
        .flat_map(|window| {
            let kept: Vec<UsageReading> = readings_of(&rows, window).into_iter().filter(|reading| in_range(reading.at)).map(|reading| UsageReading { at: reading.at, window: (*window).into(), percent: reading.percent }).collect();

            thinned(kept)
        })
        .collect();

    Ok(UsageHistoryAccount {
        agent,
        account,
        plan_price_usd: price,
        plan_price_as_of: price.map(|_| PLAN_PRICES_AS_OF.to_string()),
        plan,
        readings_since,
        orchestrator_until,
        readings,
        windows,
        tokens_per_point: TokensPerPoint { five_hour: median(five_hour_values.clone()), seven_day: median(seven_day_values.clone()), five_hour_samples: five_hour_values.len() as i64, seven_day_samples: seven_day_values.len() as i64 },
        api_equivalent_month_usd,
        api_equivalent_per_window_usd,
    })
}

fn window_of(name: &str, percent: f64, resets: Option<i64>) -> UsageWindow {
    UsageWindow { name: name.into(), used_percent: percent, resets_at: resets.filter(|epoch| *epoch > 0).and_then(|epoch| DateTime::from_timestamp(epoch, 0)), rate_per_hour: None, runs_out_at: None }
}

pub async fn import_orchestrator(State(state): State<AppState>, _user: AuthUser, Json(request): Json<ImportOrchestratorUsage>) -> ApiResult<Json<ImportedUsage>> {
    let account = request.account.as_deref().map(str::trim).unwrap_or_default().to_string();
    let node_id = match request.node_id {
        Some(node_id) => Some(node_id),
        None => sqlx::query_scalar::<_, Uuid>("select n.id from nodes n left join logins l on l.node_id = n.id and l.agent = $1 and coalesce(l.account, '') = $2 where n.revoked_at is null order by (l.node_id is not null) desc, n.created_at limit 1").bind(enum_text(&request.agent)).bind(&account).fetch_optional(&state.pool).await?,
    };
    let Some(node_id) = node_id else {
        return Err(ApiError::conflict("no_node", "Add a node before importing usage readings."));
    };

    let imported = store_orchestrator_readings(&state, node_id, request.agent, &account, request.coarse.iter().chain(request.fine.iter())).await?;

    Ok(Json(ImportedUsage { imported }))
}

pub async fn store_orchestrator_readings<'a>(state: &AppState, node_id: Uuid, agent: AgentId, account: &str, readings: impl Iterator<Item = &'a OrchestratorReading>) -> ApiResult<i64> {
    let mut imported = 0;

    for reading in readings {
        let Some(read_at) = DateTime::from_timestamp(reading.at, 0) else {
            continue;
        };
        let windows = vec![window_of(FIVE_HOUR, reading.five, reading.five_resets), window_of(SEVEN_DAY, reading.seven, reading.seven_resets)];
        let inserted = sqlx::query("insert into login_usage_history (node_id, agent, account, read_at, windows, source) values ($1, $2, $3, $4, $5, 'orchestrator') on conflict do nothing")
            .bind(node_id)
            .bind(enum_text(&agent))
            .bind(account)
            .bind(read_at)
            .bind(Sql(&windows))
            .execute(&state.pool)
            .await?;
        imported += inserted.rows_affected() as i64;
    }

    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn reading(time: &str, percent: f64, resets: &str) -> Reading {
        Reading { at: at(time), percent, resets_at: Some(at(resets)) }
    }

    fn turn(time: &str, tokens: i64) -> Turn {
        Turn { at: at(time), tokens, cost_usd: Some(tokens as f64 / 1000.0) }
    }

    #[test]
    fn readings_that_share_a_reset_time_are_one_window_and_a_new_reset_time_starts_the_next() {
        let found = cycles(
            &[
                reading("2026-10-26T08:05:00Z", 2.0, "2026-10-26T13:00:00.200Z"),
                reading("2026-10-26T10:00:00Z", 30.0, "2026-10-26T13:00:00.900Z"),
                reading("2026-10-26T13:10:00Z", 1.0, "2026-10-26T18:00:00Z"),
            ],
            Duration::hours(5),
        );

        assert_eq!(found.len(), 2);
        assert_eq!((found[0].peak_percent, found[0].started_at), (30.0, at("2026-10-26T08:00:00.200Z")));
        assert_eq!(found[1].first_percent, 1.0);
    }

    #[test]
    fn a_fall_in_percent_without_reset_times_starts_a_new_window() {
        let found = cycles(
            &[
                Reading { at: at("2026-10-26T08:00:00Z"), percent: 40.0, resets_at: None },
                Reading { at: at("2026-10-26T09:00:00Z"), percent: 60.0, resets_at: None },
                Reading { at: at("2026-10-26T14:00:00Z"), percent: 3.0, resets_at: None },
            ],
            Duration::hours(5),
        );

        assert_eq!(found.iter().map(|cycle| cycle.peak_percent).collect::<Vec<_>>(), [60.0, 3.0]);
    }

    #[test]
    fn tokens_per_point_divides_the_tokens_between_the_first_and_last_reading_by_the_rise() {
        let cycle = &cycles(&[reading("2026-10-26T08:00:00Z", 10.0, "2026-10-26T13:00:00Z"), reading("2026-10-26T10:00:00Z", 30.0, "2026-10-26T13:00:00Z")], Duration::hours(5))[0];
        let turns = [turn("2026-10-26T07:00:00Z", 999_999), turn("2026-10-26T09:00:00Z", 1_000_000), turn("2026-10-26T10:00:00Z", 1_000_000)];

        assert_eq!(tokens_per_point(cycle, &turns), Some(100_000.0));
    }

    #[test]
    fn a_window_that_rose_less_than_five_points_gives_no_sample() {
        let cycle = &cycles(&[reading("2026-10-26T08:00:00Z", 10.0, "2026-10-26T13:00:00Z"), reading("2026-10-26T10:00:00Z", 14.0, "2026-10-26T13:00:00Z")], Duration::hours(5))[0];

        assert_eq!(tokens_per_point(cycle, &[turn("2026-10-26T09:00:00Z", 500_000)]), None);
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!((median(vec![3.0, 1.0, 2.0]), median(vec![4.0, 1.0, 2.0, 3.0]), median(vec![])), (Some(2.0), Some(2.5), None));
    }

    #[test]
    fn only_a_published_plan_has_a_price() {
        assert_eq!((plan_price(AgentId::Claude, "max_5x"), plan_price(AgentId::Codex, "pro"), plan_price(AgentId::Claude, "enterprise")), (Some(100.0), Some(200.0), None));
    }

    #[test]
    fn the_cost_of_a_window_adds_up_only_the_turns_that_have_one() {
        let turns = [turn("2026-10-26T09:00:00Z", 1_000), Turn { at: at("2026-10-26T09:30:00Z"), tokens: 5, cost_usd: None }];

        assert_eq!(cost_between(&turns, at("2026-10-26T08:00:00Z"), at("2026-10-26T13:00:00Z")), Some(1.0));
        assert_eq!(cost_between(&turns, at("2026-10-27T08:00:00Z"), at("2026-10-27T13:00:00Z")), None);
    }
}
