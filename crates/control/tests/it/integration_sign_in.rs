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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct Options {
    without_registration: bool,
    token_lifetime: i64,
    challenge: Option<&'static str>,
    protected_path: &'static str,
    authorisation_paths: Vec<&'static str>,
    issuer_path: &'static str,
    authorisation_extra: Value,
    protected_extra: Value,
    registered_secret: Option<&'static str>,
    token_failure: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            without_registration: false,
            token_lifetime: 3600,
            challenge: Some("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\""),
            protected_path: "/.well-known/oauth-protected-resource/mcp",
            authorisation_paths: vec!["/.well-known/oauth-authorization-server"],
            issuer_path: "",
            authorisation_extra: Value::Null,
            protected_extra: Value::Null,
            registered_secret: None,
            token_failure: false,
        }
    }
}

struct App {
    base: Mutex<String>,
    registrations: Mutex<Vec<Value>>,
    token_requests: Mutex<Vec<Vec<(String, String)>>>,
    mcp_authorisations: Mutex<Vec<String>>,
    token_authorisations: Mutex<Vec<String>>,
    api_authorisations: Mutex<Vec<String>>,
    rejecting: AtomicBool,
    scope_missing: AtomicBool,
    options: Options,
}

fn merged(mut document: Value, extra: &Value) -> Value {
    for (key, value) in extra.as_object().into_iter().flatten() {
        match value {
            Value::Null => document.as_object_mut().unwrap().remove(key),
            other => document.as_object_mut().unwrap().insert(key.clone(), other.clone()),
        };
    }

    document
}

type Shared = State<Arc<App>>;

async fn mcp(State(app): Shared, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let authorisation = headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    app.mcp_authorisations.lock().unwrap().push(authorisation.clone());

    if app.scope_missing.load(Ordering::SeqCst) {
        let challenge = "Bearer error=\"insufficient_scope\", scope=\"files:write\"";

        return (StatusCode::FORBIDDEN, [(header::WWW_AUTHENTICATE, challenge)]).into_response();
    }

    if !authorisation.starts_with("Bearer access-") || app.rejecting.load(Ordering::SeqCst) {
        return match app.options.challenge {
            Some(challenge) => (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge.replace("{base}", &app.base.lock().unwrap()))]).into_response(),
            None => StatusCode::UNAUTHORIZED.into_response(),
        };
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

async fn api(State(app): Shared, headers: HeaderMap) -> Response {
    let authorisation = headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    app.api_authorisations.lock().unwrap().push(authorisation.clone());

    if authorisation != "Bearer access-2" || app.rejecting.load(Ordering::SeqCst) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    Json(json!({"data": []})).into_response()
}

async fn protected_resource(State(app): Shared) -> Json<Value> {
    let base = app.base.lock().unwrap().clone();
    let document = json!({"resource": format!("{base}/mcp"), "authorization_servers": [format!("{base}{}", app.options.issuer_path)], "scopes_supported": ["read", "write"]});

    Json(merged(document, &app.options.protected_extra))
}

async fn authorisation_server(State(app): Shared) -> Json<Value> {
    let base = app.base.lock().unwrap().clone();
    let mut metadata = json!({"issuer": format!("{base}{}", app.options.issuer_path), "authorization_endpoint": format!("{base}/authorize"), "token_endpoint": format!("{base}/token"), "code_challenge_methods_supported": ["S256"]});

    if !app.options.without_registration {
        metadata["registration_endpoint"] = json!(format!("{base}/register"));
    }

    Json(merged(metadata, &app.options.authorisation_extra))
}

async fn register(State(app): Shared, Json(body): Json<Value>) -> Json<Value> {
    app.registrations.lock().unwrap().push(body);

    match app.options.registered_secret {
        Some(secret) => Json(json!({"client_id": "client-1", "client_secret": secret})),
        None => Json(json!({"client_id": "client-1"})),
    }
}

