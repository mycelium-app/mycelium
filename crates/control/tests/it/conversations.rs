use crate::common;
use crate::common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use uuid::Uuid;

async fn started_conversation(server: &Server, link: &mut NodeLink, prompt: Option<&str>) -> Uuid {
    let conversation = server.create_conversation(link.id, prompt).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    conversation.id
}

async fn prompt(server: &Server, id: Uuid, text: &str) -> u16 {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": text}))).await.0
}

async fn queue(server: &Server, id: Uuid) -> Vec<serde_json::Value> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/queue"))).await;

    queue.as_array().unwrap().clone()
}

async fn status_of(server: &Server, id: Uuid) -> String {
    server.conversation(id).await["status"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_new_conversation_without_a_prompt_starts_a_session_and_waits_idle() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let conversation = server.create_conversation(link.id, None).await;
    assert_eq!(conversation.title, "New conversation");
    assert_eq!(conversation.cwd, "/work/app");

    match link.next().await {
        ControlToNode::StartSession { conversation_id, cwd, resume, .. } => {
            assert_eq!(conversation_id, conversation.id);
            assert_eq!(cwd, "/work/app");
            assert_eq!(resume, None);
        }
        other => panic!("expected start_session, got {other:?}"),
    }

    link.send(NodeToControl::SessionStarted {
        conversation_id: conversation.id,
        acp_session_id: "acp-9".into(),
        modes: Some(json!({"currentModeId": "plan"})),
        models: Some(json!({"currentModelId": "opus"})),
        config_options: None,
    })
    .await;

    let started = stream.until(|message| match message {
        StreamMessage::Conversation(started) if started.acp_session_id.is_some() => Some(started),
        _ => None,
    })
    .await;

    assert_eq!(started.mode.as_deref(), Some("plan"));
    assert_eq!(started.model.as_deref(), Some("opus"));
    assert_eq!(status_of(&server, conversation.id).await, "done");
}

#[tokio::test]
async fn the_first_prompt_is_sent_once_the_session_has_started_and_the_turn_ends_in_review() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let conversation = server.create_conversation(link.id, Some("Fix the flaky test\nthen run it")).await;
    assert_eq!(conversation.title, "Fix the flaky test");
    assert_eq!(serde_json::to_value(conversation.status).unwrap(), json!("working"));

    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    let (prompt_id, text) = link.expect_prompt().await;
    assert_eq!(text, "Fix the flaky test\nthen run it");

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 3, update: json!({"sessionUpdate": "agent_message_chunk"}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 4, stop_reason: "end_turn".into(), usage: None }).await;

    let review = stream.conversation_with_status("review").await;
    assert_eq!(review.id, conversation.id);

    let events = server.events(conversation.id).await;
    let kinds: Vec<&str> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, vec!["user_message", "acp_update", "turn_end"]);
    assert_eq!(events[0]["payload"]["text"], "Fix the flaky test\nthen run it");
    assert_eq!(events[0]["payload"]["prompt_id"], json!(prompt_id));
    assert_eq!(events[2]["payload"]["stop_reason"], "end_turn");

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{}/opened", conversation.id))).await;
    assert_eq!(status, 204);
    assert_eq!(status_of(&server, conversation.id).await, "done");

    server.user.json(server.user.post(&format!("/api/v1/conversations/{}/opened", conversation.id))).await;
    assert_eq!(status_of(&server, conversation.id).await, "done");
}

#[tokio::test]
async fn events_are_paged_by_seq() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;

    for seq in 1..=5 {
        link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update: json!({"n": seq}) }).await;
    }

    link.barrier(id, 6).await;

    let (_, page) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/events?after=2&limit=2"))).await;
    let numbers: Vec<i64> = page.as_array().unwrap().iter().map(|event| event["payload"]["n"].as_i64().unwrap()).collect();

    assert_eq!(numbers, vec![3, 4]);
}

async fn seed_turns(server: &Server, id: Uuid, turns: i64, updates_per_turn: i64) {
    let mut seq = 100;

    for turn in 0..turns {
        seq += 1;
        sqlx::query("insert into events (conversation_id, seq, kind, payload) values ($1, $2, 'user_message', $3)")
            .bind(id)
            .bind(seq)
            .bind(json!({"text": format!("turn {turn}")}))
            .execute(&server.pool)
            .await
            .unwrap();

        for _ in 0..updates_per_turn {
            seq += 1;
            sqlx::query("insert into events (conversation_id, seq, kind, payload) values ($1, $2, 'acp_update', $3)")
                .bind(id)
                .bind(seq)
                .bind(json!({}))
                .execute(&server.pool)
                .await
                .unwrap();
        }
    }
}

async fn events_before(server: &Server, id: Uuid, before: i64, limit: i64) -> Vec<(i64, String)> {
    let (_, page) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/events?before={before}&limit={limit}"))).await;

    page.as_array().unwrap().iter().map(|event| (event["seq"].as_i64().unwrap(), event["kind"].as_str().unwrap().to_string())).collect()
}

#[tokio::test]
async fn the_newest_events_page_is_ascending_and_starts_at_a_user_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    seed_turns(&server, id, 10, 4).await;

    let page = events_before(&server, id, i64::MAX, 12).await;

    let seqs: Vec<i64> = page.iter().map(|(seq, _)| *seq).collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(page[0].1, "user_message");
    assert_eq!(*seqs.last().unwrap(), 150);
    assert!(page.len() >= 12);
}

#[tokio::test]
async fn a_turn_far_longer_than_a_page_is_not_loaded_whole() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    seed_turns(&server, id, 1, 400).await;

    let page = events_before(&server, id, i64::MAX, 20).await;

    assert!(page.len() <= 40, "loaded {} events", page.len());
    assert_eq!(page.last().unwrap().0, 501);
}

#[tokio::test]
async fn paging_backwards_from_the_oldest_seq_has_no_gap_and_no_overlap() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    seed_turns(&server, id, 10, 4).await;

    let newest = events_before(&server, id, i64::MAX, 12).await;
    let previous = events_before(&server, id, newest[0].0, 12).await;

    assert_eq!(previous.last().unwrap().0 + 1, newest[0].0);
    assert_eq!(previous[0].1, "user_message");
}

#[tokio::test]
async fn a_conversation_shorter_than_a_page_is_returned_whole_before_any_seq() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    seed_turns(&server, id, 2, 2).await;

    let page = events_before(&server, id, i64::MAX, 500).await;
    let all = server.events(id).await;

    assert_eq!(page.len(), all.len());
    assert_eq!(page[0].0, all[0]["seq"].as_i64().unwrap());

    let beyond_the_start = events_before(&server, id, page[0].0, 500).await;
    assert!(beyond_the_start.is_empty());
}

