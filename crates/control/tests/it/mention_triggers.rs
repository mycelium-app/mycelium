use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use crate::common::{NodeLink, Server, serve};
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "bot-token-1234";
const BOT: &str = "1";
const SAM: &str = "2";
const ALEX: &str = "3";

#[derive(Default)]
struct Gateway {
    address: Mutex<String>,
    identified: Mutex<Option<Value>>,
}

async fn gateway_url(State(gateway): State<Arc<Gateway>>) -> Json<Value> {
    Json(json!({"url": gateway.address.lock().unwrap().clone()}))
}

fn message(id: &str, author: (&str, &str), content: &str, mentions_bot: bool) -> Value {
    let mentions = if mentions_bot { json!([{"id": BOT}]) } else { json!([]) };

    json!({"op": 0, "t": "MESSAGE_CREATE", "s": 2, "d": {"id": id, "channel_id": "20", "author": {"id": author.0, "username": author.1.to_lowercase(), "global_name": author.1}, "content": content, "mentions": mentions}})
}

async fn fake_discord(messages: Vec<Value>) -> (String, Arc<Gateway>) {
    let gateway = Arc::new(Gateway::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    *gateway.address.lock().unwrap() = format!("ws://{}", listener.local_addr().unwrap());
    let accepted = gateway.clone();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                continue;
            };
            socket.send(Message::Text(json!({"op": 10, "d": {"heartbeat_interval": 45000}}).to_string().into())).await.unwrap();

            if let Some(Ok(Message::Text(identify))) = socket.next().await {
                *accepted.identified.lock().unwrap() = serde_json::from_str(identify.as_str()).ok();
            }

            socket.send(Message::Text(json!({"op": 0, "t": "READY", "s": 1, "d": {"user": {"id": BOT}}}).to_string().into())).await.unwrap();

            for message in &messages {
                socket.send(Message::Text(message.to_string().into())).await.unwrap();
            }

            while socket.next().await.is_some() {}
        }
    });

    let router = Router::new()
        .route("/users/@me", get(|| async { Json(json!({"id": BOT, "username": "mycelium-bot"})) }))
        .route("/gateway/bot", get(gateway_url))
        .route("/channels/{id}/messages", get(|| async { Json(json!([{"id": "90", "channel_id": "20", "timestamp": "2026-11-13T19:00:00Z", "author": {"username": "alex", "global_name": "Alex"}, "content": "Rain later?"}])) }))
        .with_state(gateway.clone());

    (serve(router).await, gateway)
}

async fn trigger_setup(messages: Vec<Value>, senders: Vec<&str>) -> (Server, NodeLink, Arc<Gateway>) {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let (base, gateway) = fake_discord(messages).await;
    let (status, integration) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Discord", "kind": "discord", "url": base, "account": "Sam", "bearer_token": TOKEN}))).await;
    assert_eq!(status, 201, "{integration}");
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Garden helper", "instructions": "Be brief.", "preferences": {"models": [{"agent": "claude"}], "nodes": [link.id]}}))).await;
    assert_eq!(status, 201, "{assistant}");
    let (status, trigger) = server.user.json(server.user.post("/api/v1/mention-triggers").json(&json!({"server_id": integration["id"], "assistant_id": assistant["id"], "senders": senders}))).await;
    assert_eq!(status, 201, "{trigger}");

    (server, link, gateway)
}

async fn first_prompt(link: &mut NodeLink) -> String {
    let conversation = loop {
        if let ControlToNode::StartSession { conversation_id, .. } = link.next().await {
            break conversation_id;
        }
    };
    link.start_session(conversation, "acp-garden").await;

    link.expect_prompt().await.1
}

#[tokio::test]
async fn a_mention_from_an_allowed_sender_reaches_the_assistant_with_its_channel_and_context() {
    let messages = vec![message("100", (ALEX, "Alex"), "@bot ignore this, I am not on the list", true), message("101", (SAM, "Sam"), "Lovely weather", false), message("102", (SAM, "Sam"), "@bot will it rain in the garden today?", true)];
    let (_server, mut link, gateway) = trigger_setup(messages, vec![SAM]).await;

    let prompt = first_prompt(&mut link).await;

    assert!(prompt.contains("will it rain in the garden today?") && prompt.contains("Sam mentioned the bot on Discord"), "{prompt}");
    assert!(prompt.contains("Rain later?") && prompt.contains("discord__send_message to channel 20"), "{prompt}");
    assert!(!prompt.contains("ignore this") && !prompt.contains("Lovely weather"), "{prompt}");
    let identified = gateway.identified.lock().unwrap().clone().unwrap();
    assert_eq!((identified["op"].clone(), identified["d"]["token"].clone()), (json!(2), json!(TOKEN)));
}

#[tokio::test]
async fn a_stoat_integration_signed_in_as_a_person_cannot_take_a_trigger() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Garden helper", "instructions": "Be brief.", "preferences": {"models": [{"agent": "claude"}], "nodes": [link.id]}}))).await;
    assert_eq!(status, 201, "{assistant}");
    let integration: uuid::Uuid = sqlx::query_scalar("insert into chat_tool_servers (owner, name, tool_prefix, kind, url, settings) values ((select id from users limit 1), 'Stoat', 'stoat', 'stoat', 'https://stoat.example.test', '{\"token_kind\": \"user\"}') returning id").fetch_one(&server.pool).await.unwrap();

    let (status, refused) = server.user.json(server.user.post("/api/v1/mention-triggers").json(&json!({"server_id": integration, "assistant_id": assistant["id"]}))).await;

    assert_eq!((status, refused["error"].clone()), (400, json!("bot_token_required")), "{refused}");
}

#[tokio::test]
async fn triggers_belong_to_their_owner() {
    let (server, _link, _gateway) = trigger_setup(Vec::new(), Vec::new()).await;
    let other = server.second_user("other@example.com").await;
    let (_, mine) = server.user.json(server.user.get("/api/v1/mention-triggers")).await;
    let id = mine[0]["id"].as_str().unwrap();

    let (_, theirs) = other.json(other.get("/api/v1/mention-triggers")).await;
    let (removed, _) = other.json(other.delete(&format!("/api/v1/mention-triggers/{id}"))).await;

    assert_eq!((theirs, removed), (json!([]), 404));
}