async fn token(State(app): Shared, headers: HeaderMap, body: String) -> Response {
    app.token_authorisations.lock().unwrap().push(headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string());
    let form: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
    let refreshing = form.iter().any(|(name, value)| name == "grant_type" && value == "refresh_token");
    app.token_requests.lock().unwrap().push(form);

    if app.options.token_failure {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": "the code has expired"}))).into_response();
    }

    if refreshing {
        return Json(json!({"access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600})).into_response();
    }

    let exchanges = app.token_requests.lock().unwrap().iter().filter(|form| form.iter().any(|(name, value)| name == "grant_type" && value == "authorization_code")).count();

    if exchanges > 1 {
        return Json(json!({"access_token": "access-3", "refresh_token": "refresh-3", "expires_in": 3600})).into_response();
    }

    Json(json!({"access_token": "access-1", "refresh_token": "refresh-1", "expires_in": app.options.token_lifetime})).into_response()
}

async fn start_with(options: Options) -> (String, Arc<App>) {
    let protected_path = options.protected_path;
    let authorisation_paths = options.authorisation_paths.clone();
    let app = Arc::new(App { base: Mutex::default(), registrations: Mutex::default(), token_requests: Mutex::default(), mcp_authorisations: Mutex::default(), token_authorisations: Mutex::default(), api_authorisations: Mutex::default(), rejecting: AtomicBool::new(false), scope_missing: AtomicBool::new(false), options });
    let mut router = Router::new().route("/mcp", post(mcp)).route(protected_path, get(protected_resource)).route("/api/{action}", post(api)).route("/register", post(register)).route("/token", post(token));

    for path in authorisation_paths {
        router = router.route(path, get(authorisation_server));
    }

    let base = serve(router.with_state(app.clone())).await;
    *app.base.lock().unwrap() = base.clone();

    (base, app)
}

async fn start_app(without_registration: bool, token_lifetime: i64) -> (String, Arc<App>) {
    start_with(Options { without_registration, token_lifetime, ..Options::default() }).await
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

async fn callback_with(server: &Server, query: &str) -> (u16, String) {
    let anonymous = server.anonymous();
    let response = anonymous.http.get(format!("{}/integrations/oauth/callback?{query}", server.base)).send().await.unwrap();

    (response.status().as_u16(), response.text().await.unwrap())
}

async fn callback(server: &Server, state: &str) -> (u16, String) {
    callback_with(server, &format!("code=code-1&state={state}")).await
}

async fn add_integration(server: &Server, base: &str, sign_in: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Docs", "kind": "outline", "url": base, "sign_in": sign_in}))).await
}

async fn finish_sign_in(server: &Server, base: &str, query: &[(String, String)]) {
    let state = param(query, "state");
    callback(server, &state).await;
    let (status, added) = add_integration(server, base, &state).await;
    assert_eq!(status, 201, "{added}");
}

async fn sign_in_status(server: &Server, sign_in: &str) -> (u16, Value) {
    server.user.json(server.user.get(&format!("/integrations/oauth/{sign_in}"))).await
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
async fn the_oauth_callback_holds_the_sign_in_until_the_integration_is_added_and_then_its_tools_are_listed_with_the_new_token() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;

    let (status, page) = callback(&server, &param(&query, "state")).await;
    let (_, before_adding) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let (added_status, added) = add_integration(&server, &base, &param(&query, "state")).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(before_adding, json!([]));
    assert_eq!((status, added_status), (200, 201), "{page} {added}");
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
    finish_sign_in(&server, &base, &query).await;
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
async fn an_app_without_client_registration_asks_for_a_client_id() {
    let server = Server::start().await;
    let (base, _) = start_app(true, 3600).await;

    let (status, refused) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;

    assert_eq!((status, refused["error"].as_str(), refused["message"].as_str()), (422, Some("client_id_required"), Some("Enter the client ID from 127.0.0.1")));
}

#[tokio::test]
async fn probing_an_app_that_refuses_client_registration_does_not_offer_oauth() {
    let server = Server::start().await;
    let (base, _) = start_app(true, 3600).await;

    let (status, probed) = server.user.json(server.user.post("/api/v1/chat-tool-servers/probe").json(&json!({"url": format!("{base}/mcp")}))).await;

    assert_eq!((status, probed), (200, json!({"oauth": false, "sign_in": "client_id", "authorization_server": base})));
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
    finish_sign_in(&server, &base, &query).await;
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
    finish_sign_in(&server, &base, &query).await;
    sqlx::query("update chat_tool_servers set bearer_token = 'stale', oauth_expires_at = null").execute(&server.pool).await.unwrap();

    let tools = listed_tools(&server, &node_token).await;

    assert!(tools.iter().any(|tool| tool["name"] == "docs__search"), "{tools:?}");
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-2"));
}

#[tokio::test]
async fn signing_in_again_updates_the_existing_integration_and_keeps_its_tool_names() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;
    finish_sign_in(&server, &base, &query).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let id = listed[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Work docs"}))).await;
    sqlx::query("update chat_tool_servers set bearer_token = 'stale', oauth_refresh_token = null, oauth_expires_at = null").execute(&server.pool).await.unwrap();

    let (status, started) = begin(&server, json!({"name": "Work docs", "url": base, "kind": "outline", "server_id": id})).await;
    assert_eq!(status, 200, "{started}");
    let (status, page) = callback(&server, &param(&query_of(started["authorize_url"].as_str().unwrap()), "state")).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let tools = listed_tools(&server, &node_token).await;

    assert_eq!(status, 200, "{page}");
    assert_eq!(listed.as_array().map(Vec::len), Some(1), "{listed}");
    assert_eq!((listed[0]["id"].as_str(), listed[0]["name"].as_str(), listed[0]["settings"].clone()), (Some(id.as_str()), Some("Work docs"), json!({"app": "outline"})));
    assert!(tools.iter().any(|tool| tool["name"] == "docs__search"), "{tools:?}");
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-3"));
    let tool_prefix: String = sqlx::query_scalar("select tool_prefix from chat_tool_servers").fetch_one(&server.pool).await.unwrap();
    assert_eq!(tool_prefix, "docs");
}

#[tokio::test]
async fn adding_an_oauth_integration_with_a_name_already_taken_is_refused() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;
    finish_sign_in(&server, &base, &query).await;

    let (status, refused) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;

    assert_eq!((status, refused["error"].as_str(), refused["message"].as_str()), (409, Some("name_taken"), Some("Enter a different name. Another tool server already has this one.")));
}

#[tokio::test]
async fn signing_in_again_to_someone_elses_integration_is_refused() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;

    let (status, refused) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline", "server_id": "00000000-0000-0000-0000-000000000001"})).await;

    assert_eq!(status, 404, "{refused}");
}

async fn probe(server: &Server, address: &str) -> Value {
    let (status, probed) = server.user.json(server.user.post("/api/v1/chat-tool-servers/probe").json(&json!({"url": address}))).await;
    assert_eq!(status, 200, "{probed}");

    probed
}

const NO_DETAILS: &str = "The server asks for sign-in but publishes no authorisation details.";

#[tokio::test]
async fn probing_an_app_that_registers_clients_offers_registration_and_names_its_authorisation_server() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed, json!({"oauth": true, "sign_in": "registration", "authorization_server": base}));
}

#[tokio::test]
async fn probing_finds_the_resource_metadata_in_the_challenge_wherever_it_is_hosted() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { challenge: Some("Bearer resource_metadata={base}/custom-metadata"), protected_path: "/custom-metadata", ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed["sign_in"], "registration", "{probed}");
}