#[tokio::test]
async fn events_before_a_seq_of_another_persons_conversation_is_not_found() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let other = server.second_user("eve@example.com").await;

    let (status, _) = other.json(other.get(&format!("/api/v1/conversations/{id}/events?before=100"))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_permission_request_asks_for_input_and_the_answer_goes_back_to_the_agent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("Delete the build folder")).await;
    link.expect_prompt().await;

    link.send(NodeToControl::PermissionRequest {
        conversation_id: id,
        seq: 5,
        request_id: "req-1".into(),
        request: json!({"toolCall": {"title": "rm -rf build"}, "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}, {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]}),
    })
    .await;

    let waiting = stream.conversation_with_status("input").await;
    assert_eq!(waiting.id, id);

    let events = server.events(id).await;
    let request = events.iter().find(|event| event["kind"] == "permission_request").unwrap();
    assert_eq!(request["payload"]["request_id"], "req-1");
    assert_eq!(request["payload"]["toolCall"]["title"], "rm -rf build");

    let answer = |option: &str| json!({"request_id": "req-1", "option_id": option});
    let url = format!("/api/v1/conversations/{id}/permission");

    let (status, body) = server.user.json(server.user.post(&url).json(&answer("sudo"))).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("unknown_option")));

    let (status, _) = server.user.json(server.user.post(&url).json(&json!({"request_id": "missing", "option_id": "allow"}))).await;
    assert_eq!(status, 404);

    let (status, _) = server.user.json(server.user.post(&url).json(&answer("allow"))).await;
    assert_eq!(status, 204);

    match link.next().await {
        ControlToNode::PermissionResponse { conversation_id, request_id, outcome } => {
            assert_eq!(conversation_id, id);
            assert_eq!(request_id, "req-1");
            assert_eq!(outcome, json!({"outcome": "selected", "optionId": "allow"}));
        }
        other => panic!("expected permission_response, got {other:?}"),
    }

    stream.conversation_with_status("working").await;

    let kinds: Vec<String> = server.events(id).await.iter().map(|event| event["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, vec!["user_message", "permission_request", "permission_resolved"]);

    let (status, body) = server.user.json(server.user.post(&url).json(&answer("allow"))).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("already_resolved")));
}

async fn waiting_for_permission(server: &Server, link: &mut NodeLink) -> Uuid {
    let id = started_conversation(server, link, Some("Delete the build folder")).await;
    link.expect_prompt().await;
    link.send(NodeToControl::PermissionRequest {
        conversation_id: id,
        seq: 5,
        request_id: "req-1".into(),
        request: json!({"options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}),
    })
    .await;

    common::eventually("the conversation to wait for input", || async { (status_of(server, id).await == "input").then_some(()) }).await;

    id
}

async fn answer_permission(server: &Server, id: Uuid) -> (u16, serde_json::Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/permission")).json(&json!({"request_id": "req-1", "option_id": "allow"}))).await
}

#[tokio::test]
async fn answering_a_permission_the_turn_already_ended_without_does_not_resume_the_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = waiting_for_permission(&server, &mut link).await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 6, stop_reason: "cancelled".into(), usage: None }).await;
    link.barrier(id, 7).await;
    assert_eq!(status_of(&server, id).await, "review");

    let (status, body) = answer_permission(&server, id).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("already_resolved")));
    assert_eq!(status_of(&server, id).await, "review");
}

#[tokio::test]
async fn answering_a_permission_on_a_failed_conversation_leaves_it_failed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = waiting_for_permission(&server, &mut link).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 6, message: "agent exited with code 1".into() }).await;
    common::eventually("the conversation to fail", || async { (status_of(&server, id).await == "failed").then_some(()) }).await;

    let (status, body) = answer_permission(&server, id).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("session_failed")));
    assert_eq!(status_of(&server, id).await, "failed");
}

#[tokio::test]
async fn a_session_started_replayed_on_reconnect_keeps_the_mode_and_model_the_person_chose() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let started = |link_id: Uuid| NodeToControl::SessionStarted {
        conversation_id: link_id,
        acp_session_id: "acp-1".into(),
        modes: Some(json!({"currentModeId": "default"})),
        models: Some(json!({"currentModelId": "sonnet"})),
        config_options: None,
    };

    link.send(started(id)).await;
    common::eventually("the initial mode", || async { (server.conversation(id).await["mode"] == "default").then_some(()) }).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "plan", "model": "opus"}))).await;
    assert_eq!(status, 204);

    link.send(started(id)).await;
    link.barrier(id, 1).await;

    let conversation = server.conversation(id).await;
    assert_eq!((conversation["mode"].as_str(), conversation["model"].as_str()), (Some("plan"), Some("opus")));
}

#[tokio::test]
async fn a_started_session_reports_its_models_to_the_node_for_conversations_not_yet_open() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;

    link.send(NodeToControl::SessionStarted {
        conversation_id: id,
        acp_session_id: "acp-1".into(),
        modes: None,
        models: Some(json!({"currentModelId": "opus", "availableModels": [{"modelId": "opus", "name": "Opus 5.5"}, {"modelId": "sonnet", "name": "Sonnet 5.5"}]})),
        config_options: None,
    })
    .await;

    let user = server.user.clone();
    let reported = common::eventually("the node to carry its models", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes[0]["harness_models"].as_array().filter(|models| !models.is_empty()).map(|models| models[0].clone())
    })
    .await;

    assert_eq!(reported["agent"], "claude");
    assert_eq!(reported["default_model"], "opus");
    assert_eq!(reported["models"], json!([{"id": "opus", "name": "Opus 5.5"}, {"id": "sonnet", "name": "Sonnet 5.5"}]));
}

#[tokio::test]
async fn a_model_chosen_before_the_session_starts_is_set_once_it_has() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "model": "sonnet"}))).await;
    assert_eq!(status, 201, "{body}");
    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    link.expect_start_session().await;

    link.send(NodeToControl::SessionStarted {
        conversation_id: id,
        acp_session_id: "acp-1".into(),
        modes: None,
        models: Some(json!({"currentModelId": "opus", "availableModels": [{"modelId": "opus", "name": "Opus 5.5"}, {"modelId": "sonnet", "name": "Sonnet 5.5"}]})),
        config_options: None,
    })
    .await;

    match link.next().await {
        ControlToNode::SetConfig { model, .. } => assert_eq!(model.as_deref(), Some("sonnet")),
        other => panic!("expected set_config, got {other:?}"),
    }

    assert_eq!(server.conversation(id).await["model"], "sonnet");
}

#[tokio::test]
async fn a_send_now_whose_cancel_never_landed_does_not_release_the_queue_after_a_later_stop() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    let (prompt_id, _) = link.expect_prompt().await;
    link.send(NodeToControl::PromptReceived { conversation_id: id, prompt_id }).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/{urgent}/send-now"))).await;
    assert_eq!(link.next().await, ControlToNode::Cancel { conversation_id: id });

    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![(id, 2)]).await;
    returned.expect_ack().await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;
    assert_eq!(status, 204);
    assert_eq!(returned.next().await, ControlToNode::Cancel { conversation_id: id });

    returned.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    returned.barrier(id, 4).await;

    assert_eq!(status_of(&server, id).await, "review");
    assert_eq!(queue(&server, id).await.len(), 1);
}

#[tokio::test]
async fn a_prompt_sent_while_a_turn_runs_is_queued_and_sent_when_the_turn_ends() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;

    assert_eq!(prompt(&server, id, "second").await, 204);
    assert_eq!(prompt(&server, id, "third").await, 204);

    let waiting = queue(&server, id).await;
    let texts: Vec<&str> = waiting.iter().map(|item| item["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["second", "third"]);

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
    assert_eq!(link.expect_prompt().await.1, "second\n\nthird");

    assert!(queue(&server, id).await.is_empty());
    stream.until(|message| matches!(message, StreamMessage::Queue { queue, .. } if queue.is_empty()).then_some(())).await;
}

#[tokio::test]
async fn a_prompt_to_an_idle_conversation_is_sent_straight_away() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;

    assert_eq!(prompt(&server, id, "hello").await, 204);

    assert_eq!(link.expect_prompt().await.1, "hello");
    assert_eq!(status_of(&server, id).await, "working");
}

