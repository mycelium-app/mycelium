use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct App {
    base: Mutex<String>,
    registrations: Mutex<Vec<Value>>,
    token_requests: Mutex<Vec<Vec<(String, String)>>>,
    mcp_authorisations: Mutex<Vec<String>>,
    without_registration: bool,
    token_lifetime: i64,
}

type Shared = State<Arc<App>>;

async fn mcp(State(app): Shared, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let authorisation = headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    app.mcp_authorisations.lock().unwrap().push(authorisation.clone());

    if !authorisation.starts_with("Bearer access-") {
        let challenge = format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"", app.base.lock().unwrap());

        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge)]).into_response();
    }

    if body.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match body["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "docs", "version": "1"}}),
        "tools/list" => json!({"tools": [{"name": "search", "description": "Search documents", "inputSchema": {"type": "object"}}]}),
        "tools/call" => json!({"content": [{"type": "text", "text": "Found 3 documents"}]}),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
}

async fn protected_resource(State(app): Shared) -> Json<Value> {
    Json(json!({"resource": format!("{}/mcp", app.base.lock().unwrap()), "authorization_servers": [app.base.lock().unwrap().clone()]}))
}

async fn authorisation_server(State(app): Shared) -> Json<Value> {
    let base = app.base.lock().unwrap().clone();
    let mut metadata = json!({"authorization_endpoint": format!("{base}/authorize"), "token_endpoint": format!("{base}/token"), "scopes_supported": ["read", "write"]});

    if !app.without_registration {
        metadata["registration_endpoint"] = json!(format!("{base}/register"));
    }

    Json(metadata)
}

async fn register(State(app): Shared, Json(body): Json<Value>) -> Json<Value> {
    app.registrations.lock().unwrap().push(body);

    Json(json!({"client_id": "client-1"}))
}

async fn token(State(app): Shared, body: String) -> Json<Value> {
    let form: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
    let refreshing = form.iter().any(|(name, value)| name == "grant_type" && value == "refresh_token");
    app.token_requests.lock().unwrap().push(form);

    if refreshing {
        return Json(json!({"access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600}));
    }

    Json(json!({"access_token": "access-1", "refresh_token": "refresh-1", "expires_in": app.token_lifetime}))
}

async fn start_app(without_registration: bool, token_lifetime: i64) -> (String, Arc<App>) {
    let app = Arc::new(App { without_registration, token_lifetime, ..App::default() });
    let router = Router::new()
        .route("/mcp", post(mcp))
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(authorisation_server))
        .route("/register", post(register))
        .route("/token", post(token))
        .with_state(app.clone());
    let base = serve(router).await;
    *app.base.lock().unwrap() = base.clone();

    (base, app)
}

async fn begin(server: &Server, body: Value) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers/oauth").json(&body)).await
}

fn query_of(authorize_url: &str) -> Vec<(String, String)> {
    url::Url::parse(authorize_url).unwrap().query_pairs().into_owned().collect()
}

fn param(query: &[(String, String)], name: &str) -> String {
    query.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default()
}

async fn sign_in(server: &Server, base: &str) -> Vec<(String, String)> {
    let (status, started) = begin(server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    assert_eq!(status, 200, "{started}");

    query_of(started["authorize_url"].as_str().unwrap())
}

async fn callback(server: &Server, state: &str) -> (u16, String) {
    let anonymous = server.anonymous();
    let response = anonymous.http.get(format!("{}/integrations/oauth/callback?code=code-1&state={state}", server.base)).send().await.unwrap();

    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn signing_in_with_oauth_registers_mycelium_and_sends_the_person_to_the_apps_authorisation_page() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;

    let query = sign_in(&server, &base).await;

    assert_eq!(param(&query, "client_id"), "client-1");
    assert_eq!(param(&query, "code_challenge_method"), "S256");
    assert_eq!(param(&query, "redirect_uri"), format!("{}/integrations/oauth/callback", server.base));
    assert_eq!(param(&query, "resource"), format!("{base}/mcp"));
    assert_eq!(param(&query, "scope"), "read write");
    assert_eq!(app.registrations.lock().unwrap()[0]["redirect_uris"], json!([format!("{}/integrations/oauth/callback", server.base)]));
    assert_eq!(app.registrations.lock().unwrap()[0]["token_endpoint_auth_method"], "none");
}

#[tokio::test]
async fn the_oauth_callback_connects_the_integration_and_its_tools_are_listed_with_the_new_token() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;

    let (status, page) = callback(&server, &param(&query, "state")).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Docs is connected. You can close this tab and return to Mycelium."), "{page}");
    assert_eq!((listed[0]["kind"].as_str(), listed[0]["oauth"].as_bool(), listed[0]["settings"].clone()), (Some("mcp"), Some(true), json!({"app": "outline"})));
    assert_eq!(listed[0]["tools"][0]["name"], "search");
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-1"));

    let exchange = app.token_requests.lock().unwrap()[0].clone();
    let verifier = param(&exchange, "code_verifier");
    assert_eq!((param(&exchange, "grant_type"), param(&exchange, "code"), param(&exchange, "client_id")), ("authorization_code".to_string(), "code-1".to_string(), "client-1".to_string()));
    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())), param(&query, "code_challenge"));
    assert!(!server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap().contains("access-1"));
}

