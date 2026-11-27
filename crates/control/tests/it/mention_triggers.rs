use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
#[path = "common/fake_stoat.rs"]
mod fake_stoat;

use crate::common::{NodeLink, Server, serve};
use crate::continue_elsewhere;
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

    json!({"op": 0, "t": "MESSAGE_CREATE", "s": 2, "d": {"id": id, "channel_id": "20", "guild_id": "70", "author": {"id": author.0, "username": author.1.to_lowercase(), "global_name": author.1}, "content": content, "mentions": mentions}})
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
        .route("/users/@me/guilds", get(|| async { Json(json!([{"id": "70", "name": "Kestrel Labs"}, {"id": "71", "name": "Allotment"}])) }))
        .route("/guilds/{id}/members/search", get(|| async { Json(json!([{"user": {"id": SAM, "username": "sam", "global_name": "Sam"}}, {"user": {"id": ALEX, "username": "alex"}, "nick": "Alex"}, {"user": {"id": BOT, "username": "mycelium-bot", "bot": true}}])) }))
        .route("/channels/{id}/messages", get(|| async { Json(json!([{"id": "90", "channel_id": "20", "timestamp": "2026-11-13T19:00:00Z", "author": {"username": "alex", "global_name": "Alex"}, "content": "Rain later?"}])) }))
        .with_state(gateway.clone());

    (serve(router).await, gateway)
}

async fn discord_setup(messages: Vec<Value>) -> (Server, NodeLink, Arc<Gateway>, Value) {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let mut link = server.online_node("laptop").await;
    continue_elsewhere::allow_any_model_on(&server, &mut link).await;
    let (base, gateway) = fake_discord(messages).await;
    let (status, integration) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Discord", "kind": "discord", "url": base, "account": "Sam", "bearer_token": TOKEN}))).await;
    assert_eq!(status, 201, "{integration}");

    (server, link, gateway, integration)
}

async fn assistant(server: &Server, name: &str) -> Value {
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": name, "instructions": "Be brief."}))).await;
    assert_eq!(status, 201, "{assistant}");

    assistant
}

async fn add_rule(server: &Server, rule: Value) -> Value {
    let (status, created) = server.user.json(server.user.post("/api/v1/mention-triggers").json(&rule)).await;
    assert_eq!(status, 201, "{created}");

    created
}

async fn trigger_setup(messages: Vec<Value>, senders: Vec<&str>) -> (Server, NodeLink, Arc<Gateway>) {
    let (server, link, gateway, _) = discord_setup(messages).await;
    let assistant = assistant(&server, "Garden helper").await;
    add_rule(&server, json!({"integration": "discord", "assistant_id": assistant["id"], "senders": senders})).await;

    (server, link, gateway)
}

