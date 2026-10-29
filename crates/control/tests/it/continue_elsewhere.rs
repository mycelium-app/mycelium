use crate::common;
use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use crate::common::{NodeLink, Server, serve};
use futures_util::stream;
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type Requests = Arc<Mutex<Vec<Value>>>;

fn fake_openai() -> (Router, Requests) {
    async fn models() -> Json<Value> {
        Json(json!({"object": "list", "data": [{"id": "llama3.2"}]}))
    }

    async fn completions(State(requests): State<Requests>, Json(request): Json<Value>) -> Response {
        requests.lock().unwrap().push(request);

        let chunk = |content: &str| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": content}}]}));
        let lines = vec![chunk("Hello"), chunk(" there"), "data: [DONE]\n\n".to_string()];
        let body = stream::iter(lines.into_iter().map(|line| Ok::<Bytes, Infallible>(Bytes::from(line))));

        Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(body)).unwrap()
    }

    let requests = Requests::default();
    let router = Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(completions)).with_state(requests.clone());

    (router, requests)
}

async fn add_provider(server: &Server, base: &str) -> Value {
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

async fn finished_turns(server: &Server, id: Uuid, turns: usize) {
    common::eventually("the reply to end", || async { (server.events(id).await.iter().filter(|event| event["kind"] == "turn_end").count() == turns).then_some(()) }).await;
}

pub fn login(account: &str, state: LoginState) -> LoginInfo {
    LoginInfo { agent: AgentId::Claude, account: Some(account.into()), method: "oauth_token".into(), state, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }
}

pub async fn next_command(link: &mut NodeLink) -> ControlToNode {
    loop {
        let message = link.next().await;

        if !matches!(message, ControlToNode::Git { .. }) {
            return message;
        }
    }
}

pub async fn expect_prompt(link: &mut NodeLink) -> String {
    match next_command(link).await {
        ControlToNode::Prompt { text, .. } => text,
        other => panic!("expected prompt, got {other:?}"),
    }
}

pub async fn node_with_accounts(server: &Server, version: &str, accounts: Vec<LoginInfo>) -> NodeLink {
    let mut link = server.online_node_as("laptop", version).await;
    link.send(NodeToControl::Logins { logins: accounts, kubernetes: None }).await;

    let user = server.user.clone();
    let id = link.id;
    common::eventually("the accounts", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;
        let node = nodes.as_array().unwrap().iter().find(|node| node["id"] == json!(id)).unwrap().clone();

        (!node["logins"].as_array().unwrap().is_empty()).then_some(())
    })
    .await;

    link
}

async fn idle_conversation(server: &Server, link: &mut NodeLink, account: &str) -> Uuid {
    let (status, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": account, "prompt": "Fix the flaky test"}))).await;
    assert_eq!(status, 201, "{created}");
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

    match next_command(link).await {
        ControlToNode::StartSession { .. } => {}
        other => panic!("expected start_session, got {other:?}"),
    }
    link.start_session(id, "acp-1").await;
    expect_prompt(link).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;

    let user = server.user.clone();
    common::eventually("the turn to end", || async { (user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["status"] == "review").then_some(()) }).await;

    id
}

async fn continue_on(server: &Server, id: Uuid, body: Value) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/continue")).json(&body)).await
}

#[tokio::test]
async fn continuing_on_another_account_resumes_the_same_session_there() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["account"], "work@example.org");
    assert_eq!(body["acp_session_id"], "acp-1");

    match next_command(&mut link).await {
        ControlToNode::CloseSession { conversation_id } => assert_eq!(conversation_id, id),
        other => panic!("expected close_session, got {other:?}"),
    }

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Carry on"}))).await;
    assert_eq!(status, 204);

    match next_command(&mut link).await {
        ControlToNode::StartSession { resume, account, .. } => assert_eq!((resume.as_deref(), account.as_deref()), (Some("acp-1"), Some("work@example.org"))),
        other => panic!("expected start_session, got {other:?}"),
    }

    let events = server.events(id).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").expect("a continued event");
    assert_eq!(continued["payload"]["kind"], "account");
    assert_eq!(continued["payload"]["account"], "work@example.org");
    assert_eq!(continued["payload"]["from"]["account"], "sam@example.com");
}