#[tokio::test]
async fn probing_without_a_challenge_looks_for_resource_metadata_beside_the_endpoint_path() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { challenge: None, ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed["sign_in"], "registration", "{probed}");
}

#[tokio::test]
async fn probing_without_a_challenge_falls_back_to_resource_metadata_at_the_root() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { challenge: None, protected_path: "/.well-known/oauth-protected-resource", ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed["sign_in"], "registration", "{probed}");
}

#[tokio::test]
async fn probing_reads_authorisation_server_metadata_from_each_place_the_specification_allows() {
    let server = Server::start().await;
    let layouts: [(&str, &str); 4] = [
        ("/tenant", "/.well-known/oauth-authorization-server/tenant"),
        ("/tenant", "/.well-known/openid-configuration/tenant"),
        ("/tenant", "/tenant/.well-known/openid-configuration"),
        ("", "/.well-known/openid-configuration"),
    ];

    for (issuer_path, authorisation_path) in layouts {
        let (base, _) = start_with(Options { issuer_path, authorisation_paths: vec![authorisation_path], ..Options::default() }).await;

        let probed = probe(&server, &format!("{base}/mcp")).await;

        assert_eq!((probed["sign_in"].as_str(), probed["authorization_server"].as_str()), (Some("registration"), Some(format!("{base}{issuer_path}").as_str())), "{authorisation_path}: {probed}");
    }
}

