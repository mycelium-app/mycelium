mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn started_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    conversation.id
}

async fn teach_me(server: &Server, id: Uuid, on: bool) -> (u16, Value) {
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"teach_me": on}))).await
}

async fn prompt(server: &Server, id: Uuid, text: &str) {
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": text}))).await;
    assert!(status < 300, "{body}");
}

#[tokio::test]
async fn teach_me_adds_the_extension_to_the_session_layer() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let (status, body) = teach_me(&server, id, true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["teach_me"], true);

    prompt(&server, id, "How do I reverse a list?").await;

    match link.next().await {
        ControlToNode::StartSession { instructions, resume, .. } => {
            assert!(instructions.unwrap().contains("Teach me mode is on"));
            assert_eq!(resume.as_deref(), Some("acp-1"));
        }
        other => panic!("expected a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn turning_teach_me_off_drops_the_extension_from_the_next_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    teach_me(&server, id, true).await;
    teach_me(&server, id, false).await;

    prompt(&server, id, "Just do it").await;

    match link.next().await {
        ControlToNode::StartSession { instructions, .. } => assert!(!instructions.unwrap_or_default().contains("Teach me mode is on")),
        other => panic!("expected a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn toggling_teach_me_records_a_divider_event() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    teach_me(&server, id, true).await;
    teach_me(&server, id, true).await;
    teach_me(&server, id, false).await;

    let dividers: Vec<Value> = server.events(id).await.into_iter().filter(|event| event["kind"] == "teach_mode").map(|event| event["payload"]["on"].clone()).collect();
    assert_eq!(dividers, [json!(true), json!(false)]);
}

#[tokio::test]
async fn another_persons_conversation_cannot_be_toggled() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"teach_me": true}))).await;

    assert_eq!(status, 404);
    assert_eq!(server.conversation(id).await["teach_me"], Value::Null);
}
