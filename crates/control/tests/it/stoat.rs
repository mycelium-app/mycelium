#[path = "common/fake_stoat.rs"]
mod fake_stoat;

use axum::http::Method;
use crate::common::Server;
use fake_stoat::{CHANNEL, FIRST_HISTORY, HISTORY_CHANNEL, TOKEN, fake_stoat, history_id};
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
    let body = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("stoat__{tool}"), "arguments": arguments}});
    let reply = if tool == "send_message" { server.approved_agent_mcp(token, body).await } else { mcp(server, token, body).await };

    reply["result"].clone()
}

fn ids(read: &Value) -> Vec<String> {
    read["messages"].as_array().unwrap().iter().map(|message| message["id"].as_str().unwrap().to_string()).collect()
}

fn queries(stoat: &fake_stoat::FakeStoat) -> Vec<String> {
    stoat.requests(Method::GET, &format!("/channels/{HISTORY_CHANNEL}/messages")).into_iter().map(|(path, _)| path.split_once('?').unwrap().1.split('|').next().unwrap().to_string()).collect()
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
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
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
    let messages = messages["messages"].clone();

    assert_eq!((messages[0]["content"].clone(), messages[0]["author"].clone(), messages[0]["attachments"].clone()), (json!("Noon?"), json!("Grace"), json!([{"name": "menu.pdf", "size": 12}])));
    assert!(messages[0]["sent_at"].as_str().unwrap().starts_with("20"));
    assert_eq!((found["messages"][0]["content"].clone(), found["coverage"]["returned"].clone(), found["coverage"]["next"].clone()), (json!("Noon?"), json!(1), json!(null)));
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

async fn history_server() -> (Server, String, std::sync::Arc<fake_stoat::FakeStoat>) {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, stoat) = fake_stoat().await;
    add_stoat(&server, &base, "Stoat", TOKEN, "bot").await;

    (server, node.token.clone(), stoat)
}

#[tokio::test]
async fn a_limit_of_250_reads_the_channel_back_in_three_requests_each_before_the_oldest_message_so_far() {
    let (server, token, stoat) = history_server().await;

    let read = payload(&call(&server, &token, 1, "get_messages", json!({"channel": HISTORY_CHANNEL, "limit": 250})).await);

    let oldest_first: Vec<String> = (FIRST_HISTORY + 551..=FIRST_HISTORY + 800).map(history_id).collect();
    let requests = queries(&stoat);

    assert_eq!(ids(&read), oldest_first);
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("limit=100") && !requests[0].contains("before="), "{requests:?}");
    assert!(requests[1].contains("limit=100") && requests[1].contains(&format!("before={}", history_id(FIRST_HISTORY + 701))), "{requests:?}");
    assert!(requests[2].contains("limit=50") && requests[2].contains(&format!("before={}", history_id(FIRST_HISTORY + 601))), "{requests:?}");
    assert_eq!((read["coverage"]["returned"].clone(), read["coverage"]["found_newest"].clone(), read["coverage"]["next"].clone()), (json!(250), json!(true), json!(history_id(FIRST_HISTORY + 551))));
}

#[tokio::test]
async fn after_pages_forward_oldest_first_and_next_continues_from_the_newest_returned() {
    let (server, token, stoat) = history_server().await;

    let read = payload(&call(&server, &token, 1, "get_messages", json!({"channel": HISTORY_CHANNEL, "after": history_id(FIRST_HISTORY), "limit": 250})).await);

    let oldest_first: Vec<String> = (FIRST_HISTORY + 1..=FIRST_HISTORY + 250).map(history_id).collect();
    let requests = queries(&stoat);

    assert_eq!(ids(&read), oldest_first);
    assert!(requests[1].contains(&format!("after={}", history_id(FIRST_HISTORY + 100))) && requests[1].contains("sort=Oldest"), "{requests:?}");
    assert_eq!(read["coverage"]["next"], json!(history_id(FIRST_HISTORY + 250)));
}

#[tokio::test]
async fn a_limit_above_500_is_clamped_to_500() {
    let (server, token, stoat) = history_server().await;

    let read = payload(&call(&server, &token, 1, "get_messages", json!({"channel": HISTORY_CHANNEL, "limit": 9999})).await);

    assert_eq!(ids(&read).len(), 500);
    assert_eq!(queries(&stoat).len(), 5);
}

#[tokio::test]
async fn reading_to_the_start_of_the_channel_says_there_is_nothing_older() {
    let (server, token, _) = history_server().await;

    let read = payload(&call(&server, &token, 1, "get_messages", json!({"channel": HISTORY_CHANNEL, "before": history_id(FIRST_HISTORY + 4), "limit": 100})).await);

    assert_eq!(ids(&read).len(), 3);
    assert_eq!((read["coverage"]["found_oldest"].clone(), read["coverage"]["found_newest"].clone(), read["coverage"]["next"].clone()), (json!(true), json!(false), json!(null)));
}

#[tokio::test]
async fn search_passes_before_and_after_through() {
    let (server, token, stoat) = history_server().await;
    let before = history_id(FIRST_HISTORY + 20);
    let after = history_id(FIRST_HISTORY + 10);

    let found = payload(&call(&server, &token, 1, "search_messages", json!({"channel": HISTORY_CHANNEL, "query": "note", "before": before, "after": after})).await);

    let sent: Value = serde_json::from_str(&stoat.requests(Method::POST, &format!("/channels/{HISTORY_CHANNEL}/search"))[0].1).unwrap();

    assert_eq!((sent["before"].clone(), sent["after"].clone(), sent["sort"].clone()), (json!(before), json!(after), json!("Latest")));
    assert_eq!((ids(&found).len(), found["coverage"]["found_oldest"].clone(), found["coverage"]["found_newest"].clone()), (19, json!(true), json!(false)));
}
