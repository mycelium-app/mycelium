mod common;

#[path = "common/fake_stoat.rs"]
mod fake_stoat;

use axum::http::Method;
use common::Server;
use fake_stoat::{CHANNEL, TOKEN, fake_stoat};
use serde_json::{Value, json};

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn add_stoat(server: &Server, base: &str, name: &str, secret: &str, kind: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": name, "kind": "stoat", "url": base, "bearer_token": secret, "settings": {"token_kind": kind}}))).await
}

async fn call(server: &Server, token: &str, id: u64, tool: &str, arguments: Value) -> Value {
    let reply = mcp(server, token, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("stoat__{tool}"), "arguments": arguments}})).await;

    reply["result"].clone()
}

fn payload(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn a_stoat_integration_is_verified_with_the_token_kind_and_stored_without_returning_the_token() {
    let server = Server::start().await;
    let (base, stoat) = fake_stoat().await;
    let post = |body: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body));

    let (status, added) = add_stoat(&server, &base, "Stoat", TOKEN, "user").await;
    let (_, bot) = add_stoat(&server, &base, "Stoat bot", TOKEN, "bot").await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    let (_, wrong) = add_stoat(&server, &base, "Other", "wrong", "user").await;
    let (_, no_token) = post(json!({"name": "Other", "kind": "stoat", "url": base})).await;
    let (_, unknown_kind) = add_stoat(&server, &base, "Other", TOKEN, "robot").await;

    assert_eq!(status, 201);
    assert_eq!((added["kind"].clone(), added["token_hint"].clone(), added["settings"].clone()), (json!("stoat"), json!("••••cdef"), json!({"token_kind": "user"})));
    assert_eq!(bot["settings"], json!({"token_kind": "bot"}));
    let names: Vec<&str> = added["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_servers", "get_messages", "search_messages", "send_message"]);
    assert!(!stored.contains(TOKEN));
    assert_eq!((wrong["error"].clone(), no_token["error"].clone(), unknown_kind["error"].clone()), (json!("tool_server_failed"), json!("api_key_required"), json!("settings_invalid")));
    assert!(wrong["message"].as_str().unwrap().contains("token"));

    let verified: Vec<String> = stoat.requests(Method::GET, "/users/@me").into_iter().map(|(path, _)| path).collect();
    assert_eq!(verified, ["/users/@me|x-session-token", "/users/@me|x-bot-token"]);
}

#[tokio::test]
async fn stoat_tools_list_servers_read_search_and_send_messages() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (base, stoat) = fake_stoat().await;
    add_stoat(&server, &base, "Stoat", TOKEN, "bot").await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let send = listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "stoat__send_message").unwrap();
    assert!(send["description"].as_str().unwrap().contains("needs the person's approval"));

    let servers = payload(&call(&server, &token, 2, "list_servers", json!({})).await);
    let messages = payload(&call(&server, &token, 3, "get_messages", json!({"channel": CHANNEL, "limit": 5})).await);
    let found = payload(&call(&server, &token, 4, "search_messages", json!({"channel": CHANNEL, "query": "noon"})).await);
    let sent = call(&server, &token, 5, "send_message", json!({"channel": CHANNEL, "content": "Yes.", "reply_to": "01K8ZZZZZZZZZZZZZZZZZZZZZB"})).await;
    let bad_channel = call(&server, &token, 6, "get_messages", json!({"channel": "../users"})).await;
    let long_query = call(&server, &token, 7, "search_messages", json!({"channel": CHANNEL, "query": "x".repeat(65)})).await;

    assert_eq!(servers["servers"], json!([{"id": "01SERVER00000000000000000A", "name": "Lab", "channels": [{"id": CHANNEL, "name": "general", "type": "TextChannel"}]}]));
    assert_eq!(servers["direct_channels"], json!([{"id": "01DM00000000000000000000AA", "name": "Grace", "type": "DirectMessage"}]));
    assert_eq!((messages[0]["content"].clone(), messages[0]["author"].clone(), messages[0]["attachments"].clone()), (json!("Noon?"), json!("Grace"), json!([{"name": "menu.pdf", "size": 12}])));
    assert!(messages[0]["sent_at"].as_str().unwrap().starts_with("20"));
    assert_eq!(found[0]["content"], "Noon?");
    assert_eq!(sent["isError"], false);
    assert_eq!((bad_channel["isError"].clone(), long_query["isError"].clone()), (json!(true), json!(true)));

    let fetched = stoat.requests(Method::GET, &format!("/channels/{CHANNEL}/messages"));
    assert!(fetched[0].0.contains("limit=5") && fetched[0].0.contains("include_users=true") && fetched[0].0.ends_with("x-bot-token"));
    let searched = stoat.requests(Method::POST, &format!("/channels/{CHANNEL}/search"));
    assert_eq!(serde_json::from_str::<Value>(&searched[0].1).unwrap()["query"], "noon");
    let posted = stoat.requests(Method::POST, &format!("/channels/{CHANNEL}/messages"));
    assert_eq!(posted.len(), 1);
    assert_eq!(serde_json::from_str::<Value>(&posted[0].1).unwrap(), json!({"content": "Yes.", "replies": [{"id": "01K8ZZZZZZZZZZZZZZZZZZZZZB", "mention": false}]}));
}
