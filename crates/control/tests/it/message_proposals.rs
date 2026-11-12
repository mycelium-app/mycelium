use crate::common::{NodeLink, Server};
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
async fn finding_conversations_shows_each_ones_group_and_filters_by_group() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let caller = working_conversation(&server, &mut link, "Plan the release").await;
    let billing = working_conversation(&server, &mut link, "Fix billing export").await;
    let loose = working_conversation(&server, &mut link, "Loose notes").await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "Immich"}))).await;
    let group = group["id"].as_str().unwrap();
    server.user.json(server.user.post(&format!("/api/v1/groups/{group}/folders")).json(&json!({"prefix": "/work/billing"}))).await;
    sqlx::query("update conversations set cwd = '/work/billing/app' where id = $1").bind(billing).execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set cwd = '/elsewhere' where id = $1").bind(loose).execute(&server.pool).await.unwrap();

    let everything = text(&call(&server, &link, caller, "find_conversations", json!({})).await);
    assert!(everything.lines().any(|line| line.contains("Fix billing export") && line.contains("group Immich")), "{everything}");
    assert!(everything.lines().any(|line| line.contains("Loose notes") && line.contains("group none")), "{everything}");

    let in_group = text(&call(&server, &link, caller, "find_conversations", json!({"group": "immich"})).await);
    assert!(in_group.contains("Fix billing export") && !in_group.contains("Loose notes"), "{in_group}");

    let ungrouped = text(&call(&server, &link, caller, "find_conversations", json!({"group": "none"})).await);
    assert!(ungrouped.contains("Loose notes") && !ungrouped.contains("Fix billing export"), "{ungrouped}");
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

#[tokio::test]
async fn connecting_conversations_allows_messages_in_both_directions_until_revoked() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let target = working_conversation(&server, &mut link, "Fix billing export").await;
    let (status, connection) = server.user.json(server.user.post("/api/v1/conversation-connections").json(&json!({"source_id": source, "target_id": target}))).await;
    assert_eq!(status, 201, "{connection}");

    let sent = propose(&server, &link, source, &target.to_string()).await;
    assert!(text(&sent).starts_with("Queued for Fix billing export"), "{sent}");
    assert_eq!(queue(&server, target).await[0]["text"], "From Plan the release:\n\nThe schema is ready");
    let reply = propose(&server, &link, target, &source.to_string()).await;
    assert!(text(&reply).starts_with("Queued for Plan the release"), "{reply}");
    assert_eq!(queue(&server, source).await.len(), 1);

    let id = connection["id"].as_str().unwrap();
    let (status, revoked) = server.user.json(server.user.delete(&format!("/api/v1/conversation-connections/{id}"))).await;
    assert_eq!(status, 200, "{revoked}");
    let waiting = propose(&server, &link, source, &target.to_string()).await;
    assert!(text(&waiting).starts_with("Waiting for the person"), "{waiting}");
    assert_eq!(queue(&server, target).await.len(), 1);
}

#[tokio::test]
async fn connections_are_owned_and_require_full_person_access() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Source").await;
    let target = working_conversation(&server, &mut link, "Target").await;
    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.post("/api/v1/conversation-connections").json(&json!({"source_id": source, "target_id": target}))).await;
    assert_eq!(status, 404);
    let (status, _) = server.user.json(server.user.post("/api/v1/conversation-connections").json(&json!({"source_id": source, "target_id": source}))).await;
    assert_eq!(status, 400);

    let (_, connection) = server.user.json(server.user.post("/api/v1/conversation-connections").json(&json!({"source_id": source, "target_id": target}))).await;
    let (_, list) = other.json(other.get("/api/v1/conversation-connections")).await;
    assert_eq!(list, json!([]));
    assert_eq!(other.json(other.delete(&format!("/api/v1/conversation-connections/{}", connection["id"].as_str().unwrap()))).await.0, 404);
}

#[tokio::test]
async fn approving_a_message_and_connecting_only_sends_it_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Source").await;
    let target = working_conversation(&server, &mut link, "Target").await;
    propose(&server, &link, source, &target.to_string()).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let route = format!("/api/v1/message-proposals/{id}/connect");
    let (status, sent) = server.user.json(server.user.post(&route)).await;
    assert_eq!((status, sent["state"].clone()), (200, json!("sent")), "{sent}");
    assert_eq!(server.user.json(server.user.post(&route)).await.0, 409);
    assert_eq!(queue(&server, target).await.len(), 1);
    let (_, connections) = server.user.json(server.user.get("/api/v1/conversation-connections")).await;
    assert_eq!(connections.as_array().unwrap().len(), 1);
    assert!(text(&propose(&server, &link, source, &target.to_string()).await).starts_with("Queued for"), "the target is in the middle of a turn");
}