#[tokio::test]
async fn continuing_on_another_node_that_lacks_the_folder_is_refused_and_says_to_move() {
    let server = Server::start().await;
    let mut laptop = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut laptop, "sam@example.com").await;
    let mut desktop = server.online_node_as("desktop", "v0.11.0").await;
    desktop.send(NodeToControl::Logins { logins: vec![login("sam@example.com", LoginState::SignedIn)], kubernetes: None }).await;
    let user = server.user.clone();
    let desktop_id = desktop.id;
    common::eventually("the desktop's account", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(desktop_id) && !node["logins"].as_array().unwrap().is_empty()).then_some(())
    })
    .await;

    let answer_missing = async {
        loop {
            if let ControlToNode::Git { request_id, .. } = desktop.next().await {
                desktop.send(NodeToControl::GitResult { request_id, ok: false, output: Value::Null, error: Some("The folder /work/app does not exist on this node.".into()) }).await;

                break;
            }
        }
    };
    let ((status, body), ()) = tokio::join!(continue_on(&server, id, json!({"agent": "claude", "account": "sam@example.com", "node_id": desktop_id})), answer_missing);
    let (_, unchanged) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}"))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("folder_missing")), "{body}");
    assert_eq!(unchanged["node_id"], json!(laptop.id));
}

#[tokio::test]
async fn continuing_on_an_account_that_is_not_signed_in_is_refused() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedOut)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("account_not_signed_in")));
}

#[tokio::test]
async fn continuing_on_the_account_it_already_uses_is_refused() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "sam@example.com"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("same_target")));
}

#[tokio::test]
async fn continuing_while_the_agent_is_working_is_refused() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedIn)]).await;
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": "sam@example.com", "prompt": "Work"}))).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    match next_command(&mut link).await {
        ControlToNode::StartSession { .. } => {}
        other => panic!("expected start_session, got {other:?}"),
    }
    link.start_session(id, "acp-1").await;
    expect_prompt(&mut link).await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("conversation_busy")));
}

#[tokio::test]
async fn continuing_on_an_outdated_node_asks_for_an_update() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.9.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_outdated")));
    assert_eq!(body["message"], "Update laptop to continue on another account.");
}

#[tokio::test]
async fn continuing_on_another_harness_seeds_the_first_prompt_with_the_handoff() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;
    let touched = json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit", "locations": [{"path": "/work/app/src/lib.rs"}]}});
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 10, update: touched }).await;
    link.barrier(id, 11).await;

    let (status, body) = continue_on(&server, id, json!({"agent": "codex"})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!((body["agent"].clone(), body["acp_session_id"].clone()), (json!("codex"), Value::Null));

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Carry on"}))).await;
    match next_command(&mut link).await {
        ControlToNode::CloseSession { .. } => {}
        other => panic!("expected close_session, got {other:?}"),
    }
    match next_command(&mut link).await {
        ControlToNode::StartSession { agent, resume, .. } => assert_eq!((agent, resume), (AgentId::Codex, None)),
        other => panic!("expected start_session, got {other:?}"),
    }

    link.start_session(id, "codex-1").await;
    let first = expect_prompt(&mut link).await;
    assert!(first.starts_with("This conversation continues from Claude Code (sam@example.com)."), "{first}");
    assert!(first.contains("Fix the flaky test") && first.contains("/work/app/src/lib.rs"), "{first}");
    assert!(first.ends_with("Carry on"), "{first}");

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 20, stop_reason: "end_turn".into(), usage: None }).await;
    let user = server.user.clone();
    common::eventually("the second turn to end", || async { (user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["status"] == "review").then_some(()) }).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Again"}))).await;

    assert_eq!(expect_prompt(&mut link).await, "Again");

    let events = server.events(id).await;
    let typed: Vec<&str> = events.iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].as_str().unwrap()).collect();
    assert_eq!(typed, vec!["Fix the flaky test", "Carry on", "Again"]);
}

#[tokio::test]
async fn a_refused_start_after_continuing_elsewhere_fails_the_conversation_and_the_next_prompt_starts_afresh() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;
    continue_on(&server, id, json!({"agent": "codex"})).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Carry on"}))).await;
    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert!(matches!(next_command(&mut link).await, ControlToNode::StartSession { .. }));
    expect_prompt(&mut link).await;
    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 30, message: "codex is not installed on this node.".into() }).await;

    let user = server.user.clone();
    let failed = common::eventually("the conversation to fail", || async {
        let (_, conversation) = user.json(user.get(&format!("/api/v1/conversations/{id}"))).await;

        (conversation["status"] == "failed").then_some(conversation)
    })
    .await;
    assert_eq!(failed["agent_active"], json!(false));

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Try again"}))).await;
    assert!(matches!(next_command(&mut link).await, ControlToNode::StartSession { agent: AgentId::Codex, .. }));
}