#[tokio::test]
async fn an_oauth_callback_with_an_unknown_state_connects_nothing() {
    let server = Server::start().await;
    let (_, app) = start_app(false, 3600).await;

    let (status, page) = callback(&server, "unknown").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(status, 400);
    assert!(page.contains("This sign-in has expired. Start it again from Settings, Integrations."), "{page}");
    assert_eq!(listed, json!([]));
    assert!(app.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_expired_oauth_token_is_refreshed_before_a_tool_call() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 1).await;
    let query = sign_in(&server, &base).await;
    callback(&server, &param(&query, "state")).await;
    let anonymous = server.anonymous();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "docs__search", "arguments": {}}});

    let (status, called) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {node_token}")).json(&call)).await;

    assert_eq!(status, 200, "{called}");
    assert_eq!(called["result"]["content"][0]["text"], "Found 3 documents");
    let refresh = app.token_requests.lock().unwrap()[1].clone();
    assert_eq!((param(&refresh, "grant_type"), param(&refresh, "refresh_token")), ("refresh_token".to_string(), "refresh-1".to_string()));
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-2"));
}

#[tokio::test]
async fn an_app_without_client_registration_is_told_to_use_an_api_key() {
    let server = Server::start().await;
    let (base, _) = start_app(true, 3600).await;

    let (status, refused) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;

    assert_eq!((status, refused["error"].as_str(), refused["message"].as_str()), (400, Some("oauth_unsupported"), Some("This app does not let Mycelium sign in. Use an API key instead.")));
}

#[tokio::test]
async fn probing_an_app_that_refuses_client_registration_does_not_offer_oauth() {
    let server = Server::start().await;
    let (base, _) = start_app(true, 3600).await;

    let (status, probed) = server.user.json(server.user.post("/api/v1/chat-tool-servers/probe").json(&json!({"url": format!("{base}/mcp")}))).await;

    assert_eq!((status, probed), (200, json!({"oauth": false})));
}

async fn listed_tools(server: &Server, node_token: &str) -> Vec<Value> {
    let anonymous = server.anonymous();
    let (_, listed) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {node_token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;

    listed["result"]["tools"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_failed_oauth_refresh_tells_the_person_to_sign_in_again_in_the_tool_list() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, _) = start_app(false, 1).await;
    let query = sign_in(&server, &base).await;
    callback(&server, &param(&query, "state")).await;
    sqlx::query("update chat_tool_servers set oauth = jsonb_set(oauth, '{token_endpoint}', '\"http://127.0.0.1:1/token\"')").execute(&server.pool).await.unwrap();

    let tools = listed_tools(&server, &node_token).await;
    let notice = tools.iter().find(|tool| tool["name"] == "docs__sign_in_again").expect("a sign in again notice");

    assert_eq!(notice["description"], "Docs: Sign in to Docs again in Settings, Integrations.");
}

#[tokio::test]
async fn a_rejected_oauth_token_with_no_known_expiry_is_refreshed_and_the_tools_listed() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;
    callback(&server, &param(&query, "state")).await;
    sqlx::query("update chat_tool_servers set bearer_token = 'stale', oauth_expires_at = null").execute(&server.pool).await.unwrap();

    let tools = listed_tools(&server, &node_token).await;

    assert!(tools.iter().any(|tool| tool["name"] == "docs__search"), "{tools:?}");
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-2"));
}
