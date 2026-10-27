mod common;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use common::{NodeLink, Server, serve};
use mycelium_proto::link::{ControlToNode};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const KEY: &str = "outline-key-1234";

#[derive(Default)]
struct FakeOutline {
    api_calls: Mutex<Vec<(String, Option<String>)>>,
}

async fn mcp(headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some(&format!("Bearer {KEY}")) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if body.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match body["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "outline", "version": "1"}}),
        "tools/list" => json!({"tools": [{"name": "search", "description": "Search", "inputSchema": {"type": "object"}}]}),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
}

async fn api(State(fake): State<Arc<FakeOutline>>, axum::extract::Path(action): axum::extract::Path<String>, headers: HeaderMap) -> Response {
    let authorisation = headers.get("authorization").and_then(|value| value.to_str().ok()).map(str::to_string);
    fake.api_calls.lock().unwrap().push((action.clone(), authorisation));

    Json(json!({"data": [{"title": format!("answer to {action}")}]})).into_response()
}

async fn document_info(Json(body): Json<Value>) -> Response {
    if body["id"] != "aB3dE5fG" {
        return StatusCode::NOT_FOUND.into_response();
    }

    Json(json!({"data": {"title": "Release notes", "urlId": "aB3dE5fG"}})).into_response()
}

async fn add_outline(server: &Server) -> (String, Arc<FakeOutline>) {
    let fake = Arc::new(FakeOutline::default());
    let base = serve(Router::new().route("/mcp", post(mcp)).route("/api/documents.info", post(document_info)).route("/api/{action}", post(api)).with_state(fake.clone())).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Docs", "kind": "outline", "url": base, "bearer_token": KEY}))).await;
    assert_eq!(status, 201, "{added}");

    (added["id"].as_str().unwrap().to_string(), fake)
}

async fn working_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Upload the transcripts")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    conversation.id
}

async fn request_access(server: &Server, link: &NodeLink, conversation: Uuid, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "request_api_access", "arguments": arguments}}));
    let (_, body) = anonymous.json(request).await;

    body["result"].clone()
}

async fn call(server: &Server, token: &str, id: &str, method: &str, path: &str) -> u16 {
    let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
    let response = server.anonymous().http.request(method, format!("{}/api/v1/integrations/{id}/proxy/{path}", server.base)).header("authorization", format!("Bearer {token}")).send().await.unwrap();

    response.status().as_u16()
}

async fn granted_token(server: &Server, link: &mut NodeLink, conversation: Uuid, scopes: Value) -> String {
    let result = request_access(server, link, conversation, json!({"integration": "Docs", "scopes": scopes, "reason": "Upload 22 transcripts"})).await;
    assert_eq!(result["isError"], json!(false), "{result}");
    let (_, requests) = server.user.json(server.user.get("/api/v1/api-access-requests")).await;
    let request = requests[0].clone();
    assert_eq!(request["integration"], "Docs");
    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/api-access-requests/{}/approve", request["id"].as_str().unwrap())).json(&json!({}))).await;
    assert_eq!(status, 200, "{approved}");
    let ControlToNode::ApiToken { token, .. } = link.next().await else {
        panic!("expected api_token");
    };

    token
}

#[tokio::test]
async fn a_granted_agent_reaches_the_integration_api_without_the_key_and_the_call_is_logged() {
    let server = Server::start().await;
    let (id, fake) = add_outline(&server).await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let token = granted_token(&server, &mut link, conversation, json!(["read"])).await;

    let listed = call(&server, &token, &id, "POST", "documents.list").await;
    let calls = fake.api_calls.lock().unwrap().clone();
    let (_, logged) = server.user.json(server.user.get(&format!("/api/v1/integration-calls?server_id={id}"))).await;
    let (_, grants) = server.user.json(server.user.get(&format!("/api/v1/chat-tool-servers/{id}/api-access"))).await;

    assert_eq!(listed, 200);
    assert_eq!(calls, vec![("documents.list".to_string(), Some(format!("Bearer {KEY}")))]);
    assert_eq!(logged[0]["tool"], "api:POST documents.list");
    assert_eq!(grants[0]["scopes"], json!(["read"]));
    assert!(!token.contains(KEY));
}

