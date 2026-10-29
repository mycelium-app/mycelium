use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use crate::common::{NodeLink, Server, hash, serve};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn fake_server() -> Router {
    gateway_server("authorization")
}

fn gateway_server(key_header: &'static str) -> Router {
    async fn handle(State(key_header): State<&'static str>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
        let key_in_its_own_header = headers.contains_key(key_header) && (key_header == "authorization" || !headers.contains_key("authorization"));

        if key_header != "authorization" && !key_in_its_own_header {
            return StatusCode::UNAUTHORIZED.into_response();
        }

        if body["params"].get("_meta").is_some() {
            return StatusCode::BAD_REQUEST.into_response();
        }

        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "lab", "version": "1"}}),
            "tools/list" => json!({"tools": [{"name": "get_weather", "description": "Get the weather.", "inputSchema": {"type": "object", "properties": {"city": {"type": "string"}}}}]}),
            "tools/call" => json!({"content": [{"type": "text", "text": format!("Sunny in {}", body["params"]["arguments"]["city"].as_str().unwrap_or("nowhere"))}]}),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    Router::new().route("/mcp", post(handle)).route("/mcp/", post(handle)).with_state(key_header)
}

async fn own_session_token(server: &Server) -> String {
    let (node, _) = server.join("node", "laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select created_by from nodes where id = $1").bind(node).fetch_one(&server.pool).await.unwrap();
    let conversation = sqlx::query_scalar::<_, Uuid>("insert into conversations (owner, node_id, agent, cwd, title, status) values ($1, $2, 'claude', '/work/app', 'Mine', 'done') returning id").bind(owner).bind(node).fetch_one(&server.pool).await.unwrap();
    let secret = "own-session-secret";
    sqlx::query("insert into mcp_session_tokens (token_hash, conversation_id, node_id) values ($1, $2, $3)").bind(hash(secret)).bind(conversation).bind(node).execute(&server.pool).await.unwrap();

    format!("myc_session_{secret}")
}

async fn add_lab(server: &Server) -> (String, Uuid) {
    let base = serve(fake_server()).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Lab", "url": format!("{base}/mcp")}))).await;
    assert_eq!(status, 201, "{added}");

    (base, added["id"].as_str().unwrap().parse().unwrap())
}

fn names(listed: &Value) -> Vec<String> {
    listed["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn initialize_advertises_list_changed_and_issues_a_session_id() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let body = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})).await;

    assert_eq!(body["result"]["capabilities"]["tools"]["listChanged"], true);
}

#[tokio::test]
async fn tools_list_includes_a_connected_servers_tools_under_a_prefixed_name_and_calls_are_forwarded() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    add_lab(&server).await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    assert!(names(&listed).contains(&"lab__get_weather".to_string()));
    assert!(names(&listed).contains(&"recall".to_string()));

    let tool = listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "lab__get_weather").unwrap();
    assert!(tool["description"].as_str().unwrap().starts_with("Lab: Get the weather. Any text argument can be {\"file\""));

    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "lab__get_weather", "arguments": {"city": "Oslo"}}})).await;
    assert_eq!(called["result"]["content"][0]["text"], "Sunny in Oslo");
    assert_eq!(called["result"]["isError"], false);
}

#[tokio::test]
async fn workspace_agents_list_and_call_litellm_tools_through_mycelium() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let base = serve(gateway_server("x-litellm-api-key")).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Lab", "kind": "litellm", "url": base, "bearer_token": "sk-lite"}))).await;
    assert_eq!(status, 201, "{added}");

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "lab__get_weather", "arguments": {"city": "Oslo"}}})).await;

    assert!(names(&listed).contains(&"lab__get_weather".to_string()));
    assert_eq!(called["result"]["content"][0]["text"], "Sunny in Oslo");
}

#[tokio::test]
async fn a_disabled_server_is_left_out_and_calling_it_says_why() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (_, id) = add_lab(&server).await;
    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"enabled": false}))).await;
    assert_eq!(status, 200);

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    assert!(!names(&listed).contains(&"lab__get_weather".to_string()));

    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "lab__get_weather", "arguments": {}}})).await;
    assert_eq!(called["result"]["isError"], true);
    assert!(called["result"]["content"][0]["text"].as_str().unwrap().contains("Lab is switched off"));
}

#[tokio::test]
async fn an_open_stream_is_told_when_a_tool_server_is_added() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let anonymous = server.anonymous();
    let mut stream = anonymous.http.get(format!("{}/api/v1/mcp", anonymous.base)).header("authorization", format!("Bearer {token}")).header("accept", "text/event-stream").send().await.unwrap();
    assert_eq!(stream.status(), 200);
    assert!(stream.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));

    add_lab(&server).await;

    let mut received = String::new();

    while !received.contains("notifications/tools/list_changed") {
        let chunk = tokio::time::timeout(Duration::from_secs(10), stream.chunk()).await.expect("no notification arrived").unwrap().expect("the stream closed");
        received.push_str(&String::from_utf8_lossy(&chunk));
    }

    assert!(received.contains(r#""jsonrpc":"2.0""#));
}

#[tokio::test]
async fn an_open_stream_is_told_when_a_server_is_switched_back_on() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let (_, lab) = add_lab(&server).await;
    let anonymous = server.anonymous();
    let mut stream = anonymous.http.get(format!("{}/api/v1/mcp", anonymous.base)).header("authorization", format!("Bearer {token}")).header("accept", "text/event-stream").send().await.unwrap();
    let path = format!("/api/v1/chat-tool-servers/{lab}");

    server.user.json(server.user.patch(&path).json(&json!({"enabled": false}))).await;
    let mut received = String::new();

    while received.matches("notifications/tools/list_changed").count() < 1 {
        let chunk = tokio::time::timeout(Duration::from_secs(10), stream.chunk()).await.expect("no notification arrived").unwrap().expect("the stream closed");
        received.push_str(&String::from_utf8_lossy(&chunk));
    }

    server.user.json(server.user.patch(&path).json(&json!({"enabled": true}))).await;

    while received.matches("notifications/tools/list_changed").count() < 2 {
        let chunk = tokio::time::timeout(Duration::from_secs(10), stream.chunk()).await.expect("switching back on sent no notification").unwrap().expect("the stream closed");
        received.push_str(&String::from_utf8_lossy(&chunk));
    }
}

