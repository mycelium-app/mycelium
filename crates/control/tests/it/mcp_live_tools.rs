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
async fn a_tool_whose_input_schema_is_a_reference_is_listed_as_the_object_it_names() {
    async fn handle(Json(body): Json<Value>) -> Response {
        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "notes", "version": "1"}}),
            "tools/list" => json!({"tools": [{"name": "create_note", "description": "Create a note.", "inputSchema": {
                "$schema": "http://json-schema.org/draft-07/schema#",
                "$ref": "#/definitions/CreateNote",
                "definitions": {"CreateNote": {"type": "object", "properties": {"title": {"type": "string"}, "tag": {"$ref": "#/definitions/Tag"}}, "required": ["title"]}, "Tag": {"type": "string"}},
            }}]}),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let base = serve(Router::new().route("/mcp", post(handle))).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Notes", "url": format!("{base}/mcp")}))).await;
    assert_eq!(status, 201, "{added}");

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let tool = listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "notes__create_note").unwrap();
    let schema = &tool["inputSchema"];

    assert_eq!(schema["type"], "object");
    assert!(schema.get("$ref").is_none());
    assert_eq!(schema["required"], json!(["title"]));
    assert_eq!(schema["properties"]["title"], json!({"type": "string"}));
    assert_eq!(schema["properties"]["tag"], json!({"$ref": "#/definitions/Tag"}));
    assert_eq!(schema["definitions"]["Tag"], json!({"type": "string"}));
    assert!(schema["properties"].get("response_filter").is_some());
}

#[tokio::test]
async fn a_tool_whose_input_schema_is_not_an_object_is_left_out_and_the_rest_are_still_listed() {
    async fn handle(Json(body): Json<Value>) -> Response {
        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "notes", "version": "1"}}),
            "tools/list" => json!({"tools": [
                {"name": "create_note", "description": "Create a note.", "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}}}},
                {"name": "broken_note", "description": "Has a string schema.", "inputSchema": {"type": "string"}},
                {"name": "typeless_note", "description": "Has no schema type.", "inputSchema": {"properties": {}}},
            ]}),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let base = serve(Router::new().route("/mcp", post(handle))).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Notes", "url": format!("{base}/mcp")}))).await;
    assert_eq!(status, 201, "{added}");

    let listed = names(&mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await);

    assert!(listed.contains(&"notes__create_note".to_string()));
    assert!(listed.contains(&"recall".to_string()));
    assert!(!listed.contains(&"notes__broken_note".to_string()));
    assert!(!listed.contains(&"notes__typeless_note".to_string()));
}

#[tokio::test]
async fn a_client_carrying_a_session_id_this_server_never_issued_can_list_and_call_tools() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let anonymous = server.anonymous();

    let (status, listed) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).header("mcp-session-id", "issued-before-the-restart").json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;

    assert_eq!(status, 200, "{listed}");
    assert!(names(&listed).contains(&"recall".to_string()));
}

#[tokio::test]
async fn a_stream_opened_after_a_restart_is_told_at_once_to_fetch_the_tools_again() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let anonymous = server.anonymous();
    let mut stream = anonymous.http.get(format!("{}/api/v1/mcp", anonymous.base)).header("authorization", format!("Bearer {token}")).header("accept", "text/event-stream").header("mcp-session-id", "issued-before-the-restart").send().await.unwrap();

    let chunk = tokio::time::timeout(Duration::from_secs(10), stream.chunk()).await.expect("no notification arrived").unwrap().expect("the stream closed");

    assert!(String::from_utf8_lossy(&chunk).contains("notifications/tools/list_changed"));
}

#[tokio::test]
async fn a_session_that_never_lists_its_tools_gets_a_notice_in_the_conversation() {
    let server = Server::start_with(|config| config.tools_listing_grace = Duration::from_millis(200)).await;
    let mut link = server.online_node("laptop").await;
    let id = server.create_conversation(link.id, None).await.id;
    let _ = session_token_from_start(&mut link).await;

    crate::common::eventually("the notice to be stored", || async { server.events(id).await.iter().any(|event| event["kind"] == "session_notice" && event["payload"]["message"].as_str().is_some_and(|message| message.contains("started without Mycelium's tools"))).then_some(()) }).await;
}

