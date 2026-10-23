mod common;

use common::Server;
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::json;

fn login(account: &str, state: LoginState) -> LoginInfo {
    LoginInfo { agent: AgentId::Claude, account: Some(account.into()), method: "oauth_token".into(), state }
}

async fn node_with_accounts(server: &Server, accounts: Vec<LoginInfo>) -> common::NodeLink {
    let mut link = server.online_node("laptop").await;
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

#[tokio::test]
async fn a_node_lists_every_account_signed_in_to_one_agent_and_forgets_the_ones_it_stops_reporting() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let accounts: Vec<&str> = nodes[0]["logins"].as_array().unwrap().iter().map(|login| login["account"].as_str().unwrap()).collect();
    assert_eq!(accounts, vec!["sam@example.com", "work@acme.example"]);

    link.send(NodeToControl::Logins { logins: vec![login("work@acme.example", LoginState::SignedIn)] }).await;

    let user = server.user.clone();
    common::eventually("the dropped account", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["logins"].as_array().unwrap().len() == 1).then_some(())
    })
    .await;
}

#[tokio::test]
async fn a_conversation_starts_as_the_account_it_was_created_with() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;

    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": "work@acme.example"}))).await;

    assert_eq!(status, 201, "{body}");
    assert_eq!(body["account"], "work@acme.example");

    match link.next().await {
        ControlToNode::StartSession { account, .. } => assert_eq!(account.as_deref(), Some("work@acme.example")),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn an_account_that_is_not_signed_in_on_the_node_is_refused_with_a_reason() {
    let server = Server::start().await;
    let link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("side@example.com", LoginState::SignedOut)]).await;

    for account in ["side@example.com", "nobody@example.com"] {
        let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": account}))).await;

        assert_eq!(status, 409);
        assert_eq!(body["error"], "account_not_signed_in");
        assert_eq!(body["message"], format!("{account} is not signed in on this node. Sign in to it there, or choose another account."));
    }
}

#[tokio::test]
async fn the_account_can_change_until_the_conversation_has_a_session_and_is_then_locked() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;
    let created = server.create_conversation(link.id, None).await;
    let conversation_id = link.expect_start_session().await;
    let path = format!("/api/v1/conversations/{}", created.id);

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["account"], "work@acme.example");

    link.start_session(conversation_id, "s1").await;
    let user = server.user.clone();
    common::eventually("the session", || async { (!user.json(user.get(&path)).await.1["acp_session_id"].is_null()).then_some(()) }).await;

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"account": "sam@example.com"}))).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "account_locked");

    let (status, _) = server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_sign_in_can_name_the_account_it_is_for() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "oauth_token", "account": "  work@acme.example "}))).await;
    assert_eq!(status, 200);

    match link.next().await {
        ControlToNode::StartLogin { account, .. } => assert_eq!(account.as_deref(), Some("work@acme.example")),
        other => panic!("expected start_login, got {other:?}"),
    }
}
