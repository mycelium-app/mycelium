use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "tandoor-token-1234";

#[derive(Default)]
struct FakeTandoor {
    posted: Mutex<Vec<(String, Value)>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

fn refuse() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"detail": "Authentication credentials were not provided."}))).into_response()
}

async fn space(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    Json(json!([{"id": 1, "name": "Home kitchen"}])).into_response()
}

async fn users(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    Json(json!([{"id": 1, "username": "sam", "display_name": "Sam"}])).into_response()
}

async fn recipes(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return refuse();
    }

    Json(json!({"count": 42, "results": [{"id": 3, "name": "Dal", "description": "", "servings": 4, "keywords": []}]})).into_response()
}

async fn create_recipe(State(fake): State<Arc<FakeTandoor>>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push(("recipe".to_string(), body.clone()));

    Json(json!({"id": 9, "name": body["name"], "description": "", "servings": 1, "keywords": []}))
}

async fn meal_types() -> Json<Value> {
    Json(json!([{"id": 2, "name": "Dinner"}]))
}

async fn shopping() -> Json<Value> {
    Json(json!({"results": [{"id": 1, "food": {"name": "Rice"}, "amount": 1, "unit": null, "checked": false}, {"id": 2, "food": {"name": "Salt"}, "amount": 1, "unit": null, "checked": true}]}))
}

async fn check(State(fake): State<Arc<FakeTandoor>>, Path(id): Path<u64>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push((format!("entry {id}"), body.clone()));

    Json(json!({"id": id, "food": {"name": "Rice"}, "amount": 1, "unit": null, "checked": body["checked"]}))
}

async fn fake_tandoor() -> (String, Arc<FakeTandoor>) {
    let fake = Arc::new(FakeTandoor::default());
    let router = Router::new()
        .route("/api/space/", get(space))
        .route("/api/user/", get(users))
        .route("/api/recipe/", get(recipes).post(create_recipe))
        .route("/api/meal-type/", get(meal_types))
        .route("/api/shopping-list-entry/", get(shopping))
        .route("/api/shopping-list-entry/{id}/", patch(check))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("tandoor__{tool}"), "arguments": arguments}});
    let asks = matches!(tool, "create_recipe" | "plan_meal" | "add_to_shopping_list" | "check_off_item");

    if asks {
        return server.approved_agent_mcp(token, body).await["result"].clone();
    }

    let anonymous = server.anonymous();
    let (status, reply) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{reply}");

    reply["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_setup_test_names_the_space_the_user_and_the_recipe_count() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;

    let (status, found) = server.user.json(server.user.post("/api/v1/chat-tool-servers/tandoor-test").json(&json!({"url": base, "token": TOKEN}))).await;

    assert_eq!(status, 200, "{found}");
    assert_eq!((found["space"].clone(), found["user"].clone(), found["recipes"].clone()), (json!("Home kitchen"), json!("Sam"), json!(42)));
}

#[tokio::test]
async fn the_setup_test_says_how_to_fix_a_bad_token_an_unreachable_server_and_a_bad_address() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;
    let test = |url: &str, token: &str| server.user.json(server.user.post("/api/v1/chat-tool-servers/tandoor-test").json(&json!({"url": url, "token": token})));

    let (refused, token) = test(&base, "wrong").await;
    let (unreachable, gone) = test("http://127.0.0.1:9", TOKEN).await;
    let (invalid, address) = test("recipes", TOKEN).await;

    assert_eq!((refused, token["error"].clone()), (400, json!("tandoor_token")));
    assert!(token["message"].as_str().unwrap().contains("did not accept the API token. Check the token has the scope read write and has not expired, or create a new one in Tandoor under Settings, then API."));
    assert_eq!((unreachable, gone["error"].clone()), (400, json!("tandoor_address")));
    assert!(gone["message"].as_str().unwrap().starts_with("Could not reach 127.0.0.1. Check the address and that Tandoor is running."));
    assert_eq!((invalid, address["message"].clone()), (400, json!("Enter the server address, like https://recipes.example.com")));
}

#[tokio::test]
async fn the_token_is_stored_but_never_returned() {
    let server = Server::start().await;
    let (base, _) = fake_tandoor().await;

    let (status, _) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 201);
    assert!(!stored.contains(TOKEN));
}

#[tokio::test]
async fn the_agent_looks_recipes_and_the_shopping_list_up_without_asking() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_tandoor().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;

    let found = text(&call(&server, &node.token, "search_recipes", json!({"query": "dal"})).await);
    let list = text(&call(&server, &node.token, "get_shopping_list", json!({})).await);

    assert!(found.contains("Dal") && found.contains("42"), "{found}");
    assert!(list.contains("Rice") && !list.contains("Salt"), "{list}");
}

#[tokio::test]
async fn changing_anything_asks_first_and_sends_what_was_approved() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_tandoor().await;
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Tandoor", "kind": "tandoor", "url": base, "bearer_token": TOKEN}))).await;

    let created = call(&server, &node.token, "create_recipe", json!({"name": "Flapjacks", "ingredients": [{"food": "Oats", "amount": 200, "unit": "g"}]})).await;
    let checked = call(&server, &node.token, "check_off_item", json!({"entry": 1})).await;
    let posted = fake.posted.lock().unwrap().clone();

    assert_ne!(created["isError"], true, "{created}");
    assert_ne!(checked["isError"], true, "{checked}");
    assert_eq!(posted[0].1["steps"][0]["ingredients"][0]["food"], json!({"name": "Oats"}));
    assert_eq!(posted[1], ("entry 1".to_string(), json!({"checked": true})));
}
