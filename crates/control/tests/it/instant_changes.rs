use crate::common::{NodeLink, Server};
use crate::watches::begun;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::json;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROMPT_INTERRUPTS_WITHIN: Duration = Duration::from_secs(1);

async fn until_cancelled(link: &mut NodeLink, id: Uuid) {
    loop {
        if let ControlToNode::Cancel { conversation_id } = link.next().await
            && conversation_id == id
        {
            return;
        }
    }
}

#[tokio::test]
async fn changing_the_model_mid_turn_interrupts_the_turn_and_the_queued_message_runs_on_the_new_model() {
    let server = Server::start().await;
    let mut link = server.online_node("atlas").await;
    let id = begun(&server, &mut link, "Refactor the importer").await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Then add tests"}))).await;
    let user = server.user.clone();
    let asked = Instant::now();
    let change = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"model": "opus"}))).await.0 });

    until_cancelled(&mut link, id).await;
    let interrupted_after = asked.elapsed();
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "cancelled".into(), usage: None }).await;
    let mut seen = Vec::new();

    loop {
        match link.next().await {
            ControlToNode::SetConfig { conversation_id, model, .. } if conversation_id == id => seen.push(format!("model {}", model.unwrap_or_default())),
            ControlToNode::Prompt { conversation_id, text, .. } if conversation_id == id => {
                seen.push(format!("prompt {text}"));
                break;
            }
            _ => {}
        }
    }

    assert!(interrupted_after < PROMPT_INTERRUPTS_WITHIN, "the turn was interrupted after {interrupted_after:?}");
    assert_eq!(change.await.unwrap(), 204);
    assert_eq!(seen, ["model opus", "prompt Then add tests"]);
}
