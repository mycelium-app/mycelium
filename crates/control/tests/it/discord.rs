use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "bot-token-1234";

#[derive(Default)]
struct FakeDiscord {
    posted: Mutex<Vec<(String, Value)>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bot {TOKEN}"))
}

async fn me(headers: HeaderMap) -> Response {
    match authorised(&headers) {
        true => Json(json!({"id": "1", "username": "mycelium-bot"})).into_response(),
        false => (StatusCode::UNAUTHORIZED, Json(json!({"message": "401: Unauthorized"}))).into_response(),
    }
}

async fn servers() -> Json<Value> {
    Json(json!([{"id": "10", "name": "Garden club"}]))
}

async fn channels(Path(server): Path<String>) -> Json<Value> {
    Json(json!([{"id": "20", "name": "general", "type": 0, "parent_id": null, "server": server}]))
}

async fn messages(Path(channel): Path<String>) -> Json<Value> {
    Json(json!([{"id": "31", "channel_id": channel, "timestamp": "2026-10-27T09:00:00Z", "author": {"username": "ada", "global_name": "Ada"}, "content": "Seeds arrived"}]))
}

async fn post_message(State(fake): State<Arc<FakeDiscord>>, Path(channel): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push((channel.clone(), body));

    Json(json!({"id": "99", "channel_id": channel}))
}

async fn search(Path(_server): Path<String>) -> Json<Value> {
    Json(json!({"messages": [[{"id": "31", "channel_id": "20", "timestamp": "2026-10-27T09:00:00Z", "author": {"username": "ada"}, "content": "Seeds arrived"}]]}))
}

async fn fake_discord() -> (String, Arc<FakeDiscord>) {
    let fake = Arc::new(FakeDiscord::default());
    let router = Router::new()
        .route("/users/@me", get(me))
        .route("/users/@me/guilds", get(servers))
        .route("/guilds/{id}/channels", get(channels))
        .route("/guilds/{id}/messages/search", get(search))
        .route("/channels/{id}/messages", get(messages).post(post_message))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_discord(server: &Server, base: &str, token: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Discord", "kind": "discord", "url": base, "account": "Sam", "bearer_token": token}))).await
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("discord__{tool}"), "arguments": arguments}});
    let reply = if tool == "send_message" {
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
async fn a_bot_token_discord_does_not_accept_is_refused_and_a_good_one_is_stored_without_being_returned() {
    let server = Server::start().await;
    let (base, _) = fake_discord().await;

    let (refused, wrong) = add_discord(&server, &base, "wrong").await;
    let (added_status, added) = add_discord(&server, &base, TOKEN).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!((refused, wrong["error"].clone()), (400, json!("tool_server_failed")));
    assert!(wrong["message"].as_str().unwrap().contains("bot token"));
    assert_eq!((added_status, added["policy"]["preset"].clone()), (201, json!("ask_writes")));
    assert!(!stored.contains(TOKEN));
}

#[tokio::test]
async fn the_agent_lists_servers_and_channels_and_reads_and_searches_messages() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let servers = text(&call(&server, &node.token, "list_servers", json!({})).await);
    let channels = text(&call(&server, &node.token, "list_channels", json!({"server": "10"})).await);
    let read = text(&call(&server, &node.token, "get_messages", json!({"channel": "20", "limit": 5})).await);
    let found = text(&call(&server, &node.token, "search_messages", json!({"server": "10", "query": "seeds"})).await);

    assert!(servers.contains("Garden club"), "{servers}");
    assert!(channels.contains("general"), "{channels}");
    assert!(read.contains("Seeds arrived") && read.contains("Ada"), "{read}");
    assert!(found.contains("Seeds arrived"), "{found}");
}

#[tokio::test]
async fn a_sent_message_opens_with_a_line_saying_the_mycelium_agent_posted_it_on_the_persons_behalf() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let sent = call(&server, &node.token, "send_message", json!({"channel": "20", "content": "Seeds are in the shed."})).await;
    let posted = fake.posted.lock().unwrap().clone();
    let content = posted[0].1["content"].as_str().unwrap();

    assert_ne!(sent["isError"], true, "{sent}");
    assert_eq!(posted[0].0, "20");
    assert!(content.starts_with("*Posted by the Mycelium agent on behalf of Sam.*"), "{content}");
    assert!(content.ends_with("Seeds are in the shed."), "{content}");
}
