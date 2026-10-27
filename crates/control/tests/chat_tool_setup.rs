mod common;

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use common::{Server, serve};
use serde_json::{Value, json};

const KEY: &str = "key-1234";

async fn mcp(headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some(&format!("Bearer {KEY}")) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if body.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match body["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "lab", "version": "1"}}),
        "tools/list" => json!({"tools": [
            {"name": "search", "description": "Search", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}},
            {"name": "create_doc", "description": "Create", "inputSchema": {"type": "object"}},
        ]}),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
}

async fn fake_mcp() -> String {
    format!("{}/mcp", serve(Router::new().route("/mcp", post(mcp))).await)
}

#[tokio::test]
async fn testing_lists_the_tools_with_their_kind_and_creates_nothing() {
    let server = Server::start().await;
    let address = fake_mcp().await;

    let (status, tested) = server.user.json(server.user.post("/api/v1/chat-tool-servers/test").json(&json!({"url": address, "credential": KEY}))).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 200, "{tested}");
    assert_eq!(tested["tools"], json!([{"name": "search", "kind": "read"}, {"name": "create_doc", "kind": "write"}]));
    assert_eq!(stored, "[]");
}

#[tokio::test]
async fn testing_with_a_wrong_credential_says_the_server_refused_it() {
    let server = Server::start().await;
    let address = fake_mcp().await;

    let (status, refused) = server.user.json(server.user.post("/api/v1/chat-tool-servers/test").json(&json!({"url": address, "credential": "wrong"}))).await;

    assert_eq!(status, 400);
    assert_eq!(refused["error"], "tool_server_failed");
}

#[tokio::test]
async fn probing_a_server_without_sign_in_reports_no_oauth() {
    let server = Server::start().await;
    let address = fake_mcp().await;

    let (status, probed) = server.user.json(server.user.post("/api/v1/chat-tool-servers/probe").json(&json!({"url": address}))).await;
    let (bad_status, bad) = server.user.json(server.user.post("/api/v1/chat-tool-servers/probe").json(&json!({"url": "not a url"}))).await;

    assert_eq!((status, probed), (200, json!({"oauth": false})));
    assert_eq!(bad_status, 400, "{bad}");
}