#[tokio::test]
async fn queued_prompts_can_be_edited_and_removed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;

    prompt(&server, id, "second").await;
    prompt(&server, id, "third").await;
    let waiting = queue(&server, id).await;
    let second = waiting[0]["id"].as_str().unwrap();
    let third = waiting[1]["id"].as_str().unwrap();

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}/queue/{second}")).json(&json!({"text": "second, reworded"}))).await;
    assert_eq!(status, 204);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}/queue/{third}"))).await;
    assert_eq!(status, 204);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}/queue/{third}"))).await;
    assert_eq!(status, 404);

    let remaining = queue(&server, id).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["text"], "second, reworded");
}

#[tokio::test]
async fn send_now_cancels_the_running_turn_then_sends_that_message_first() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;

    prompt(&server, id, "later").await;
    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[1]["id"].as_str().unwrap().to_string();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/{urgent}/send-now"))).await;
    assert_eq!(status, 204);

    assert_eq!(link.next().await, ControlToNode::Cancel { conversation_id: id });

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    assert_eq!(link.expect_prompt().await.1, "urgent");

    let remaining = queue(&server, id).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["text"], "later");
}

#[tokio::test]
async fn queued_messages_arrive_as_one_prompt_when_the_turn_ends() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;

    prompt(&server, id, "first").await;
    prompt(&server, id, "second").await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;

    assert_eq!(link.expect_prompt().await.1, "first\n\nsecond");
    assert!(queue(&server, id).await.is_empty());
}

#[tokio::test]
async fn send_all_now_cancels_the_running_turn_then_sends_the_combined_prompt() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;

    prompt(&server, id, "first").await;
    prompt(&server, id, "second").await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/send-all-now"))).await;
    assert_eq!(status, 204);

    assert_eq!(link.next().await, ControlToNode::Cancel { conversation_id: id });

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    assert_eq!(link.expect_prompt().await.1, "first\n\nsecond");
    assert!(queue(&server, id).await.is_empty());
}

const STEERING_NODE: &str = "0.10.1";

async fn next_but_git(link: &mut NodeLink) -> ControlToNode {
    loop {
        let message = link.next().await;

        if !matches!(message, ControlToNode::Git { .. }) {
            return message;
        }
    }
}

async fn answer_steer(link: &mut NodeLink, outcome: &str) -> String {
    let ControlToNode::Steer { request_id, text, .. } = next_but_git(link).await else {
        panic!("expected a steer");
    };
    link.send(NodeToControl::Steered { request_id, outcome: outcome.into(), error: None }).await;

    text
}

async fn user_message_texts(server: &Server, id: Uuid) -> Vec<String> {
    server.events(id).await.iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].as_str().unwrap().to_string()).collect()
}

async fn assert_turn_not_cancelled(server: &Server, link: &mut NodeLink, id: Uuid) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;

    assert_eq!(next_but_git(link).await, ControlToNode::Cancel { conversation_id: id });
}

#[tokio::test]
async fn send_now_on_a_steering_node_injects_into_the_running_turn_instead_of_stopping_it() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "later").await;
    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[1]["id"].as_str().unwrap().to_string();

    let url = format!("/api/v1/conversations/{id}/queue/{urgent}/send-now");
    let ((status, _), text) = tokio::join!(server.user.json(server.user.post(&url)), answer_steer(&mut link, "injected"));

    assert_eq!((status, text.as_str()), (204, "urgent"));
    assert_eq!(user_message_texts(&server, id).await, vec!["long task", "urgent"]);
    let remaining = queue(&server, id).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["text"], "later");
    assert_eq!(status_of(&server, id).await, "working");
    assert_turn_not_cancelled(&server, &mut link, id).await;
}

#[tokio::test]
async fn send_all_now_on_a_steering_node_injects_the_combined_message() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "first").await;
    prompt(&server, id, "second").await;

    let url = format!("/api/v1/conversations/{id}/queue/send-all-now");
    let ((status, _), text) = tokio::join!(server.user.json(server.user.post(&url)), answer_steer(&mut link, "injected"));

    assert_eq!((status, text.as_str()), (204, "first\n\nsecond"));
    assert_eq!(user_message_texts(&server, id).await, vec!["long task", "first\n\nsecond"]);
    assert!(queue(&server, id).await.is_empty());
    assert_turn_not_cancelled(&server, &mut link, id).await;
}

#[tokio::test]
async fn send_now_whose_turn_had_just_ended_is_sent_as_a_normal_prompt() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    let url = format!("/api/v1/conversations/{id}/queue/{urgent}/send-now");
    let answering = async {
        let ControlToNode::Steer { request_id, .. } = next_but_git(&mut link).await else {
            panic!("expected a steer");
        };
        link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
        link.send(NodeToControl::Steered { request_id, outcome: "prompt_required".into(), error: None }).await;
    };
    let ((status, _), ()) = tokio::join!(server.user.json(server.user.post(&url)), answering);

    assert_eq!(status, 204);
    assert!(matches!(next_but_git(&mut link).await, ControlToNode::Prompt { text, .. } if text == "urgent"));
    assert_turn_not_cancelled(&server, &mut link, id).await;
}

#[tokio::test]
async fn send_now_falls_back_to_stopping_the_turn_when_the_agent_cannot_be_steered() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    let url = format!("/api/v1/conversations/{id}/queue/{urgent}/send-now");
    let ((status, _), _) = tokio::join!(server.user.json(server.user.post(&url)), answer_steer(&mut link, "unsupported"));

    assert_eq!(status, 204);
    assert_eq!(queue(&server, id).await.len(), 1);
    assert_eq!(next_but_git(&mut link).await, ControlToNode::Cancel { conversation_id: id });

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    assert!(matches!(next_but_git(&mut link).await, ControlToNode::Prompt { text, .. } if text == "urgent"));
}

