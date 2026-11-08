use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use crate::common::{Server, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "paperless-token-1234";
const NOT_TEXT: [u8; 8] = [0xff, 0xfe, 0x00, 0x25, 0x50, 0x44, 0x46, 0x80];
const LONG_DOCUMENT: u64 = 6;
const LONG_TEXT_CHARACTERS: usize = 25_000;

#[derive(Default)]
struct FakePaperless {
    queries: Mutex<Vec<HashMap<String, String>>>,
    patched: Mutex<Vec<Value>>,
    created: Mutex<Vec<Value>>,
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
        LONG_DOCUMENT => Json(json!({"id": LONG_DOCUMENT, "title": "Long scan", "content": "a".repeat(LONG_TEXT_CHARACTERS), "notes": []})).into_response(),
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

async fn download(Path(id): Path<u64>, Query(query): Query<HashMap<String, String>>) -> Response {
    match (id, query.get("original").map(String::as_str)) {
        (5, Some("true")) => ([("content-type", "application/pdf")], NOT_TEXT.to_vec()).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn pages(State(fake): State<Arc<FakePaperless>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    let page: u64 = query.get("page").and_then(|page| page.parse().ok()).unwrap_or(1);
    let host = headers.get("host").and_then(|value| value.to_str().ok()).unwrap_or_default();
    let next = (page < 3).then(|| format!("http://{host}/api/paged/?page={}", page + 1));

    fake.queries.lock().unwrap().push(query);

    Json(json!({"count": 3, "next": next, "previous": null, "results": [{"id": page}]}))
}

async fn create_tag(State(fake): State<Arc<FakePaperless>>, Json(body): Json<Value>) -> Json<Value> {
    fake.created.lock().unwrap().push(body.clone());

    Json(json!({"id": 9, "name": body["name"]}))
}

async fn fake_paperless() -> (String, Arc<FakePaperless>) {
    let fake = Arc::new(FakePaperless::default());
    let router = Router::new()
        .route("/api/documents/", get(documents))
        .route("/api/documents/{id}/", get(one).patch(patch))
        .route("/api/documents/{id}/download/", get(download))
        .route("/api/paged/", get(pages))
        .route("/api/tags/", get(tags).post(create_tag))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_paperless(server: &Server, base: &str, token: &str) -> (u16, Value) {
    add_named_paperless(server, "Paperless", base, token).await
}

async fn add_named_paperless(server: &Server, name: &str, base: &str, token: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": name, "kind": "paperless", "url": base, "bearer_token": token}))).await
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    call_on(server, "paperless", token, tool, arguments).await
}

async fn call_on(server: &Server, prefix: &str, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("{prefix}__{tool}"), "arguments": arguments}});
    let reply = if tool == "update_document" || arguments["method"].as_str().is_some_and(|method| method != "GET") {
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

async fn call_saving(server: &Server, tool: &str, mut arguments: Value, path: &str) -> (Value, Vec<u8>) {
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Collect the receipts")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    arguments["save_to"] = json!(path);

    let token = link.token.clone();
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("paperless__{tool}"), "arguments": arguments}});
    let (reply, written) = tokio::join!(server.agent_mcp(&token, conversation.id, request), async {
        let ControlToNode::Git { request_id, command, .. } = link.next().await else {
            panic!("expected the node to be asked to write the answer");
        };
        let GitCommand::WriteFile { path: written_path, data_base64 } = command else {
            panic!("expected WriteFile, got {command:?}");
        };
        let bytes = base64::engine::general_purpose::STANDARD.decode(&data_base64).unwrap();
        link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": written_path, "size": bytes.len()}), error: None }).await;

        assert_eq!(written_path, path);

        bytes
    });

    (reply["result"].clone(), written)
}

#[tokio::test]
async fn the_api_tool_follows_every_page_and_returns_all_the_results() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;
    fake.queries.lock().unwrap().clear();

    let result = call(&server, &node.token, "api", json!({"method": "GET", "path": "paged/", "all_pages": true})).await;
    let answer: Value = serde_json::from_str(&text(&result)).unwrap();
    let first = fake.queries.lock().unwrap().first().cloned().unwrap();

    assert_eq!((answer["pages"].clone(), answer["rows"].clone()), (json!(3), json!(3)), "{answer}");
    assert_eq!(answer["body"]["results"], json!([{"id": 1}, {"id": 2}, {"id": 3}]));
    assert_eq!(first.get("page_size").map(String::as_str), Some("100"));
}