#[tokio::test]
async fn find_and_call_integration_tool_reach_a_server_added_after_tools_list() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let before = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    add_lab(&server).await;

    let found = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "find_integration_tools", "arguments": {"query": "weather"}}})).await;
    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "call_integration_tool", "arguments": {"name": "lab__get_weather", "arguments": {"city": "Paris"}}}})).await;
    let wrong = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "call_integration_tool", "arguments": {"name": "recall", "arguments": {"query": "x"}}}})).await;

    assert!(names(&before).iter().all(|name| !name.starts_with("lab__")));
    assert!(names(&before).contains(&"find_integration_tools".to_string()));
    assert!(found["result"]["content"][0]["text"].as_str().unwrap().contains("lab__get_weather"));
    assert_eq!(called["result"]["content"][0]["text"], "Sunny in Paris");
    assert_eq!(wrong["result"]["isError"], true);
}

async fn session_token_from_start(link: &mut NodeLink) -> String {
    loop {
        if let ControlToNode::StartSession { mcp_token, .. } = link.next().await {
            return mcp_token;
        }
    }
}

async fn tools_with_header(server: &Server, token: &str, named_conversation: Option<Uuid>) -> (u16, Value) {
    let anonymous = server.anonymous();
    let mut request = anonymous.http.post(format!("{}/api/v1/mcp", anonymous.base)).header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));

    if let Some(conversation) = named_conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    let response = request.send().await.unwrap();
    let status = response.status().as_u16();

    (status, response.json().await.unwrap_or(Value::Null))
}

async fn conversation_of_another_user(server: &Server, node: Uuid) -> (Uuid, String) {
    let _ = server.second_user("other@example.com").await;
    let other = sqlx::query_scalar::<_, Uuid>("select id from users where email = 'other@example.com'").fetch_one(&server.pool).await.unwrap();
    let conversation = sqlx::query_scalar::<_, Uuid>("insert into conversations (owner, node_id, agent, cwd, title, status) values ($1, $2, 'claude', '/work/app', 'Other', 'done') returning id").bind(other).bind(node).fetch_one(&server.pool).await.unwrap();
    let secret = "other-session-secret";
    sqlx::query("insert into mcp_session_tokens (token_hash, conversation_id, node_id) values ($1, $2, $3)").bind(hash(secret)).bind(conversation).bind(node).execute(&server.pool).await.unwrap();

    (conversation, format!("myc_session_{secret}"))
}

#[tokio::test]
async fn the_node_token_is_refused_on_the_mcp_endpoint() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let (status, _) = tools_with_header(&server, &token, None).await;
    let anonymous = server.anonymous();
    let stream = anonymous.http.get(format!("{}/api/v1/mcp", anonymous.base)).header("authorization", format!("Bearer {token}")).header("accept", "text/event-stream").send().await.unwrap();

    assert_eq!((status, stream.status().as_u16()), (401, 401));
}

#[tokio::test]
async fn a_started_session_is_given_a_token_that_works_on_the_mcp_endpoint() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    server.create_conversation(link.id, None).await;
    let token = session_token_from_start(&mut link).await;

    let (status, listed) = tools_with_header(&server, &token, None).await;

    assert_eq!(status, 200, "{listed}");
    assert!(names(&listed).contains(&"recall".to_string()));
}

#[tokio::test]
async fn a_session_token_ignores_the_conversation_a_request_names() {
    let server = Server::start().await;
    add_lab(&server).await;
    let mut link = server.online_node("laptop").await;
    server.create_conversation(link.id, None).await;
    let own = session_token_from_start(&mut link).await;
    let (other_conversation, _) = conversation_of_another_user(&server, link.id).await;

    let (_, listed) = tools_with_header(&server, &own, Some(other_conversation)).await;

    assert!(names(&listed).contains(&"lab__get_weather".to_string()));
}

#[tokio::test]
async fn tools_act_for_the_conversations_owner_not_the_node_creator() {
    let server = Server::start().await;
    add_lab(&server).await;
    let link = server.online_node("laptop").await;
    let (_, other_token) = conversation_of_another_user(&server, link.id).await;

    let (status, listed) = tools_with_header(&server, &other_token, None).await;

    assert_eq!(status, 200, "{listed}");
    assert!(!names(&listed).contains(&"lab__get_weather".to_string()));
}