#[tokio::test]
async fn send_now_whose_steer_goes_unanswered_leaves_the_message_queued_without_stopping_the_turn() {
    let server = Server::start_with(|config| config.steer_timeout = std::time::Duration::from_millis(200)).await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/{urgent}/send-now"))).await;
    let ControlToNode::Steer { request_id, .. } = next_but_git(&mut link).await else {
        panic!("expected a steer");
    };

    assert_eq!(status, 204);
    assert_eq!(queue(&server, id).await.len(), 1);

    link.send(NodeToControl::Steered { request_id, outcome: "injected".into(), error: None }).await;

    for _ in 0..100 {
        if queue(&server, id).await.is_empty() {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(queue(&server, id).await.is_empty());
    assert_eq!(user_message_texts(&server, id).await, vec!["long task", "urgent"]);
    assert_turn_not_cancelled(&server, &mut link, id).await;
}

#[tokio::test]
async fn send_now_injected_after_the_queue_row_was_already_sent_still_records_the_injection() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", STEERING_NODE).await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    let url = format!("/api/v1/conversations/{id}/queue/{urgent}/send-now");
    let answering = async {
        let ControlToNode::Steer { request_id, .. } = next_but_git(&mut link).await else {
            panic!("expected a steer");
        };
        link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
        assert!(matches!(next_but_git(&mut link).await, ControlToNode::Prompt { text, .. } if text == "urgent"));
        link.send(NodeToControl::Steered { request_id, outcome: "injected".into(), error: None }).await;
    };
    let ((status, _), ()) = tokio::join!(server.user.json(server.user.post(&url)), answering);

    assert_eq!(status, 204);
    assert_eq!(user_message_texts(&server, id).await, vec!["long task", "urgent", "urgent"]);
}

#[tokio::test]
async fn send_now_on_a_node_older_than_steering_stops_the_turn_without_asking() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", "0.9.0").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    next_but_git(&mut link).await;

    prompt(&server, id, "urgent").await;
    let urgent = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/{urgent}/send-now"))).await;

    assert_eq!(status, 204);
    assert_eq!(next_but_git(&mut link).await, ControlToNode::Cancel { conversation_id: id });
}

#[tokio::test]
async fn send_all_now_on_an_empty_queue_is_not_found() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/send-all-now"))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn send_now_on_an_idle_conversation_just_sends_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;

    prompt(&server, id, "queued").await;
    let queued = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    link.barrier(id, 4).await;
    assert_eq!(queue(&server, id).await.len(), 1);

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/queue/{queued}/send-now"))).await;

    assert_eq!(link.expect_prompt().await.1, "queued");
}

#[tokio::test]
async fn stopping_a_turn_holds_the_queue_until_the_person_decides() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;
    prompt(&server, id, "held").await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::Cancel { conversation_id: id });

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into(), usage: None }).await;
    link.barrier(id, 4).await;

    assert_eq!(status_of(&server, id).await, "review");
    assert_eq!(queue(&server, id).await.len(), 1);
}

#[tokio::test]
async fn a_failed_session_marks_the_conversation_failed_and_a_message_to_it_retries_the_session_first() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("go")).await;
    link.expect_prompt().await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "agent exited with code 1".into() }).await;
    stream.conversation_with_status("failed").await;

    let events = server.events(id).await;
    let error = events.last().unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["payload"]["message"], "agent exited with code 1");

    assert_eq!(prompt(&server, id, "again").await, 204);

    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, .. } => assert_eq!((conversation_id, resume.as_deref()), (id, Some("acp-1"))),
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(link.expect_prompt().await.1, "again");
    assert_eq!(status_of(&server, id).await, "working");
}

async fn failed_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let id = started_conversation(server, link, None).await;
    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "This node already runs 4 conversations, its limit.".into() }).await;
    common::eventually("the conversation to fail", || async { (status_of(server, id).await == "failed").then_some(()) }).await;

    id
}

#[tokio::test]
async fn retrying_a_failed_conversation_starts_its_session_again_and_delivers_what_was_queued() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = failed_conversation(&server, &mut link).await;
    sqlx::query("insert into queued_prompts (conversation_id, position, text) values ($1, 1, 'waiting')").bind(id).execute(&server.pool).await.unwrap();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    assert_eq!(status, 204);
    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, .. } => assert_eq!((conversation_id, resume.as_deref()), (id, Some("acp-1"))),
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(link.expect_prompt().await.1, "waiting");
}

#[tokio::test]
async fn retrying_a_failed_conversation_with_nothing_queued_tells_the_agent_to_continue() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = failed_conversation(&server, &mut link).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    link.expect_start_session().await;
    assert_eq!(link.expect_prompt().await.1, "Continue from where you stopped.");
}

#[tokio::test]
async fn retrying_a_failed_conversation_that_never_started_asks_for_a_new_session_in_the_same_folder() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.send(NodeToControl::SessionFailed { conversation_id: conversation.id, seq: 1, message: "limit".into() }).await;
    common::eventually("the conversation to fail", || async { (status_of(&server, conversation.id).await == "failed").then_some(()) }).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{}/retry", conversation.id))).await;

    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, cwd, .. } => assert_eq!((conversation_id, resume, cwd.as_str()), (conversation.id, None, "/work/app")),
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(status_of(&server, conversation.id).await, "done");
}

#[tokio::test]
async fn retrying_while_the_node_is_offline_says_to_start_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = failed_conversation(&server, &mut link).await;
    let node = link.id;
    drop(link);
    common::eventually("the node to go offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|candidate| candidate["id"] == json!(node) && candidate["online"] == false).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (409, Some("node_offline"), Some("This node is offline. Start it, then try again.")));
    assert_eq!(status_of(&server, id).await, "failed");
}

#[tokio::test]
async fn retrying_a_conversation_whose_node_was_removed_says_to_start_a_new_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = failed_conversation(&server, &mut link).await;
    sqlx::query("update nodes set revoked_at = now() where id = $1").bind(link.id).execute(&server.pool).await.unwrap();

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_removed")));
}

#[tokio::test]
async fn another_person_cannot_retry_a_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = failed_conversation(&server, &mut link).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    assert_eq!((status, status_of(&server, id).await.as_str()), (404, "failed"));
}

#[tokio::test]
async fn mode_and_model_changes_reach_the_node_and_the_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "acceptEdits", "model": "sonnet"}))).await;
    assert_eq!(status, 204);

    match link.next().await {
        ControlToNode::SetConfig { conversation_id, mode, model, config } => {
            assert_eq!((conversation_id, mode.as_deref(), model.as_deref(), config), (id, Some("acceptEdits"), Some("sonnet"), None));
        }
        other => panic!("expected set_config, got {other:?}"),
    }

    let conversation = server.conversation(id).await;
    assert_eq!(conversation["mode"], "acceptEdits");
    assert_eq!(conversation["model"], "sonnet");
}

#[tokio::test]
async fn renaming_and_archiving_move_a_conversation_between_the_lists_and_close_its_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let url = format!("/api/v1/conversations/{id}");

    let (status, renamed) = server.user.json(server.user.patch(&url).json(&json!({"title": "  Tidy the build  "}))).await;
    assert_eq!((status, renamed["title"].as_str()), (200, Some("Tidy the build")));

    let (status, body) = server.user.json(server.user.patch(&url).json(&json!({"title": " "}))).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("title_required")));

    let (_, archived) = server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;
    assert!(archived["archived_at"].is_string());
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });

    let (_, active) = server.user.json(server.user.get("/api/v1/conversations?archived=false")).await;
    let (_, past) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    assert!(active.as_array().unwrap().is_empty());
    assert_eq!(past[0]["id"], json!(id));

    let (_, restored) = server.user.json(server.user.patch(&url).json(&json!({"archived": false}))).await;
    assert_eq!(restored["archived_at"], json!(null));
}

