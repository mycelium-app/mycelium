use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const BRAVE_KEY: &str = "brave-key-1234";

type Seen = (HashMap<String, String>, Option<String>);

#[derive(Default)]
struct FakeSearch {
    json_enabled: AtomicBool,
    seen: Mutex<Vec<Seen>>,
}

async fn searx(State(fake): State<Arc<FakeSearch>>, Query(query): Query<HashMap<String, String>>, headers: HeaderMap) -> Response {
    let token = headers.get("x-subscription-token").and_then(|value| value.to_str().ok()).map(str::to_string);
    let question = query.get("q").cloned().unwrap_or_default();

    fake.seen.lock().unwrap().push((query, token));

    if !fake.json_enabled.load(Ordering::SeqCst) {
        return StatusCode::FORBIDDEN.into_response();
    }

    match question.as_str() {
        "boom" => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        "nothing" => Json(json!({"results": []})).into_response(),
        _ => Json(json!({"results": [
            {"title": "Ceiling track", "img_src": "https://images.example.org/a.png", "url": "https://shop.example.com/track"},
            {"title": "Plain http", "img_src": "http://images.example.org/b.png", "url": "https://shop.example.com/b"},
            {"title": "Has space", "img_src": "https://images.example.org/c d.png", "url": "https://shop.example.com/c"},
            {"title": "Has [brackets]", "img_src": "https://cdn.example.net/d.jpg"},
            {"title": "Has parens", "img_src": "https://images.example.org/e_(1).png", "url": "https://shop.example.com/e"}
        ]}))
        .into_response(),
    }
}

async fn brave(State(fake): State<Arc<FakeSearch>>, Query(query): Query<HashMap<String, String>>, headers: HeaderMap) -> Response {
    let token = headers.get("x-subscription-token").and_then(|value| value.to_str().ok()).map(str::to_string);

    fake.seen.lock().unwrap().push((query, token.clone()));

    if token.as_deref() != Some(BRAVE_KEY) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    Json(json!({"results": [{"type": "image_result", "title": "Hook", "url": "https://shop.example.com/hook", "properties": {"url": "https://images.example.org/hook.jpg"}}]})).into_response()
}

async fn fake_search(json_enabled: bool) -> (String, Arc<FakeSearch>) {
    let fake = Arc::new(FakeSearch::default());
    fake.json_enabled.store(json_enabled, Ordering::SeqCst);
    let router = Router::new().route("/search", get(searx)).route("/brave/images", get(brave)).with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_search(server: &Server, address: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Search", "kind": "web_search", "url": address}))).await
}

async fn conversation_of(server: &Server, token: &str) -> Uuid {
    server.node_conversation(token).await
}

async fn search_images(server: &Server, token: &str, conversation: Uuid, query: &str) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search__search_images", "arguments": {"query": query}}});

    server.agent_mcp(token, conversation, body).await["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn recorded_images(server: &Server, conversation: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as::<_, (String, Option<String>)>("select i.url, i.page_url from integration_call_images i join integration_calls c on c.id = i.call_id where c.conversation_id = $1 order by i.url").bind(conversation).fetch_all(&server.pool).await.unwrap()
}

#[tokio::test]
async fn web_search_with_searxng_is_added_without_a_key_after_a_json_search_succeeds() {
    let server = Server::start().await;
    let (base, fake) = fake_search(true).await;

    let (status, added) = add_search(&server, &base).await;
    let seen = fake.seen.lock().unwrap().clone();

    assert_eq!(status, 201, "{added}");
    assert_eq!(added["kind"], "web_search");
    assert_eq!(added["tools"][0]["name"], "search_images");
    assert_eq!(added["tools"][0]["asks"], false);
    assert_eq!((seen[0].0["categories"].as_str(), seen[0].0["format"].as_str()), ("images", "json"));
}

#[tokio::test]
async fn searxng_with_json_turned_off_says_how_to_turn_it_on() {
    let server = Server::start().await;
    let (base, _) = fake_search(false).await;

    let (status, refused) = add_search(&server, &base).await;

    assert_eq!((status, refused["error"].clone()), (400, json!("tool_server_failed")));
    assert!(refused["message"].as_str().unwrap().starts_with("Turn on the JSON format in your SearXNG settings"), "{refused}");
}

#[tokio::test]
async fn brave_needs_a_key_and_sends_it_as_the_subscription_token() {
    let server = Server::start().await;
    let (base, fake) = fake_search(true).await;
    let address = format!("{base}/brave/images");
    let brave = |key: Option<&str>| {
        let mut body = json!({"name": "Search", "kind": "web_search", "url": address, "settings": {"provider": "brave"}});

        if let Some(key) = key {
            body["bearer_token"] = json!(key);
        }

        server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body))
    };

    let (without, missing) = brave(None).await;
    let (wrong, refused) = brave(Some("not-the-key")).await;
    let (with, added) = brave(Some(BRAVE_KEY)).await;

    assert_eq!((without, missing["error"].clone()), (400, json!("api_key_required")));
    assert_eq!(wrong, 400, "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("did not accept the API key"));
    assert_eq!(with, 201, "{added}");
    assert!(!serde_json::to_string(&added).unwrap().contains(BRAVE_KEY));
    assert_eq!(fake.seen.lock().unwrap().last().unwrap().1.as_deref(), Some(BRAVE_KEY));
}

