use crate::common::{NodeLink, Server};
use crate::continue_elsewhere;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

const NUDGE: &str = "Before you stop, update your to-do list with set_todos";

async fn assistant_thread(server: &Server, link: &mut NodeLink, enforce: bool) -> Uuid {
    continue_elsewhere::allow_any_model_on(server, link).await;
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Work", "instructions": "Be brief."}))).await;
    assert_eq!(status, 201, "{assistant}");
    let id = assistant["id"].as_str().unwrap();
    let (status, patched) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"enforce_todos": enforce}))).await;
    assert_eq!((status, patched["enforce_todos"].as_bool().unwrap_or(false)), (200, enforce), "{patched}");
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/prompt")).json(&json!({"text": "Plan the release", "model": {"agent": "claude"}, "node_id": link.id}))).await;
    assert_eq!(status, 200, "{started}");
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();

    loop {
        if let ControlToNode::StartSession { .. } = link.next().await {
            break;
        }
    }

    link.start_session(conversation, "acp-work").await;
    link.expect_prompt().await;

    conversation
}

async fn end_turn(link: &mut NodeLink, id: Uuid, seq: i64) {
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(id, seq + 1).await;
}

async fn set_todos(server: &Server, link: &NodeLink, id: Uuid, todos: Value) {
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "set_todos", "arguments": {"todos": todos}}});
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", id.to_string()).json(&call)).await;
    assert_eq!((status, body["result"]["isError"].clone()), (200, json!(false)), "{body}");
}

async fn nudges(server: &Server, id: Uuid) -> usize {
    server.events(id).await.iter().filter(|event| event["kind"] == "user_message" && event["payload"]["text"].as_str().unwrap_or_default().starts_with(NUDGE)).count()
}

#[tokio::test]
async fn a_turn_that_never_set_the_to_do_list_gets_one_nudge_and_only_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = assistant_thread(&server, &mut link, true).await;

    end_turn(&mut link, id, 1).await;
    assert_eq!(nudges(&server, id).await, 1);
    end_turn(&mut link, id, 3).await;

    assert_eq!(nudges(&server, id).await, 1);
}

#[tokio::test]
async fn a_turn_that_set_the_list_without_changing_it_is_not_nudged() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = assistant_thread(&server, &mut link, true).await;
    let list = json!([{"content": "Tag the release", "status": "in_progress"}]);
    set_todos(&server, &link, id, list.clone()).await;
    set_todos(&server, &link, id, list).await;

    end_turn(&mut link, id, 1).await;

    assert_eq!(nudges(&server, id).await, 0);
}

#[tokio::test]
async fn with_the_setting_off_a_turn_without_the_list_is_not_nudged() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = assistant_thread(&server, &mut link, false).await;

    end_turn(&mut link, id, 1).await;

    assert_eq!(nudges(&server, id).await, 0);
}