#[tokio::test]
async fn probing_refuses_authorisation_server_metadata_issued_for_another_issuer() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_extra: json!({"issuer": "https://elsewhere.example.test"}), ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed, json!({"oauth": false, "sign_in": "none", "reason": NO_DETAILS}));
}

#[tokio::test]
async fn probing_refuses_an_authorisation_server_that_lists_methods_without_s256() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_extra: json!({"code_challenge_methods_supported": ["plain"]}), ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed, json!({"oauth": false, "sign_in": "none", "reason": NO_DETAILS}));
}

#[tokio::test]
async fn probing_accepts_an_authorisation_server_that_lists_no_challenge_methods() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_extra: json!({"code_challenge_methods_supported": null}), ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed["sign_in"], "registration", "{probed}");
}

#[tokio::test]
async fn probing_a_server_that_asks_for_sign_in_without_authorisation_details_says_so() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_paths: vec![], ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!(probed, json!({"oauth": false, "sign_in": "none", "reason": NO_DETAILS}));
}

#[tokio::test]
async fn probing_an_address_that_neither_challenges_nor_publishes_metadata_says_it_does_not_ask_for_sign_in() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;

    let probed = probe(&server, &format!("{base}/open")).await;

    assert_eq!(probed, json!({"oauth": false, "sign_in": "none", "reason": "This server does not ask for sign-in."}));
}

#[tokio::test]
async fn probing_reports_a_client_id_metadata_document_when_the_authorisation_server_supports_one() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_extra: json!({"client_id_metadata_document_supported": true}), ..Options::default() }).await;

    let probed = probe(&server, &format!("{base}/mcp")).await;

    assert_eq!((probed["sign_in"].as_str(), probed["oauth"].as_bool()), (Some("metadata_document"), Some(true)), "{probed}");
}

#[tokio::test]
async fn the_scope_asked_for_is_the_challenges_else_the_resources_supported_scopes_else_none() {
    let server = Server::start().await;
    let (challenged, _) = start_with(Options { challenge: Some("Bearer scope=\"files:read\", resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\""), ..Options::default() }).await;
    let (listed, _) = start_app(false, 3600).await;
    let (unlisted, _) = start_with(Options { protected_extra: json!({"scopes_supported": null}), ..Options::default() }).await;

    let from_challenge = sign_in(&server, &challenged).await;
    let from_resource = sign_in(&server, &listed).await;
    let neither = sign_in(&server, &unlisted).await;

    assert_eq!(param(&from_challenge, "scope"), "files:read");
    assert_eq!(param(&from_resource, "scope"), "read write");
    assert!(!neither.iter().any(|(name, _)| name == "scope"), "{neither:?}");
}