#[tokio::test]
async fn search_images_lists_results_with_a_markdown_block_and_records_the_urls_for_that_conversation() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_search(true).await;
    add_search(&server, &base).await;
    let conversation = conversation_of(&server, &node.token).await;
    let other = conversation_of(&server, &node.token).await;

    let result = search_images(&server, &node.token, conversation, "ceiling track").await;
    let shown = text(&result);

    assert_ne!(result["isError"], true, "{result}");
    assert!(shown.contains("image_url: https://images.example.org/a.png") && shown.contains("page_url: https://shop.example.com/track") && shown.contains("host: images.example.org"), "{shown}");
    assert!(shown.contains("![Ceiling track](https://images.example.org/a.png \"https://shop.example.com/track\")"), "{shown}");
    assert!(shown.contains("![Has brackets](https://cdn.example.net/d.jpg)"), "{shown}");
    assert_eq!(
        recorded_images(&server, conversation).await,
        [("https://cdn.example.net/d.jpg".to_string(), None), ("https://images.example.org/a.png".to_string(), Some("https://shop.example.com/track".to_string()))]
    );
    assert!(recorded_images(&server, other).await.is_empty());
}

#[tokio::test]
async fn results_that_cannot_be_written_as_an_exact_https_markdown_image_are_left_out() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_search(true).await;
    add_search(&server, &base).await;
    let conversation = conversation_of(&server, &node.token).await;

    let shown = text(&search_images(&server, &node.token, conversation, "ceiling track").await);

    assert!(!shown.contains("b.png") && !shown.contains("c d.png") && !shown.contains("e_(1)"), "{shown}");
}

#[tokio::test]
async fn a_failed_search_and_an_empty_search_record_no_images() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_search(true).await;
    add_search(&server, &base).await;
    let failed = conversation_of(&server, &node.token).await;
    let empty = conversation_of(&server, &node.token).await;

    let broken = search_images(&server, &node.token, failed, "boom").await;
    let none = search_images(&server, &node.token, empty, "nothing").await;

    assert_eq!(broken["isError"], true, "{broken}");
    assert!(text(&none).starts_with("No images were found"));
    assert!(recorded_images(&server, failed).await.is_empty() && recorded_images(&server, empty).await.is_empty());
}

