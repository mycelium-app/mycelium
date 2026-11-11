use crate::common::{Server, serve};
use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use chrono::{Datelike, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const API_KEY: &str = "sk-ant-api03-0123456789abcdefghij";
const ADMIN_KEY: &str = "sk-ant-admin01-0123456789wxyz";

#[derive(Default)]
struct Fake {
    revoked: AtomicBool,
}

fn allowed(fake: &Fake, headers: &HeaderMap) -> bool {
    !fake.revoked.load(Ordering::SeqCst) && headers.get("x-api-key").and_then(|value| value.to_str().ok()) == Some(ADMIN_KEY) && headers.get("anthropic-version").is_some()
}

fn fake_anthropic() -> (Router, Arc<Fake>) {
    async fn models() -> Json<Value> {
        Json(json!({"data": [{"id": "claude-sonnet-5-5"}], "has_more": false}))
    }

    async fn cost_report(State(fake): State<Arc<Fake>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Result<Json<Value>, StatusCode> {
        if !allowed(&fake, &headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let first = Utc::now().format("%Y-%m-01").to_string();

        Ok(Json(match query.get("page").map(String::as_str) {
            None => json!({"data": [{"starting_at": format!("{first}T00:00:00Z"), "results": [{"amount": "1000.50", "currency": "USD"}, {"amount": "0.25", "currency": "USD"}]}], "has_more": true, "next_page": "page-2"}),
            Some(_) => json!({"data": [{"starting_at": format!("{today}T00:00:00Z"), "results": [{"amount": "640.25", "currency": "USD"}]}], "has_more": false, "next_page": null}),
        }))
    }

    async fn messages_report(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        if !allowed(&fake, &headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let today = Utc::now().format("%Y-%m-%d").to_string();

        Ok(Json(json!({"data": [{"starting_at": format!("{today}T00:00:00Z"), "results": [
            {"uncached_input_tokens": 1000, "cache_read_input_tokens": 200, "cache_creation": {"ephemeral_1h_input_tokens": 30, "ephemeral_5m_input_tokens": 20}, "output_tokens": 50},
            {"uncached_input_tokens": 100, "cache_read_input_tokens": 0, "cache_creation": {"ephemeral_1h_input_tokens": 0, "ephemeral_5m_input_tokens": 0}, "output_tokens": 0}
        ]}], "has_more": false, "next_page": null})))
    }

    let fake = Arc::new(Fake::default());
    let router = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/organizations/cost_report", get(cost_report))
        .route("/v1/organizations/usage_report/messages", get(messages_report))
        .with_state(fake.clone());

    (router, fake)
}

async fn add_provider(server: &Server, base: &str, admin_key: Option<&str>) -> (u16, Value) {
    let (status, body) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "anthropic", "name": "Anthropic", "base_url": base, "api_key": API_KEY, "admin_key": admin_key}))).await;

    (status, body)
}

async fn provider_usage(server: &Server) -> Value {
    let (status, usage) = server.user.json(server.user.get("/api/v1/model-provider-usage")).await;
    assert_eq!(status, 200, "{usage}");

    usage
}

#[tokio::test]
async fn an_admin_key_is_saved_and_only_its_last_four_characters_are_returned() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let (status, provider) = add_provider(&server, &base, Some(ADMIN_KEY)).await;

    assert_eq!(status, 201, "{provider}");
    assert_eq!(provider["has_admin_key"], true);
    assert_eq!(provider["admin_key_hint"], "wxyz");

    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert!(!listed.to_string().contains(ADMIN_KEY));
    assert!(!provider.to_string().contains(ADMIN_KEY));
}

#[tokio::test]
async fn a_key_without_the_admin_prefix_is_refused_and_nothing_is_stored() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let (status, refused) = add_provider(&server, &base, Some(API_KEY)).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!(status, 400);
    assert_eq!(refused["message"], "Enter an Admin API key that starts with sk-ant-admin");
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn an_admin_key_anthropic_refuses_is_not_saved() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let (_, provider) = add_provider(&server, &base, None).await;
    let id = provider["id"].as_str().unwrap();
    let (status, refused) = server.user.json(server.user.patch(&format!("/api/v1/model-providers/{id}")).json(&json!({"admin_key": "sk-ant-admin01-not-the-right-one"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!(status, 400);
    assert_eq!(refused["message"], "Anthropic refused this Admin API key. Check it is active in the Console, or create a new one");
    assert_eq!(listed[0]["has_admin_key"], false);
}

#[tokio::test]
async fn an_admin_key_is_removed_by_an_empty_string_or_null_and_kept_when_left_out() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let (_, provider) = add_provider(&server, &base, Some(ADMIN_KEY)).await;
    let path = format!("/api/v1/model-providers/{}", provider["id"].as_str().unwrap());
    let (_, renamed) = server.user.json(server.user.patch(&path).json(&json!({"name": "Work"}))).await;
    let (_, emptied) = server.user.json(server.user.patch(&path).json(&json!({"admin_key": ""}))).await;
    let (_, restored) = server.user.json(server.user.patch(&path).json(&json!({"admin_key": ADMIN_KEY}))).await;
    let (_, nulled) = server.user.json(server.user.patch(&path).json(&json!({"admin_key": null}))).await;

    assert_eq!(renamed["has_admin_key"], true);
    assert_eq!(emptied["has_admin_key"], false);
    assert_eq!(restored["has_admin_key"], true);
    assert_eq!(nulled["has_admin_key"], false);
    assert_eq!(nulled["admin_key_hint"], Value::Null);
}

#[tokio::test]
async fn month_and_today_cost_and_today_tokens_are_summed_across_pages() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;

    add_provider(&server, &base, Some(ADMIN_KEY)).await;

    let usage = provider_usage(&server).await;
    let cost = &usage[0]["api_cost"];
    let today = if Utc::now().day() == 1 { 1641.0 } else { 640.25 };

    assert_eq!(usage[0]["status"], "ok");
    assert_eq!(cost["month_usd"], 1641.0 / 100.0);
    assert_eq!(cost["today_usd"], today / 100.0);
    assert_eq!(cost["today_tokens"], 1400);
    assert!(usage[0]["read_at"].is_string());
    assert!(!usage.to_string().contains(ADMIN_KEY));
}

#[tokio::test]
async fn a_provider_without_an_admin_key_has_no_usage_row() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;

    add_provider(&server, &base, None).await;

    assert_eq!(provider_usage(&server).await, json!([]));
}

#[tokio::test]
async fn a_key_refused_at_read_time_shows_refused_and_keeps_the_last_good_values() {
    let server = Server::start().await;
    let (router, fake) = fake_anthropic();
    let base = serve(router).await;

    add_provider(&server, &base, Some(ADMIN_KEY)).await;

    let good = provider_usage(&server).await;

    fake.revoked.store(true, Ordering::SeqCst);
    sqlx::query("update model_provider_usage set attempted_at = now() - interval '1 hour'").execute(&server.pool).await.unwrap();

    let refused = provider_usage(&server).await;

    assert_eq!(refused[0]["status"], "refused");
    assert_eq!(refused[0]["api_cost"], good[0]["api_cost"]);
    assert_eq!(refused[0]["read_at"], good[0]["read_at"]);
}