#[tokio::test]
async fn the_handoff_is_prepended_once_even_if_the_new_session_has_not_reported_its_id() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;
    continue_on(&server, id, json!({"agent": "codex"})).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Carry on"}))).await;
    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert!(matches!(next_command(&mut link).await, ControlToNode::StartSession { .. }));
    let first = expect_prompt(&mut link).await;
    assert!(first.starts_with("This conversation continues from"), "{first}");

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 20, stop_reason: "end_turn".into(), usage: None }).await;
    let user = server.user.clone();
    common::eventually("the first turn to end", || async { (user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["status"] == "review").then_some(()) }).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Again"}))).await;

    assert_eq!(expect_prompt(&mut link).await, "Again");
}

#[tokio::test]
async fn the_previous_session_is_not_offered_for_import_after_a_harness_switch() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;
    continue_on(&server, id, json!({"agent": "codex"})).await;
    sqlx::query("insert into transcripts (node_id, path, bytes) values ($1, '.claude/projects/-work-app/acp-1.jsonl', 10)").bind(link.id).execute(&server.pool).await.unwrap();

    let (_, statuses) = server.user.json(server.user.get("/api/v1/imports")).await;
    let claude = statuses.as_array().unwrap().iter().find(|row| row["node_id"] == json!(link.id) && row["agent"] == "claude").unwrap();

    assert_eq!((claude["found"].clone(), claude["imported"].clone()), (json!(1), json!(1)));
}

#[tokio::test]
async fn continuing_on_a_chat_model_sends_the_handoff_then_only_new_turns_and_can_return_to_the_harness() {
    let server = Server::start().await;
    let (router, requests) = fake_openai();
    let base = serve(router).await;
    let provider = add_provider(&server, &base).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "chat", "provider_id": provider["id"]})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!((body["agent"].clone(), body["model"].clone()), (json!("chat"), json!("llama3.2")));

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "More"}))).await;
    finished_turns(&server, id, 2).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Again"}))).await;
    finished_turns(&server, id, 3).await;

    let sent = requests.lock().unwrap().clone();
    let said = |request: &Value| request["messages"].as_array().unwrap().iter().filter(|message| message["role"] != "system").cloned().collect::<Vec<_>>();
    let first = said(&sent[0]);
    let content = first[0]["content"].as_str().unwrap();
    assert_eq!(first.len(), 1);
    assert!(content.starts_with("This conversation continues from Claude Code (sam@example.com)."), "{content}");
    assert!(content.contains("Fix the flaky test") && content.ends_with("More"), "{content}");
    assert_eq!(said(&sent[1]).len(), 3);

    let (status, back) = continue_on(&server, id, json!({"agent": "claude"})).await;
    assert_eq!(status, 200, "{back}");
    assert_eq!((back["node_id"].clone(), back["cwd"].clone(), back["agent"].clone()), (json!(link.id), json!("/work/app"), json!("claude")));
}

async fn said(server: &Server, link: &mut NodeLink, id: Uuid, user_text: &str, answer: &str, seq: i64) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": user_text}))).await;
    expect_prompt(link).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": answer}}}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: seq + 1, stop_reason: "end_turn".into(), usage: None }).await;
    let user = server.user.clone();
    common::eventually("the turn to end", || async { (user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["status"] == "review").then_some(()) }).await;
}

fn earlier_turns(arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "read_earlier_turns", "arguments": arguments}})
}

