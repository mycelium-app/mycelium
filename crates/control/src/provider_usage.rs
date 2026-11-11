use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::model_providers;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use crate::model_providers::ProviderRow;
use chrono::{DateTime, Datelike, SubsecRound, Utc};
use mycelium_proto::api::ModelProviderKind;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as DbJson;
use std::time::Duration;
use uuid::Uuid;

const MAX_PAGES: usize = 20;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OpenRouterSpend {
    #[serde(rename(deserialize = "usage"))]
    pub total: f64,
    #[serde(rename(deserialize = "usage_daily"))]
    pub daily: Option<f64>,
    #[serde(rename(deserialize = "usage_weekly"))]
    pub weekly: Option<f64>,
    #[serde(rename(deserialize = "usage_monthly"))]
    pub monthly: Option<f64>,
    pub limit_remaining: Option<f64>,
    pub limit: Option<f64>,
    pub limit_reset: Option<String>,
}

#[derive(Deserialize)]
struct KeyResponse {
    data: OpenRouterSpend,
}

#[derive(Serialize)]
pub struct ProviderUsage {
    provider_id: Uuid,
    name: String,
    read_at: Option<DateTime<Utc>>,
    status: String,
    spend: Option<serde_json::Value>,
    api_cost: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ApiCost {
    pub today_usd: f64,
    pub month_usd: f64,
    pub today_tokens: i64,
}

#[derive(Debug, PartialEq)]
enum ReadFailure {
    Refused,
    Unreadable,
}

async fn read_buckets(
    http: &reqwest::Client,
    provider: &ProviderRow,
    key: &str,
    path: &str,
    starting_at: DateTime<Utc>,
) -> Result<Vec<serde_json::Value>, ReadFailure> {
    let mut buckets = Vec::new();
    let mut page: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let mut query = vec![
            ("starting_at", starting_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            ("bucket_width", "1d".to_string()),
            ("limit", "31".to_string()),
        ];

        if let Some(page) = page.take() {
            query.push(("page", page));
        }

        let response = provider
            .read_admin(http, key, path, &query)
            .await
            .map_err(|_| ReadFailure::Unreadable)?;

        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(ReadFailure::Refused);
        }

        let body: serde_json::Value = response
            .error_for_status()
            .map_err(|_| ReadFailure::Unreadable)?
            .json()
            .await
            .map_err(|_| ReadFailure::Unreadable)?;

        buckets.extend(body["data"].as_array().cloned().unwrap_or_default());

        match body["next_page"].as_str() {
            Some(next) if body["has_more"].as_bool() == Some(true) => page = Some(next.to_string()),
            _ => return Ok(buckets),
        }
    }

    Err(ReadFailure::Unreadable)
}

async fn read_api_cost(
    http: &reqwest::Client,
    provider: &ProviderRow,
    key: &str,
    now: DateTime<Utc>,
) -> Result<ApiCost, ReadFailure> {
    let today = now.format("%Y-%m-%d").to_string();
    let month_start = now
        .date_naive()
        .with_day(1)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|start| start.and_utc())
        .ok_or(ReadFailure::Unreadable)?;
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|start| start.and_utc())
        .ok_or(ReadFailure::Unreadable)?;
    let costs = read_buckets(http, provider, key, "cost_report", month_start).await?;
    let tokens = read_buckets(http, provider, key, "usage_report/messages", today_start).await?;
    let in_cents = |bucket: &serde_json::Value| -> f64 {
        bucket["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|result| result["amount"].as_str().and_then(|amount| amount.parse::<f64>().ok()))
            .sum()
    };
    let token_count = |bucket: &serde_json::Value| -> i64 {
        bucket["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|result| {
                let creation = &result["cache_creation"];
                let creation: i64 = if creation.is_object() {
                    creation.as_object().into_iter().flatten().filter_map(|(_, count)| count.as_i64()).sum()
                } else {
                    result["cache_creation_input_tokens"].as_i64().unwrap_or(0)
                };

                result["uncached_input_tokens"].as_i64().unwrap_or(0)
                    + result["cache_read_input_tokens"].as_i64().unwrap_or(0)
                    + creation
                    + result["output_tokens"].as_i64().unwrap_or(0)
            })
            .sum()
    };
    let on_today = |bucket: &&serde_json::Value| bucket["starting_at"].as_str().is_some_and(|at| at.starts_with(&today));

    Ok(ApiCost {
        today_usd: costs.iter().filter(on_today).map(in_cents).sum::<f64>() / 100.0,
        month_usd: costs.iter().map(in_cents).sum::<f64>() / 100.0,
        today_tokens: tokens.iter().filter(on_today).map(token_count).sum(),
    })
}