#[tokio::test]
async fn pinning_a_conversation_keeps_pin_order_and_unpinning_clears_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let first = started_conversation(&server, &mut link, None).await;
    link.barrier(first, 1).await;
    let second = started_conversation(&server, &mut link, None).await;
    link.barrier(second, 1).await;
    sqlx::query("update conversations set updated_at = '2026-09-01T00:00:00Z'").execute(&server.pool).await.unwrap();

    let (_, pinned) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{second}")).json(&json!({"pinned": true}))).await;
    let (_, repinned) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{second}")).json(&json!({"pinned": true}))).await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{first}")).json(&json!({"pinned": true}))).await;

    assert!(pinned["pinned_at"].is_string());
    assert_eq!(repinned["pinned_at"], pinned["pinned_at"]);
    assert_eq!(pinned["updated_at"], json!("2026-09-01T00:00:00Z"));
    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations?archived=false")).await;
    let mut order: Vec<(String, String)> = listed.as_array().unwrap().iter().map(|row| (row["pinned_at"].as_str().unwrap().to_string(), row["id"].as_str().unwrap().to_string())).collect();
    order.sort();
    assert_eq!(order.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>(), vec![second.to_string(), first.to_string()]);

    let (_, unpinned) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{second}")).json(&json!({"pinned": false}))).await;
    assert_eq!(unpinned["pinned_at"], json!(null));
}

#[tokio::test]
async fn renaming_or_archiving_a_conversation_leaves_its_last_activity_alone() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let url = format!("/api/v1/conversations/{id}");
    sqlx::query("update conversations set updated_at = '2026-09-01T00:00:00Z'").execute(&server.pool).await.unwrap();

    server.user.json(server.user.patch(&url).json(&json!({"title": "Renamed"}))).await;
    let (_, archived) = server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;

    assert_eq!(archived["updated_at"], json!("2026-09-01T00:00:00Z"));
}

#[tokio::test]
async fn sending_a_message_to_an_archived_conversation_unarchives_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let url = format!("/api/v1/conversations/{id}");
    server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;

    let (status, _) = server.user.json(server.user.post(&format!("{url}/prompt")).json(&json!({"text": "hello again"}))).await;
    assert_eq!(status, 204);

    let (_, active) = server.user.json(server.user.get("/api/v1/conversations?archived=false")).await;
    assert_eq!(active[0]["id"], json!(id));
    assert_eq!(active[0]["archived_at"], json!(null));
}

async fn close(server: &Server, id: Uuid) -> u16 {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/close"))).await.0
}

#[tokio::test]
async fn closing_a_working_conversation_ends_its_agent_and_leaves_it_done_and_not_archived() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;
    assert_eq!(status_of(&server, id).await, "working");

    assert_eq!(close(&server, id).await, 204);

    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });
    assert_eq!(status_of(&server, id).await, "done");
    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations")).await;
    assert!(listed.as_array().unwrap().iter().any(|conversation| conversation["id"] == id.to_string()));
}

#[tokio::test]
async fn a_message_after_closing_a_working_conversation_resumes_the_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("long task")).await;
    link.expect_prompt().await;
    close(&server, id).await;
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });

    prompt(&server, id, "carry on").await;

    link.expect_start_session().await;
    assert_eq!(link.expect_prompt().await.1, "carry on");
}

#[tokio::test]
async fn closing_an_idle_conversation_ends_its_running_agent_and_keeps_its_status() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    sqlx::query("update conversations set status = 'review' where id = $1").bind(id).execute(&server.pool).await.unwrap();

    assert_eq!(close(&server, id).await, 204);

    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });
    assert_eq!(status_of(&server, id).await, "review");
}

#[tokio::test]
async fn closing_an_idle_conversation_without_a_running_agent_tells_the_node_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    close(&server, id).await;
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });

    assert_eq!(close(&server, id).await, 204);

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;
    assert_eq!(link.next().await, ControlToNode::Cancel { conversation_id: id });
}

#[tokio::test]
async fn a_conversation_belongs_to_its_owner() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("secret plans")).await;
    let other = server.second_user("eve@example.com").await;
    let mut other_stream = other.stream().await;

    for request in [
        other.get(&format!("/api/v1/conversations/{id}")),
        other.get(&format!("/api/v1/conversations/{id}/events")),
        other.post(&format!("/api/v1/conversations/{id}/cancel")),
        other.post(&format!("/api/v1/conversations/{id}/close")),
        other.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "hi"})),
    ] {
        assert_eq!(other.json(request).await.0, 404);
    }

    let (_, listed) = other.json(other.get("/api/v1/conversations")).await;
    assert!(listed.as_array().unwrap().is_empty());

    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;
    let first = other_stream.next().await;
    assert!(matches!(first, StreamMessage::Node(_)), "{first:?}");
}

#[tokio::test]
async fn the_conversation_api_needs_a_signed_in_person() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    for path in ["/api/v1/conversations", "/api/v1/nodes", "/api/v1/me"] {
        let (status, body) = anonymous.json(anonymous.get(path)).await;

        assert_eq!((status, body["error"].as_str()), (401, Some("unauthorised")), "{path}");
    }
}

#[tokio::test]
async fn a_client_token_can_open_the_stream() {
    let server = Server::start().await;
    let (_, token) = server.join("client", "desktop").await;
    let link = server.online_node("laptop").await;

    let url = format!("ws://127.0.0.1:{}/api/v1/stream", server.address.port());
    let mut request = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(url).unwrap();
    request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;

    use futures_util::StreamExt;

    loop {
        let frame = tokio::time::timeout(common::WAIT, socket.next()).await.unwrap().unwrap().unwrap();
        let message: StreamMessage = serde_json::from_str(frame.to_text().unwrap()).unwrap();

        if matches!(message, StreamMessage::Node(node) if node.draining) {
            return;
        }
    }
}

#[tokio::test]
async fn queued_prompts_wait_for_a_node_that_is_offline_and_go_out_when_it_returns() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let user = server.user.clone();
    common::eventually("the node to go offline", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let (status, answer) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "while you were away"}))).await;
    assert_eq!(status, 202);
    assert_eq!(answer["message"], "The node is not connected. Your message is queued and goes as soon as it reconnects.");
    assert_eq!(queue(&server, id).await.len(), 1);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![(id, 1)]).await;

    assert_eq!(returned.expect_prompt().await.1, "while you were away");
}

#[tokio::test]
async fn the_queue_is_sent_when_a_turn_ends_even_if_recording_the_turn_fails() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;
    sqlx::query("alter table usage_turns rename to usage_turns_gone").execute(&server.pool).await.unwrap();

    assert_eq!(prompt(&server, id, "second").await, 204);
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;

    assert_eq!(link.expect_prompt().await.1, "second");
    assert!(queue(&server, id).await.is_empty());
}

#[tokio::test]
async fn a_prompt_after_archiving_and_restoring_resumes_the_session_before_sending() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let url = format!("/api/v1/conversations/{id}");

    server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });
    server.user.json(server.user.patch(&url).json(&json!({"archived": false}))).await;

    assert_eq!(prompt(&server, id, "back again").await, 204);

    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, cwd, .. } => {
            assert_eq!((conversation_id, resume.as_deref(), cwd.as_str()), (id, Some("acp-1"), "/work/app"));
        }
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(link.expect_prompt().await.1, "back again");
}

