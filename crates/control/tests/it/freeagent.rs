use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const CLIENT_ID: &str = "client-id-1";
const CLIENT_SECRET: &str = "client-secret-5678";

struct FakeFreeAgent {
    valid_access: Mutex<String>,
    grants: Mutex<Vec<String>>,
    created: Mutex<Vec<(String, Value)>>,
}

fn bearer(headers: &HeaderMap) -> String {
    headers.get("authorization").and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).unwrap_or_default().to_string()
}

fn signed_request(fake: &FakeFreeAgent, headers: &HeaderMap) -> bool {
    bearer(headers) == *fake.valid_access.lock().unwrap()
}

fn unauthorised() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"errors": [{"message": "Access token expired"}]}))).into_response()
}

async fn token_endpoint(State(fake): State<Arc<FakeFreeAgent>>, headers: HeaderMap, body: String) -> Response {
    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}")));

    if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_client"}))).into_response();
    }

    let form: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
    let field = |name: &str| form.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default();
    fake.grants.lock().unwrap().push(field("grant_type"));

    match (field("grant_type").as_str(), field("code").as_str(), field("refresh_token").as_str()) {
        ("authorization_code", "code-1", _) => {
            *fake.valid_access.lock().unwrap() = "access-1".to_string();

            Json(json!({"access_token": "access-1", "refresh_token": "refresh-1", "expires_in": 3600, "token_type": "bearer"})).into_response()
        }
        ("refresh_token", _, "refresh-1") => {
            *fake.valid_access.lock().unwrap() = "access-2".to_string();

            Json(json!({"access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600, "token_type": "bearer"})).into_response()
        }
        _ => (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"}))).into_response(),
    }
}

async fn contacts(State(fake): State<Arc<FakeFreeAgent>>, headers: HeaderMap) -> Response {
    match signed_request(&fake, &headers) {
        true => Json(json!({"contacts": [{"url": "https://api.freeagent.com/v2/contacts/2", "organisation_name": "Acme Ltd"}]})).into_response(),
        false => unauthorised(),
    }
}

async fn invoice(State(fake): State<Arc<FakeFreeAgent>>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    match signed_request(&fake, &headers) {
        true => Json(json!({"invoice": {"url": format!("https://api.freeagent.com/v2/invoices/{id}"), "status": "Open", "total_value": "120.0"}})).into_response(),
        false => unauthorised(),
    }
}

async fn create_invoice(State(fake): State<Arc<FakeFreeAgent>>, Json(body): Json<Value>) -> Response {
    fake.created.lock().unwrap().push(("invoice".to_string(), body));

    (StatusCode::CREATED, [("location", "https://api.freeagent.com/v2/invoices/9")], Json(json!({"invoice": {"status": "Draft"}}))).into_response()
}

async fn me() -> Json<Value> {
    Json(json!({"user": {"url": "https://api.freeagent.com/v2/users/1"}}))
}

async fn create_expense(State(fake): State<Arc<FakeFreeAgent>>, Json(body): Json<Value>) -> Response {
    fake.created.lock().unwrap().push(("expense".to_string(), body));

    (StatusCode::CREATED, [("location", "https://api.freeagent.com/v2/expenses/4")], Json(json!({"expense": {}}))).into_response()
}

async fn fake_freeagent() -> (String, Arc<FakeFreeAgent>) {
    let fake = Arc::new(FakeFreeAgent { valid_access: Mutex::new(String::new()), grants: Mutex::default(), created: Mutex::default() });
    let router = Router::new()
        .route("/v2/token_endpoint", post(token_endpoint))
        .route("/v2/contacts", get(contacts))
        .route("/v2/invoices/{id}", get(invoice))
        .route("/v2/invoices", post(create_invoice))
        .route("/v2/users/me", get(me))
        .route("/v2/expenses", post(create_expense))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_freeagent(server: &Server, base: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "FreeAgent", "kind": "freeagent", "url": base, "account": CLIENT_ID, "bearer_token": CLIENT_SECRET}))).await
}

