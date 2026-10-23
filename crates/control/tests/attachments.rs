mod common;

use common::{Client, Server};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

const PNG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

async fn upload(client: &Client, conversation: Uuid, name: &str, media_type: &str, bytes: Vec<u8>) -> (u16, Value) {
    client.json(client.post(&format!("/api/v1/conversations/{conversation}/attachments")).query(&[("name", name)]).header("content-type", media_type).body(bytes)).await
}

async fn started(server: &Server) -> (common::NodeLink, Uuid) {
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("hello")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    link.send(mycelium_proto::link::NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

    (link, conversation.id)
}

#[tokio::test]
async fn an_uploaded_file_is_served_back_to_its_owner_with_its_type_and_caching_headers() {
    let server = Server::start().await;
    let (_link, id) = started(&server).await;

    let (status, attachment) = upload(&server.user, id, "shot.png", "image/png", PNG.to_vec()).await;
    let sha256 = attachment["sha256"].as_str().unwrap();
    let served = server.user.get(&format!("/api/v1/attachments/{sha256}")).send().await.unwrap();

    assert_eq!((status, attachment["name"].as_str(), attachment["media_type"].as_str(), attachment["size"].as_i64()), (201, Some("shot.png"), Some("image/png"), Some(8)));
    assert_eq!(served.headers()["content-type"], "image/png");
    assert!(served.headers()["cache-control"].to_str().unwrap().contains("immutable"));
    assert_eq!(served.bytes().await.unwrap().as_ref(), PNG);
}

#[tokio::test]
async fn the_same_bytes_are_stored_once_however_often_they_are_uploaded() {
    let server = Server::start().await;
    let (_link, id) = started(&server).await;

    let (_, first) = upload(&server.user, id, "a.png", "image/png", PNG.to_vec()).await;
    let (_, second) = upload(&server.user, id, "copy of a.png", "image/png", PNG.to_vec()).await;
    let stored = std::fs::read_dir(server.data_dir.join("attachments")).unwrap().count();
    let rows: i64 = sqlx::query_scalar("select count(*) from attachments").fetch_one(&server.pool).await.unwrap();

    assert_eq!(first["sha256"], second["sha256"]);
    assert_eq!((stored, rows), (1, 1));
}

#[tokio::test]
async fn another_person_cannot_read_an_attachment_or_attach_it_to_their_own_message() {
    let server = Server::start().await;
    let (_link, id) = started(&server).await;
    let (_, attachment) = upload(&server.user, id, "secret.png", "image/png", PNG.to_vec()).await;
    let sha256 = attachment["sha256"].as_str().unwrap();
    let other = server.second_user("other@example.com").await;

    let read = other.get(&format!("/api/v1/attachments/{sha256}")).send().await.unwrap().status();
    let (upload_status, _) = upload(&other, id, "x.png", "image/png", PNG.to_vec()).await;
    let anonymous = server.anonymous().get(&format!("/api/v1/attachments/{sha256}")).send().await.unwrap().status();

    assert_eq!((read.as_u16(), upload_status, anonymous.as_u16()), (404, 404, 401));
}

#[tokio::test]
async fn a_file_over_the_limit_is_refused_with_what_to_do() {
    let server = Server::start().await;
    let (_link, id) = started(&server).await;

    let (status, error) = upload(&server.user, id, "big.bin", "application/octet-stream", vec![1; 20 * 1024 * 1024 + 1]).await;
    let (empty_status, _) = upload(&server.user, id, "empty.txt", "text/plain", Vec::new()).await;

    assert_eq!((status, error["error"].as_str(), error["message"].as_str()), (413, Some("attachment_too_large"), Some("The file is larger than 20 MB. Choose a smaller file.")));
    assert_eq!(empty_status, 400);
}

#[tokio::test]
async fn an_image_sent_with_a_message_is_recorded_on_the_event_and_carried_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = started(&server).await;
    let (_, attachment) = upload(&server.user, id, "shot.png", "image/png", PNG.to_vec()).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "what is this", "attachments": [attachment]}))).await;
    let sent = match link.next().await {
        ControlToNode::Prompt { attachments, text, .. } => (text, attachments),
        other => panic!("expected a prompt, got {other:?}"),
    };
    let events = server.events(id).await;
    let message = events.iter().rev().find(|event| event["kind"] == "user_message").unwrap();

    assert_eq!(status, 204);
    assert_eq!(sent.0, "what is this");
    assert_eq!((sent.1[0].name.as_str(), sent.1[0].media_type.as_str(), sent.1[0].data.as_str()), ("shot.png", "image/png", "iVBORw0KGgo="));
    assert_eq!(message["payload"]["attachments"], json!([attachment]));
}

#[tokio::test]
async fn a_message_may_be_only_attachments_and_one_that_was_never_uploaded_is_refused() {
    let server = Server::start().await;
    let (mut link, id) = started(&server).await;
    let (_, attachment) = upload(&server.user, id, "shot.png", "image/png", PNG.to_vec()).await;
    let missing = json!({"sha256": "0".repeat(64), "name": "ghost.png", "media_type": "image/png", "size": 1});

    let (sent, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"attachments": [attachment]}))).await;
    let (refused, error) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "x", "attachments": [missing]}))).await;
    let delivered = matches!(link.next().await, ControlToNode::Prompt { attachments, .. } if attachments.len() == 1);

    assert_eq!((sent, refused, error["error"].as_str()), (204, 400, Some("attachment_missing")));
    assert!(delivered);
}

#[tokio::test]
async fn an_attachment_on_a_queued_message_waits_in_the_queue_and_arrives_with_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("hello")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    let (_, attachment) = upload(&server.user, conversation.id, "shot.png", "image/png", PNG.to_vec()).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{}/prompt", conversation.id)).json(&json!({"text": "later", "attachments": [attachment]}))).await;
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation.id))).await;

    assert_eq!(queue[0]["attachments"], json!([attachment]));
}
