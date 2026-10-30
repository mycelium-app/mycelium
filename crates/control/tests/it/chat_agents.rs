use crate::common;
use crate::common::{NodeLink, Server};
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::json;
use uuid::Uuid;

fn claude_login(account: &str) -> LoginInfo {
    LoginInfo { managed: false, agent: AgentId::Claude, account: Some(account.into()), method: "oauth_token".into(), state: LoginState::SignedIn, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }
}

async fn node_signed_in(server: &Server, name: &str, account: &str) -> NodeLink {
    let mut link = server.online_node(name).await;
    link.send(NodeToControl::Logins { logins: vec![claude_login(account)] }).await;

    let user = server.user.clone();
    let id = link.id;
    common::eventually("the account", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;
        let node = nodes.as_array().unwrap().iter().find(|node| node["id"] == json!(id)).unwrap().clone();

        (!node["logins"].as_array().unwrap().is_empty()).then_some(())
    })
    .await;

    link
}

#[tokio::test]
async fn a_chat_on_an_agent_account_gets_a_node_and_scratch_folder_and_its_first_prompt_reaches_the_agent() {
    let server = Server::start().await;
    let mut link = node_signed_in(&server, "laptop", "sam@example.com").await;

    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude", "account": "sam@example.com", "prompt": "What is in this folder?"}))).await;
    let id: Uuid = chat["id"].as_str().unwrap().parse().unwrap();

    assert_eq!(status, 201, "{chat}");
    assert_eq!((chat["agent"].clone(), chat["node_id"].clone(), chat["scratch"].clone(), chat["account"].clone()), (json!("claude"), json!(link.id), json!(true), json!("sam@example.com")));

    let folder = match link.next().await {
        ControlToNode::WriteFolder { root, files, replace } => {
            assert!(files.is_empty() && replace);
            root
        }
        other => panic!("expected write_folder, got {other:?}"),
    };
    let cwd = match link.next().await {
        ControlToNode::StartSession { cwd, .. } => cwd,
        other => panic!("expected start_session, got {other:?}"),
    };

    assert_eq!(folder, format!(".mycelium-assistants/{id}"));
    assert_eq!(cwd, folder);

    link.start_session(id, "acp-1").await;
    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "What is in this folder?");

    let events = server.events(id).await;
    let started = events.iter().find(|event| event["kind"] == "continued").unwrap();
    assert_eq!((started["payload"]["kind"].clone(), started["payload"]["node_name"].clone()), (json!("scratch"), json!("laptop")));
}

#[tokio::test]
async fn any_account_picks_a_node_with_the_agent_signed_in() {
    let server = Server::start().await;
    let link = node_signed_in(&server, "laptop", "sam@example.com").await;

    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude"}))).await;

    assert_eq!(status, 201, "{chat}");
    assert_eq!((chat["node_id"].clone(), chat["account"].clone(), chat["routing"].clone()), (json!(link.id), json!("sam@example.com"), json!("claude_any")));
}

#[tokio::test]
async fn a_chat_on_an_agent_starts_on_the_node_the_request_names() {
    let server = Server::start().await;
    let _first = node_signed_in(&server, "atlas", "sam@example.com").await;
    let second = node_signed_in(&server, "laptop", "sam@example.com").await;

    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude", "account": "sam@example.com", "node_id": second.id}))).await;

    assert_eq!(status, 201, "{chat}");
    assert_eq!(chat["node_id"], json!(second.id));
}

#[tokio::test]
async fn a_chat_on_a_node_without_that_account_is_refused() {
    let server = Server::start().await;
    let _signed_in = node_signed_in(&server, "atlas", "sam@example.com").await;
    let bare = server.online_node("laptop").await;

    let (status, body) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude", "account": "sam@example.com", "node_id": bare.id}))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("no_node_online")));
}

#[tokio::test]
async fn a_chat_on_an_agent_with_no_node_online_says_what_to_do() {
    let server = Server::start().await;
    server.join("node", "willow").await;

    let (status, body) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude", "account": "sam@example.com", "prompt": "Hello"}))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("no_node_online")));
    assert_eq!(body["message"], json!("No node with Claude Code signed in as sam@example.com is online. Start one from Nodes, or choose another account."));
}

#[tokio::test]
async fn deleting_a_chat_on_an_agent_deletes_its_scratch_folder() {
    let server = Server::start().await;
    let mut link = node_signed_in(&server, "laptop", "sam@example.com").await;
    let (_, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"agent": "claude", "account": "sam@example.com"}))).await;
    let id: Uuid = chat["id"].as_str().unwrap().parse().unwrap();

    assert!(matches!(link.next().await, ControlToNode::WriteFolder { .. }));
    link.expect_start_session().await;
    link.start_session(id, "acp-1").await;
    let user = server.user.clone();
    common::eventually("the session", || async { (!user.json(user.get(&format!("/api/v1/conversations/{id}"))).await.1["acp_session_id"].is_null()).then_some(()) }).await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"archived": true}))).await;
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}"))).await;

    assert_eq!(status, 204);

    let removed = loop {
        match link.next().await {
            ControlToNode::DeleteFolder { root } => break root,
            ControlToNode::CloseSession { .. } => {}
            other => panic!("expected delete_folder, got {other:?}"),
        }
    };

    assert_eq!(removed, format!(".mycelium-assistants/{id}"));
}
