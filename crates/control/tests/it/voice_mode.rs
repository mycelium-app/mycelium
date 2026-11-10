use crate::chats::{add_openai, fake_openai, new_chat, reply_finished};
use crate::common::{NodeLink, Server, eventually, serve};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn started_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    conversation.id
}

async fn voice_mode(server: &Server, id: &str, on: bool) -> (u16, Value) {
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"voice_mode": on}))).await
}

async fn prompt(server: &Server, id: &str, body: Value) {
    let (status, reply) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&body)).await;
    assert!(status < 300, "{reply}");
}

async fn next_message(link: &mut NodeLink) -> ControlToNode {
    loop {
        match link.next().await {
            message @ (ControlToNode::Prompt { .. } | ControlToNode::StartSession { .. }) => return message,
            _ => continue,
        }
    }
}

async fn next_prompt(link: &mut NodeLink) -> String {
    match next_message(link).await {
        ControlToNode::Prompt { text, .. } => text,
        other => panic!("expected the prompt without a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn entering_voice_mode_tells_the_agent_with_its_next_message_and_keeps_the_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    let (status, body) = voice_mode(&server, &id.to_string(), true).await;
    prompt(&server, &id.to_string(), json!({"text": "Which tests are failing?"})).await;
    let text = next_prompt(&mut link).await;
    let said: Vec<Value> = server.events(id).await.into_iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].clone()).collect();

    assert_eq!(status, 200, "{body}");
    assert_eq!(body["voice_mode"], true);
    assert!(text.starts_with("Voice mode is on") && text.contains("mycelium-spoken"), "{text}");
    assert!(text.ends_with("Which tests are failing?"), "{text}");
    assert_eq!(said, [json!("Which tests are failing?")]);
}

#[tokio::test]
async fn leaving_voice_mode_tells_the_agent_with_its_next_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    voice_mode(&server, &id.to_string(), true).await;
    prompt(&server, &id.to_string(), json!({"text": "Hello"})).await;
    next_prompt(&mut link).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

    voice_mode(&server, &id.to_string(), false).await;
    prompt(&server, &id.to_string(), json!({"text": "Back to typing"})).await;
    let text = next_prompt(&mut link).await;

    assert!(text.starts_with("Voice mode is off"), "{text}");
    assert!(!text.contains("Voice mode is on"), "{text}");
}

#[tokio::test]
async fn a_new_session_carries_the_spoken_block_instructions_while_voice_mode_is_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    voice_mode(&server, &id.to_string(), true).await;
    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"teach_me": true}))).await;
    assert_eq!(status, 200);

    prompt(&server, &id.to_string(), json!({"text": "Hello"})).await;

    match next_message(&mut link).await {
        ControlToNode::StartSession { instructions, .. } => assert!(instructions.unwrap_or_default().contains("language is mycelium-spoken")),
        other => panic!("expected a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn toggling_voice_mode_records_a_divider_event_only_on_a_change() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    voice_mode(&server, &id.to_string(), true).await;
    voice_mode(&server, &id.to_string(), true).await;
    voice_mode(&server, &id.to_string(), false).await;

    let dividers: Vec<Value> = server.events(id).await.into_iter().filter(|event| event["kind"] == "voice_mode").map(|event| event["payload"]["on"].clone()).collect();
    assert_eq!(dividers, [json!(true), json!(false)]);
}

#[tokio::test]
async fn a_spoken_message_is_marked_as_spoken_in_the_thread() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    prompt(&server, &id.to_string(), json!({"text": "Read me the plan", "spoken": true})).await;
    next_prompt(&mut link).await;

    let message = server.events(id).await.into_iter().find(|event| event["kind"] == "user_message").unwrap();
    assert_eq!(message["payload"]["spoken"], true);
}

#[tokio::test]
async fn a_spoken_message_queued_behind_a_running_turn_does_not_mark_the_earlier_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    prompt(&server, &id.to_string(), json!({"text": "Typed first"})).await;
    next_prompt(&mut link).await;

    prompt(&server, &id.to_string(), json!({"text": "Said while it works", "spoken": true})).await;

    let message = server.events(id).await.into_iter().find(|event| event["kind"] == "user_message").unwrap();
    assert_eq!(message["payload"]["text"], "Typed first");
    assert!(message["payload"].get("spoken").is_none(), "{message}");
}

#[tokio::test]
async fn a_chat_in_voice_mode_gets_the_spoken_block_instructions_on_its_next_reply() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "Plan the rollout").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;

    voice_mode(&server, id, true).await;
    prompt(&server, id, json!({"text": "And then?"})).await;
    eventually("the second reply", || async { (server.events(id.parse().unwrap()).await.iter().filter(|event| event["kind"] == "turn_end").count() == 2).then_some(()) }).await;
    let requests = fake.requests.lock().unwrap().clone();

    assert!(!requests[0]["messages"][0]["content"].as_str().unwrap().contains("mycelium-spoken"));
    assert!(requests[1]["messages"][0]["content"].as_str().unwrap().contains("language is mycelium-spoken"));
}