#[tokio::test]
async fn a_declined_search_records_no_images_and_an_approved_one_does() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_search(true).await;
    let (_, added) = add_search(&server, &base).await;
    let id = added["id"].as_str().unwrap();
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"policy": {"preset": "ask_all"}}))).await;
    let declined = conversation_of(&server, &node.token).await;
    let approved = conversation_of(&server, &node.token).await;
    let asked = fake.seen.lock().unwrap().len();

    let (refusal, ()) = tokio::join!(search_images(&server, &node.token, declined, "ceiling track"), async {
        let request = server.open_request(declined).await;

        server.answer(declined, &request, "deny").await;
    });
    let (answer, ()) = tokio::join!(search_images(&server, &node.token, approved, "ceiling track"), async {
        let request = server.open_request(approved).await;

        server.answer(approved, &request, "approve").await;
    });

    assert_eq!(refusal["isError"], true, "{refusal}");
    assert_ne!(answer["isError"], true, "{answer}");
    assert_eq!(fake.seen.lock().unwrap().len(), asked + 1);
    assert!(recorded_images(&server, declined).await.is_empty());
    assert_eq!(recorded_images(&server, approved).await.len(), 2);
}

#[tokio::test]
async fn a_result_the_person_chose_not_to_send_or_edited_a_url_out_of_records_only_what_was_sent() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_search(true).await;
    let (_, added) = add_search(&server, &base).await;
    let id = added["id"].as_str().unwrap();
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"policy": {"preset": "ask_all_edit"}}))).await;
    let withheld = conversation_of(&server, &node.token).await;
    let edited = conversation_of(&server, &node.token).await;

    searched_and_answered(&server, &node.token, withheld, Value::Null).await;
    searched_and_answered(&server, &node.token, edited, json!("Only this one: https://cdn.example.net/d.jpg")).await;

    assert!(recorded_images(&server, withheld).await.is_empty());
    assert_eq!(recorded_images(&server, edited).await, [("https://cdn.example.net/d.jpg".to_string(), None)]);
}

async fn searched_and_answered(server: &Server, token: &str, conversation: Uuid, send: Value) {
    let (_, ()) = tokio::join!(search_images(server, token, conversation, "ceiling track"), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "approve_edit").await;

        let held = server_held(server, conversation).await;
        let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation}/responses/{held}")).json(&json!({"send": send}))).await;

        assert_eq!(status, 204);
    });
}

async fn server_held(server: &Server, conversation: Uuid) -> String {
    crate::common::eventually("a review request", || async {
        let events = server.events(conversation).await;
        let open = events.iter().find(|event| event["kind"] == "review_request" && !events.iter().any(|other| other["kind"] == "review_resolved" && other["payload"]["request_id"] == event["payload"]["request_id"]));

        open.map(|event| event["payload"]["request_id"].as_str().unwrap().to_string())
    })
    .await
}

async fn fake_tandoor_with_an_image_url() -> String {
    let router = Router::new()
        .route("/api/space/", get(|| async { Json(json!([{"id": 1, "name": "Home"}])) }))
        .route("/api/user/", get(|| async { Json(json!([{"id": 1, "username": "sam", "display_name": "Sam"}])) }))
        .route("/api/recipe/", get(|| async { Json(json!({"count": 1, "results": [{"id": 3, "name": "Dal at https://images.example.org/dal.png", "description": "", "servings": 4, "keywords": []}]})) }));

    serve(router).await
}

#[tokio::test]
async fn an_integration_other_than_web_search_never_makes_an_image_url_trusted() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let base = fake_tandoor_with_an_image_url().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": "tandoor-token-1234"}))).await;
    let conversation = conversation_of(&server, &node.token).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "tandoor__search_recipes", "arguments": {"query": "dal"}}});

    let result = server.agent_mcp(&node.token, conversation, body).await["result"].clone();

    assert!(text(&result).contains("https://images.example.org/dal.png"), "{result}");
    assert!(recorded_images(&server, conversation).await.is_empty());
}