#[tokio::test]
async fn a_prompt_to_a_session_the_node_reports_as_ended_resumes_it_first() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(node_id, &token).await;
    returned
        .send(NodeToControl::Hello {
            version: "0.1.0".into(),
            hostname: "laptop".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            agents: vec![],
            sessions: vec![mycelium_proto::conversation::SessionPosition { conversation_id: id, last_seq: 1, live: false }],
        })
        .await;
    common::eventually("the node to be back online", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(node_id) && node["online"] == true).then_some(())
    })
    .await;

    assert_eq!(prompt(&server, id, "wake up").await, 204);

    assert!(matches!(returned.next().await, ControlToNode::StartSession { resume: Some(_), .. }));
    assert_eq!(returned.expect_prompt().await.1, "wake up");
}

#[tokio::test]
async fn a_running_conversation_the_node_no_longer_holds_is_failed_when_the_node_reconnects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("go")).await;
    link.expect_prompt().await;
    assert_eq!(status_of(&server, id).await, "working");
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![]).await;

    common::eventually("the conversation to fail", || async { (status_of(&server, id).await == "failed").then_some(()) }).await;
    let events = server.events(id).await;
    assert_eq!(events.last().unwrap()["kind"], "error");
}

#[tokio::test]
async fn a_running_conversation_the_node_still_holds_is_left_running_when_it_reconnects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("go")).await;
    link.expect_prompt().await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![(id, 0)]).await;
    returned.barrier(id, 1).await;

    assert_eq!(status_of(&server, id).await, "working");
}

#[tokio::test]
async fn a_prompt_sent_before_the_node_says_hello_waits_for_the_sessions_it_reports() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let user = server.user.clone();
    common::eventually("the node to go offline", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let mut returned = server.connect_node(node_id, &token).await;
    assert_eq!(prompt(&server, id, "before hello").await, 202);
    returned.hello(vec![(id, 1)]).await;

    assert_eq!(returned.expect_prompt().await.1, "before hello");
    assert_eq!(status_of(&server, id).await, "working");
}

async fn stream_request(server: &Server, origin: Option<&str>) -> Result<(), u16> {
    let url = format!("ws://127.0.0.1:{}/api/v1/stream", server.address.port());
    let mut request = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(url).unwrap();
    request.headers_mut().insert("cookie", server.user.cookie.parse().unwrap());

    if let Some(origin) = origin {
        request.headers_mut().insert("origin", origin.parse().unwrap());
    }

    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => Ok(()),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(other) => panic!("unexpected error {other:?}"),
    }
}

#[tokio::test]
async fn the_stream_refuses_a_browser_page_from_another_origin() {
    let server = Server::start().await;

    assert_eq!(stream_request(&server, Some("https://evil.example")).await, Err(403));
    assert_eq!(stream_request(&server, Some("http://127.0.0.1:1")).await, Err(403));
}

#[tokio::test]
async fn the_stream_accepts_the_control_planes_own_origin_and_clients_that_send_none() {
    let server = Server::start().await;

    assert_eq!(stream_request(&server, Some(&server.base)).await, Ok(()));
    assert_eq!(stream_request(&server, None).await, Ok(()));
}

#[tokio::test]
async fn files_are_listed_by_the_node_running_the_conversation_in_its_folder() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.get(&format!("/api/v1/conversations/{}/files?query=flake", created.id))).await });

    let (request_id, cwd, query) = match link.next().await {
        ControlToNode::ListFiles { request_id, cwd, query } => (request_id, cwd, query),
        other => panic!("expected list_files, got {other:?}"),
    };
    assert_eq!((cwd.as_str(), query.as_str()), ("/work/app", "flake"));

    link.send(NodeToControl::Files { request_id, paths: vec!["flake.nix".into()] }).await;

    let (status, body) = asking.await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(body, json!(["flake.nix"]));
}

async fn agent_titles(server: &Server, link: &mut NodeLink, id: Uuid, seq: i64, title: &str) {
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update: json!({"sessionId": "acp-1", "update": {"sessionUpdate": "session_info_update", "title": title}}) }).await;

    common::eventually("the predicted title", || async {
        let predicted = sqlx::query_scalar::<_, Option<String>>("select predicted_title from conversations where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();

        (predicted.as_deref() == Some(title)).then_some(())
    })
    .await;
}

#[tokio::test]
async fn the_title_follows_the_agent_until_the_user_sets_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let url = format!("/api/v1/conversations/{id}");

    agent_titles(&server, &mut link, id, 1, "Move restore copy into shared strings").await;
    let followed = server.conversation(id).await;
    assert_eq!((followed["title"].as_str(), followed["title_user_set"].as_bool()), (Some("Move restore copy into shared strings"), Some(false)));

    let (_, renamed) = server.user.json(server.user.patch(&url).json(&json!({"title": "Restore work"}))).await;
    assert_eq!((renamed["title"].as_str(), renamed["title_user_set"].as_bool()), (Some("Restore work"), Some(true)));

    agent_titles(&server, &mut link, id, 2, "Share the restore strings").await;
    assert_eq!(server.conversation(id).await["title"], "Restore work");
}

#[tokio::test]
async fn clearing_a_title_returns_to_the_latest_prediction_and_follows_the_agent_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let url = format!("/api/v1/conversations/{id}");
    server.user.json(server.user.patch(&url).json(&json!({"title": "Restore work"}))).await;
    agent_titles(&server, &mut link, id, 1, "Share the restore strings").await;

    let (status, cleared) = server.user.json(server.user.patch(&url).json(&json!({"clear_title": true}))).await;
    assert_eq!((status, cleared["title"].as_str(), cleared["title_user_set"].as_bool()), (200, Some("Share the restore strings"), Some(false)));

    agent_titles(&server, &mut link, id, 2, "Share restore strings with Android").await;
    common::eventually("the title to follow", || async { (server.conversation(id).await["title"] == "Share restore strings with Android").then_some(()) }).await;
}

#[tokio::test]
async fn a_conversation_started_without_a_title_is_named_from_its_first_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    assert_eq!(server.conversation(id).await["title"], "New conversation");

    prompt(&server, id, "Fix the flaky restore test\nit fails on CI").await;

    let named = server.conversation(id).await;
    assert_eq!((named["title"].as_str(), named["title_user_set"].as_bool()), (Some("Fix the flaky restore test"), Some(false)));
}

#[tokio::test]
async fn a_title_given_at_creation_is_the_users_and_the_agent_does_not_replace_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "title": "Release"}))).await;
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    link.expect_start_session().await;
    link.start_session(id, "acp-1").await;

    agent_titles(&server, &mut link, id, 1, "Cut the release").await;

    let kept = server.conversation(id).await;
    assert_eq!((kept["title"].as_str(), kept["title_user_set"].as_bool()), (Some("Release"), Some(true)));
}

async fn start_side(server: &Server, parent: Uuid, body: serde_json::Value) -> (u16, serde_json::Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{parent}/side")).json(&body)).await
}

#[tokio::test]
async fn a_btw_question_opens_a_linked_thread_on_the_same_node_folder_and_agent_and_sends_the_question() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = started_conversation(&server, &mut link, None).await;

    let (status, side) = start_side(&server, parent, json!({"kind": "btw", "text": "What does RESTORE_BACKOFF_MS default to?"})).await;

    assert_eq!(status, 201, "{side}");
    assert_eq!((side["parent_id"].clone(), side["side_kind"].as_str(), side["cwd"].as_str(), side["agent"].as_str()), (json!(parent), Some("btw"), Some("/work/app"), Some("claude")));
    assert_eq!(side["title"], "What does RESTORE_BACKOFF_MS default to?");
    let id: Uuid = side["id"].as_str().unwrap().parse().unwrap();
    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, fork, .. } => assert_eq!((conversation_id, resume, fork), (id, None, false)),
        other => panic!("expected start_session, got {other:?}"),
    }
    link.start_session(id, "acp-side").await;
    assert_eq!(link.expect_prompt().await.1, "What does RESTORE_BACKOFF_MS default to?");
}