fn answer_text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn read_earlier_turns_pages_back_and_searches_this_conversation_only() {
    let server = Server::start().await;
    let (mut link, token) = server.online_node_with_token("laptop", "v0.11.0").await;
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app"}))).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    let (_, other) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/other"}))).await;
    let other_id: Uuid = serde_json::from_value(other["id"].clone()).unwrap();

    for conversation in [id, other_id] {
        match next_command(&mut link).await {
            ControlToNode::StartSession { conversation_id, .. } => assert_eq!(conversation_id, conversation),
            other => panic!("expected start_session, got {other:?}"),
        }
        link.start_session(conversation, &format!("s-{conversation}")).await;
    }

    said(&server, &mut link, id, "Plan the zeppelin hangar", "Understood.", 3).await;
    said(&server, &mut link, other_id, "Secret kumquat budget", "Understood.", 3).await;

    for (n, text) in ["Second", "Third", "Fourth"].iter().enumerate() {
        said(&server, &mut link, id, text, &format!("Reply {text}"), 10 + n as i64 * 2).await;
    }

    let first_page = answer_text(&server.agent_mcp(&token, id, earlier_turns(json!({"limit": 2}))).await);
    assert!(first_page.contains("Agent: Reply Fourth") && first_page.contains("User: Fourth"), "{first_page}");
    assert!(!first_page.contains("zeppelin") && first_page.contains("Call again with before="), "{first_page}");

    let before: i64 = first_page.rsplit("before=").next().unwrap().split(' ').next().unwrap().parse().unwrap();
    let older = answer_text(&server.agent_mcp(&token, id, earlier_turns(json!({"limit": 2, "before": before}))).await);
    assert!(older.contains("Reply Third") && !older.contains("Fourth"), "{older}");

    let found = answer_text(&server.agent_mcp(&token, id, earlier_turns(json!({"query": "zeppelin"}))).await);
    assert!(found.contains("Plan the zeppelin hangar"), "{found}");

    let elsewhere = answer_text(&server.agent_mcp(&token, id, earlier_turns(json!({"query": "kumquat"}))).await);
    assert_eq!(elsewhere, "No earlier messages match.");
}

pub fn limit_message() -> String {
    let resets = chrono::Utc::now() + chrono::Duration::hours(2);

    format!("Internal error: You've hit your session limit · resets {} (UTC)", resets.format("%-I:%M%P"))
}

async fn switch_on_usage_limit(server: &Server) {
    let (status, _) = server.user.json(server.user.put("/api/v1/usage-limit").json(&json!({"on_usage_limit": "switch"}))).await;
    assert_eq!(status, 200);
}

pub fn with_usage(account: &str, left: u8) -> LoginInfo {
    LoginInfo { usage_left: Some(left), ..login(account, LoginState::SignedIn) }
}

async fn working_on(server: &Server, link: &mut NodeLink, account: &str) -> Uuid {
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": account, "prompt": "Build it"}))).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    next_command(link).await;
    link.start_session(id, "acp-1").await;
    expect_prompt(link).await;

    id
}

#[tokio::test]
async fn a_full_window_switches_to_the_account_with_most_usage_left() {
    let server = Server::start().await;
    switch_on_usage_limit(&server).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 0), with_usage("work@example.org", 30), with_usage("spare@example.org", 80)]).await;
    let id = working_on(&server, &mut link, "sam@example.com").await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: limit_message() }).await;

    match next_command(&mut link).await {
        ControlToNode::CloseSession { conversation_id } => assert_eq!(conversation_id, id),
        other => panic!("expected close_session, got {other:?}"),
    }
    match next_command(&mut link).await {
        ControlToNode::StartSession { resume, account, .. } => assert_eq!((resume.as_deref(), account.as_deref()), (Some("acp-1"), Some("spare@example.org"))),
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(expect_prompt(&mut link).await, "The usage limit was reached on sam@example.com. Continue from where you stopped.");

    let events = server.events(id).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").expect("a continued event");
    assert_eq!((continued["payload"]["reason"].clone(), continued["payload"]["account"].clone()), (json!("usage_limit"), json!("spare@example.org")));
    assert!(server.conversation(id).await["resume_at"].is_null());
}

#[tokio::test]
async fn a_goal_is_held_not_stopped_when_the_switch_fails_and_no_reset_is_known() {
    let server = Server::start().await;
    switch_on_usage_limit(&server).await;
    let mut link = node_with_accounts(&server, "v0.9.0", vec![with_usage("sam@example.com", 0), with_usage("work@example.org", 80)]).await;
    let id = working_on(&server, &mut link, "sam@example.com").await;
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": "Every test passes", "token_budget": 100000}))).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "You've hit your usage limit. Try again later.".into() }).await;
    let user = server.user.clone();
    let goal = common::eventually("the goal to be held", || async { Some(user.json(user.get(&format!("/api/v1/conversations/{id}/goal"))).await.1).filter(|goal| goal["held_since"].is_string()) }).await;
    assert_eq!(goal["state"], "active");
}

#[tokio::test]
async fn a_full_window_with_no_other_account_waits_for_the_reset() {
    let server = Server::start().await;
    switch_on_usage_limit(&server).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 0), with_usage("work@example.org", 0)]).await;
    let id = working_on(&server, &mut link, "sam@example.com").await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: limit_message() }).await;
    let user = server.user.clone();
    common::eventually("the resume to be scheduled", || async { (!user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["resume_at"].is_null()).then_some(()) }).await;

    assert!(server.events(id).await.iter().all(|event| event["kind"] != "continued"));
}
