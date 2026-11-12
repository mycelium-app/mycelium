use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "bot-token-1234";

#[derive(Default)]
struct FakeDiscord {
    posted: Mutex<Vec<(String, Value)>>,
    reads: Mutex<Vec<HashMap<String, String>>>,
}

const FIRST_ID: u64 = 1001;
const SEARCH_MATCHES: u64 = 300;

fn history_length(channel: &str) -> u64 {
    match channel {
        "30" => 250,
        "40" => 800,
        _ => 0,
    }
}

fn invented_message(channel: &str, id: u64) -> Value {
    let author = if id % 2 == 0 { "Sam" } else { "Alex" };

    json!({"id": id.to_string(), "channel_id": channel, "timestamp": "2026-10-27T09:00:00Z", "author": {"username": author.to_lowercase(), "global_name": author}, "content": format!("Note {id}")})
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

async fn messages(State(fake): State<Arc<FakeDiscord>>, Path(channel): Path<String>, Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    fake.reads.lock().unwrap().push(query.clone());

    let length = history_length(&channel);

    if length > 0 {
        let last = FIRST_ID + length - 1;
        let limit: usize = query["limit"].parse().unwrap();
        let ids: Vec<u64> = match (query.get("before"), query.get("after")) {
            (Some(before), _) => (FIRST_ID..before.parse().unwrap()).rev().take(limit).collect(),
            (_, Some(after)) => {
                let mut forward: Vec<u64> = (after.parse::<u64>().unwrap() + 1..=last).take(limit).collect();

                forward.reverse();

                forward
            }
            _ => (FIRST_ID..=last).rev().take(limit).collect(),
        };

        return Json(Value::Array(ids.into_iter().map(|id| invented_message(&channel, id)).collect()));
    }

    Json(json!([{"id": "31", "channel_id": channel, "timestamp": "2026-10-27T09:00:00Z", "author": {"username": "ada", "global_name": "Ada"}, "content": "Seeds arrived"}]))
}

async fn post_message(State(fake): State<Arc<FakeDiscord>>, Path(channel): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    fake.posted.lock().unwrap().push((channel.clone(), body));

    Json(json!({"id": "99", "channel_id": channel}))
}

async fn search(State(fake): State<Arc<FakeDiscord>>, Path(_server): Path<String>, Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    fake.reads.lock().unwrap().push(query.clone());

    if query["content"] == "notes" {
        let offset: u64 = query.get("offset").map(|offset| offset.parse().unwrap()).unwrap_or(0);
        let limit: u64 = query["limit"].parse().unwrap();
        let hits: Vec<Value> = (offset..SEARCH_MATCHES.min(offset + limit)).map(|index| json!([invented_message("30", FIRST_ID + SEARCH_MATCHES - 1 - index)])).collect();

        return Json(json!({"messages": hits, "total_results": SEARCH_MATCHES}));
    }

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

#[tokio::test]
async fn an_id_that_is_not_digits_is_refused_instead_of_extending_the_request_path() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let read = call(&server, &node.token, "get_messages", json!({"channel": "20/../../users/@me"})).await;
    let channels = call(&server, &node.token, "list_channels", json!({"server": "10?x=1"})).await;
    let sent = call(&server, &node.token, "send_message", json!({"channel": "20/../../users/@me", "content": "Seeds are in the shed."})).await;

    assert_eq!((read["isError"].clone(), channels["isError"].clone(), sent["isError"].clone()), (json!(true), json!(true), json!(true)));
    assert!(text(&read).contains("digits only"), "{read}");
    assert!(fake.posted.lock().unwrap().is_empty());
}

fn payload(result: &Value) -> Value {
    serde_json::from_str(&text(result)).unwrap()
}

fn ids(read: &Value) -> Vec<String> {
    read["messages"].as_array().unwrap().iter().map(|message| message["id"].as_str().unwrap().to_string()).collect()
}

fn cursors(fake: &FakeDiscord, key: &str) -> Vec<(String, Option<String>)> {
    fake.reads.lock().unwrap().iter().map(|query| (query["limit"].clone(), query.get(key).cloned())).collect()
}

#[tokio::test]
async fn a_limit_of_250_reads_the_channel_back_in_three_requests_each_before_the_oldest_message_so_far() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let read = payload(&call(&server, &node.token, "get_messages", json!({"channel": "30", "limit": 250})).await);

    let newest_first: Vec<String> = (1001..=1250).rev().map(|id| id.to_string()).collect();

    assert_eq!(ids(&read), newest_first);
    assert_eq!(cursors(&fake, "before"), [("100".to_string(), None), ("100".to_string(), Some("1151".to_string())), ("50".to_string(), Some("1051".to_string()))]);
    assert_eq!((read["coverage"]["returned"].clone(), read["coverage"]["found_newest"].clone(), read["coverage"]["next"].clone()), (json!(250), json!(true), json!("1001")));
}

#[tokio::test]
async fn reading_before_a_message_starts_below_it_and_ends_when_the_channel_does() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let read = payload(&call(&server, &node.token, "get_messages", json!({"channel": "30", "before": "1011", "limit": 100})).await);

    assert_eq!(ids(&read).len(), 10);
    assert_eq!(read["coverage"], json!({"returned": 10, "found_oldest": true, "found_newest": false, "oldest_id": "1001", "newest_id": "1010", "next": null}));
}

#[tokio::test]
async fn after_pages_forward_from_a_message_and_next_continues_from_the_newest_returned() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let read = payload(&call(&server, &node.token, "get_messages", json!({"channel": "30", "after": "1000", "limit": 250})).await);
    let following = payload(&call(&server, &node.token, "get_messages", json!({"channel": "30", "after": "1240", "limit": 100})).await);

    let newest_first: Vec<String> = (1001..=1250).rev().map(|id| id.to_string()).collect();

    assert_eq!(ids(&read), newest_first);
    assert_eq!(cursors(&fake, "after")[..3], [("100".to_string(), Some("1000".to_string())), ("100".to_string(), Some("1100".to_string())), ("50".to_string(), Some("1200".to_string()))]);
    assert_eq!(read["coverage"]["next"], "1250");
    assert_eq!((ids(&following).len(), following["coverage"]["found_newest"].clone(), following["coverage"]["next"].clone()), (10, json!(true), json!(null)));
}

#[tokio::test]
async fn a_limit_above_500_is_clamped_to_500() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let read = payload(&call(&server, &node.token, "get_messages", json!({"channel": "40", "limit": 9999})).await);

    assert_eq!(ids(&read).len(), 500);
    assert_eq!(fake.reads.lock().unwrap().len(), 5);
    assert_eq!(read["coverage"]["next"], "1301");
}

#[tokio::test]
async fn search_pages_with_offsets_of_25_and_stops_at_the_page_cap_saying_where_to_continue() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_discord().await;
    add_discord(&server, &base, TOKEN).await;

    let found = payload(&call(&server, &node.token, "search_messages", json!({"server": "10", "query": "notes", "limit": 500})).await);
    let rest = payload(&call(&server, &node.token, "search_messages", json!({"server": "10", "query": "notes", "limit": 500, "offset": 250})).await);

    let offsets: Vec<Option<String>> = fake.reads.lock().unwrap().iter().take(10).map(|query| query.get("offset").cloned()).collect();
    let expected: Vec<Option<String>> = (0..10).map(|page| Some((page * 25).to_string())).collect();

    assert_eq!(ids(&found).len(), 250);
    assert_eq!(offsets, expected);
    assert_eq!(found["coverage"]["next"], "250");
    assert_eq!((ids(&rest).len(), rest["coverage"]["next"].clone()), (50, json!(null)));
}