#[tokio::test]
async fn a_btw_needs_a_question_but_a_side_chat_can_open_empty() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = started_conversation(&server, &mut link, None).await;

    let (status, body) = start_side(&server, parent, json!({"kind": "btw"})).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("text_required")));

    let (status, side) = start_side(&server, parent, json!({"kind": "side"})).await;
    assert_eq!((status, side["title"].as_str(), side["status"].as_str()), (201, Some("Side chat"), Some("done")));
}

#[tokio::test]
async fn forking_a_claude_conversation_continues_its_session_through_acp() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = started_conversation(&server, &mut link, None).await;
    common::eventually("the session to be recorded", || async { (!server.conversation(parent).await["acp_session_id"].is_null()).then_some(()) }).await;

    let (status, fork) = start_side(&server, parent, json!({"kind": "fork"})).await;

    assert_eq!((status, fork["side_kind"].as_str(), fork["title"].as_str()), (201, Some("fork"), Some("New conversation (fork)")));
    match link.next().await {
        ControlToNode::StartSession { resume, fork, .. } => assert_eq!((resume.as_deref(), fork), (Some("acp-1"), true)),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn forking_a_conversation_the_agent_cannot_fork_starts_a_session_that_is_told_what_was_said() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "codex", "cwd": "/work/app", "prompt": "Fix the flaky test"}))).await;
    let parent: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    link.expect_start_session().await;
    link.start_session(parent, "acp-1").await;
    link.expect_prompt().await;
    link.send(NodeToControl::AcpUpdate { conversation_id: parent, seq: 1, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Fixed the race in restore"}}}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: parent, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
    common::eventually("the turn to end", || async { (status_of(&server, parent).await == "review").then_some(()) }).await;

    let (status, fork) = start_side(&server, parent, json!({"kind": "fork", "text": "Now do the Android side"})).await;
    let id: Uuid = fork["id"].as_str().unwrap().parse().unwrap();

    assert_eq!(status, 201, "{fork}");
    match link.next().await {
        ControlToNode::StartSession { resume, fork, .. } => assert_eq!((resume, fork), (None, false)),
        other => panic!("expected start_session, got {other:?}"),
    }
    link.start_session(id, "acp-fork").await;
    let (_, told) = link.expect_prompt().await;
    assert!(told.contains("User: Fix the flaky test") && told.contains("Agent: Fixed the race in restore") && told.ends_with("Now do the Android side"), "{told}");
}

#[tokio::test]
async fn a_side_chat_belongs_to_its_owner_and_its_node() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = started_conversation(&server, &mut link, None).await;
    let stranger = server.second_user("other@example.com").await;

    let (status, _) = stranger.json(stranger.post(&format!("/api/v1/conversations/{parent}/side")).json(&json!({"kind": "side"}))).await;
    assert_eq!(status, 404);

    let other = server.online_node("desktop").await;
    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": other.id, "agent": "claude", "cwd": "/work/app", "parent_id": parent, "side_kind": "side"}))).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("parent_invalid")));
}

async fn archived_conversation(server: &Server, link: &mut NodeLink, prompt_text: Option<&str>) -> Uuid {
    let id = started_conversation(server, link, prompt_text).await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"archived": true}))).await;

    id
}

async fn count_rows(server: &Server, table: &str, id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("select count(*) from {table} where conversation_id = $1"))).bind(id).fetch_one(&server.pool).await.unwrap()
}

#[tokio::test]
async fn a_conversation_that_is_not_archived_is_not_deleted() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;

    let (status, body) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}"))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("not_archived")));
    assert_eq!(server.conversation(id).await["id"], json!(id));
}

#[tokio::test]
async fn another_person_cannot_delete_a_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = archived_conversation(&server, &mut link, None).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.delete(&format!("/api/v1/conversations/{id}"))).await;

    assert_eq!(status, 404);
    assert_eq!(server.conversation(id).await["id"], json!(id));
}

#[tokio::test]
async fn deleting_an_archived_conversation_removes_its_messages_usage_and_side_chats_and_closes_its_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("Fix the flaky restore test")).await;
    link.expect_prompt().await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: Some(json!({"inputTokens": 10, "outputTokens": 5})) }).await;
    let (_, side) = start_side(&server, id, json!({"kind": "btw", "text": "why?"})).await;
    let side_id: Uuid = side["id"].as_str().unwrap().parse().unwrap();
    link.expect_start_session().await;
    link.start_session(side_id, "acp-2").await;
    link.expect_prompt().await;
    let kept = started_conversation(&server, &mut link, Some("Keep me")).await;
    link.expect_prompt().await;
    link.send(NodeToControl::TurnEnd { conversation_id: kept, seq: 1, stop_reason: "end_turn".into(), usage: Some(json!({"inputTokens": 1, "outputTokens": 1})) }).await;
    common::eventually("both turns recorded", || async { (count_rows(&server, "usage_turns", id).await == 1 && count_rows(&server, "usage_turns", kept).await == 1).then_some(()) }).await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"archived": true}))).await;
    assert_eq!(link.next().await, ControlToNode::CloseSession { conversation_id: id });

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}"))).await;

    assert_eq!(status, 204);
    assert_eq!(server.user.get(&format!("/api/v1/conversations/{id}")).send().await.unwrap().status(), 404);
    assert_eq!(server.user.get(&format!("/api/v1/conversations/{side_id}")).send().await.unwrap().status(), 404);
    for table in ["events", "usage_turns", "queued_prompts", "search_entries"] {
        assert_eq!((table, count_rows(&server, table, id).await), (table, 0));
    }
    assert_eq!(count_rows(&server, "usage_turns", kept).await, 1);
    let closed: Vec<Uuid> = vec![link.next().await, link.next().await].into_iter().filter_map(|message| match message {
        ControlToNode::CloseSession { conversation_id } => Some(conversation_id),
        _ => None,
    }).collect();
    assert_eq!(closed.len(), 2);
    assert!(closed.contains(&id) && closed.contains(&side_id));
    let removed = stream.until(|message| match message {
        StreamMessage::ConversationRemoved { conversation_id } if conversation_id == id => Some(conversation_id),
        _ => None,
    })
    .await;
    assert_eq!(removed, id);
    let (_, past) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    assert!(past.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn deleting_a_conversation_removes_the_attachments_nothing_else_uses_and_keeps_shared_ones() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let doomed = started_conversation(&server, &mut link, None).await;
    let kept = started_conversation(&server, &mut link, None).await;
    let upload = |conversation: Uuid, bytes: Vec<u8>| {
        let request = server.user.post(&format!("/api/v1/conversations/{conversation}/attachments")).query(&[("name", "a.txt")]).header("content-type", "text/plain").body(bytes);

        async { server.user.json(request).await.1 }
    };
    let only_here = upload(doomed, b"only here".to_vec()).await;
    let shared = upload(doomed, b"shared".to_vec()).await;
    for (conversation, attachments) in [(doomed, json!([only_here, shared])), (kept, json!([shared]))] {
        let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation}/prompt")).json(&json!({"text": "see", "attachments": attachments}))).await;
        assert_eq!(status, 204, "{body}");
    }
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{doomed}")).json(&json!({"archived": true}))).await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{doomed}"))).await;

    let file = |attachment: &serde_json::Value| server.data_dir.join("attachments").join(attachment["sha256"].as_str().unwrap());
    assert_eq!(status, 204);
    assert!(!file(&only_here).exists());
    assert!(file(&shared).exists());
    assert_eq!(server.user.get(&format!("/api/v1/attachments/{}", only_here["sha256"].as_str().unwrap())).send().await.unwrap().status(), 404);
}