#[tokio::test]
async fn a_read_grant_cannot_write_and_a_grant_for_one_account_cannot_reach_another() {
    let server = Server::start().await;
    let (id, fake) = add_outline(&server).await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let token = granted_token(&server, &mut link, conversation, json!(["read"])).await;

    let created = call(&server, &token, &id, "POST", "documents.create").await;
    let elsewhere = call(&server, &token, &Uuid::new_v4().to_string(), "POST", "documents.list").await;

    assert_eq!((created, elsewhere), (403, 403));
    assert!(fake.api_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_write_grant_can_write_and_revoking_it_stops_the_proxy() {
    let server = Server::start().await;
    let (id, _) = add_outline(&server).await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let token = granted_token(&server, &mut link, conversation, json!(["read", "write"])).await;

    let created = call(&server, &token, &id, "POST", "documents.create").await;
    let (_, grants) = server.user.json(server.user.get(&format!("/api/v1/chat-tool-servers/{id}/api-access"))).await;
    let revoked = server.user.delete(&format!("/api/v1/sessions/{}", grants[0]["token_id"].as_str().unwrap())).send().await.unwrap().status().as_u16();
    let after = call(&server, &token, &id, "POST", "documents.list").await;

    assert_eq!((created, revoked, after), (200, 204, 401));
}

#[tokio::test]
async fn an_integration_without_an_api_cannot_be_requested() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let (status, plain) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Plain", "url": "http://127.0.0.1:1/mcp"}))).await;

    let unknown = request_access(&server, &link, conversation, json!({"integration": "Missing", "scopes": ["read"], "reason": "x"})).await;

    assert_ne!(status, 201, "{plain}");
    assert_eq!(unknown["isError"], json!(true));
    assert!(unknown["content"][0]["text"].as_str().unwrap().contains("no integration"));
}

async fn open_outline(server: &Server, link: &NodeLink, conversation: Uuid, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "open_outline", "arguments": arguments}}));
    let (_, body) = anonymous.json(request).await;

    body["result"].clone()
}

#[tokio::test]
async fn open_outline_records_the_page_in_the_thread_with_its_title_and_address() {
    let server = Server::start().await;
    let (id, _) = add_outline(&server).await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    let opened = open_outline(&server, &link, conversation, json!({"document": "https://docs.example/doc/release-notes-aB3dE5fG"})).await;
    let (_, events) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/events"))).await;
    let event = events.as_array().unwrap().iter().find(|event| event["kind"] == "open_outline").unwrap().clone();

    assert_eq!(opened["isError"], json!(false), "{opened}");
    assert_eq!(event["payload"]["title"], "Release notes");
    assert!(event["payload"]["url"].as_str().unwrap().ends_with("/doc/aB3dE5fG"));
    assert_eq!(event["payload"]["integration_id"], id);
}

#[tokio::test]
async fn open_outline_without_an_outline_integration_says_where_to_add_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    let opened = open_outline(&server, &link, conversation, json!({"document": "aB3dE5fG"})).await;

    assert_eq!(opened["isError"], json!(true));
    assert_eq!(opened["content"][0]["text"], "This conversation has no Outline integration. Add one in Settings, Integrations.");
}

#[tokio::test]
async fn open_outline_with_two_integrations_asks_which_one() {
    let server = Server::start().await;
    add_outline(&server).await;
    let fake = Arc::new(FakeOutline::default());
    let base = serve(Router::new().route("/mcp", post(mcp)).route("/api/{action}", post(api)).with_state(fake)).await;
    let (status, _) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Wiki", "kind": "outline", "url": base, "bearer_token": KEY}))).await;
    assert_eq!(status, 201);
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    let opened = open_outline(&server, &link, conversation, json!({"document": "aB3dE5fG"})).await;

    assert_eq!(opened["isError"], json!(true));
    assert!(opened["content"][0]["text"].as_str().unwrap().starts_with("This conversation has more than one Outline integration"));
}
