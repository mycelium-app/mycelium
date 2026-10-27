mod common;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const KEY: &str = "jev-key-1234";

#[derive(Default)]
struct FakeJev {
    asked: Mutex<Vec<Value>>,
    fail_with: Mutex<Option<u16>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

async fn models(State(fake): State<Arc<FakeJev>>, headers: HeaderMap) -> Response {
    let _ = fake;

    if !authorised(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"detail": {"error_type": "auth", "message": "no"}}))).into_response();
    }

    Json(json!({"data": [{"id": "openjev-0.1"}, {"id": "diffusiongemma-26b"}]})).into_response()
}

async fn systemone(State(fake): State<Arc<FakeJev>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    fake.asked.lock().unwrap().push(body.clone());

    if let Some(code) = *fake.fail_with.lock().unwrap() {
        let mut response = (StatusCode::from_u16(code).unwrap(), "busy").into_response();
        response.headers_mut().insert("retry-after", "7".parse().unwrap());

        return response;
    }

    if !authorised(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    Json(json!({"model": body["model"], "answers": {"urgent": {"noul": 0.97}}, "usage": {"input_tokens": 10, "output_tokens": 0}})).into_response()
}

async fn fake_jev() -> (String, Arc<FakeJev>) {
    let fake = Arc::new(FakeJev::default());
    let base = serve(Router::new().route("/v1/models", get(models)).route("/v1/systemone", post(systemone)).with_state(fake.clone())).await;

    (base, fake)
}

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn decide(server: &Server, token: &str) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "decide", "arguments": {"state": "Customer wrote: where is my refund?", "questions": {"urgent": {"type": "noul", "instructions": "Does this need a reply within the hour?"}}}}});

    mcp(server, token, body).await["result"].clone()
}

async fn add(server: &Server, base: &str, name: &str, uses: Value) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/decision-providers").json(&json!({"name": name, "base_url": base, "api_key": KEY, "model": "openjev-0.1", "uses": uses}))).await
}

#[tokio::test]
async fn testing_a_connection_lists_the_models_and_a_wrong_key_says_so() {
    let server = Server::start().await;
    let (base, _) = fake_jev().await;

    let (status, listed) = server.user.json(server.user.post("/api/v1/decision-providers/test").json(&json!({"base_url": base, "key": KEY}))).await;
    let (wrong_status, wrong) = server.user.json(server.user.post("/api/v1/decision-providers/test").json(&json!({"base_url": base, "key": "nope"}))).await;

    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["models"], json!(["openjev-0.1", "diffusiongemma-26b"]));
    assert_eq!(wrong_status, 400);
    assert!(wrong["message"].as_str().unwrap().contains("refused that key"), "{wrong}");
}

#[tokio::test]
async fn saving_a_use_moves_it_from_the_provider_that_had_it() {
    let server = Server::start().await;
    let (base, _) = fake_jev().await;
    let (_, first) = add(&server, &base, "Home", json!(["decide"])).await;
    let (status, second) = add(&server, &base, "Cloud", json!(["decide"])).await;
    let listed = server.user.get("/api/v1/decision-providers").send().await.unwrap().text().await.unwrap();
    let listed: Value = serde_json::from_str(&listed).unwrap();

    assert_eq!(status, 201, "{second}");
    assert_eq!(listed[0]["id"], first["id"]);
    assert_eq!(listed[0]["uses"], json!([]));
    assert_eq!(listed[1]["uses"], json!(["decide"]));
    assert!(!listed.to_string().contains(KEY));
}

#[tokio::test]
async fn an_unknown_use_is_refused_with_the_choices() {
    let server = Server::start().await;
    let (base, _) = fake_jev().await;

    let (status, body) = add(&server, &base, "Home", json!(["gossip"])).await;

    assert_eq!(status, 400);
    assert!(body["message"].as_str().unwrap().contains("decide"), "{body}");
}

#[tokio::test]
async fn the_decide_tool_asks_the_provider_and_returns_its_answers() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_jev().await;
    add(&server, &base, "Home", json!(["decide"])).await;

    let result = decide(&server, &node.token).await;
    let asked = fake.asked.lock().unwrap().clone();

    assert_eq!(result["isError"], json!(false), "{result}");
    assert!(result["content"][0]["text"].as_str().unwrap().contains("0.97"));
    assert_eq!(asked[0]["model"], "openjev-0.1");
    assert_eq!(asked[0]["questions"]["urgent"]["type"], "noul");
}

#[tokio::test]
async fn the_decide_tool_explains_each_failure_in_plain_words() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_jev().await;

    let without_provider = decide(&server, &node.token).await;
    add(&server, &base, "Home", json!(["decide"])).await;
    *fake.fail_with.lock().unwrap() = Some(503);
    let busy = decide(&server, &node.token).await;
    *fake.fail_with.lock().unwrap() = Some(529);
    let overloaded = decide(&server, &node.token).await;

    assert!(without_provider["content"][0]["text"].as_str().unwrap().contains("Settings, Providers"));
    assert!(busy["content"][0]["text"].as_str().unwrap().contains("in 7 seconds"));
    assert!(overloaded["content"][0]["text"].as_str().unwrap().contains("overloaded"));
    assert_eq!(busy["isError"], json!(true));
}
