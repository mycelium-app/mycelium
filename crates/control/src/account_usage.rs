use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::enum_text;
use axum::Json as AxumJson;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use mycelium_proto::api::{UseResetRequest, UseResetResponse};
use mycelium_proto::conversation::{AgentId, LoginInfo, ResetCredits, UsageWindow};
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

const HISTORY_DAYS: i64 = 8;
const SHORTEST_SPAN_MINUTES: i64 = 10;
const FALL_THAT_MEANS_A_RESET: f64 = 0.5;

type Point = (DateTime<Utc>, Vec<UsageWindow>);

pub async fn cache(tx: &mut Transaction<'_, Postgres>, node_id: Uuid, login: &mut LoginInfo) -> ApiResult<()> {
    let account = login.account.as_deref().unwrap_or_default();

    if let Some(read_at) = login.usage_read_at.filter(|_| !login.windows.is_empty()) {
        sqlx::query("insert into login_usage_history (node_id, agent, account, read_at, windows) values ($1, $2, $3, $4, $5) on conflict do nothing")
            .bind(node_id)
            .bind(enum_text(&login.agent))
            .bind(account)
            .bind(read_at)
            .bind(Json(&login.windows))
            .execute(&mut **tx)
            .await?;
    }

    let latest = sqlx::query_as::<_, (DateTime<Utc>, Json<Vec<UsageWindow>>)>("select read_at, windows from login_usage_history where node_id = $1 and agent = $2 and account = $3 order by read_at desc limit 1")
        .bind(node_id)
        .bind(enum_text(&login.agent))
        .bind(account)
        .fetch_optional(&mut **tx)
        .await?;

    if let Some((read_at, windows)) = latest {
        if login.windows.is_empty() || login.usage_read_at.is_none_or(|incoming| incoming < read_at) {
            login.usage_left = windows.0.iter().map(|window| window.used_percent).reduce(f64::max).map(|used| (100.0 - used).round().clamp(0.0, 100.0) as u8);
            login.windows = windows.0;
            login.usage_read_at = Some(read_at);
        }

        sqlx::query("delete from login_usage_history where node_id = $1 and agent = $2 and account = $3 and read_at < now() - make_interval(days => $4::int) and read_at < $5")
            .bind(node_id)
            .bind(enum_text(&login.agent))
            .bind(account)
            .bind(HISTORY_DAYS as i32)
            .bind(read_at)
            .execute(&mut **tx)
            .await?;
    }

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

    let now = Utc::now();

    Ok(windows.map(|windows| windows.0.into_iter().filter(|window| window.resets_at.is_none_or(|reset| reset > now)).collect()).unwrap_or_default())
}

const RESET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub async fn use_reset(State(state): State<AppState>, _user: AuthUser, AxumJson(request): AxumJson<UseResetRequest>) -> ApiResult<AxumJson<UseResetResponse>> {
    if request.agent != AgentId::Codex {
        return Err(ApiError::bad_request("reset_unsupported", "Only Codex accounts have banked resets."));
    }

    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty()).map(str::to_string);
    let holders = sqlx::query_as::<_, (Uuid, Option<Json<ResetCredits>>)>(
        "select l.node_id, l.reset_credits from logins l join nodes n on n.id = l.node_id where l.agent = $1 and coalesce(l.account, '') = coalesce($2, '') and l.state = 'signed_in' and n.revoked_at is null",
    )
    .bind(enum_text(&request.agent))
    .bind(&account)
    .fetch_all(&state.pool)
    .await?;
    let with_credits: Vec<Uuid> = holders.into_iter().filter(|(_, credits)| credits.as_ref().is_some_and(|credits| credits.0.available_count > 0)).map(|(node_id, _)| node_id).collect();

    if with_credits.is_empty() {
        return Err(ApiError::conflict("no_reset", "This account has no banked resets."));
    }

    let asked = with_credits.into_iter().filter(|node_id| state.hub.is_online(*node_id)).find_map(|node_id| state.hub.request_reset(node_id, request.agent, account.clone(), request.idempotency_key.clone()));
    let Some((request_id, answered)) = asked else {
        return Err(crate::nodes::node_offline());
    };

    let answer = tokio::time::timeout(RESET_TIMEOUT, answered).await.ok().and_then(Result::ok);
    state.hub.forget_reset_request(request_id);

    let Some(answer) = answer else {
        return Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "reset_timeout", "Codex did not answer. Check the account's usage before trying again."));
    };

    match answer.outcome {
        Some(outcome) => Ok(AxumJson(UseResetResponse { outcome })),
        None => Err(ApiError::conflict("reset_failed", answer.error.unwrap_or_else(|| "Codex could not use the reset.".into()))),
    }
}