async fn sign_in(server: &Server, id: &str, code: &str) -> u16 {
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/chat-tool-servers/{id}/freeagent-sign-in"))).await;
    assert_eq!(status, 200, "{started}");
    let authorise = url::Url::parse(started["authorize_url"].as_str().unwrap()).unwrap();
    let query: Vec<(String, String)> = authorise.query_pairs().into_owned().collect();
    let field = |name: &str| query.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap();

    assert_eq!(authorise.path(), "/v2/approve_app");
    assert_eq!((field("client_id"), field("response_type")), (CLIENT_ID.to_string(), "code".to_string()));

    let response = server.anonymous().http.get(format!("{}?code={code}&state={}", field("redirect_uri"), field("state"))).send().await.unwrap();

    response.status().as_u16()
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("freeagent__{tool}"), "arguments": arguments}});
    let reply = if tool.starts_with("create_") {
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
async fn nothing_can_be_read_until_the_person_has_signed_in() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_freeagent().await;
    let (status, added) = add_freeagent(&server, &base).await;

    let result = call(&server, &node.token, "list_contacts", json!({})).await;

    assert_eq!(status, 201, "{added}");
    assert_eq!((result["isError"].clone(), text(&result)), (json!(true), "Sign in to FreeAgent in Settings, Integrations.".to_string()));
}

#[tokio::test]
async fn signing_in_stores_tokens_that_are_never_returned_and_reads_reach_freeagent() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_freeagent().await;
    let (_, added) = add_freeagent(&server, &base).await;
    let id = added["id"].as_str().unwrap();

    let rejected = sign_in(&server, id, "wrong-code").await;
    let signed_in = sign_in(&server, id, "code-1").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let contacts = text(&call(&server, &node.token, "list_contacts", json!({})).await);
    let invoice = text(&call(&server, &node.token, "get_invoice", json!({"id": "https://api.freeagent.com/v2/invoices/77"})).await);

    assert_eq!((rejected, signed_in), (502, 200));
    assert_eq!(listed[0]["signed_in"], true);
    assert!(!listed.to_string().contains("access-1") && !listed.to_string().contains("refresh-1") && !listed.to_string().contains(CLIENT_SECRET));
    assert!(contacts.contains("Acme Ltd"), "{contacts}");
    assert!(invoice.contains("invoices/77") && invoice.contains("Open"), "{invoice}");
}

#[tokio::test]
async fn an_access_token_freeagent_rejects_is_renewed_with_the_refresh_token_and_the_call_goes_through() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_freeagent().await;
    let (_, added) = add_freeagent(&server, &base).await;
    sign_in(&server, added["id"].as_str().unwrap(), "code-1").await;
    *fake.valid_access.lock().unwrap() = "revoked-elsewhere".to_string();

    let contacts = call(&server, &node.token, "list_contacts", json!({})).await;

    assert_ne!(contacts["isError"], true, "{contacts}");
    assert_eq!(fake.grants.lock().unwrap().clone(), ["authorization_code", "refresh_token"]);
}

#[tokio::test]
async fn a_draft_invoice_and_an_expense_ask_first_and_are_created_with_freeagent_urls() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_freeagent().await;
    let (_, added) = add_freeagent(&server, &base).await;
    sign_in(&server, added["id"].as_str().unwrap(), "code-1").await;

    let invoice = call(&server, &node.token, "create_draft_invoice", json!({"contact": "2", "dated_on": "2026-10-27", "items": [{"description": "Consulting", "price": 100, "quantity": 2, "item_type": "Hours"}]})).await;
    let expense = call(&server, &node.token, "create_expense", json!({"category": "285", "dated_on": "2026-10-27", "gross_value": 12.5, "description": "Train"})).await;
    let created = fake.created.lock().unwrap().clone();

    assert!(text(&invoice).contains("Draft") && text(&invoice).contains("invoices/9"), "{invoice}");
    assert!(text(&expense).contains("expenses/4"), "{expense}");
    assert_eq!(created[0].1["invoice"]["contact"], format!("{base}/v2/contacts/2"));
    assert_eq!(created[0].1["invoice"]["payment_terms_in_days"], 30);
    assert_eq!(created[0].1["invoice"]["invoice_items"][0]["quantity"], "2");
    assert_eq!((created[1].1["expense"]["user"].clone(), created[1].1["expense"]["category"].clone()), (json!("https://api.freeagent.com/v2/users/1"), json!(format!("{base}/v2/categories/285"))));
}