#[tokio::test]
async fn an_authorisation_server_that_reads_client_metadata_documents_is_given_mycelium_hosted_one_instead_of_a_registration() {
    let server = Server::start().await;
    let (base, app) = start_with(Options { authorisation_extra: json!({"client_id_metadata_document_supported": true}), ..Options::default() }).await;
    let document_url = format!("{}/integrations/oauth/client.json", server.base);

    let query = sign_in(&server, &base).await;

    assert_eq!(param(&query, "client_id"), document_url);
    assert!(app.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_authorisation_server_that_reads_client_metadata_documents_needs_no_registration_endpoint() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { without_registration: true, authorisation_extra: json!({"client_id_metadata_document_supported": true}), ..Options::default() }).await;

    let query = sign_in(&server, &base).await;

    assert_eq!(param(&query, "client_id"), format!("{}/integrations/oauth/client.json", server.base));
}

#[tokio::test]
async fn the_client_metadata_document_is_public_and_names_its_own_address_and_the_callback() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let response = anonymous.http.get(format!("{}/integrations/oauth/client.json", server.base)).send().await.unwrap();
    let status = response.status().as_u16();
    let document: Value = serde_json::from_str(&response.text().await.unwrap()).expect("a JSON document");

    assert_eq!(status, 200);
    assert_eq!(document["client_id"], format!("{}/integrations/oauth/client.json", server.base));
    assert_eq!(document["client_name"], "Mycelium");
    assert_eq!(document["redirect_uris"], json!([format!("{}/integrations/oauth/callback", server.base)]));
    assert_eq!(document["token_endpoint_auth_method"], "none");
    assert_eq!(document["grant_types"], json!(["authorization_code", "refresh_token"]));
}

async fn pre_registered_sign_in(server: &Server, base: &str, secret: Option<&str>) -> Vec<(String, String)> {
    let mut body = json!({"name": "Docs", "url": base, "kind": "outline", "client_id": "pre-1"});

    if let Some(secret) = secret {
        body["client_secret"] = json!(secret);
    }

    let (status, started) = begin(server, body).await;
    assert_eq!(status, 200, "{started}");
    assert!(!started.to_string().contains("pre-secret"), "{started}");

    query_of(started["authorize_url"].as_str().unwrap())
}

#[tokio::test]
async fn a_client_id_entered_by_the_person_is_used_without_registering_and_sends_no_secret_when_there_is_none() {
    let server = Server::start().await;
    let (base, app) = start_with(Options { without_registration: true, ..Options::default() }).await;
    let query = pre_registered_sign_in(&server, &base, None).await;

    finish_sign_in(&server, &base, &query).await;

    let exchange = app.token_requests.lock().unwrap()[0].clone();
    assert_eq!(param(&query, "client_id"), "pre-1");
    assert!(app.registrations.lock().unwrap().is_empty());
    assert_eq!((param(&exchange, "client_id"), param(&exchange, "client_secret")), ("pre-1".to_string(), String::new()));
    assert_eq!(app.token_authorisations.lock().unwrap()[0], "");
}

