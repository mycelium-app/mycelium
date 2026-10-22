mod common;

use common::{NodeLink, Server};
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
    link.send(NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 4, stop_reason: "end_turn".into() }).await;

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

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 6, stop_reason: "cancelled".into() }).await;
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

    returned.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into() }).await;
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

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into() }).await;
    assert_eq!(link.expect_prompt().await.1, "second");

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 4, stop_reason: "end_turn".into() }).await;
    assert_eq!(link.expect_prompt().await.1, "third");

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

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into() }).await;
    assert_eq!(link.expect_prompt().await.1, "urgent");

    let remaining = queue(&server, id).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["text"], "later");
}

#[tokio::test]
async fn send_now_on_an_idle_conversation_just_sends_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;

    prompt(&server, id, "queued").await;
    let queued = queue(&server, id).await[0]["id"].as_str().unwrap().to_string();

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into() }).await;
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

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "cancelled".into() }).await;
    link.barrier(id, 4).await;

    assert_eq!(status_of(&server, id).await, "review");
    assert_eq!(queue(&server, id).await.len(), 1);
}

#[tokio::test]
async fn a_failed_session_marks_the_conversation_failed_and_refuses_prompts() {
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

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "again"}))).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("session_failed")));
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

    assert_eq!(prompt(&server, id, "while you were away").await, 204);
    assert_eq!(queue(&server, id).await.len(), 1);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![(id, 1)]).await;

    assert_eq!(returned.expect_prompt().await.1, "while you were away");
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
    assert_eq!(prompt(&server, id, "before hello").await, 204);
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