#[tokio::test]
async fn the_api_tool_passes_query_parameters_through_and_reads_without_asking() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;
    fake.queries.lock().unwrap().clear();

    let result = call(&server, &node.token, "api", json!({"method": "GET", "path": "/api/documents/", "query": {"created__date__gte": "2026-04-06", "created__date__lte": "2027-04-05", "tags__id__all": [3, 4]}})).await;
    let query = fake.queries.lock().unwrap().first().cloned().unwrap();

    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(query.get("created__date__gte").map(String::as_str), Some("2026-04-06"));
    assert_eq!(query.get("created__date__lte").map(String::as_str), Some("2027-04-05"));
    assert_eq!(query.get("tags__id__all").map(String::as_str), Some("3,4"));
}

#[tokio::test]
async fn an_api_write_asks_first_and_is_not_made_without_an_answer() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;
    let body = json!({"method": "POST", "path": "tags/", "body": {"name": "Receipts"}});

    let conversation = server.node_conversation(&node.token).await;
    let request_body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "paperless__api", "arguments": body}});
    let (declined, asked) = tokio::join!(server.agent_mcp(&node.token, conversation, request_body), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "deny").await;

        request
    });

    assert_eq!(declined["result"]["isError"], true, "{declined}");
    assert!(asked.to_string().contains("paperless__api"), "{asked}");
    assert!(fake.created.lock().unwrap().is_empty());

    let approved = call(&server, &node.token, "api", body).await;

    assert_eq!(approved["isError"], false, "{approved}");
    assert_eq!(fake.created.lock().unwrap().clone(), vec![json!({"name": "Receipts"})]);
}

#[tokio::test]
async fn downloading_an_original_saves_the_exact_bytes_to_the_conversation_folder() {
    let server = Server::start().await;
    let (base, _) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let (downloaded, written) = call_saving(&server, "download_document", json!({"document": 5}), "receipts/boiler.pdf").await;

    assert_eq!(written, NOT_TEXT.to_vec());
    assert_eq!(text(&downloaded), "Saved the answer, 8 bytes, to receipts/boiler.pdf in the conversation folder.");
}

#[tokio::test]
async fn the_api_tool_saves_a_file_answer_unchanged_too() {
    let server = Server::start().await;
    let (base, _) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let (_, written) = call_saving(&server, "api", json!({"method": "GET", "path": "documents/5/download/", "query": {"original": true}}), "receipts/again.pdf").await;

    assert_eq!(written, NOT_TEXT.to_vec());
}

#[tokio::test]
async fn a_file_answer_without_save_to_asks_for_save_to_instead_of_returning_bytes() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let result = call(&server, &node.token, "download_document", json!({"document": 5})).await;

    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("save_to"), "{result}");
}

#[tokio::test]
async fn a_long_documents_text_is_cut_unless_it_is_saved_whole() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_paperless().await;
    add_paperless(&server, &base, TOKEN).await;

    let cut: Value = serde_json::from_str(&text(&call(&server, &node.token, "get_document", json!({"document": LONG_DOCUMENT})).await)).unwrap();
    let (_, written) = call_saving(&server, "get_document", json!({"document": LONG_DOCUMENT}), "scans/long.json").await;
    let saved: Value = serde_json::from_slice(&written).unwrap();

    assert_eq!((cut["text"].as_str().unwrap().len(), cut["text_truncated"].clone()), (20_000, json!(true)));
    assert_eq!((saved["text"].as_str().unwrap().len(), saved["text_truncated"].clone()), (LONG_TEXT_CHARACTERS, json!(false)));
}

#[tokio::test]
async fn two_paperless_accounts_get_their_own_tool_names_and_each_call_reaches_its_own_instance() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (first_base, first) = fake_paperless().await;
    let (second_base, second) = fake_paperless().await;
    add_named_paperless(&server, "Paperless Alice", &first_base, TOKEN).await;
    add_named_paperless(&server, "Paperless Bob", &second_base, TOKEN).await;

    first.queries.lock().unwrap().clear();
    second.queries.lock().unwrap().clear();

    let anonymous = server.anonymous();
    let (_, listed) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", node.token)).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect();

    call_on(&server, "paperless_bob", &node.token, "search_documents", json!({"query": "boiler"})).await;

    assert!(names.contains(&"paperless_alice__api") && names.contains(&"paperless_bob__api"), "{names:?}");
    assert_eq!((first.queries.lock().unwrap().len(), second.queries.lock().unwrap().len()), (0, 1));
}
