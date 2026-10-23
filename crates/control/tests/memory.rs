mod common;

use common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

async fn mcp(server: &Server, token: &str, conversation: Option<Uuid>, body: Value) -> (u16, Value) {
    let anonymous = server.anonymous();
    let mut request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body);

    if let Some(conversation) = conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    anonymous.json(request).await
}

async fn call(server: &Server, token: &str, conversation: Option<Uuid>, tool: &str, arguments: Value) -> Value {
    let (status, body) = mcp(server, token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn text_of(body: &Value) -> String {
    body["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn initialize_echoes_the_requested_protocol_version_and_offers_tools() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let (status, body) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).await;

    assert_eq!(status, 200);
    assert_eq!(body["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(body["result"]["capabilities"], json!({"tools": {}}));
    assert_eq!(body["result"]["serverInfo"]["name"], "mycelium");

    let (_, fallback) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {}})).await;
    assert_eq!(fallback["result"]["protocolVersion"], "2025-06-18");
}

#[tokio::test]
async fn the_initialized_notification_is_accepted_with_an_empty_body() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let (status, body) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;

    assert_eq!(status, 202);
    assert_eq!(body, Value::Null);
}

#[tokio::test]
async fn ping_and_tools_list_answer_and_unknown_methods_are_json_rpc_errors() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let (_, ping) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await;
    assert_eq!(ping["result"], json!({}));

    let (_, tools) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["recall", "remember", "propose_task", "ask_question", "propose_schedule", "walkthrough"]);

    let (_, unknown) = mcp(&server, &token, None, json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"})).await;
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn the_endpoint_needs_a_node_token() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, _) = anonymous.json(anonymous.post("/api/v1/mcp").json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))).await;
    assert_eq!(status, 401);

    let (status, _) = mcp(&server, "myc_node_wrong", None, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn a_remembered_finding_is_recalled_by_another_node() {
    let server = Server::start().await;
    let (_, writer) = server.join("node", "laptop").await;
    let (_, reader) = server.join("node", "desk").await;

    let stored = call(&server, &writer, None, "remember", json!({"text": "The staging database listens on port 5433"})).await;
    let id = text_of(&stored).trim_start_matches("Remembered as ").split(',').next().unwrap().to_string();

    let found = call(&server, &reader, None, "recall", json!({"query": "staging port"})).await;

    assert!(text_of(&found).contains(&id));
    assert!(text_of(&found).contains("The staging database listens on port 5433"));
}

#[tokio::test]
async fn recall_says_plainly_when_nothing_matches() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let body = call(&server, &token, None, "recall", json!({"query": "kubernetes"})).await;

    assert_eq!(text_of(&body), "No memories match that query.");
    assert_eq!(body["result"]["isError"], false);
}

#[tokio::test]
async fn remember_records_the_node_and_conversation_it_came_from() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;

    call(&server, &link.token, Some(conversation.id), "remember", json!({"text": "Login redirects need the public URL"})).await;

    let (_, memories) = server.user.json(server.user.get("/api/v1/memories")).await;
    assert_eq!(memories[0]["conversation_id"], json!(conversation.id));
    assert_eq!(memories[0]["created_by"], format!("laptop/{}", conversation.title));
    assert_eq!(memories[0]["scopes"], json!(["global"]));
}

#[tokio::test]
async fn remember_ignores_a_conversation_that_belongs_to_another_node() {
    let server = Server::start().await;
    let first = server.online_node("laptop").await;
    let conversation = server.create_conversation(first.id, None).await;
    let (_, other) = server.join("node", "desk").await;

    call(&server, &other, Some(conversation.id), "remember", json!({"text": "Not linked"})).await;

    let (_, memories) = server.user.json(server.user.get("/api/v1/memories")).await;
    assert_eq!(memories[0]["conversation_id"], Value::Null);
}

#[tokio::test]
async fn remember_refuses_empty_text_and_bad_params_are_json_rpc_errors() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let empty = call(&server, &token, None, "remember", json!({"text": "   "})).await;
    assert_eq!(empty["result"]["isError"], true);
    assert_eq!(text_of(&empty), "Nothing was stored. Give the text to remember.");

    let missing = call(&server, &token, None, "remember", json!({})).await;
    assert_eq!(missing["error"]["code"], -32602);

    let no_query = call(&server, &token, None, "recall", json!({})).await;
    assert_eq!(no_query["error"]["code"], -32602);

    let unknown = call(&server, &token, None, "forget", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32602);

    let (_, memories) = server.user.json(server.user.get("/api/v1/memories")).await;
    assert_eq!(memories, json!([]));
}

#[tokio::test]
async fn recall_only_sees_global_memories() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    sqlx::query("insert into memories (text, scopes, created_by) values ('Only for the payments group', '{group:payments}', 'someone')").execute(&server.pool).await.unwrap();

    let body = call(&server, &token, None, "recall", json!({"query": "payments"})).await;

    assert_eq!(text_of(&body), "No memories match that query.");
}

#[tokio::test]
async fn memories_are_listed_newest_first_and_searched_by_words() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    call(&server, &token, None, "remember", json!({"text": "Deploys run on Fridays"})).await;
    call(&server, &token, None, "remember", json!({"text": "The cache is Redis"})).await;

    let (_, all) = server.user.json(server.user.get("/api/v1/memories")).await;
    assert_eq!(all[0]["text"], "The cache is Redis");
    assert_eq!(all[1]["text"], "Deploys run on Fridays");

    let (_, found) = server.user.json(server.user.get("/api/v1/memories?query=deploy")).await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["text"], "Deploys run on Fridays");
}

#[tokio::test]
async fn a_memory_can_be_edited_and_deleted_and_an_unknown_one_is_not_found() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    call(&server, &token, None, "remember", json!({"text": "Old wording"})).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/memories")).await;
    let id = listed[0]["id"].as_str().unwrap().to_string();

    let (status, edited) = server.user.json(server.user.patch(&format!("/api/v1/memories/{id}")).json(&json!({"text": " New wording "}))).await;
    assert_eq!(status, 200);
    assert_eq!(edited["text"], "New wording");

    let (blank, body) = server.user.json(server.user.patch(&format!("/api/v1/memories/{id}")).json(&json!({"text": " "}))).await;
    assert_eq!(blank, 400);
    assert_eq!(body["message"], "Enter what to remember.");

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/memories/{id}"))).await;
    assert_eq!(status, 204);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/memories/{id}"))).await;
    assert_eq!(status, 404);

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/memories/{}", Uuid::new_v4())).json(&json!({"text": "x"}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn managing_memories_needs_a_signed_in_user() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, _) = anonymous.json(anonymous.get("/api/v1/memories")).await;

    assert_eq!(status, 401);
}