#[tokio::test]
async fn a_client_id_entered_by_the_person_wins_over_a_client_metadata_document_and_registration() {
    let server = Server::start().await;
    let (base, app) = start_with(Options { authorisation_extra: json!({"client_id_metadata_document_supported": true}), ..Options::default() }).await;

    let query = pre_registered_sign_in(&server, &base, None).await;

    assert_eq!(param(&query, "client_id"), "pre-1");
    assert!(app.registrations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_client_secret_is_sent_in_the_body_when_the_server_takes_it_there_on_exchange_and_refresh_and_is_never_shown() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_with(Options { without_registration: true, token_lifetime: 1, authorisation_extra: json!({"token_endpoint_auth_methods_supported": ["client_secret_post", "client_secret_basic"]}), ..Options::default() }).await;
    let query = pre_registered_sign_in(&server, &base, Some("pre-secret")).await;
    finish_sign_in(&server, &base, &query).await;

    listed_tools(&server, &node_token).await;

    let requests = app.token_requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!((param(&requests[0], "grant_type"), param(&requests[0], "client_secret")), ("authorization_code".to_string(), "pre-secret".to_string()));
    assert_eq!((param(&requests[1], "grant_type"), param(&requests[1], "client_secret")), ("refresh_token".to_string(), "pre-secret".to_string()));
    let listed = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    assert!(!listed.contains("pre-secret"), "{listed}");
    let stored: String = sqlx::query_scalar("select oauth->>'client_secret' from chat_tool_servers").fetch_one(&server.pool).await.unwrap();
    assert!(stored.starts_with("enc:v1:") && !stored.contains("pre-secret"));
}

#[tokio::test]
async fn a_client_secret_is_sent_as_basic_authentication_when_that_is_all_the_server_takes() {
    let server = Server::start().await;
    let (base, app) = start_with(Options { without_registration: true, authorisation_extra: json!({"token_endpoint_auth_methods_supported": ["client_secret_basic"]}), ..Options::default() }).await;
    let query = pre_registered_sign_in(&server, &base, Some("pre-secret")).await;

    finish_sign_in(&server, &base, &query).await;

    let exchange = app.token_requests.lock().unwrap()[0].clone();
    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("pre-1:pre-secret"));
    assert_eq!(app.token_authorisations.lock().unwrap()[0], expected);
    assert_eq!(param(&exchange, "client_secret"), "");
}

#[tokio::test]
async fn a_secret_returned_by_registration_is_kept_and_sent_on_exchange() {
    let server = Server::start().await;
    let (base, app) = start_with(Options { registered_secret: Some("dcr-secret"), authorisation_extra: json!({"token_endpoint_auth_methods_supported": ["client_secret_post"]}), ..Options::default() }).await;
    let query = sign_in(&server, &base).await;

    finish_sign_in(&server, &base, &query).await;

    let exchange = app.token_requests.lock().unwrap()[0].clone();
    assert_eq!((param(&exchange, "client_id"), param(&exchange, "client_secret")), ("client-1".to_string(), "dcr-secret".to_string()));
    let listed = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    assert!(!listed.contains("dcr-secret"), "{listed}");
}

#[tokio::test]
async fn a_sign_in_is_waiting_then_connected_and_the_integration_exists_once_it_is_added() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let id = started["sign_in"].as_str().unwrap().to_string();

    let waiting = sign_in_status(&server, &id).await;
    callback(&server, &id).await;
    let connected = sign_in_status(&server, &id).await;
    let (added, _) = add_integration(&server, &base, &id).await;
    let gone = sign_in_status(&server, &id).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(waiting, (200, json!({"state": "waiting"})));
    assert_eq!(connected, (200, json!({"state": "connected"})));
    assert_eq!((added, gone.0, listed.as_array().map(Vec::len)), (201, 404, Some(1)));
    assert_eq!(listed[0]["oauth"], true);
}

#[tokio::test]
async fn adding_an_integration_with_an_unknown_sign_in_is_refused_and_one_for_another_address_keeps_the_sign_in() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;
    let (other, _) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;
    let id = param(&query, "state");
    callback(&server, &id).await;

    let (unknown, refused) = add_integration(&server, &base, "unknown").await;
    let (mismatch, mismatched) = add_integration(&server, &other, &id).await;
    let (added, _) = add_integration(&server, &base, &id).await;

    assert_eq!((unknown, refused["error"].as_str()), (409, Some("sign_in_expired")));
    assert_eq!((mismatch, mismatched["error"].as_str()), (400, Some("sign_in_mismatch")));
    assert_eq!(added, 201);
}

