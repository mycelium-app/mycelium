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

const TOKEN: &str = "paperless-token-1234";

#[derive(Default)]
struct FakePaperless {
    queries: Mutex<Vec<HashMap<String, String>>>,
    patched: Mutex<Vec<Value>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Token {TOKEN}"))
}

fn document() -> Value {
    json!({"id": 5, "title": "Boiler invoice", "created": "2026-09-01", "added": "2026-09-02T10:00:00Z", "correspondent": 2, "document_type": 3, "tags": [1], "original_file_name": "boiler.pdf", "content": "Boiler service, total 120 pounds", "notes": []})
}

async fn documents(State(fake): State<Arc<FakePaperless>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    if !authorised(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"detail": "Invalid token."}))).into_response();
    }

    fake.queries.lock().unwrap().push(query);

    Json(json!({"count": 1, "results": [document()]})).into_response()
}

async fn one(Path(id): Path<u64>) -> Response {
    match id {
        5 => Json(document()).into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({"detail": "Not found."}))).into_response(),
    }
}

async fn patch(State(fake): State<Arc<FakePaperless>>, Json(body): Json<Value>) -> Json<Value> {
    fake.patched.lock().unwrap().push(body.clone());

    let mut updated = document();

    for (key, value) in body.as_object().unwrap() {
        updated[key] = value.clone();
    }

    Json(updated)
}

async fn tags() -> Json<Value> {
    Json(json!({"count": 1, "results": [{"id": 1, "name": "Home", "document_count": 4}]}))
}

async fn fake_paperless() -> (String, Arc<FakePaperless>) {
    let fake = Arc::new(FakePaperless::default());
    let router = Router::new().route("/api/documents/", get(documents)).route("/api/documents/{id}/", get(one).patch(patch)).route("/api/tags/", get(tags)).with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_paperless(server: &Server, base: &str, token: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Paperless", "kind": "paperless", "url": base, "bearer_token": token}))).await
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("paperless__{tool}"), "arguments": arguments}});
    let reply = if tool == "update_document" {
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
async fn an_api_token_paperless_does_not_accept_is_refused_and_a_good_one_is_not_returned() {
    let server = Server::start().await;
    let (base, _) = fake_paperless().await;

    let (refused, wrong) = add_paperless(&server, &base, "wrong").await;
    let (status, _) = add_paperless(&server, &base, TOKEN).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!((refused, wrong["error"].clone()), (400, json!("tool_server_failed")));
    assert!(wrong["message"].as_str().unwrap().contains("API token"));
    assert_eq!(status, 201);
    assert!(!stored.contains(TOKEN));
}

#[tokio::test]
async fn the_agent_searches_reads_a_documents_text_and_lists_tags() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let found = text(&call(&server, &node.token, "search_documents", json!({"query": "boiler", "tag": 1})).await);
    let read = text(&call(&server, &node.token, "get_document", json!({"document": 5})).await);
    let missing = call(&server, &node.token, "get_document", json!({"document": 404})).await;
    let listed = text(&call(&server, &node.token, "list_tags", json!({})).await);
    let search = fake.queries.lock().unwrap().iter().find(|query| query.contains_key("query")).cloned().unwrap();

    assert!(found.contains("Boiler invoice") && !found.contains("Boiler service"), "{found}");
    assert!(read.contains("Boiler service, total 120 pounds"), "{read}");
    assert_eq!((missing["isError"].clone(), text(&missing)), (json!(true), "Paperless-ngx has no document with that id.".to_string()));
    assert!(listed.contains("Home"), "{listed}");
    assert_eq!((search["query"].as_str(), search["tags__id__all"].as_str()), ("boiler", "1"));
}

#[tokio::test]
async fn setting_tags_and_the_correspondent_asks_first_and_changes_only_what_was_given() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let changed = call(&server, &node.token, "update_document", json!({"document": 5, "tags": [1, 7], "correspondent": null})).await;
    let nothing = call(&server, &node.token, "update_document", json!({"document": 5})).await;

    assert_ne!(changed["isError"], true, "{changed}");
    assert_eq!(fake.patched.lock().unwrap().clone(), vec![json!({"tags": [1, 7], "correspondent": null})]);
    assert_eq!(text(&nothing), "Give tags, correspondent or both to change.");
}
