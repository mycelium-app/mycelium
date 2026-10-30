use crate::common::Server;
use mycelium_proto::link::NodeToControl;
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, token: &str, conversation: Option<Uuid>, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let mut request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "set_todos", "arguments": arguments}}));

    if let Some(conversation) = conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn latest_plan(server: &Server, conversation: Uuid) -> Value {
    let events = server.events(conversation).await;
    let plan = events.iter().rev().find(|event| event["kind"] == "acp_update" && event["payload"]["update"]["sessionUpdate"] == "plan");

    plan.expect("a plan event")["payload"]["update"]["entries"].clone()
}

#[tokio::test]
async fn set_todos_records_the_whole_list_as_the_conversations_plan() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    let todos = json!([{"content": "Write the test", "status": "completed"}, {"content": "Fix the redirect", "status": "in_progress"}, {"content": "Open a merge request", "status": "pending"}]);

    let reply = call(&server, &link.token, Some(conversation.id), json!({"todos": todos})).await;

    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(latest_plan(&server, conversation.id).await, todos);
}

#[tokio::test]
async fn whichever_list_arrives_last_is_the_current_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;

    call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "From the tool", "status": "pending"}]})).await;
    let plan = json!({"update": {"sessionUpdate": "plan", "entries": [{"content": "From the agent", "status": "in_progress", "priority": "medium"}]}});
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 10, update: plan }).await;
    link.barrier(conversation.id, 11).await;

    assert_eq!(latest_plan(&server, conversation.id).await[0]["content"], "From the agent");

    call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Back to the tool", "status": "completed"}]})).await;

    assert_eq!(latest_plan(&server, conversation.id).await, json!([{"content": "Back to the tool", "status": "completed"}]));
}

#[tokio::test]
async fn set_todos_refuses_an_unknown_status_and_a_missing_list() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;

    let unknown = call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Fix it", "status": "half done"}]})).await;
    assert_eq!(unknown["error"]["code"], -32602);

    let missing = call(&server, &link.token, Some(conversation.id), json!({})).await;
    assert_eq!(missing["error"]["code"], -32602);
}
