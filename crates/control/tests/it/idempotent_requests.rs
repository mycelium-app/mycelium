use crate::chats::{add_openai, fake_openai};
use crate::common::{Server, serve};
use serde_json::{Value, json};
use uuid::Uuid;

async fn post(server: &Server, path: &str, body: Value) -> (u16, Value) {
    server.user.json(server.user.post(path).json(&body)).await
}

async fn user_messages(server: &Server, id: Uuid) -> usize {
    server.events(id).await.iter().filter(|event| event["kind"] == "user_message").count()
}

#[tokio::test]
async fn the_same_prompt_posted_twice_with_one_client_id_is_sent_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.barrier(conversation.id, 1).await;
    let path = format!("/api/v1/conversations/{}/prompt", conversation.id);
    let body = json!({"text": "hello", "client_id": Uuid::new_v4()});

    let (first, _) = post(&server, &path, body.clone()).await;
    let (second, _) = post(&server, &path, body).await;

    assert_eq!((first, second), (204, 204));
    assert_eq!(link.expect_prompt().await.1, "hello");
    assert_eq!(user_messages(&server, conversation.id).await, 1);
}

#[tokio::test]
async fn the_same_prompt_queued_twice_with_one_client_id_is_queued_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("first")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    let path = format!("/api/v1/conversations/{}/prompt", conversation.id);
    let body = json!({"text": "second", "client_id": Uuid::new_v4()});

    post(&server, &path, body.clone()).await;
    post(&server, &path, body).await;

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation.id))).await;
    assert_eq!(queue.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn prompts_with_different_client_ids_are_both_sent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("first")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    let path = format!("/api/v1/conversations/{}/prompt", conversation.id);

    post(&server, &path, json!({"text": "same words", "client_id": Uuid::new_v4()})).await;
    post(&server, &path, json!({"text": "same words", "client_id": Uuid::new_v4()})).await;

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation.id))).await;
    assert_eq!(queue.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn prompts_without_a_client_id_are_never_deduplicated() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("first")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    let path = format!("/api/v1/conversations/{}/prompt", conversation.id);

    post(&server, &path, json!({"text": "again"})).await;
    post(&server, &path, json!({"text": "again"})).await;

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation.id))).await;
    assert_eq!(queue.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn the_same_conversation_created_twice_with_one_client_id_is_one_conversation() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let body = json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "client_id": Uuid::new_v4()});

    let (_, first) = post(&server, "/api/v1/conversations", body.clone()).await;
    let (_, second) = post(&server, "/api/v1/conversations", body).await;

    assert_eq!(first["id"], second["id"]);
    let count = sqlx::query_scalar::<_, i64>("select count(*) from conversations where owner = (select owner from conversations where id = $1)").bind(first["id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(&server.pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn conversations_created_with_different_client_ids_are_two_conversations() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (_, first) = post(&server, "/api/v1/conversations", json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "client_id": Uuid::new_v4()})).await;
    let (_, second) = post(&server, "/api/v1/conversations", json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "client_id": Uuid::new_v4()})).await;

    assert_ne!(first["id"], second["id"]);
}

#[tokio::test]
async fn the_same_chat_created_twice_with_one_client_id_is_one_chat_with_one_message() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let body = json!({"provider_id": provider["id"], "prompt": "hi", "client_id": Uuid::new_v4()});

    let (_, first) = post(&server, "/api/v1/chats", body.clone()).await;
    let (_, second) = post(&server, "/api/v1/chats", body).await;

    assert_eq!(first["id"], second["id"]);
    assert_eq!(user_messages(&server, first["id"].as_str().unwrap().parse().unwrap()).await, 1);
}
