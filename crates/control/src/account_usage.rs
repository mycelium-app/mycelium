use crate::error::ApiResult;
use crate::state::AppState;
use crate::store::enum_text;
use chrono::{DateTime, Duration, Utc};
use mycelium_proto::conversation::{AgentId, LoginInfo, UsageWindow};
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

const HISTORY_DAYS: i64 = 8;
const SHORTEST_SPAN_MINUTES: i64 = 10;
const FALL_THAT_MEANS_A_RESET: f64 = 0.5;

type Point = (DateTime<Utc>, Vec<UsageWindow>);

pub async fn record(tx: &mut Transaction<'_, Postgres>, node_id: Uuid, login: &LoginInfo) -> ApiResult<()> {
    let Some(read_at) = login.usage_read_at.filter(|_| !login.windows.is_empty()) else {
        return Ok(());
    };
    let account = login.account.clone().unwrap_or_default();

    sqlx::query("insert into login_usage_history (node_id, agent, account, read_at, windows) values ($1, $2, $3, $4, $5) on conflict do nothing")
        .bind(node_id)
        .bind(enum_text(&login.agent))
        .bind(&account)
        .bind(read_at)
        .bind(Json(&login.windows))
        .execute(&mut **tx)
        .await?;
    sqlx::query("delete from login_usage_history where node_id = $1 and agent = $2 and account = $3 and read_at < now() - make_interval(days => $4::int)")
        .bind(node_id)
        .bind(enum_text(&login.agent))
        .bind(&account)
        .bind(HISTORY_DAYS as i32)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

pub async fn project_windows(state: &AppState, node_ids: &[Uuid]) -> ApiResult<Vec<(Uuid, String, String, Vec<Point>)>> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>, Json<Vec<UsageWindow>>)>("select node_id, agent, account, read_at, windows from login_usage_history where node_id = any($1) order by read_at").bind(node_ids).fetch_all(&state.pool).await?;
    let mut grouped: Vec<(Uuid, String, String, Vec<Point>)> = Vec::new();

    for (node_id, agent, account, read_at, windows) in rows {
        match grouped.iter_mut().find(|group| group.0 == node_id && group.1 == agent && group.2 == account) {
            Some(group) => group.3.push((read_at, windows.0)),
            None => grouped.push((node_id, agent, account, vec![(read_at, windows.0)])),
        }
    }

    Ok(grouped)
}

pub fn with_projections(windows: Vec<UsageWindow>, history: &[Point]) -> Vec<UsageWindow> {
    windows
        .into_iter()
        .map(|window| {
            let (rate_per_hour, runs_out_at) = project(&window, history);

            UsageWindow { rate_per_hour, runs_out_at, ..window }
        })
        .collect()
}

fn project(window: &UsageWindow, history: &[Point]) -> (Option<f64>, Option<DateTime<Utc>>) {
    let points: Vec<(DateTime<Utc>, f64)> = history.iter().filter_map(|(at, windows)| windows.iter().find(|candidate| candidate.name == window.name).map(|found| (*at, found.used_percent))).collect();
    let cycle_start = points.windows(2).rposition(|pair| pair[1].1 < pair[0].1 - FALL_THAT_MEANS_A_RESET).map_or(0, |index| index + 1);
    let cycle = &points[cycle_start..];
    let (Some(first), Some(last)) = (cycle.first(), cycle.last()) else {
        return (None, None);
    };

    if last.0 - first.0 < Duration::minutes(SHORTEST_SPAN_MINUTES) {
        return (None, None);
    }

    let hours = (last.0 - first.0).num_seconds() as f64 / 3600.0;
    let rate = (last.1 - first.1) / hours;

    if rate <= 0.0 {
        return (None, None);
    }

    let hours_left = (100.0 - last.1).max(0.0) / rate;
    let runs_out_at = last.0 + Duration::seconds((hours_left * 3600.0) as i64);
    let before_reset = window.resets_at.is_none_or(|resets_at| runs_out_at < resets_at);

    (Some(rate), before_reset.then_some(runs_out_at))
}

pub async fn windows_of(state: &AppState, node_id: Uuid, agent: AgentId, account: Option<&str>) -> ApiResult<Vec<UsageWindow>> {
    let windows = sqlx::query_scalar::<_, Json<Vec<UsageWindow>>>(
        "select usage_windows from logins where node_id = $1 and agent = $2 and ($3::text is null or coalesce(account, '') = $3)
         order by jsonb_array_length(usage_windows) > 0 desc limit 1",
    )
    .bind(node_id)
    .bind(enum_text(&agent))
    .bind(account)
    .fetch_optional(&state.pool)
    .await?;

    Ok(windows.map(|windows| windows.0).unwrap_or_default())
}
