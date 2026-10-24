use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::model_providers;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json as DbJson;
use std::time::Duration;
use uuid::Uuid;

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
    spend: Option<serde_json::Value>,
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
        let address = provider
            .base_url
            .as_deref()
            .and_then(|value| url::Url::parse(value).ok());

        if !address.is_some_and(|address| {
            address.scheme() == "https"
                && address.host_str() == Some("openrouter.ai")
                && address.port_or_known_default() == Some(443)
                && address.path().trim_end_matches('/') == "/api/v1"
        }) {
            continue;
        }

        let mut cached = sqlx::query_as::<_, (DateTime<Utc>, DbJson<serde_json::Value>)>(
            "select read_at, spend from model_provider_usage where provider_id = $1",
        )
        .bind(provider.id)
        .fetch_optional(&state.pool)
        .await?;

        if cached.as_ref().is_none_or(|(read_at, _)| {
            Utc::now().signed_duration_since(*read_at).num_seconds() >= 60
        }) {
            if let Some(key) = provider.api_key.as_deref() {
                if let Some(spend) =
                    fetch(&state.http, "https://openrouter.ai/api/v1/key", key).await
                {
                    let read_at = Utc::now();
                    let spend = serde_json::to_value(spend).expect("finite spend");
                    sqlx::query("insert into model_provider_usage (provider_id, read_at, spend) values ($1, $2, $3) on conflict (provider_id) do update set read_at = excluded.read_at, spend = excluded.spend where model_provider_usage.read_at < excluded.read_at")
                        .bind(provider.id).bind(read_at).bind(&spend).execute(&state.pool).await?;
                    cached = Some((read_at, DbJson(spend)));
                }
            }
        }

        usage.push(ProviderUsage {
            provider_id: provider.id,
            name: provider.name,
            read_at: cached.as_ref().map(|(at, _)| *at),
            spend: cached.map(|(_, spend)| spend.0),
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
                json!({"data":{"usage":12.3,"usage_daily":0.4,"usage_weekly":2.0,"usage_monthly":8.0,"limit_remaining":null}}),
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
        task.abort();
    }
}