#[tokio::test]
async fn an_authorisation_server_error_fails_the_sign_in_with_the_reason_in_plain_words() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let reasons = [
        ("access_denied", "127.0.0.1 refused the sign-in: access denied"),
        ("invalid_client", "127.0.0.1 refused the sign-in: it does not recognise this client ID"),
        ("unauthorized_client", "127.0.0.1 refused the sign-in: it does not recognise this client ID"),
        ("invalid_scope", "127.0.0.1 refused the sign-in: it does not allow the access Mycelium asked for"),
        ("server_error", "127.0.0.1 refused the sign-in: it had a problem, try again later"),
        ("temporarily_unavailable", "127.0.0.1 refused the sign-in: it had a problem, try again later"),
    ];

    for (error, reason) in reasons {
        let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
        let id = started["sign_in"].as_str().unwrap();

        let (status, _) = callback_with(&server, &format!("error={error}&state={id}")).await;
        let failed = sign_in_status(&server, id).await;

        assert_eq!((status, failed), (400, (200, json!({"state": "failed", "reason": reason}))), "{error}");
    }

    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    assert_eq!(listed, json!([]));
    assert!(app.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_authorisation_server_error_without_a_known_code_fails_with_its_own_description() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let id = started["sign_in"].as_str().unwrap();

    callback_with(&server, &format!("error=odd_error&error_description=Your%20plan%20does%20not%20allow%20apps&state={id}")).await;

    assert_eq!(sign_in_status(&server, id).await, (200, json!({"state": "failed", "reason": "127.0.0.1 refused the sign-in: Your plan does not allow apps"})));
}

#[tokio::test]
async fn a_token_exchange_the_server_refuses_fails_the_sign_in_with_the_servers_description() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { token_failure: true, ..Options::default() }).await;
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let id = started["sign_in"].as_str().unwrap();

    let (status, _) = callback(&server, id).await;

    assert_eq!(status, 502);
    assert_eq!(sign_in_status(&server, id).await, (200, json!({"state": "failed", "reason": "127.0.0.1 did not give Mycelium access: the code has expired"})));
}

#[tokio::test]
async fn an_answer_from_another_issuer_is_refused_before_the_code_is_exchanged() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let id = started["sign_in"].as_str().unwrap();

    callback_with(&server, &format!("code=code-1&iss=https%3A%2F%2Fevil.example.test&state={id}")).await;

    let (_, failed) = sign_in_status(&server, id).await;
    assert_eq!(failed["state"], "failed");
    assert!(app.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_answer_without_an_issuer_is_refused_when_the_server_says_it_sends_one_and_accepted_with_the_right_one() {
    let server = Server::start().await;
    let (base, _) = start_with(Options { authorisation_extra: json!({"authorization_response_iss_parameter_supported": true}), ..Options::default() }).await;
    let (_, without) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let (_, with) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let without = without["sign_in"].as_str().unwrap();
    let with = with["sign_in"].as_str().unwrap();

    callback_with(&server, &format!("code=code-1&state={without}")).await;
    callback_with(&server, &format!("code=code-1&iss={}&state={with}", base.replace(':', "%3A").replace('/', "%2F"))).await;

    assert_eq!(sign_in_status(&server, without).await.1["state"], "failed");
    assert_eq!(sign_in_status(&server, with).await.1["state"], "connected");
}

#[tokio::test]
async fn cancelling_a_sign_in_discards_it_and_a_late_callback_connects_nothing() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline"})).await;
    let id = started["sign_in"].as_str().unwrap();

    let cancelled = server.user.delete(&format!("/integrations/oauth/{id}")).send().await.unwrap().status().as_u16();
    let (status, page) = callback(&server, id).await;

    assert_eq!(cancelled, 204);
    assert_eq!(sign_in_status(&server, id).await.0, 404);
    assert_eq!(status, 400, "{page}");
    assert!(app.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_sign_in_that_does_not_exist_is_not_found() {
    let server = Server::start().await;

    assert_eq!(sign_in_status(&server, "unknown").await.0, 404);
}

#[tokio::test]
async fn signing_in_again_reports_connected_with_the_existing_integration() {
    let server = Server::start().await;
    let (base, _) = start_app(false, 3600).await;
    let query = sign_in(&server, &base).await;
    finish_sign_in(&server, &base, &query).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let existing = listed[0]["id"].as_str().unwrap().to_string();
    let (_, started) = begin(&server, json!({"name": "Docs", "url": base, "kind": "outline", "server_id": existing})).await;
    let id = started["sign_in"].as_str().unwrap();

    callback(&server, id).await;

    assert_eq!(sign_in_status(&server, id).await, (200, json!({"state": "connected", "server": existing})));
}

async fn connected_integration(server: &Server, base: &str) -> String {
    let query = sign_in(server, base).await;
    finish_sign_in(server, base, &query).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    listed[0]["id"].as_str().unwrap().to_string()
}

fn refresh_requests(app: &App) -> usize {
    app.token_requests.lock().unwrap().iter().filter(|form| form.iter().any(|(name, value)| name == "grant_type" && value == "refresh_token")).count()
}

#[tokio::test]
async fn a_rejected_oauth_token_is_refreshed_and_the_tools_listed_even_when_an_expiry_is_known() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 3600).await;
    connected_integration(&server, &base).await;
    sqlx::query("update chat_tool_servers set bearer_token = 'stale'").execute(&server.pool).await.unwrap();

    let tools = listed_tools(&server, &node_token).await;

    assert!(tools.iter().any(|tool| tool["name"] == "docs__search"), "{tools:?}");
    assert_eq!(refresh_requests(&app), 1);
    assert_eq!(app.mcp_authorisations.lock().unwrap().last().map(String::as_str), Some("Bearer access-2"));
}