#[tokio::test]
async fn a_message_to_a_conversation_in_the_middle_of_a_turn_is_reported_as_queued() {
    let server = crate::watches::fast().await;
    let mut link = server.online_node("laptop").await;
    let sender = crate::watches::begun(&server, &mut link, "Plan the release").await;
    let busy = crate::watches::begun(&server, &mut link, "Build the release").await;
    crate::watches::connect(&server, sender, busy).await;

    let reply = call(&server, &link, sender, "send_to_conversation", json!({"conversation": busy.to_string(), "message": "Use v0.29.0", "reason": "The number changed"})).await;
    let said = text(&reply);
    crate::watches::end_turn(&mut link, busy, 1).await;
    let delivered = crate::watches::told(&server, busy, 1).await;

    assert!(said.starts_with("Queued for") && said.contains("goes when that turn ends"), "{said}");
    assert!(delivered.contains("Use v0.29.0"), "{delivered}");
}

fn proposal_id(reply: &Value) -> String {
    let reply = text(reply);
    let start = reply.find("(proposal ").unwrap() + "(proposal ".len();

    reply[start..start + 36].to_string()
}

#[tokio::test]
async fn a_pending_proposal_keeps_the_reason_the_agent_gave_for_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    working_conversation(&server, &mut link, "Fix billing export").await;

    let reply = propose(&server, &link, source, "Fix billing export").await;

    assert!(text(&reply).contains("Waiting for the person"), "{reply}");
    let listed = proposals(&server).await;
    assert_eq!(listed[0]["reason"], "You asked for the schema");
    assert_eq!(listed[0]["id"], proposal_id(&reply));
}

#[tokio::test]
async fn the_proposing_conversation_withdraws_its_pending_proposal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    working_conversation(&server, &mut link, "Fix billing export").await;
    let id = proposal_id(&propose(&server, &link, source, "Fix billing export").await);

    let withdrawn = call(&server, &link, source, "withdraw_message", json!({"proposal": id})).await;

    assert_ne!(withdrawn["isError"], true, "{withdrawn}");
    assert_eq!(proposals(&server).await[0]["state"], "withdrawn");

    let again = call(&server, &link, source, "withdraw_message", json!({"proposal": id})).await;

    assert_eq!(again["isError"], true, "{again}");
}

#[tokio::test]
async fn another_conversation_cannot_withdraw_a_proposal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    let other = working_conversation(&server, &mut link, "Fix billing export").await;
    let id = proposal_id(&propose(&server, &link, source, "Fix billing export").await);

    let refused = call(&server, &link, other, "withdraw_message", json!({"proposal": id})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(proposals(&server).await[0]["state"], "proposed");
}

#[tokio::test]
async fn a_newer_proposal_to_the_same_conversation_supersedes_the_older_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;
    working_conversation(&server, &mut link, "Fix billing export").await;
    working_conversation(&server, &mut link, "Write the changelog").await;
    let older = proposal_id(&propose(&server, &link, source, "Fix billing export").await);
    let elsewhere = proposal_id(&propose(&server, &link, source, "Write the changelog").await);

    let newer = propose(&server, &link, source, "Fix billing export").await;

    assert!(text(&newer).contains("superseded"), "{newer}");
    let states: std::collections::HashMap<String, String> = proposals(&server).await.iter().map(|found| (found["id"].as_str().unwrap().to_string(), found["state"].as_str().unwrap().to_string())).collect();
    assert_eq!(states[&older], "superseded");
    assert_eq!(states[&elsewhere], "proposed");
    assert_eq!(states[&proposal_id(&newer)], "proposed");
}

async fn group(server: &Server, name: &str) -> String {
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": name}))).await;

    group["id"].as_str().unwrap().to_string()
}

async fn assistant_with_chat(server: &Server, link: &mut NodeLink, name: &str, group: &str) -> (String, Uuid) {
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": name, "group_id": group}))).await;
    assert_eq!(status, 201, "{assistant}");
    let id = assistant["id"].as_str().unwrap().to_string();
    let chat = working_conversation(server, link, "New chat").await;
    sqlx::query("update conversations set assistant_id = $2 where id = $1").bind(chat).bind(Uuid::parse_str(&id).unwrap()).execute(&server.pool).await.unwrap();
    sqlx::query("update assistants set conversation_id = $2 where id = $1").bind(Uuid::parse_str(&id).unwrap()).bind(chat).execute(&server.pool).await.unwrap();

    (id, chat)
}

