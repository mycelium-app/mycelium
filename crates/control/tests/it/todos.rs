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

async fn save(server: &Server, conversation: Uuid, todos: Value, tell_agent: bool) {
    let (status, body) = server.user.json(server.user.put(&format!("/api/v1/conversations/{conversation}/todos")).json(&json!({"todos": todos, "tell_agent": tell_agent}))).await;
    assert_eq!(status, 204, "{body}");
}

async fn queued(server: &Server, conversation: Uuid) -> Vec<String> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;

    queue.as_array().unwrap().iter().filter_map(|prompt| prompt["text"].as_str().map(str::to_string)).collect()
}

#[tokio::test]
async fn set_todos_keeps_each_items_description() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    let todos = json!([{"content": "Fix the redirect", "status": "in_progress", "description": "The callback drops the next parameter."}, {"content": "Open a merge request", "status": "pending"}]);

    call(&server, &link.token, Some(conversation.id), json!({"todos": todos})).await;

    assert_eq!(latest_plan(&server, conversation.id).await, todos);
}

#[tokio::test]
async fn saving_without_telling_the_agent_stores_the_list_and_sends_nothing() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Fix the redirect", "status": "pending"}]})).await;
    let edited = json!([{"content": "Fix the redirect", "status": "pending", "description": "Only on Safari."}, {"content": "Ask Sam to review", "status": "pending"}]);

    save(&server, conversation.id, edited.clone(), false).await;

    assert_eq!(latest_plan(&server, conversation.id).await, edited);
    assert_eq!(queued(&server, conversation.id).await, vec!["Fix the login".to_string()]);
}

#[tokio::test]
async fn saving_and_telling_the_agent_sends_what_changed_as_a_message() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Fix the redirect", "status": "in_progress"}, {"content": "Commit the change", "status": "pending"}]})).await;

    save(&server, conversation.id, json!([{"content": "Fix the redirect", "status": "in_progress", "description": "Only on Safari."}, {"content": "Ask Sam to review", "status": "pending"}]), true).await;

    assert_eq!(queued(&server, conversation.id).await, vec!["Fix the login".to_string(), "I changed the to-do list:\n- Added: Ask Sam to review\n- Dismissed: Commit the change\n- Changed the details of Fix the redirect: Only on Safari.".to_string()]);
}

#[tokio::test]
async fn an_item_with_no_text_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;

    let (status, body) = server.user.json(server.user.put(&format!("/api/v1/conversations/{}/todos", conversation.id)).json(&json!({"todos": [{"content": "  ", "status": "pending"}]}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("todo_content_required")));
}

#[tokio::test]
async fn the_agents_next_list_keeps_the_persons_silent_edits_and_says_so() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Fix the redirect", "status": "in_progress"}, {"content": "Commit the change", "status": "pending"}]})).await;
    save(&server, conversation.id, json!([{"content": "Fix the redirect", "status": "in_progress"}, {"content": "Ask Sam to review", "status": "pending"}]), false).await;

    let reply = call(&server, &link.token, Some(conversation.id), json!({"todos": [{"content": "Fix the redirect", "status": "completed"}, {"content": "Commit the change", "status": "in_progress"}]})).await;

    assert_eq!(latest_plan(&server, conversation.id).await, json!([{"content": "Fix the redirect", "status": "completed"}, {"content": "Ask Sam to review", "status": "pending"}]));
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("- Added: Ask Sam to review") && text.contains("- Dismissed: Commit the change"), "{text}");
}

#[tokio::test]
async fn a_harness_plan_keeps_the_persons_silent_edits_until_the_agent_is_told() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Fix the login")).await;
    let plan = |entries: Value| json!({"update": {"sessionUpdate": "plan", "entries": entries}});
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 10, update: plan(json!([{"content": "Fix the redirect", "status": "in_progress", "priority": "medium"}])) }).await;
    link.barrier(conversation.id, 11).await;
    save(&server, conversation.id, json!([{"content": "Fix the redirect", "status": "in_progress"}, {"content": "Ask Sam to review", "status": "pending"}]), false).await;

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 12, update: plan(json!([{"content": "Fix the redirect", "status": "completed", "priority": "medium"}])) }).await;
    link.barrier(conversation.id, 13).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 14, update: plan(json!([{"content": "Fix the redirect", "status": "completed", "priority": "medium"}])) }).await;
    link.barrier(conversation.id, 15).await;

    assert_eq!(latest_plan(&server, conversation.id).await, json!([{"content": "Fix the redirect", "status": "completed", "priority": "medium"}, {"content": "Ask Sam to review", "status": "pending"}]));
}