async fn started_conversation(server: &Server, link: &mut NodeLink) -> (Option<uuid::Uuid>, Option<uuid::Uuid>, String) {
    let conversation = loop {
        if let ControlToNode::StartSession { conversation_id, .. } = link.next().await {
            break conversation_id;
        }
    };
    link.start_session(conversation, "acp-garden").await;
    let prompt = link.expect_prompt().await.1;
    let (assistant_id, project_id) = sqlx::query_as::<_, (Option<uuid::Uuid>, Option<uuid::Uuid>)>("select assistant_id, project_id from conversations where id = $1").bind(conversation).fetch_one(&server.pool).await.unwrap();

    (assistant_id, project_id, prompt)
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
async fn a_stoat_account_signed_in_as_the_person_takes_a_trigger() {
    let server = Server::start().await;
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Garden helper", "instructions": "Be brief."}))).await;
    assert_eq!(status, 201, "{assistant}");
    let integration: uuid::Uuid = sqlx::query_scalar("insert into chat_tool_servers (owner, name, tool_prefix, kind, url, settings) values ((select id from users limit 1), 'Stoat', 'stoat', 'stoat', 'https://stoat.example.test', '{\"token_kind\": \"user\"}') returning id").fetch_one(&server.pool).await.unwrap();

    let (status, trigger) = server.user.json(server.user.post("/api/v1/mention-triggers").json(&json!({"server_id": integration, "assistant_id": assistant["id"]}))).await;

    assert_eq!((status, trigger["assistant_id"].clone()), (201, assistant["id"].clone()), "{trigger}");
}

#[tokio::test]
async fn the_first_rule_that_matches_decides_and_later_rules_are_not_woken() {
    let messages = vec![message("105", (ALEX, "Alex"), "@bot is the shed locked?", true)];
    let (server, mut link, _gateway, _) = discord_setup(messages).await;
    let (captain, helper) = (assistant(&server, "Release captain").await, assistant(&server, "Garden helper").await);
    add_rule(&server, json!({"integration": "discord", "guild_id": "70", "guild_name": "Kestrel Labs", "senders": [SAM], "sender_names": ["Sam"], "assistant_id": captain["id"]})).await;
    add_rule(&server, json!({"integration": "discord", "guild_id": "70", "guild_name": "Kestrel Labs", "assistant_id": helper["id"]})).await;
    add_rule(&server, json!({"integration": "discord", "assistant_id": captain["id"]})).await;

    let (assistant_id, _, prompt) = started_conversation(&server, &mut link).await;

    assert_eq!(assistant_id.map(|id| id.to_string()), helper["id"].as_str().map(str::to_string), "{prompt}");
    let woken: i64 = sqlx::query_scalar("select count(*) from conversations where assistant_id is not null").fetch_one(&server.pool).await.unwrap();
    assert_eq!(woken, 1);
}

#[tokio::test]
async fn a_rule_sending_to_an_agent_starts_a_conversation_in_its_project() {
    let messages = vec![message("106", (SAM, "Sam"), "@bot the build is red", true)];
    let (server, mut link, _gateway, _) = discord_setup(messages).await;
    let user = server.user.clone();
    let node_id = link.id;
    let creating = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&json!({"node_id": node_id, "path": "/work/harbour"}))).await });
    let request_id = match link.next().await {
        ControlToNode::Git { request_id, .. } => request_id,
        other => panic!("expected project registration, got {other:?}"),
    };
    link.send(mycelium_proto::link::NodeToControl::GitResult { request_id, ok: true, output: json!({"path": "/work/harbour"}), error: None }).await;
    let (status, project) = creating.await.unwrap();
    assert_eq!(status, 201, "{project}");
    add_rule(&server, json!({"integration": "discord", "project_id": project["id"], "agent": "claude"})).await;

    let (assistant_id, project_id, prompt) = started_conversation(&server, &mut link).await;

    assert_eq!((assistant_id, project_id.map(|id| id.to_string())), (None, project["id"].as_str().map(str::to_string)));
    assert!(prompt.contains("Sam mentioned the bot on Discord") && prompt.contains("the build is red"), "{prompt}");
}

#[tokio::test]
async fn rules_keep_the_order_they_are_given_and_new_rules_go_last() {
    let (server, _link, _gateway, _) = discord_setup(Vec::new()).await;
    let helper = assistant(&server, "Garden helper").await;
    let first = add_rule(&server, json!({"integration": "discord", "assistant_id": helper["id"]})).await;
    let second = add_rule(&server, json!({"integration": "discord", "guild_id": "70", "assistant_id": helper["id"]})).await;
    assert_eq!((first["position"].clone(), second["position"].clone()), (json!(0), json!(1)));

    let (status, ordered) = server.user.json(server.user.put("/api/v1/mention-triggers/order").json(&json!({"integration": "discord", "ids": [second["id"], first["id"]]}))).await;
    let (partial, _) = server.user.json(server.user.put("/api/v1/mention-triggers/order").json(&json!({"integration": "discord", "ids": [first["id"]]}))).await;

    assert_eq!(status, 200, "{ordered}");
    assert_eq!((ordered[0]["id"].clone(), ordered[1]["id"].clone()), (second["id"].clone(), first["id"].clone()));
    assert_eq!(partial, 400);
}

#[tokio::test]
async fn an_open_rule_can_be_changed_in_place() {
    let (server, _link, _gateway, _) = discord_setup(Vec::new()).await;
    let helper = assistant(&server, "Garden helper").await;
    let rule = add_rule(&server, json!({"integration": "discord", "assistant_id": helper["id"]})).await;

    let (status, changed) = server.user.json(server.user.put(&format!("/api/v1/mention-triggers/{}", rule["id"].as_str().unwrap())).json(&json!({"integration": "discord", "guild_id": "70", "guild_name": "Kestrel Labs", "senders": [SAM], "sender_names": ["Sam"], "assistant_id": helper["id"]}))).await;

    assert_eq!(status, 200, "{changed}");
    assert_eq!((changed["guild_name"].clone(), changed["sender_names"].clone(), changed["position"].clone()), (json!("Kestrel Labs"), json!(["Sam"]), json!(0)));
}

