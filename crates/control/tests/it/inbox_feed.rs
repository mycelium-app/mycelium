use crate::common::{Server, serve};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Murmur {
    keys: Arc<Mutex<Vec<String>>>,
    failing: Arc<AtomicBool>,
}

fn updates() -> Value {
    json!({
        "needsYou": [
            {"id": "n1", "author": "Ada", "project": "Mycelium", "kind": "needs_you", "message": "Pick a launch date", "links": [{"url": "https://murmur.example/p/n1"}], "createdAt": "2026-11-08T09:00:00Z", "doneAt": null},
        ],
        "posts": [
            {"id": "p2", "author": "Linus", "project": "Murmur", "kind": "update", "message": "Reworked the feed", "links": [{"url": "https://murmur.example/p/p2"}, {"url": "https://murmur.example/other"}], "createdAt": "2026-11-08T10:00:00Z", "doneAt": null},
            {"id": "p1", "author": "Grace", "project": null, "kind": "shipped", "message": "Released 0.30", "links": [], "createdAt": "2026-11-08T08:00:00Z", "doneAt": null},
            {"id": "n0", "author": "Ada", "project": null, "kind": "needs_you", "message": "Picked a name", "links": [], "createdAt": "2026-11-08T07:00:00Z", "doneAt": "2026-11-08T07:30:00Z"},
        ],
    })
}

async fn handle(State(murmur): State<Murmur>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if body.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    if murmur.failing.load(Ordering::SeqCst) && body["method"] == "tools/call" {
        return StatusCode::BAD_GATEWAY.into_response();
    }

    if let Some(key) = headers.get("authorization").and_then(|value| value.to_str().ok()) {
        murmur.keys.lock().unwrap().push(key.to_string());
    }

    let result = match body["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "murmur", "version": "1"}}),
        "tools/list" => json!({"tools": [{"name": "list_updates", "description": "List updates.", "inputSchema": {"type": "object"}}]}),
        "tools/call" => json!({"content": [{"type": "text", "text": updates().to_string()}]}),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
}

async fn add_murmur(server: &Server, murmur: &Murmur) {
    let base = serve(Router::new().route("/mcp", post(handle)).with_state(murmur.clone())).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Murmur", "kind": "murmur", "url": format!("{base}/mcp"), "bearer_token": "murmur-key"}))).await;
    assert_eq!(status, 201, "{added}");
}

#[tokio::test]
async fn the_feed_is_not_configured_without_a_murmur_integration() {
    let server = Server::start().await;

    let (status, feed) = server.user.json(server.user.get("/api/v1/inbox/feed")).await;

    assert_eq!(status, 200);
    assert_eq!(feed["configured"], false);
    assert_eq!(feed["posts"], json!([]));
}

#[tokio::test]
async fn the_feed_shows_only_murmurs_posts_and_sends_the_key_as_a_bearer() {
    let server = Server::start().await;
    let murmur = Murmur::default();
    add_murmur(&server, &murmur).await;

    let (_, feed) = server.user.json(server.user.get("/api/v1/inbox/feed")).await;

    let ids: Vec<&str> = feed["posts"].as_array().unwrap().iter().map(|post| post["id"].as_str().unwrap()).collect();
    assert_eq!(feed["configured"], true);
    assert_eq!(feed["error"], Value::Null);
    assert_eq!(ids, ["p2", "p1", "n0"]);
    assert_eq!(feed["posts"][0]["url"], "https://murmur.example/p/p2");
    assert_eq!(feed["posts"][1]["url"], Value::Null);
    assert_eq!(feed["posts"][1]["project"], Value::Null);
    assert_eq!(feed["posts"][1]["created_at"], "2026-11-08T08:00:00Z");
    assert!(!murmur.keys.lock().unwrap().is_empty());
    assert!(murmur.keys.lock().unwrap().iter().all(|key| key == "Bearer murmur-key"));
}

#[tokio::test]
async fn when_murmur_stops_answering_the_last_posts_come_back_with_when_they_were_fetched() {
    let server = Server::start().await;
    let murmur = Murmur::default();
    add_murmur(&server, &murmur).await;
    server.user.json(server.user.get("/api/v1/inbox/feed")).await;
    murmur.failing.store(true, Ordering::SeqCst);

    let (status, feed) = server.user.json(server.user.get("/api/v1/inbox/feed?refresh=true")).await;

    assert_eq!(status, 200);
    assert_eq!(feed["posts"].as_array().unwrap().len(), 3);
    assert_eq!(feed["error"], "Murmur did not answer.");
    assert!(feed["stale_since"].is_string());
}
