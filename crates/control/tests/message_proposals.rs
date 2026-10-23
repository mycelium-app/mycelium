mod common;

use common::{NodeLink, Server};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, link: &NodeLink, conversation: Uuid, tool: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn working_conversation(server: &Server, link: &mut NodeLink, title: &str) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;
    sqlx::query("update conversations set title = $2 where id = $1").bind(conversation.id).bind(title).execute(&server.pool).await.unwrap();

    conversation.id
}

async fn queue(server: &Server, conversation: Uuid) -> Vec<Value> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;

    queue.as_array().unwrap().clone()
}

async fn proposals(server: &Server) -> Vec<Value> {
    let (_, proposals) = server.user.json(server.user.get("/api/v1/message-proposals")).await;

    proposals.as_array().unwrap().clone()
}

async fn propose(server: &Server, link: &NodeLink, from: Uuid, to: &str) -> Value {
    call(server, link, from, "send_to_conversation", json!({"conversation": to, "message": "The schema is ready", "reason": "You asked for the schema"})).await
}

#[tokio::test]
async fn finding_conversations_lists_only_the_persons_own_unarchived_ones_matching_every_word() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let caller = working_conversation(&server, &mut link, "Plan the release").await;
    let billing = working_conversation(&server, &mut link, "Fix billing export").await;
    let archived = working_conversation(&server, &mut link, "Old billing notes").await;
    let foreign = working_conversation(&server, &mut link, "Billing for someone else").await;
    sqlx::query("update conversations set archived_at = now() where id = $1").bind(archived).execute(&server.pool).await.unwrap();
    server.second_user("other@example.com").await;
    sqlx::query("update conversations set owner = (select id from users where email = 'other@example.com') where id = $1").bind(foreign).execute(&server.pool).await.unwrap();

    let found = text(&call(&server, &link, caller, "find_conversations", json!({"query": "billing export"})).await);

    assert!(found.contains(&billing.to_string()) && found.contains("Fix billing export") && found.contains("node laptop") && found.contains("agent claude"), "{found}");
    assert!(!found.contains(&archived.to_string()) && !found.contains(&foreign.to_string()) && !found.contains(&caller.to_string()), "{found}");

    let everything = text(&call(&server, &link, caller, "find_conversations", json!({"limit": 1})).await);
    assert_eq!(everything.lines().count(), 1);

    let nothing = text(&call(&server, &link, caller, "find_conversations", json!({"query": "zebra"})).await);
    assert_eq!(nothing, "No conversations match that query.");
}

#[tokio::test]
async fn proposing_a_message_waits_for_the_person_and_queues_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;

    let result = propose(&server, &link, source, "Fix billing export").await;

    let waiting = proposals(&server).await;
    assert_eq!((result["isError"].clone(), text(&result)), (json!(false), format!("Waiting for the person to approve sending this to Fix billing export (proposal {}).", waiting[0]["id"].as_str().unwrap())));
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0]["state"].clone(), waiting[0]["conversation_id"].clone(), waiting[0]["target_id"].clone(), waiting[0]["target_title"].clone(), waiting[0]["target_node"].clone(), waiting[0]["message"].clone(), waiting[0]["reason"].clone()),
        (json!("proposed"), json!(source), json!(target), json!("Fix billing export"), json!("laptop"), json!("The schema is ready"), json!("You asked for the schema")),
    );
    assert_eq!(queue(&server, target).await.len(), 0);
}

#[tokio::test]
async fn a_target_can_be_named_by_its_id() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;

    let result = propose(&server, &link, source, &target.to_string()).await;

    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(proposals(&server).await[0]["target_id"], json!(target));
}

#[tokio::test]
async fn approving_queues_the_message_on_the_target_prefixed_with_the_source_title() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;
    propose(&server, &link, source, "Fix billing export").await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let mut stream = server.user.stream().await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/message-proposals/{id}/approve"))).await;

    assert_eq!((status, body["state"].clone()), (200, json!("sent")), "{body}");
    let queued = queue(&server, target).await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0]["text"], json!("From Plan the release:\n\nThe schema is ready"));
    assert_eq!(queue(&server, source).await.len(), 0);

    let streamed = stream
        .until(|message| match message {
            StreamMessage::MessageProposal(proposal) if proposal.id.to_string() == id => Some(proposal),
            _ => None,
        })
        .await;
    assert_eq!(streamed.state, mycelium_proto::api::MessageProposalState::Sent);

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/message-proposals/{id}/approve"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("proposal_not_proposed")));
    assert_eq!(queue(&server, target).await.len(), 1);
}

#[tokio::test]
async fn declining_queues_nothing_on_the_target_and_tells_the_agent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;
    propose(&server, &link, source, "Fix billing export").await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/message-proposals/{id}/decline"))).await;

    assert_eq!((status, body["state"].clone()), (200, json!("declined")), "{body}");
    assert_eq!(queue(&server, target).await.len(), 0);
    let told = queue(&server, source).await;
    assert!(told[0]["text"].as_str().unwrap().contains("declined") && told[0]["text"].as_str().unwrap().contains("Fix billing export"), "{told:?}");

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/message-proposals/{id}/approve"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("proposal_not_proposed")));
    assert_eq!(queue(&server, target).await.len(), 0);
}

#[tokio::test]
async fn a_message_to_itself_an_archived_conversation_or_someone_elses_is_refused_and_stores_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let archived = working_conversation(&server, &mut link, "Old notes").await;
    let foreign = working_conversation(&server, &mut link, "Not mine").await;
    sqlx::query("update conversations set archived_at = now() where id = $1").bind(archived).execute(&server.pool).await.unwrap();
    server.second_user("other@example.com").await;
    sqlx::query("update conversations set owner = (select id from users where email = 'other@example.com') where id = $1").bind(foreign).execute(&server.pool).await.unwrap();

    for (target, fix) in [
        ("Plan the release", "this conversation"),
        (&source.to_string(), "this conversation"),
        ("Old notes", "archived"),
        ("Not mine", "None of the person's conversations"),
        (&foreign.to_string(), "None of the person's conversations"),
        ("No such thing", "None of the person's conversations"),
    ] {
        let result = propose(&server, &link, source, target).await;

        assert_eq!(result["isError"], true, "{target}");
        assert!(text(&result).contains(fix), "{target}: {result}");
    }

    assert_eq!(proposals(&server).await.len(), 0);
}

#[tokio::test]
async fn another_person_cannot_decide_a_proposal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;
    propose(&server, &link, source, "Fix billing export").await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let other = server.second_user("other@example.com").await;

    for action in ["approve", "decline"] {
        assert_eq!(other.json(other.post(&format!("/api/v1/message-proposals/{id}/{action}"))).await.0, 404);
    }

    assert_eq!(queue(&server, target).await.len(), 0);
}

#[tokio::test]
async fn a_proposal_needs_a_message_and_a_reason() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    working_conversation(&server, &mut link, "Fix billing export").await;

    for arguments in [json!({"conversation": "Fix billing export", "message": " ", "reason": "Why"}), json!({"conversation": "Fix billing export", "message": "Hi", "reason": ""})] {
        let result = call(&server, &link, source, "send_to_conversation", arguments.clone()).await;

        assert_eq!(result["isError"], true, "{arguments}");
        assert!(text(&result).contains("Give the conversation, the message and the reason"), "{result}");
    }

    assert_eq!(proposals(&server).await.len(), 0);
}
