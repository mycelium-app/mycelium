use crate::common::{NodeLink, Server};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, link: &NodeLink, conversation: Uuid, request: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", conversation.to_string()).json(&request);
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn call_tool(server: &Server, link: &NodeLink, conversation: Uuid, tool: &str, arguments: Value) -> Value {
    call(server, link, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})).await
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn conversation(server: &Server, link: &mut NodeLink, title: &str) -> Uuid {
    let created = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(created.id, &format!("acp-{}", created.id)).await;
    link.expect_prompt().await;
    sqlx::query("update conversations set title = $2 where id = $1").bind(created.id).bind(title).execute(&server.pool).await.unwrap();

    created.id
}

async fn voice_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let id = conversation(server, link, "Voice").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    let assistant = sqlx::query_scalar::<_, Uuid>("insert into assistants (owner, name, description, builtin_key) values ($1, 'Voice', '', 'voice') returning id").bind(owner).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set assistant_id = $2 where id = $1").bind(id).bind(assistant).execute(&server.pool).await.unwrap();

    id
}

async fn waiting(server: &Server, link: &mut NodeLink, title: &str) -> Uuid {
    let id = conversation(server, link, title).await;
    crate::watches::end_turn(link, id, 2).await;
    sqlx::query("update conversations set status = 'input' where id = $1").bind(id).execute(&server.pool).await.unwrap();

    id
}

async fn event(server: &Server, conversation: Uuid, kind: &str, payload: Value) -> i64 {
    sqlx::query_scalar::<_, i64>("insert into events (conversation_id, seq, kind, payload) values ($1, (select coalesce(max(seq), 0) + 1 from events where conversation_id = $1), $2, $3) returning seq").bind(conversation).bind(kind).bind(payload).fetch_one(&server.pool).await.unwrap()
}

async fn proposal(server: &Server, source: Uuid, target: Uuid) -> (Uuid, String) {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(source).fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into message_proposals (owner, conversation_id, target_id, message, reason) values ($1, $2, $3, 'The schema is ready', 'You asked') returning id").bind(owner).bind(source).bind(target).fetch_one(&server.pool).await.unwrap();
    let seq = event(server, source, "acp_update", json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "send_to_conversation"}})).await;
    event(server, source, "acp_update", json!({"update": {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "rawOutput": format!("Waiting for the person to approve sending this to Target (proposal {id}).")}})).await;

    (id, format!("{source}/e{seq}"))
}

async fn workflow_gate(server: &Server, conversation: Uuid) -> Uuid {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation).fetch_one(&server.pool).await.unwrap();
    let steps = json!([{"key": "review", "status": "gate", "conversation_id": null, "workflow_id": null, "output": ""}]);

    sqlx::query_scalar::<_, Uuid>("insert into workflows (id, owner, conversation_id, title, status, kind, plan, steps) values (gen_random_uuid(), $1, $2, 'Release', 'running', 'mycelium', $4, $3) returning id").bind(owner).bind(conversation).bind(steps).bind(json!({"title": "Release", "steps": [{"key": "review", "title": "Review", "gate": true}]})).fetch_one(&server.pool).await.unwrap()
}

async fn updated_at(server: &Server, conversation: Uuid, at: &str) {
    sqlx::query("update conversations set updated_at = $2::timestamptz where id = $1").bind(conversation).bind(at).execute(&server.pool).await.unwrap();
}