#[tokio::test]
async fn a_token_still_rejected_after_a_refresh_is_refreshed_once_not_again() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 3600).await;
    connected_integration(&server, &base).await;
    app.rejecting.store(true, Ordering::SeqCst);

    listed_tools(&server, &node_token).await;

    assert_eq!(refresh_requests(&app), 1);
}

#[tokio::test]
async fn the_integration_proxy_retries_once_with_a_refreshed_token_after_a_401() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let id = connected_integration(&server, &base).await;

    let (status, answer) = server.user.json(server.user.post(&format!("/api/v1/integrations/{id}/proxy/collections.list")).json(&json!({}))).await;

    assert_eq!((status, answer), (200, json!({"data": []})));
    assert_eq!(*app.api_authorisations.lock().unwrap(), vec!["Bearer access-1".to_string(), "Bearer access-2".to_string()]);
    assert_eq!(refresh_requests(&app), 1);
}

#[tokio::test]
async fn the_integration_proxy_does_not_retry_a_second_401_after_refreshing() {
    let server = Server::start().await;
    let (base, app) = start_app(false, 3600).await;
    let id = connected_integration(&server, &base).await;
    app.rejecting.store(true, Ordering::SeqCst);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/integrations/{id}/proxy/collections.list")).json(&json!({}))).await;

    assert_eq!(status, 401);
    assert_eq!(app.api_authorisations.lock().unwrap().len(), 2);
    assert_eq!(refresh_requests(&app), 1);
}

#[tokio::test]
async fn a_403_insufficient_scope_tells_the_person_to_sign_in_again_and_names_the_scope_without_refreshing() {
    let server = Server::start().await;
    let (_, node_token) = server.join("node", "laptop").await;
    let (base, app) = start_app(false, 3600).await;
    connected_integration(&server, &base).await;
    app.scope_missing.store(true, Ordering::SeqCst);
    let anonymous = server.anonymous();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "docs__search", "arguments": {}}});

    let (_, called) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {node_token}")).json(&call)).await;

    assert_eq!(called["result"]["content"][0]["text"], "Docs needs more access than the sign-in gave it. Sign in to Docs again in Settings, Integrations and allow: files:write.");
    assert_eq!(refresh_requests(&app), 0);
}
