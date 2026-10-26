mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
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

async fn walkthrough(server: &Server, link: &mut NodeLink, conversation: Uuid, extra: Value) -> Value {
    let token = link.token.clone();

    walkthrough_reading(server, Some(link), token, conversation, extra).await
}

async fn walkthrough_reading(server: &Server, link: Option<&mut NodeLink>, token: String, conversation: Uuid, extra: Value) -> Value {
    let mut arguments = json!({"steps": [
        {"title": "Add the strings file", "files": [{"path": "src/strings.ts", "diff": "+export const strings = {}"}]},
        {"title": "Read the title from strings", "files": [{"path": "src/Screen.tsx", "diff": "+const title = strings.title"}]},
    ]});
    arguments.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let diff = "diff --git a/src/strings.ts b/src/strings.ts\n--- a/src/strings.ts\n+++ b/src/strings.ts\n@@ -1 +1 @@\n+export const strings = {}\ndiff --git a/src/Screen.tsx b/src/Screen.tsx\n--- a/src/Screen.tsx\n+++ b/src/Screen.tsx\n@@ -1 +1 @@\n+const title = strings.title\n";
    let anonymous = server.anonymous();
    let asking = tokio::spawn(async move {
        let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).header("x-mycelium-conversation", conversation.to_string()).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": arguments}}));
        let (_, body) = anonymous.json(request).await;

        body["result"].clone()
    });

    if let Some(link) = link {
        loop {
            if let ControlToNode::Git { request_id, .. } = link.next().await {
                link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"diff": diff, "truncated": false}), error: None }).await;

                break;
            }
        }
    }

    asking.await.unwrap()
}

fn question() -> Value {
    json!({"checks": [{"step": 2, "question": "Where does the title come from?", "choices": ["A literal", "The strings file"], "answer": 1, "explanation": "Step 2 reads it from strings."}]})
}

async fn latest_walkthrough(server: &Server, conversation: Uuid) -> Value {
    server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/walkthrough"))).await.1
}

async fn answer(server: &Server, walkthrough: &str, step: i32, choice: i32) -> (u16, Value) {
    server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{walkthrough}/answers/{step}")).json(&json!({"choice": choice}))).await
}

#[tokio::test]
async fn a_check_answer_says_correct_or_not_quite() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let stored = walkthrough(&server, &mut link, id, question()).await;
    assert_eq!(stored["isError"], false, "{stored}");
    let shown = latest_walkthrough(&server, id).await;
    let walkthrough_id = shown["id"].as_str().unwrap().to_string();
    assert_eq!(shown["checks"][0]["choices"], json!(["A literal", "The strings file"]));
    assert!(shown["checks"][0].get("answer").is_none());

    let (status, wrong) = answer(&server, &walkthrough_id, 2, 0).await;
    assert_eq!((status, wrong["correct"].clone()), (200, json!(false)));
    let (_, right) = answer(&server, &walkthrough_id, 2, 1).await;
    assert_eq!((right["correct"].clone(), right["explanation"].clone()), (json!(true), json!("Step 2 reads it from strings.")));

    let (_, recorded) = server.user.json(server.user.get(&format!("/api/v1/walkthroughs/{walkthrough_id}/answers"))).await;
    assert_eq!(recorded, json!([{"step": 2, "choice": 1, "correct": true, "explanation": ""}]));
}

#[tokio::test]
async fn answering_a_step_without_a_check_is_refused_and_records_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    walkthrough(&server, &mut link, id, question()).await;
    let walkthrough_id = latest_walkthrough(&server, id).await["id"].as_str().unwrap().to_string();

    let (status, body) = answer(&server, &walkthrough_id, 1, 0).await;

    assert_eq!((status, body["error"].clone()), (400, json!("no_such_check")));
    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.put(&format!("/api/v1/walkthroughs/{walkthrough_id}/answers/2")).json(&json!({"choice": 1}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn walkthrough_with_a_bad_check_stores_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    for checks in [
        json!([{"step": 3, "question": "q", "choices": ["a", "b"], "answer": 0}]),
        json!([{"step": 1, "question": "q", "choices": ["a"], "answer": 0}]),
        json!([{"step": 1, "question": "q", "choices": ["a", "b"], "answer": 2}]),
    ] {
        let result = walkthrough_reading(&server, None, link.token.clone(), id, json!({"checks": checks})).await;

        assert_eq!(result["isError"], true, "{result}");
        assert!(result["content"][0]["text"].as_str().unwrap().starts_with("Nothing was stored."));
    }

    assert_eq!(latest_walkthrough(&server, id).await, Value::Null);
}
