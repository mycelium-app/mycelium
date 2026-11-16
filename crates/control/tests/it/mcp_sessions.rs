use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Remote {
    methods: Mutex<Vec<String>>,
    issued: Mutex<u32>,
    expired: Mutex<HashSet<String>>,
}

impl Remote {
    fn count(&self, method: &str) -> usize {
        self.methods.lock().unwrap().iter().filter(|seen| *seen == method).count()
    }

    fn forget_seen(&self) {
        self.methods.lock().unwrap().clear();
    }

    fn expire(&self, session: &str) {
        self.expired.lock().unwrap().insert(session.to_string());
    }
}

fn result_for(method: &str, body: &Value) -> Option<Value> {
    match method {
        "initialize" => Some(json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "lab", "version": "1"}})),
        "tools/list" => Some(json!({"tools": [{"name": "get_weather", "description": "Get the weather.", "inputSchema": {"type": "object", "properties": {"city": {"type": "string"}}}}]})),
        "tools/call" => Some(json!({"content": [{"type": "text", "text": format!("Sunny in {}", body["params"]["arguments"]["city"].as_str().unwrap_or("nowhere"))}]})),
        _ => None,
    }
}

fn session_server() -> (Router, Arc<Remote>) {
    async fn handle(State(remote): State<Arc<Remote>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
        let method = body["method"].as_str().unwrap_or_default().to_string();
        remote.methods.lock().unwrap().push(method.clone());

        if body["params"].get("_meta").is_some() {
            return StatusCode::BAD_REQUEST.into_response();
        }

        if method == "initialize" {
            let mut issued = remote.issued.lock().unwrap();
            *issued += 1;
            let answer = json!({"jsonrpc": "2.0", "id": body["id"], "result": result_for(&method, &body)});

            return Response::builder().header("content-type", "application/json").header("mcp-session-id", format!("sess-{issued}")).body(Body::from(answer.to_string())).unwrap();
        }

        let Some(session) = headers.get("mcp-session-id").and_then(|value| value.to_str().ok()).map(str::to_string) else {
            return StatusCode::BAD_REQUEST.into_response();
        };

        if remote.expired.lock().unwrap().contains(&session) {
            return StatusCode::NOT_FOUND.into_response();
        }

        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let Some(result) = result_for(&method, &body) else {
            return StatusCode::NOT_FOUND.into_response();
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    let remote = Arc::new(Remote::default());

    (Router::new().route("/mcp", post(handle)).with_state(remote.clone()), remote)
}

fn stateless_server() -> (Router, Arc<Remote>) {
    async fn handle(State(remote): State<Arc<Remote>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
        let method = body["method"].as_str().unwrap_or_default().to_string();
        remote.methods.lock().unwrap().push(method.clone());

        if headers.get("mcp-method").and_then(|value| value.to_str().ok()) != Some(method.as_str()) || body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] != "2026-07-28" {
            return (StatusCode::BAD_REQUEST, Json(json!({"jsonrpc": "2.0", "id": body["id"], "error": {"code": -32020, "message": "Header mismatch"}}))).into_response();
        }

        let Some(result) = result_for(&method, &body) else {
            return StatusCode::NOT_FOUND.into_response();
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    let remote = Arc::new(Remote::default());

    (Router::new().route("/mcp", post(handle)).with_state(remote.clone()), remote)
}

async fn add_lab(server: &Server, router: Router) {
    let base = serve(router).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Lab", "url": format!("{base}/mcp")}))).await;
    assert_eq!(status, 201, "{added}");
}

async fn weather(server: &Server, token: &str, city: &str) -> String {
    let anonymous = server.anonymous();
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "lab__get_weather", "arguments": {"city": city}}});
    let (status, called) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{called}");
    assert_eq!(called["result"]["isError"], false, "{called}");

    called["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn consecutive_calls_to_a_session_server_initialise_once() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (router, remote) = session_server();
    add_lab(&server, router).await;
    remote.forget_seen();

    assert_eq!(weather(&server, &token, "Oslo").await, "Sunny in Oslo");
    assert_eq!(weather(&server, &token, "Paris").await, "Sunny in Paris");

    assert_eq!(remote.count("initialize"), 1);
    assert_eq!(remote.count("tools/list"), 1);
    assert_eq!(remote.count("tools/call"), 2);
}

#[tokio::test]
async fn an_expired_session_is_initialised_again_once_and_the_call_succeeds() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (router, remote) = session_server();
    add_lab(&server, router).await;
    assert_eq!(weather(&server, &token, "Oslo").await, "Sunny in Oslo");
    let issued = *remote.issued.lock().unwrap();
    remote.expire(&format!("sess-{issued}"));
    remote.forget_seen();

    assert_eq!(weather(&server, &token, "Paris").await, "Sunny in Paris");

    assert_eq!(remote.count("initialize"), 1);
    assert_eq!(remote.count("tools/call"), 2);
}

#[tokio::test]
async fn a_stateless_server_is_never_sent_initialize() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (router, remote) = stateless_server();
    add_lab(&server, router).await;
    remote.forget_seen();

    assert_eq!(weather(&server, &token, "Oslo").await, "Sunny in Oslo");
    assert_eq!(weather(&server, &token, "Paris").await, "Sunny in Paris");

    assert_eq!(remote.count("initialize"), 0);
    assert_eq!(remote.count("tools/list"), 1);
    assert_eq!(remote.count("tools/call"), 2);
}
