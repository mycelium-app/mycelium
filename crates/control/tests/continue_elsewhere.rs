mod common;

use common::{NodeLink, Server};
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

fn login(account: &str, state: LoginState) -> LoginInfo {
    LoginInfo { agent: AgentId::Claude, account: Some(account.into()), method: "oauth_token".into(), state, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }
}

async fn next_command(link: &mut NodeLink) -> ControlToNode {
    loop {
        let message = link.next().await;

        if !matches!(message, ControlToNode::Git { .. }) {
            return message;
        }
    }
}

async fn expect_prompt(link: &mut NodeLink) -> String {
    match next_command(link).await {
        ControlToNode::Prompt { text, .. } => text,
        other => panic!("expected prompt, got {other:?}"),
    }
}

async fn enable(server: &Server) {
    let flags = json!({"tasks": false, "workflows": false, "instructions": false, "details": false, "subagents": false, "walkthrough": false, "move_chats": false, "groups": false, "continue_elsewhere": true});
    let (status, _) = server.user.json(server.user.put("/api/v1/experiments").json(&flags)).await;
    assert_eq!(status, 200);
}

async fn node_with_accounts(server: &Server, version: &str, accounts: Vec<LoginInfo>) -> NodeLink {
    let mut link = server.online_node_as("laptop", version).await;
    link.send(NodeToControl::Logins { logins: accounts }).await;

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
async fn continue_is_refused_while_the_experiment_is_off() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (403, Some("experiment_disabled")));
}

#[tokio::test]
async fn continuing_on_another_account_resumes_the_same_session_there() {
    let server = Server::start().await;
    enable(&server).await;
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
async fn continuing_on_an_account_that_is_not_signed_in_is_refused() {
    let server = Server::start().await;
    enable(&server).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedOut)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("account_not_signed_in")));
}

#[tokio::test]
async fn continuing_on_the_account_it_already_uses_is_refused() {
    let server = Server::start().await;
    enable(&server).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![login("sam@example.com", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "sam@example.com"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("same_target")));
}

#[tokio::test]
async fn continuing_while_the_agent_is_working_is_refused() {
    let server = Server::start().await;
    enable(&server).await;
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
    enable(&server).await;
    let mut link = node_with_accounts(&server, "v0.10.0", vec![login("sam@example.com", LoginState::SignedIn), login("work@example.org", LoginState::SignedIn)]).await;
    let id = idle_conversation(&server, &mut link, "sam@example.com").await;

    let (status, body) = continue_on(&server, id, json!({"agent": "claude", "account": "work@example.org"})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_outdated")));
    assert_eq!(body["message"], "Update laptop to continue on another account.");
}
