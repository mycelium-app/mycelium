use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const API_KEY: &str = "actual-key-1234";
const SYNC_ID: &str = "sync-1";

#[derive(Default)]
struct FakeActual {
    added: Mutex<Vec<Value>>,
    ranges: Mutex<Vec<HashMap<String, String>>>,
}

fn authorised(headers: &HeaderMap, budget: &str) -> bool {
    headers.get("x-api-key").and_then(|value| value.to_str().ok()) == Some(API_KEY) && budget == SYNC_ID
}

async fn accounts(headers: HeaderMap, Path(budget): Path<String>) -> Response {
    match authorised(&headers, &budget) {
        true => Json(json!({"data": [{"id": "a1", "name": "Current", "balance": 123450, "closed": false, "offbudget": false}, {"id": "a2", "name": "Old", "balance": 0, "closed": true}]})).into_response(),
        false => (StatusCode::UNAUTHORIZED, Json(json!({"error": "Unauthorized"}))).into_response(),
    }
}

async fn categories() -> Json<Value> {
    Json(json!({"data": [{"id": "c1", "name": "Groceries", "group_id": "g1", "is_income": false, "hidden": false}]}))
}

async fn transactions(State(fake): State<Arc<FakeActual>>, Path((_, account)): Path<(String, String)>, Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    fake.ranges.lock().unwrap().push(query);

    Json(json!({"data": [{"id": format!("t-{account}"), "account": account, "date": "2026-10-02", "amount": -1250, "payee_name": "Corner shop", "category": "c1", "notes": null}]}))
}

async fn add_transaction(State(fake): State<Arc<FakeActual>>, Path((_, account)): Path<(String, String)>, Json(body): Json<Value>) -> Json<Value> {
    fake.added.lock().unwrap().push(json!({"account": account, "body": body}));

    Json(json!({"message": "ok"}))
}

async fn month() -> Json<Value> {
    Json(json!({"data": [{"id": "c1", "name": "Groceries", "budgeted": 30000, "spent": -12500, "balance": 17500}]}))
}

async fn fake_actual() -> (String, Arc<FakeActual>) {
    let fake = Arc::new(FakeActual::default());
    let router = Router::new()
        .route("/v1/budgets/{budget}/accounts", get(accounts))
        .route("/v1/budgets/{budget}/categories", get(categories))
        .route("/v1/budgets/{budget}/accounts/{account}/transactions", get(transactions).post(add_transaction))
        .route("/v1/budgets/{budget}/months/{month}/categories", get(month))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_actual(server: &Server, base: &str, key: &str, settings: Value) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Actual Budget", "kind": "actual", "url": base, "bearer_token": key, "settings": settings}))).await
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("actual_budget__{tool}"), "arguments": arguments}});
    let reply = if tool == "add_transaction" {
        server.approved_agent_mcp(token, body).await
    } else {
        let anonymous = server.anonymous();
        let (status, reply) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
        assert_eq!(status, 200, "{reply}");

        reply
    };

    reply["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn adding_actual_needs_a_sync_id_and_checks_the_key_against_the_budget() {
    let server = Server::start().await;
    let (base, _) = fake_actual().await;

    let (_, no_sync_id) = add_actual(&server, &base, API_KEY, json!({})).await;
    let (_, wrong_key) = add_actual(&server, &base, "wrong", json!({"budget_sync_id": SYNC_ID})).await;
    let (status, added) = add_actual(&server, &base, API_KEY, json!({"budget_sync_id": SYNC_ID})).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(no_sync_id["error"], "settings_invalid");
    assert!(no_sync_id["message"].as_str().unwrap().contains("sync id"));
    assert_eq!(wrong_key["error"], "tool_server_failed");
    assert_eq!((status, added["settings"]["budget_sync_id"].clone()), (201, json!(SYNC_ID)));
    assert!(!stored.contains(API_KEY));
}

#[tokio::test]
async fn the_agent_reads_accounts_categories_transactions_in_a_range_and_a_months_budget_in_currency() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_actual().await;
    add_actual(&server, &base, API_KEY, json!({"budget_sync_id": SYNC_ID})).await;

    let accounts = text(&call(&server, &node.token, "list_accounts", json!({})).await);
    let categories = text(&call(&server, &node.token, "list_categories", json!({})).await);
    let transactions: Value = serde_json::from_str(&text(&call(&server, &node.token, "get_transactions", json!({"since": "2026-10-01", "until": "2026-10-31"})).await)).unwrap();
    let month = text(&call(&server, &node.token, "get_budget_month", json!({"month": "2026-10"})).await);
    let range = fake.ranges.lock().unwrap().clone();

    assert!(accounts.contains("1234.5"), "{accounts}");
    assert!(categories.contains("Groceries"), "{categories}");
    assert_eq!(transactions.as_array().unwrap().len(), 1, "only the open account is read");
    assert_eq!((transactions[0]["amount"].clone(), transactions[0]["account"].clone()), (json!(-12.5), json!("a1")));
    assert_eq!((range[0]["since_date"].as_str(), range[0]["until_date"].as_str()), ("2026-10-01", "2026-10-31"));
    assert!(month.contains("300") && month.contains("175"), "{month}");
}

#[tokio::test]
async fn adding_a_transaction_asks_first_and_sends_the_amount_in_minor_units() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_actual().await;
    add_actual(&server, &base, API_KEY, json!({"budget_sync_id": SYNC_ID})).await;

    let added = call(&server, &node.token, "add_transaction", json!({"account": "a1", "date": "2026-10-27", "amount": -12.5, "payee": "Corner shop", "category": "c1"})).await;
    let sent = fake.added.lock().unwrap().clone();

    assert_ne!(added["isError"], true, "{added}");
    assert_eq!(sent[0]["account"], "a1");
    assert_eq!(sent[0]["body"]["transaction"], json!({"date": "2026-10-27", "amount": -1250, "payee_name": "Corner shop", "category": "c1"}));
}