#[tokio::test]
async fn the_servers_and_their_people_are_listed_to_choose_from() {
    let (server, _link, _gateway, _) = discord_setup(Vec::new()).await;

    let (status, places) = server.user.json(server.user.get("/api/v1/mention-triggers/places?integration=discord")).await;
    let (_, people) = server.user.json(server.user.get("/api/v1/mention-triggers/people?integration=discord&guild=70&query=a")).await;

    assert_eq!(status, 200, "{places}");
    assert_eq!(places.as_array().unwrap().iter().map(|place| place["name"].clone()).collect::<Vec<_>>(), vec![json!("Allotment"), json!("Kestrel Labs")]);
    assert_eq!(people, json!([{"id": ALEX, "name": "Alex"}, {"id": SAM, "name": "Sam"}]));
}

#[tokio::test]
async fn an_accounts_single_assistant_becomes_one_rule_for_any_server_and_everyone() {
    let (server, _link, _gateway, integration) = discord_setup(Vec::new()).await;
    let helper = assistant(&server, "Garden helper").await;
    let pool = &server.pool;
    sqlx::raw_sql(
        "drop table mention_triggers;
         create table mention_triggers (id uuid primary key default gen_random_uuid(), owner uuid not null references users(id) on delete cascade, server_id uuid not null references chat_tool_servers(id) on delete cascade, assistant_id uuid not null references assistants(id) on delete cascade, channels text[] not null default '{}', senders text[] not null default '{}', created_at timestamptz not null default now());
         create index mention_triggers_server on mention_triggers (server_id);
         create unique index mention_triggers_one_per_account on mention_triggers (server_id);",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("insert into mention_triggers (owner, server_id, assistant_id) values ((select owner from chat_tool_servers where id = $1), $1, $2)").bind(integration["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).bind(helper["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).execute(pool).await.unwrap();

    sqlx::raw_sql(include_str!("../../migrations/0206_mention_rules.sql")).execute(pool).await.unwrap();

    let (_, rules) = server.user.json(server.user.get("/api/v1/mention-triggers")).await;
    assert_eq!(rules.as_array().unwrap().len(), 1);
    let rule = &rules[0];
    assert_eq!((rule["integration"].clone(), rule["position"].clone(), rule["server_id"].clone(), rule["guild_id"].clone(), rule["senders"].clone(), rule["assistant_id"].clone()), (json!("discord"), json!(0), Value::Null, Value::Null, json!([]), helper["id"].clone()));
}

#[tokio::test]
async fn a_direct_message_to_the_bot_reaches_the_assistant_as_a_direct_message() {
    let mut direct = message("103", (SAM, "Sam"), "Is the greenhouse shut?", false);
    direct["d"]["guild_id"] = Value::Null;
    let in_server = message("104", (ALEX, "Alex"), "Not for the bot", false);
    let (_server, mut link, _gateway) = trigger_setup(vec![in_server, direct], Vec::new()).await;

    let prompt = first_prompt(&mut link).await;

    assert!(prompt.contains("Sam messaged the bot directly on Discord") && prompt.contains("Is the greenhouse shut?"), "{prompt}");
    assert!(!prompt.contains("Not for the bot"), "{prompt}");
}

#[tokio::test]
async fn a_direct_message_to_the_persons_own_stoat_account_reaches_the_assistant() {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let mut link = server.online_node("laptop").await;
    continue_elsewhere::allow_any_model_on(&server, &mut link).await;
    let (base, _stoat) = fake_stoat::fake_stoat().await;
    let (status, integration) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Stoat", "kind": "stoat", "url": base, "bearer_token": fake_stoat::TOKEN, "settings": {"token_kind": "user"}}))).await;
    assert_eq!(status, 201, "{integration}");
    let assistant = assistant(&server, "Garden helper").await;
    add_rule(&server, json!({"integration": "stoat", "server_id": integration["id"], "assistant_id": assistant["id"]})).await;

    let prompt = first_prompt(&mut link).await;

    assert!(prompt.contains("Grace messaged you directly on Stoat") && prompt.contains("Can you water the tomatoes?"), "{prompt}");
    assert!(!prompt.contains("Lunch at one?"), "{prompt}");
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