#[tokio::test]
async fn only_the_voice_assistant_sees_and_may_call_the_inbox_tools() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let ordinary = conversation(&server, &mut link, "Plan").await;

    let listed = call(&server, &link, ordinary, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let names: Vec<&str> = listed["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    let refused = call_tool(&server, &link, ordinary, "list_needs_you", json!({})).await;

    assert!(!names.contains(&"list_needs_you") && !names.contains(&"resolve_needs_you") && !names.contains(&"focus_needs_you"));
    assert_eq!(refused["isError"], true);
}

#[tokio::test]
async fn an_agent_that_is_not_voice_cannot_resolve_what_is_waiting_on_the_person() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let agent = conversation(&server, &mut link, "Plan").await;
    let source = waiting(&server, &mut link, "Release notes").await;
    let target = conversation(&server, &mut link, "Target").await;
    let (id, key) = proposal(&server, source, target).await;

    let refused = call_tool(&server, &link, agent, "resolve_needs_you", json!({"key": key, "action": "approve"})).await;

    assert_eq!(refused["isError"], true);
    assert_eq!(text(&refused), "Only the Voice assistant can act on the Inbox. Nothing was done.");
    let state = sqlx::query_scalar::<_, String>("select state from message_proposals where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(state, "proposed");
}

#[tokio::test]
async fn voice_lists_a_waiting_question_and_a_message_proposal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let question = waiting(&server, &mut link, "Pick a name").await;
    event(&server, question, "acp_update", json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"text": "Which name do you want?"}}})).await;
    let source = waiting(&server, &mut link, "Release notes").await;
    let target = conversation(&server, &mut link, "Target").await;
    let (_, key) = proposal(&server, source, target).await;

    let listed = call_tool(&server, &link, voice, "list_needs_you", json!({})).await;
    let items: Vec<Value> = serde_json::from_str(&text(&listed)).unwrap();

    let asked = items.iter().find(|item| item["kind"] == "question").unwrap();
    let proposed = items.iter().find(|item| item["kind"] == "message_proposal").unwrap();
    assert_eq!(asked["key"], format!("{question}/flag"));
    assert_eq!(asked["text"], "Which name do you want?");
    assert_eq!(proposed["key"], key);
    assert_eq!(proposed["conversation"], "Release notes");
}

#[tokio::test]
async fn voice_answering_a_question_sends_the_answer_into_that_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let question = waiting(&server, &mut link, "Pick a name").await;
    let mut stream = server.user.stream().await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": format!("{question}/flag"), "action": "answer", "answer": "Call it Hyphae"})).await;

    assert_eq!(resolved["isError"], false, "{resolved}");
    let sent = link.expect_prompt().await;
    assert!(format!("{sent:?}").contains("Call it Hyphae"));
    let focused = stream.until(|message| match message { StreamMessage::NeedsYouFocus { key } => Some(key), _ => None }).await;
    assert_eq!(focused, format!("{question}/flag"));
}

#[tokio::test]
async fn voice_declining_a_message_proposal_declines_it_for_the_person() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let source = waiting(&server, &mut link, "Release notes").await;
    let target = conversation(&server, &mut link, "Target").await;
    let (id, key) = proposal(&server, source, target).await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": key, "action": "decline"})).await;

    assert_eq!(resolved["isError"], false, "{resolved}");
    let state = sqlx::query_scalar::<_, String>("select state from message_proposals where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(state, "declined");
}

#[tokio::test]
async fn voice_never_resolves_a_permission_request_and_outlines_it_instead() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let asking = waiting(&server, &mut link, "Run the build").await;
    let seq = event(&server, asking, "permission_request", json!({"request_id": "r1", "toolCall": {"title": "cargo build"}, "options": [{"optionId": "allow", "name": "Approve", "kind": "allow_once"}]})).await;
    let mut stream = server.user.stream().await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": format!("{asking}/e{seq}"), "action": "approve"})).await;

    assert_eq!(resolved["isError"], true, "{resolved}");
    assert!(text(&resolved).contains("needs your tap on Approve"));
    let answers = sqlx::query_scalar::<_, i64>("select count(*) from events where conversation_id = $1 and kind = 'permission_resolved'").bind(asking).fetch_one(&server.pool).await.unwrap();
    assert_eq!(answers, 0);
    let focused = stream.until(|message| match message { StreamMessage::NeedsYouFocus { key } => Some(key), _ => None }).await;
    assert_eq!(focused, format!("{asking}/e{seq}"));
}