#[tokio::test]
async fn migration_0033_gives_back_the_predicted_title_only_to_titles_the_user_never_set() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let untouched = started_conversation(&server, &mut link, Some("Fix the flaky restore test\nit fails on CI")).await;
    link.expect_prompt().await;
    let renamed = started_conversation(&server, &mut link, Some("Fix the flaky restore test")).await;
    link.expect_prompt().await;
    let untitled = started_conversation(&server, &mut link, None).await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{renamed}")).json(&json!({"title": "Restore work"}))).await;
    sqlx::query("update conversations set title = 'Untitled conversation', title_user_set = true where id = $1").bind(untitled).execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set title_user_set = true where id = $1").bind(untouched).execute(&server.pool).await.unwrap();

    sqlx::query(sqlx::AssertSqlSafe(include_str!("../../migrations/0033_restore_predicted_titles.sql"))).execute(&server.pool).await.unwrap();

    let flags = (server.conversation(untouched).await["title_user_set"].as_bool(), server.conversation(renamed).await["title_user_set"].as_bool(), server.conversation(untitled).await["title_user_set"].as_bool());
    assert_eq!(flags, (Some(false), Some(true), Some(false)));
}

#[tokio::test]
async fn a_conversation_whose_idle_session_the_node_closed_resumes_it_on_the_next_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;

    link.send(NodeToControl::SessionClosed { conversation_id: id }).await;
    link.barrier(id, 1).await;
    prompt(&server, id, "carry on").await;

    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, .. } => assert_eq!((conversation_id, resume.as_deref()), (id, Some("acp-1"))),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn the_latest_commands_settings_and_mode_are_kept_for_a_conversation_whose_early_events_are_not_loaded() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let updates = [
        json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "old", "description": "gone"}]}),
        json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review the diff"}]}),
        json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}),
        json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "effort", "currentValue": "high"}]}),
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hello"}}),
    ];

    for (index, update) in updates.into_iter().enumerate() {
        link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: index as i64 + 1, update: json!({"update": update}) }).await;
    }

    link.barrier(id, 6).await;

    let (status, kept) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/session"))).await;
    let kinds: Vec<&str> = kept.as_array().unwrap().iter().map(|update| update["sessionUpdate"].as_str().unwrap()).collect();

    assert_eq!(status, 200, "{kept}");
    assert_eq!(kinds, ["available_commands_update", "config_option_update", "current_mode_update"]);
    assert_eq!(kept[0]["availableCommands"][0]["name"], "review");
    assert_eq!(kept[2]["currentModeId"], "plan");
}

#[tokio::test]
async fn a_conversation_that_has_had_no_session_updates_has_no_kept_state() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;

    let (status, kept) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/session"))).await;

    assert_eq!((status, kept), (200, json!([])));
}

async fn answer_head(link: &mut NodeLink, branch: Option<&str>) -> String {
    loop {
        match link.next().await {
            ControlToNode::Git { request_id, cwd, command: mycelium_proto::link::GitCommand::Head } => {
                link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"branch": branch}), error: None }).await;

                return cwd;
            }
            _ => continue,
        }
    }
}

async fn branch_of(server: &Server, id: Uuid, wanted: Option<&str>) -> serde_json::Value {
    common::eventually("the recorded branch", || async {
        let conversation = server.conversation(id).await;

        (conversation["branch"].as_str() == wanted).then_some(conversation)
    })
    .await
}

#[tokio::test]
async fn a_turn_end_records_the_branch_the_node_reports() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", "v0.10.0").await;
    let id = started_conversation(&server, &mut link, None).await;

    let cwd = answer_head(&mut link, Some("main")).await;
    branch_of(&server, id, Some("main")).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;
    answer_head(&mut link, Some("topic")).await;

    assert_eq!(cwd, "/work/app");
    branch_of(&server, id, Some("topic")).await;
}

#[tokio::test]
async fn a_node_that_cannot_report_a_branch_is_not_asked() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", "v0.9.1").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

    common::eventually("the turn to end", || async { (status_of(&server, id).await == "review").then_some(()) }).await;

    assert_eq!(server.conversation(id).await["branch"], serde_json::Value::Null);
}

#[tokio::test]
async fn a_conversation_started_in_a_new_worktree_has_its_branch_straight_away() {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", "v0.10.0").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    let user = server.user.clone();
    let node_id = link.id;
    let starting = tokio::spawn(async move { user.json(user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "project_id": project, "branch": "feature/login"}))).await });

    let (request_id, cwd) = loop {
        if let ControlToNode::Git { request_id, cwd, .. } = link.next().await {
            break (request_id, cwd);
        }
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": "/work/app-worktrees/feature-login", "branch": "feature/login"}), error: None }).await;
    let (status, conversation) = starting.await.unwrap();

    assert_eq!((status, cwd.as_str()), (201, "/work/app"));
    assert_eq!((conversation["cwd"].clone(), conversation["branch"].clone()), (json!("/work/app-worktrees/feature-login"), json!("feature/login")));
}

async fn fake_github() -> String {
    use axum::Json;
    use axum::routing::get;

    let pulls = || async { Json(json!([{"number": 12, "title": "Rework", "state": "open", "draft": false, "merged_at": null, "html_url": "https://forge.example/acme-org/acme-backups/pull/12", "head": {"ref": "topic", "repo": {"full_name": "acme-org/acme-backups"}}}])) };
    let router = axum::Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/repos/{owner}/{name}/pulls", get(pulls));

    common::serve(router).await
}

#[tokio::test]
async fn the_branch_of_a_project_conversation_carries_its_open_pull_request() {
    let server = Server::start().await;
    let github = fake_github().await;
    let mut link = server.online_node_as("laptop", "v0.10.0").await;
    let (_, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": "github", "base_url": github, "token": "ghp_0123456789abcdefghijklmnopqrstuvwxyz"}))).await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let forge_id: Uuid = forge["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("insert into projects (owner, node_id, name, path, forge_id, repository) values ($1, $2, 'backups', '/work/app', $3, 'acme-org/acme-backups')").bind(owner).bind(link.id).bind(forge_id).execute(&server.pool).await.unwrap();
    let id = started_conversation(&server, &mut link, None).await;

    answer_head(&mut link, Some("topic")).await;
    let recorded = branch_of(&server, id, Some("topic")).await;
    assert_eq!((recorded["pull_request_label"].clone(), recorded["pull_request_url"].clone()), (json!("PR #12"), json!("https://forge.example/acme-org/acme-backups/pull/12")));
}
