use crate::common::{Server, serve};
use crate::continue_elsewhere::{node_with_accounts, with_usage};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

async fn set_price(server: &Server, price: Value) -> (u16, Value) {
    server.user.json(server.user.put("/api/v1/account-prices").json(&json!({"agent": "claude", "account": "sam@example.com", "price": price}))).await
}

async fn your_price(server: &Server) -> Value {
    let (status, history) = server.user.json(server.user.get("/api/v1/usage/history?range=all")).await;
    assert_eq!(status, 200, "{history}");

    history["accounts"][0]["your_price"].clone()
}

async fn fake_rates() -> (String, Arc<AtomicUsize>) {
    async fn latest(State(fetched): State<Arc<AtomicUsize>>) -> Json<Value> {
        fetched.fetch_add(1, Ordering::SeqCst);

        Json(json!({"amount": 1.0, "base": "GBP", "date": "2026-10-27", "rates": {"USD": 1.25}}))
    }

    let fetched = Arc::new(AtomicUsize::new(0));
    let url = serve(Router::new().route("/v1/latest", get(latest)).with_state(fetched.clone())).await;

    (url, fetched)
}

#[tokio::test]
async fn a_yearly_price_is_listed_and_shown_in_usage_history_as_a_monthly_dollar_amount() {
    let (rates, fetched) = fake_rates().await;
    let server = Server::start_with(|config| config.exchange_rates_url = rates).await;
    node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;

    let (status, _) = set_price(&server, json!({"amount": 1800.0, "currency": "GBP", "period": "yearly"})).await;
    assert_eq!(status, 204);

    let (_, prices) = server.user.json(server.user.get("/api/v1/account-prices")).await;
    assert_eq!(prices, json!([{"agent": "claude", "account": "sam@example.com", "amount": 1800.0, "currency": "GBP", "period": "yearly"}]));
    let price = your_price(&server).await;
    assert_eq!((price["monthly_amount"].clone(), price["monthly_usd"].clone()), (json!(150.0), json!(187.5)));
    your_price(&server).await;
    assert_eq!(fetched.load(Ordering::SeqCst), 1, "the rate is cached for the day");
}

#[tokio::test]
async fn without_an_exchange_rate_the_price_stays_in_its_own_currency() {
    let server = Server::start().await;
    node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    set_price(&server, json!({"amount": 150.0, "currency": "GBP", "period": "monthly"})).await;

    let price = your_price(&server).await;

    assert_eq!((price["currency"].clone(), price["monthly_amount"].clone()), (json!("GBP"), json!(150.0)));
    assert!(price["monthly_usd"].is_null(), "{price}");
}

#[tokio::test]
async fn a_dollar_price_needs_no_exchange_rate() {
    let server = Server::start().await;
    node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    set_price(&server, json!({"amount": 200.0, "currency": "USD", "period": "monthly"})).await;

    assert_eq!(your_price(&server).await["monthly_usd"], json!(200.0));
}

#[tokio::test]
async fn using_the_list_price_clears_the_price() {
    let server = Server::start().await;
    node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    set_price(&server, json!({"amount": 150.0, "currency": "GBP", "period": "monthly"})).await;

    let (status, _) = server.user.json(server.user.put("/api/v1/account-prices").json(&json!({"agent": "claude", "account": "sam@example.com"}))).await;

    assert_eq!(status, 204);
    assert!(your_price(&server).await.is_null());
    let (_, prices) = server.user.json(server.user.get("/api/v1/account-prices")).await;
    assert_eq!(prices, json!([]));
}

#[tokio::test]
async fn a_price_with_no_amount_or_an_unknown_currency_is_refused_in_plain_words() {
    let server = Server::start().await;

    let (status, body) = set_price(&server, json!({"amount": 0.0, "currency": "GBP", "period": "monthly"})).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("price_amount_invalid")));
    assert_eq!(body["message"], "Enter an amount greater than 0, like 1800 or 150.50.");

    let (status, body) = set_price(&server, json!({"amount": 10.0, "currency": "pounds", "period": "monthly"})).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("price_currency_invalid")));
}