async fn fetch(http: &reqwest::Client, address: &str, key: &str) -> Option<OpenRouterSpend> {
    let response = http
        .get(address)
        .bearer_auth(key)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let body: KeyResponse = response.error_for_status().ok()?.json().await.ok()?;
    let spend = body.data;

    if !spend.total.is_finite()
        || spend.total < 0.0
        || [spend.daily, spend.weekly, spend.monthly]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || value < 0.0)
        || spend
            .limit_remaining
            .is_some_and(|value| !value.is_finite())
        || spend.limit.is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return None;
    }

    Some(spend)
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ProviderUsage>>> {
    let mut usage = Vec::new();

    for provider in model_providers::list_owned(&state, user.id).await? {
        let anthropic = provider.parsed_kind()? == ModelProviderKind::Anthropic;
        let address = provider
            .base_url
            .as_deref()
            .and_then(|value| url::Url::parse(value).ok());
        let openrouter = address.is_some_and(|address| {
            address.scheme() == "https"
                && address.host_str() == Some("openrouter.ai")
                && address.port_or_known_default() == Some(443)
                && address.path().trim_end_matches('/') == "/api/v1"
        });

        if !(openrouter || anthropic && provider.admin_key.is_some()) {
            continue;
        }

        let mut cached = sqlx::query_as::<_, (Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<DbJson<serde_json::Value>>, String)>(
            "select read_at, attempted_at, spend, status from model_provider_usage where provider_id = $1",
        )
        .bind(provider.id)
        .fetch_optional(&state.pool)
        .await?
        .map(|(read_at, attempted_at, spend, status)| (read_at, attempted_at.or(read_at), spend.map(|spend| spend.0), status));
        let stale = cached.as_ref().is_none_or(|(_, attempted_at, _, _)| {
            attempted_at.is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() >= 60)
        });

        let key = if openrouter { provider.api_key.as_deref() } else { provider.admin_key.as_deref() };

        if let Some(key) = key.filter(|_| stale) {
            let now = Utc::now().trunc_subsecs(6);
            let previous = cached.clone();
            let reading = if openrouter {
                fetch(&state.http, "https://openrouter.ai/api/v1/key", key)
                    .await
                    .and_then(|spend| serde_json::to_value(spend).ok())
                    .ok_or(ReadFailure::Unreadable)
            } else {
                read_api_cost(&state.http, &provider, key, now)
                    .await
                    .and_then(|cost| serde_json::to_value(cost).map_err(|_| ReadFailure::Unreadable))
            };
            let (read_at, spend, status) = match reading {
                Ok(spend) => (Some(now), Some(spend), "ok"),
                Err(failure) => (
                    previous.as_ref().and_then(|(at, ..)| *at),
                    previous.as_ref().and_then(|(_, _, spend, _)| spend.clone()),
                    if failure == ReadFailure::Refused { "refused" } else { "unreadable" },
                ),
            };

            sqlx::query("insert into model_provider_usage (provider_id, read_at, attempted_at, spend, status) values ($1, $2, $3, $4, $5) on conflict (provider_id) do update set read_at = excluded.read_at, attempted_at = excluded.attempted_at, spend = excluded.spend, status = excluded.status where coalesce(model_provider_usage.attempted_at, model_provider_usage.read_at) is null or coalesce(model_provider_usage.attempted_at, model_provider_usage.read_at) < excluded.attempted_at")
                .bind(provider.id).bind(read_at).bind(now).bind(&spend).bind(status).execute(&state.pool).await?;
            cached = Some((read_at, Some(now), spend, status.to_string()));
        }

        let (read_at, spend, status) = cached.map_or((None, None, "ok".to_string()), |(read_at, _, spend, status)| (read_at, spend, status));

        usage.push(ProviderUsage {
            provider_id: provider.id,
            name: provider.name,
            read_at,
            status,
            spend: spend.clone().filter(|_| openrouter),
            api_cost: spend.filter(|_| !openrouter),
        });
    }

    Ok(Json(usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;
    use serde_json::json;

    #[tokio::test]
    async fn key_spend_is_read_in_dollars_and_failed_or_invalid_readings_are_unknown() {
        async fn key(headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
            if headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-key")
            {
                return Err(StatusCode::UNAUTHORIZED);
            }

            Ok(Json(
                json!({"data":{"usage":12.3,"usage_daily":0.4,"usage_weekly":2.0,"usage_monthly":8.0,"limit_remaining":null,"limit":25.0,"limit_reset":"monthly"}}),
            ))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}/key", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/key", get(key))
                    .route(
                        "/invalid",
                        get(|| async { Json(json!({"data":{"usage":-1.0}})) }),
                    )
                    .route(
                        "/negative",
                        get(|| async { Json(json!({"data":{"usage":1.0,"limit":-1.0}})) }),
                    )
                    .route(
                        "/exhausted",
                        get(|| async {
                            Json(json!({"data":{"usage":10.0,"limit_remaining":-0.1}}))
                        }),
                    ),
            )
            .await
            .unwrap()
        });
        let http = reqwest::Client::new();
        let spend = fetch(&http, &address, "test-key").await.unwrap();
        assert_eq!(spend.total, 12.3);
        assert_eq!(spend.daily, Some(0.4));
        assert_eq!(spend.weekly, Some(2.0));
        assert_eq!(spend.monthly, Some(8.0));
        assert_eq!(spend.limit_remaining, None);
        assert_eq!(spend.limit, Some(25.0));
        assert_eq!(spend.limit_reset.as_deref(), Some("monthly"));
        assert!(fetch(&http, &address, "wrong-key").await.is_none());
        assert!(
            fetch(&http, &address.replace("/key", "/invalid"), "test-key")
                .await
                .is_none()
        );
        let exhausted = fetch(&http, &address.replace("/key", "/exhausted"), "test-key")
            .await
            .unwrap();
        assert_eq!(exhausted.limit_remaining, Some(-0.1));
        assert_eq!(exhausted.daily, None);
        assert_eq!(exhausted.limit, None);
        assert_eq!(exhausted.limit_reset, None);
        let negative = fetch(&http, &address.replace("/key", "/negative"), "test-key").await;
        assert!(negative.is_none());
        task.abort();
    }
}