#[tokio::test]
async fn a_message_to_an_assistant_in_the_same_group_lands_in_its_own_chat_by_id_and_by_name() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let ops = group(&server, "Ops").await;
    let (_, sender_chat) = assistant_with_chat(&server, &mut link, "Planner", &ops).await;
    let (infrastructure_id, infrastructure_chat) = assistant_with_chat(&server, &mut link, "Infrastructure", &ops).await;
    let sub_chat = working_conversation(&server, &mut link, "Side chat").await;
    sqlx::query("update conversations set assistant_id = $2, parent_id = $3, side_kind = 'subagent' where id = $1").bind(sub_chat).bind(Uuid::parse_str(&infrastructure_id).unwrap()).bind(infrastructure_chat).execute(&server.pool).await.unwrap();

    let by_name = propose(&server, &link, sender_chat, "infrastructure").await;
    let by_id = propose(&server, &link, sender_chat, &infrastructure_id).await;

    assert!(text(&by_name).starts_with("Queued for Infrastructure"), "{by_name}");
    assert!(text(&by_id).starts_with("Queued for Infrastructure"), "{by_id}");
    let queued = queue(&server, infrastructure_chat).await;
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0]["text"], json!("From Planner:\n\nThe schema is ready"));
    assert_eq!(queue(&server, sub_chat).await.len(), 0);
}

#[tokio::test]
async fn a_message_to_an_assistant_in_an_unconnected_group_waits_for_the_person() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (ops, finance) = (group(&server, "Ops").await, group(&server, "Finance").await);
    let (_, sender_chat) = assistant_with_chat(&server, &mut link, "Planner", &ops).await;
    let (_, accounts_chat) = assistant_with_chat(&server, &mut link, "Accounts", &finance).await;

    let result = propose(&server, &link, sender_chat, "Accounts").await;

    assert!(text(&result).starts_with("Waiting for the person to approve sending this to Accounts"), "{result}");
    assert_eq!(queue(&server, accounts_chat).await.len(), 0);
    let waiting = proposals(&server).await;
    assert_eq!((waiting[0]["state"].clone(), waiting[0]["target_id"].clone()), (json!("proposed"), json!(accounts_chat)));

    let id = waiting[0]["id"].as_str().unwrap();
    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/message-proposals/{id}/approve"))).await;
    assert_eq!((status, approved["state"].clone()), (200, json!("sent")), "{approved}");
    assert_eq!(queue(&server, accounts_chat).await[0]["text"], json!("From Planner:\n\nThe schema is ready"));
}

#[tokio::test]
async fn the_receiving_assistant_replies_by_naming_the_sender() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let ops = group(&server, "Ops").await;
    let (_, planner_chat) = assistant_with_chat(&server, &mut link, "Planner", &ops).await;
    let (_, infrastructure_chat) = assistant_with_chat(&server, &mut link, "Infrastructure", &ops).await;
    propose(&server, &link, planner_chat, "Infrastructure").await;

    let reply = call(&server, &link, infrastructure_chat, "send_to_conversation", json!({"conversation": "Planner", "message": "Done", "reason": "Answering you"})).await;

    assert!(text(&reply).starts_with("Queued for Planner"), "{reply}");
    assert_eq!(queue(&server, planner_chat).await[0]["text"], json!("From Infrastructure:\n\nDone"));
}

#[tokio::test]
async fn reading_and_watching_an_assistant_resolve_to_its_current_chat() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let ops = group(&server, "Ops").await;
    let (_, planner_chat) = assistant_with_chat(&server, &mut link, "Planner", &ops).await;
    let (infrastructure_id, infrastructure_chat) = assistant_with_chat(&server, &mut link, "Infrastructure", &ops).await;

    let read = text(&call(&server, &link, planner_chat, "read_conversation", json!({"conversation": infrastructure_id})).await);
    let watched = text(&call(&server, &link, planner_chat, "watch_conversation", json!({"conversation": "Infrastructure"})).await);

    assert!(read.contains(&infrastructure_chat.to_string()), "{read}");
    assert!(watched.contains("Infrastructure") || watched.contains(&infrastructure_chat.to_string()), "{watched}");
}

#[tokio::test]
async fn an_unknown_assistant_name_says_how_to_find_the_id() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = working_conversation(&server, &mut link, "Plan the release").await;

    let result = propose(&server, &link, source, "Nobody").await;

    assert_eq!(result["isError"], true);
    assert!(text(&result).contains("assistants") && text(&result).contains("assistant_list") && text(&result).contains("find_conversations"), "{result}");
}