#[tokio::test]
async fn a_session_that_lists_its_tools_gets_no_notice() {
    let server = Server::start_with(|config| config.tools_listing_grace = Duration::from_millis(300)).await;
    let mut link = server.online_node("laptop").await;
    let id = server.create_conversation(link.id, None).await.id;
    let token = session_token_from_start(&mut link).await;
    let (status, _) = tools_with_header(&server, &token, None).await;
    assert_eq!(status, 200);

    tokio::time::sleep(Duration::from_millis(900)).await;

    assert!(server.events(id).await.iter().all(|event| event["kind"] != "session_notice"));
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
async fn renaming_a_server_changes_its_title_and_leaves_the_prefix_agents_use_alone() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (_, id) = add_lab(&server).await;
    let (status, renamed) = server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Weather"}))).await;
    assert_eq!((status, renamed["name"].clone()), (200, json!("Weather")));

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "lab__get_weather", "arguments": {"city": "Oslo"}}})).await;
    let (_, servers) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert!(names(&listed).contains(&"lab__get_weather".to_string()));
    assert!(!names(&listed).contains(&"weather__get_weather".to_string()));
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(servers[0]["name"], "Weather");
}

#[tokio::test]
async fn a_new_server_never_takes_the_prefix_a_renamed_one_still_uses() {
    let server = Server::start().await;
    let (_, id) = add_lab(&server).await;
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Weather"}))).await;
    let base = serve(fake_server()).await;
    let (status, _) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Lab", "kind": "litellm", "url": base, "bearer_token": "sk-lite"}))).await;
    assert_eq!(status, 201);
    let (_, token) = server.join("node", "laptop").await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;

    assert!(names(&listed).contains(&"lab__get_weather".to_string()));
    assert!(names(&listed).contains(&"lab_2__get_weather".to_string()));
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

    while received.matches("notifications/tools/list_changed").count() < 2 {
        let chunk = tokio::time::timeout(Duration::from_secs(10), stream.chunk()).await.expect("no notification arrived").unwrap().expect("the stream closed");
        received.push_str(&String::from_utf8_lossy(&chunk));
    }

    server.user.json(server.user.patch(&path).json(&json!({"enabled": true}))).await;

    while received.matches("notifications/tools/list_changed").count() < 3 {
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

fn sha256_hex(secret: &str) -> String {
    hash(secret).iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn mcp_status(server: &Server, token: &str) -> u16 {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))).await.0
}

#[tokio::test]
async fn a_session_token_revoked_by_its_digest_no_longer_reaches_the_tools() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    assert_eq!(mcp_status(&server, &token).await, 200);

    let (status, body) = server.user.json(server.user.post("/api/v1/mcp-session-tokens/revoke").json(&json!({"sha256": sha256_hex("own-session-secret")}))).await;

    assert_eq!((status, body), (200, json!({"revoked": 1})));
    assert_eq!(mcp_status(&server, &token).await, 401);
}

#[tokio::test]
async fn a_session_token_of_another_persons_conversation_is_left_alone() {
    let server = Server::start().await;
    let (node, _) = server.join("node", "laptop").await;
    let (_, token) = conversation_of_another_user(&server, node).await;

    let (status, body) = server.user.json(server.user.post("/api/v1/mcp-session-tokens/revoke").json(&json!({"sha256": sha256_hex("other-session-secret")}))).await;

    assert_eq!((status, body), (200, json!({"revoked": 0})));
    assert_eq!(mcp_status(&server, &token).await, 200);
}

#[tokio::test]
async fn every_session_token_of_a_conversation_can_be_revoked_at_once() {
    let server = Server::start().await;
    let token = own_session_token(&server).await;
    let conversation = sqlx::query_scalar::<_, Uuid>("select conversation_id from mcp_session_tokens").fetch_one(&server.pool).await.unwrap();

    let (status, body) = server.user.json(server.user.post("/api/v1/mcp-session-tokens/revoke").json(&json!({"conversation_id": conversation}))).await;

    assert_eq!((status, body), (200, json!({"revoked": 1})));
    assert_eq!(mcp_status(&server, &token).await, 401);
}

#[tokio::test]
async fn a_digest_that_is_not_sha256_hex_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;

    let (status, body) = server.user.json(server.user.post("/api/v1/mcp-session-tokens/revoke").json(&json!({"sha256": "myc_session_abc"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("invalid_sha256")));
}