#[tokio::test]
async fn listing_outlines_the_item_the_app_shows_first() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let older = waiting(&server, &mut link, "Older chat").await;
    let newer = waiting(&server, &mut link, "Newer chat").await;
    let seq = event(&server, newer, "permission_request", json!({"request_id": "r1", "toolCall": {"title": "cargo build"}, "options": []})).await;
    updated_at(&server, older, "2026-11-08T09:00:00Z").await;
    updated_at(&server, newer, "2026-11-08T10:00:00Z").await;
    let mut stream = server.user.stream().await;

    let listed = call_tool(&server, &link, voice, "list_needs_you", json!({})).await;

    let items: Vec<Value> = serde_json::from_str(&text(&listed)).unwrap();
    assert_eq!(items[0]["key"], format!("{newer}/e{seq}"));
    let focused = stream.until(|message| match message { StreamMessage::NeedsYouFocus { key } => Some(key), _ => None }).await;
    assert_eq!(focused, format!("{newer}/e{seq}"));
}

#[tokio::test]
async fn saying_approve_on_a_question_sends_that_word_at_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let question = waiting(&server, &mut link, "Tag it").await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": format!("{question}/flag"), "action": "approve"})).await;

    assert_eq!(resolved["isError"], false, "{resolved}");
    assert_eq!(text(&resolved).lines().next().unwrap(), "Answered in Tag it: Approve");
    assert!(format!("{:?}", link.expect_prompt().await).contains("Approve"));
}

#[tokio::test]
async fn saying_approve_on_a_message_proposal_sends_it_at_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let source = waiting(&server, &mut link, "Release notes").await;
    let target = conversation(&server, &mut link, "Target").await;
    let (id, key) = proposal(&server, source, target).await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": key, "action": "approve"})).await;

    assert_eq!(resolved["isError"], false, "{resolved}");
    let state = sqlx::query_scalar::<_, String>("select state from message_proposals where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    assert_ne!(state, "proposed");
}

#[tokio::test]
async fn voice_never_approves_a_workflow_gate_and_outlines_it_instead() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    let host = conversation(&server, &mut link, "Release").await;
    let workflow = workflow_gate(&server, host).await;
    let mut stream = server.user.stream().await;

    let resolved = call_tool(&server, &link, voice, "resolve_needs_you", json!({"key": format!("{host}/workflow-{workflow}"), "action": "approve"})).await;

    assert_eq!(resolved["isError"], true, "{resolved}");
    assert!(text(&resolved).contains("tap on Approve"), "{resolved}");
    let steps = sqlx::query_scalar::<_, Value>("select steps from workflows where id = $1").bind(workflow).fetch_one(&server.pool).await.unwrap();
    assert_eq!(steps[0]["status"], "gate");
    let focused = stream.until(|message| match message { StreamMessage::NeedsYouFocus { key } => Some(key), _ => None }).await;
    assert_eq!(focused, format!("{host}/workflow-{workflow}"));
}

#[tokio::test]
async fn a_voice_conversation_waiting_for_a_reply_is_not_listed_while_another_assistants_is() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let voice = voice_conversation(&server, &mut link).await;
    crate::watches::end_turn(&mut link, voice, 2).await;
    sqlx::query("update conversations set status = 'input' where id = $1").bind(voice).execute(&server.pool).await.unwrap();
    let archivist = waiting(&server, &mut link, "Archivist").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(archivist).fetch_one(&server.pool).await.unwrap();
    let assistant = sqlx::query_scalar::<_, Uuid>("insert into assistants (owner, name, description) values ($1, 'Archivist', '') returning id").bind(owner).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set assistant_id = $2 where id = $1").bind(archivist).bind(assistant).execute(&server.pool).await.unwrap();

    let listed = call_tool(&server, &link, voice, "list_needs_you", json!({})).await;
    let items: Vec<Value> = serde_json::from_str(&text(&listed)).unwrap();

    let conversations: Vec<&Value> = items.iter().map(|item| &item["conversation"]).collect();
    assert_eq!(conversations, vec!["Archivist"]);
}
