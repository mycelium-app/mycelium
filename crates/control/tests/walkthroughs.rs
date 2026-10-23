mod common;

use common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, token: &str, conversation: Option<Uuid>, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let mut request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": arguments}}));

    if let Some(conversation) = conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn latest(server: &Server, conversation: Uuid) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/walkthrough"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn steps() -> Value {
    json!({"steps": [
        {"title": "Add the shared strings file", "explanation": "New file.", "files": [{"path": "src/strings.ts", "diff": "+export const strings = {}"}]},
        {"title": "Restore reads its copy from strings", "key": true, "explanation": "The change itself.", "files": [{"path": "src/Screen.tsx", "hunks": ["-const title = \"Restore\"", "+const title = strings.title"]}]},
    ]})
}

#[tokio::test]
async fn the_walkthrough_tool_is_listed_with_a_description_that_says_when_to_use_it() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let anonymous = server.anonymous();

    let (_, tools) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;

    let tool = tools["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "walkthrough").unwrap().clone();
    assert!(tool["description"].as_str().unwrap().contains("when the person asks for one"));
}

#[tokio::test]
async fn a_walkthrough_is_stored_for_the_conversation_with_steps_in_order_and_the_key_step_marked() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let result = call(&server, &link.token, Some(conversation.id), steps()).await;

    assert_eq!(result["isError"], false, "{result}");
    let stored = latest(&server, conversation.id).await;
    let titles: Vec<&str> = stored["steps"].as_array().unwrap().iter().map(|step| step["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Add the shared strings file", "Restore reads its copy from strings"]);
    assert_eq!((stored["steps"][0]["key"].clone(), stored["steps"][1]["key"].clone()), (json!(false), json!(true)));
    assert_eq!(stored["steps"][0]["files"][0], json!({"path": "src/strings.ts", "diff": "+export const strings = {}"}));
}

#[tokio::test]
async fn hunks_are_joined_into_the_files_diff() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    call(&server, &link.token, Some(conversation.id), steps()).await;

    let stored = latest(&server, conversation.id).await;
    assert_eq!(stored["steps"][1]["files"][0]["diff"], "-const title = \"Restore\"\n+const title = strings.title");
}

#[tokio::test]
async fn a_later_walkthrough_replaces_the_earlier_one_and_the_earlier_one_is_kept() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &link.token, Some(conversation.id), steps()).await;

    call(&server, &link.token, Some(conversation.id), json!({"steps": [{"title": "Only step", "files": [{"path": "a.rs", "diff": "+a"}]}]})).await;

    let stored = latest(&server, conversation.id).await;
    assert_eq!(stored["steps"].as_array().unwrap().len(), 1);
    let kept = sqlx::query_scalar::<_, i64>("select count(*) from walkthroughs where conversation_id = $1").bind(conversation.id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(kept, 2);
}

#[tokio::test]
async fn a_conversation_without_a_walkthrough_has_none() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_walkthrough_without_steps_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let result = call(&server, &link.token, Some(conversation.id), json!({"steps": []})).await;

    assert_eq!(result["isError"], true);
    assert!(result["content"][0]["text"].as_str().unwrap().contains("Give the walkthrough's steps"));
    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_step_without_a_title_and_a_file_without_a_diff_are_refused_and_nothing_is_stored() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let untitled = call(&server, &link.token, Some(conversation.id), json!({"steps": [{"explanation": "x"}]})).await;
    assert_eq!(untitled["isError"], true);
    assert!(untitled["content"][0]["text"].as_str().unwrap().contains("Step 1 has no title"));

    let no_diff = call(&server, &link.token, Some(conversation.id), json!({"steps": [{"title": "t", "files": [{"path": "a.rs"}]}]})).await;
    assert_eq!(no_diff["isError"], true);
    assert!(no_diff["content"][0]["text"].as_str().unwrap().contains("a.rs in step 1 has no diff"));

    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_call_that_does_not_come_from_a_conversation_is_refused() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let result = call(&server, &token, None, steps()).await;

    assert_eq!(result["isError"], true);
}
