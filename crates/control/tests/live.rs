mod common;

use common::{NodeLink, Server, StreamClient};
use mycelium_proto::conversation::{AgentId, Conversation, ConversationStatus, DeviceKind, DeviceState, Event, EventKind, LoginInfo, LoginState, QueuedPrompt};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;
use uuid::Uuid;

const PUBLISHING_ROUTES: &[&str] = &[
    "POST /api/v1/device",
    "POST /api/v1/device/token",
    "POST /api/v1/device/{user_code}/approve",
    "POST /api/v1/device/{user_code}/deny",
    "DELETE /api/v1/nodes/{id}",
    "POST /api/v1/nodes/{id}/drain",
    "PATCH /api/v1/nodes/{id}",
    "DELETE /api/v1/logins/{login_id}",
    "POST /api/v1/conversations",
    "POST /api/v1/projects/{id}/issues/{number}/workspace",
    "PATCH /api/v1/conversations/{id}",
    "POST /api/v1/conversations/{id}/opened",
    "POST /api/v1/conversations/{id}/prompt",
    "PATCH /api/v1/conversations/{id}/queue/{queue_id}",
    "DELETE /api/v1/conversations/{id}/queue/{queue_id}",
    "POST /api/v1/conversations/{id}/queue/{queue_id}/send-now",
    "POST /api/v1/conversations/{id}/permission",
    "POST /api/v1/conversations/{id}/config",
    "POST /api/v1/imports",
    "DELETE /api/v1/terminals/{terminal_id}",
];

const SILENT_ROUTES: &[&str] = &[
    "POST /auth/logout",
    "POST /api/v1/nodes/{id}/logins",
    "POST /api/v1/logins/{login_id}/input",
    "POST /api/v1/conversations/{id}/cancel",
    "POST /api/v1/conversations/{id}/terminal",
    "POST /api/v1/conversations/{id}/git/stage",
    "POST /api/v1/conversations/{id}/git/unstage",
    "POST /api/v1/conversations/{id}/git/stage-all",
    "POST /api/v1/conversations/{id}/git/commit",
    "POST /api/v1/conversations/{id}/git/fetch",
    "POST /api/v1/conversations/{id}/git/undo",
    "POST /api/v1/terminals/{terminal_id}/input",
    "POST /api/v1/terminals/{terminal_id}/resize",
    "POST /api/v1/forges",
    "DELETE /api/v1/forges/{id}",
    "POST /api/v1/projects",
    "POST /api/v1/mcp",
    "PATCH /api/v1/memories/{id}",
    "DELETE /api/v1/memories/{id}",
];

fn routes_that_change_state() -> BTreeSet<String> {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let mut routes = BTreeSet::new();

    for line in source.lines().filter(|line| line.contains(".route(\"")) {
        let path = line.split('"').nth(1).unwrap();

        for (needle, method) in [("post(", "POST"), ("patch(", "PATCH"), ("delete(", "DELETE")] {
            if line.contains(needle) {
                routes.insert(format!("{method} {path}"));
            }
        }
    }

    routes
}

#[test]
fn every_route_that_changes_state_is_either_covered_here_or_listed_as_silent() {
    let known: BTreeSet<String> = PUBLISHING_ROUTES.iter().chain(SILENT_ROUTES).map(|route| route.to_string()).collect();

    assert_eq!(routes_that_change_state(), known, "a route was added or removed: publish its change to the stream and test it in live.rs, or list it as silent with a reason in docs/STAGE1.md");
}

async fn device_message(stream: &mut StreamClient, user_code: &str, wanted: DeviceState) -> (DeviceKind, String, String, Option<Uuid>) {
    stream
        .until(|message| match message {
            StreamMessage::Device { user_code: code, kind, label, hostname, state, result_id } if code == user_code && state == wanted => Some((kind, label, hostname, result_id)),
            _ => None,
        })
        .await
}

async fn start_device(server: &Server, kind: &str, label: &str) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/device").json(&json!({"kind": kind, "label": label, "hostname": "box", "os": "linux", "arch": "x86_64"}))).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn exchange(server: &Server, started: &Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/device/token").json(&json!({"device_code": started["device_code"]}))).await;
    assert_eq!(status, 200, "{body}");

    body
}

#[tokio::test]
async fn a_new_device_code_is_streamed_as_pending() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;

    let started = start_device(&server, "node", "laptop").await;
    let (kind, label, hostname, result_id) = device_message(&mut stream, started["user_code"].as_str().unwrap(), DeviceState::Pending).await;

    assert_eq!((kind, label.as_str(), hostname.as_str(), result_id), (DeviceKind::Node, "laptop", "box", None));
}

#[tokio::test]
async fn approving_a_device_code_is_streamed() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "client", "desktop").await;
    let user_code = started["user_code"].as_str().unwrap();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    assert_eq!(status, 204);

    let (kind, _, _, result_id) = device_message(&mut stream, user_code, DeviceState::Approved).await;
    assert_eq!((kind, result_id), (DeviceKind::Client, None));
}

#[tokio::test]
async fn denying_a_device_code_is_streamed() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/deny"))).await;
    assert_eq!(status, 204);

    device_message(&mut stream, user_code, DeviceState::Denied).await;
}

#[tokio::test]
async fn a_device_code_that_runs_out_is_streamed_as_expired() {
    let server = Server::start_with(|config| config.device_code_lifetime = Duration::from_secs(1)).await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "node", "laptop").await;

    device_message(&mut stream, started["user_code"].as_str().unwrap(), DeviceState::Expired).await;
}

#[tokio::test]
async fn a_code_that_was_exchanged_does_not_expire_afterwards() {
    let server = Server::start_with(|config| config.device_code_lifetime = Duration::from_secs(1)).await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "client", "desktop").await;
    let user_code = started["user_code"].as_str().unwrap().to_string();

    server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    exchange(&server, &started).await;

    let deadline = tokio::time::Instant::now() + Duration::from_millis(2500);

    while let Ok(message) = tokio::time::timeout_at(deadline, stream.next()).await {
        assert!(!matches!(message, StreamMessage::Device { state: DeviceState::Expired, .. }), "{message:?}");
    }
}

#[tokio::test]
async fn exchanging_a_client_code_is_streamed_with_the_token_id() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "client", "desktop").await;
    let user_code = started["user_code"].as_str().unwrap();

    server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    let token = exchange(&server, &started).await;

    let (kind, _, _, result_id) = device_message(&mut stream, user_code, DeviceState::Consumed).await;
    assert_eq!((kind, result_id), (DeviceKind::Client, Some(token["id"].as_str().unwrap().parse().unwrap())));
}

#[tokio::test]
async fn issuing_a_node_credential_publishes_the_node_offline_before_it_connects() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();

    server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    let token = exchange(&server, &started).await;
    let node_id: Uuid = token["id"].as_str().unwrap().parse().unwrap();

    let node = stream
        .until(|message| match message {
            StreamMessage::Node(node) if node.id == node_id => Some(node),
            _ => None,
        })
        .await;
    assert_eq!((node.name.as_str(), node.online), ("laptop", false));

    let (_, _, _, result_id) = device_message(&mut stream, user_code, DeviceState::Consumed).await;
    assert_eq!(result_id, Some(node_id));
}

async fn node_message(stream: &mut StreamClient, node_id: Uuid, wanted: impl Fn(&mycelium_proto::api::NodeSummary) -> bool) -> mycelium_proto::api::NodeSummary {
    stream
        .until(|message| match message {
            StreamMessage::Node(node) if node.id == node_id && wanted(&node) => Some(node),
            _ => None,
        })
        .await
}

#[tokio::test]
async fn a_node_connecting_greeting_reporting_logins_and_leaving_is_streamed() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "laptop").await;
    let mut stream = server.user.stream().await;

    let mut link = server.connect_node(id, &token).await;
    node_message(&mut stream, id, |node| !node.online && node.agents.is_empty()).await;

    link.hello(vec![]).await;
    let greeted = node_message(&mut stream, id, |node| node.online && !node.agents.is_empty()).await;
    assert_eq!(greeted.agents[0].agent, AgentId::Claude);

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { agent: AgentId::Claude, account: Some("me@example.com".into()), method: "token".into(), state: LoginState::SignedIn }] }).await;
    let signed_in = node_message(&mut stream, id, |node| !node.logins.is_empty()).await;
    assert_eq!(signed_in.logins[0].state, LoginState::SignedIn);

    drop(link);
    node_message(&mut stream, id, |node| !node.online).await;
}

#[tokio::test]
async fn renaming_a_node_is_streamed() {
    let server = Server::start().await;
    let link = server.online_node("willow-3").await;
    let mut stream = server.user.stream().await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{}", link.id)).json(&json!({"name": "willow"}))).await;
    assert_eq!(status, 204);

    node_message(&mut stream, link.id, |node| node.name == "willow").await;
}

#[tokio::test]
async fn draining_a_node_is_streamed() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;
    assert_eq!(status, 204);
    node_message(&mut stream, link.id, |node| node.draining).await;

    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": false}))).await;
    node_message(&mut stream, link.id, |node| !node.draining).await;
}

#[tokio::test]
async fn revoking_a_node_is_streamed_to_everyone() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let mut stream = server.user.stream().await;
    let mut other_stream = other.stream().await;

    server.user.json(server.user.delete(&format!("/api/v1/nodes/{}", link.id))).await;

    for stream in [&mut stream, &mut other_stream] {
        let removed = stream.until(|message| matches!(message, StreamMessage::NodeRemoved { node_id } if node_id == link.id).then_some(())).await;
        assert_eq!(removed, ());
    }
}

async fn start_login(server: &Server, link: &mut NodeLink) -> Uuid {
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "token"}))).await;
    assert_eq!(status, 200);
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();
    link.next().await;

    login_id
}

#[tokio::test]
async fn sign_in_output_and_the_end_of_a_sign_in_are_streamed_to_the_person_who_started_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let login_id = start_login(&server, &mut link).await;

    link.send(NodeToControl::LoginOutput { login_id, data: "open the link".into() }).await;
    stream.until(|message| matches!(message, StreamMessage::LoginOutput { login_id: id, data } if id == login_id && data == "open the link").then_some(())).await;

    link.send(NodeToControl::LoginFinished { login_id, ok: true }).await;
    stream.until(|message| matches!(message, StreamMessage::LoginFinished { login_id: id, ok: true } if id == login_id).then_some(())).await;
}

#[tokio::test]
async fn stopping_a_sign_in_is_streamed_as_finished() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let login_id = start_login(&server, &mut link).await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/logins/{login_id}"))).await;
    assert_eq!(status, 204);

    stream.until(|message| matches!(message, StreamMessage::LoginFinished { login_id: id, ok: false } if id == login_id).then_some(())).await;
}

#[tokio::test]
async fn a_sign_in_ends_in_the_stream_when_its_node_disconnects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let login_id = start_login(&server, &mut link).await;

    drop(link);

    stream.until(|message| matches!(message, StreamMessage::LoginFinished { login_id: id, ok: false } if id == login_id).then_some(())).await;
}

#[tokio::test]
async fn a_sign_in_ends_in_the_stream_when_its_node_is_revoked() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let login_id = start_login(&server, &mut link).await;

    server.user.json(server.user.delete(&format!("/api/v1/nodes/{}", link.id))).await;

    stream.until(|message| matches!(message, StreamMessage::LoginFinished { login_id: id, ok: false } if id == login_id).then_some(())).await;
}

async fn start_terminal(server: &Server, link: &mut NodeLink, conversation_id: Uuid) -> Uuid {
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation_id}/terminal")).json(&json!({"cols": 80, "rows": 24}))).await;
    assert_eq!(status, 200, "{started}");
    let terminal_id: Uuid = started["terminal_id"].as_str().unwrap().parse().unwrap();

    let ControlToNode::StartTerminal { terminal_id: asked, conversation_id: for_conversation, cwd, cols, rows } = link.next().await else {
        panic!("expected the node to be told to start a terminal");
    };
    assert_eq!((asked, for_conversation, cwd.as_str(), cols, rows), (terminal_id, conversation_id, "/work/app", 80, 24));

    terminal_id
}

#[tokio::test]
async fn terminal_output_and_its_end_are_streamed_only_to_the_person_who_opened_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let mut stream = server.user.stream().await;
    let terminal_id = start_terminal(&server, &mut link, conversation.id).await;

    link.send(NodeToControl::TerminalOutput { terminal_id, data: "bHM=".into() }).await;
    stream.until(|message| matches!(message, StreamMessage::TerminalOutput { terminal_id: id, data } if id == terminal_id && data == "bHM=").then_some(())).await;

    link.send(NodeToControl::TerminalExited { terminal_id }).await;
    stream.until(|message| matches!(message, StreamMessage::TerminalExited { terminal_id: id } if id == terminal_id).then_some(())).await;
}

#[tokio::test]
async fn terminal_input_and_resizes_reach_the_node_and_stopping_tells_it_to_detach() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let mut stream = server.user.stream().await;
    let terminal_id = start_terminal(&server, &mut link, conversation.id).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/terminals/{terminal_id}/input")).json(&json!({"data": "bHMNCg=="}))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::TerminalInput { terminal_id, data: "bHMNCg==".into() });

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/terminals/{terminal_id}/resize")).json(&json!({"cols": 120, "rows": 40}))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::ResizeTerminal { terminal_id, cols: 120, rows: 40 });

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/terminals/{terminal_id}"))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::StopTerminal { terminal_id });
    stream.until(|message| matches!(message, StreamMessage::TerminalExited { terminal_id: id } if id == terminal_id).then_some(())).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/terminals/{terminal_id}/input")).json(&json!({"data": "eA=="}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn another_user_cannot_open_or_drive_a_terminal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let terminal_id = start_terminal(&server, &mut link, conversation.id).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.post(&format!("/api/v1/conversations/{}/terminal", conversation.id)).json(&json!({"cols": 80, "rows": 24}))).await;
    assert_eq!(status, 404);

    let (status, _) = other.json(other.post(&format!("/api/v1/terminals/{terminal_id}/input")).json(&json!({"data": "eA=="}))).await;
    assert_eq!(status, 404);

    let (status, _) = other.json(other.delete(&format!("/api/v1/terminals/{terminal_id}"))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_node_cannot_stream_output_for_a_terminal_it_does_not_own() {
    let server = Server::start().await;
    let mut owner = server.online_node("laptop").await;
    let mut intruder = server.online_node("server").await;
    let conversation = server.create_conversation(owner.id, None).await;
    owner.expect_start_session().await;
    let mut stream = server.user.stream().await;
    let terminal_id = start_terminal(&server, &mut owner, conversation.id).await;

    intruder.send(NodeToControl::TerminalOutput { terminal_id, data: "Zm9yZ2Vk".into() }).await;
    intruder.send(NodeToControl::TerminalExited { terminal_id }).await;
    owner.send(NodeToControl::TerminalOutput { terminal_id, data: "cmVhbA==".into() }).await;

    let first = stream.until(|message| matches!(message, StreamMessage::TerminalOutput { .. } | StreamMessage::TerminalExited { .. }).then_some(message)).await;
    assert_eq!(first, StreamMessage::TerminalOutput { terminal_id, data: "cmVhbA==".into() });
}

#[tokio::test]
async fn a_terminal_ends_in_the_stream_when_its_node_disconnects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let mut stream = server.user.stream().await;
    let terminal_id = start_terminal(&server, &mut link, conversation.id).await;

    drop(link);

    stream.until(|message| matches!(message, StreamMessage::TerminalExited { terminal_id: id } if id == terminal_id).then_some(())).await;
}

#[tokio::test]
async fn a_terminal_cannot_be_opened_while_the_node_is_offline_or_with_no_size() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{}/terminal", conversation.id)).json(&json!({"cols": 0, "rows": 24}))).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("size_required")));

    drop(link);
    let path = format!("/api/v1/conversations/{}/terminal", conversation.id);
    let body = common::eventually("the node to show as offline", || async {
        let (status, body) = server.user.json(server.user.post(&path).json(&json!({"cols": 80, "rows": 24}))).await;
        (status == 409).then_some(body)
    })
    .await;
    assert_eq!(body["error"], "node_offline");
}

async fn conversation_where(stream: &mut StreamClient, id: Uuid, wanted: impl Fn(&Conversation) -> bool) -> Conversation {
    stream
        .until(|message| match message {
            StreamMessage::Conversation(conversation) if conversation.id == id && wanted(&conversation) => Some(conversation),
            _ => None,
        })
        .await
}

async fn event_of(stream: &mut StreamClient, id: Uuid, kind: EventKind) -> Event {
    stream
        .until(|message| match message {
            StreamMessage::Event(event) if event.conversation_id == id && event.kind == kind => Some(event),
            _ => None,
        })
        .await
}

async fn queue_where(stream: &mut StreamClient, id: Uuid, wanted: impl Fn(&[QueuedPrompt]) -> bool) -> Vec<QueuedPrompt> {
    stream
        .until(|message| match message {
            StreamMessage::Queue { conversation_id, queue } if conversation_id == id && wanted(&queue) => Some(queue),
            _ => None,
        })
        .await
}

async fn started_conversation(server: &Server, link: &mut NodeLink, prompt: Option<&str>) -> Uuid {
    let conversation = server.create_conversation(link.id, prompt).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    conversation.id
}

#[tokio::test]
async fn creating_a_conversation_streams_it_and_its_first_queued_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let conversation = server.create_conversation(link.id, Some("hello")).await;

    conversation_where(&mut stream, conversation.id, |conversation| conversation.status == ConversationStatus::Working).await;
    let queue = queue_where(&mut stream, conversation.id, |queue| queue.len() == 1).await;
    assert_eq!(queue[0].text, "hello");

    link.expect_start_session().await;
}

#[tokio::test]
async fn a_conversation_streams_every_step_of_a_turn() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;

    let started = conversation_where(&mut stream, id, |conversation| conversation.acp_session_id.is_some()).await;
    assert_eq!(started.acp_session_id.as_deref(), Some("acp-1"));

    let sent = event_of(&mut stream, id, EventKind::UserMessage).await;
    assert_eq!(sent.payload["text"], "run it");
    queue_where(&mut stream, id, |queue| queue.is_empty()).await;
    link.expect_prompt().await;

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 1, update: json!({"sessionUpdate": "agent_message_chunk"}) }).await;
    event_of(&mut stream, id, EventKind::AcpUpdate).await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started"}) }).await;
    event_of(&mut stream, id, EventKind::Workflow).await;

    link.send(NodeToControl::PermissionRequest { conversation_id: id, seq: 2, request_id: "req-1".into(), request: json!({"options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}) }).await;
    event_of(&mut stream, id, EventKind::PermissionRequest).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Input).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/permission")).json(&json!({"request_id": "req-1", "option_id": "allow"}))).await;
    assert_eq!(status, 204);
    event_of(&mut stream, id, EventKind::PermissionResolved).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into() }).await;
    event_of(&mut stream, id, EventKind::TurnEnd).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Review).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/opened"))).await;
    assert_eq!(status, 204);
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Done).await;
}

#[tokio::test]
async fn a_session_failure_is_streamed_as_an_error_and_a_failed_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;
    let mut stream = server.user.stream().await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 1, message: "crashed".into() }).await;

    let error = event_of(&mut stream, id, EventKind::Error).await;
    assert_eq!(error.payload["message"], "crashed");
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Failed).await;
}

#[tokio::test]
async fn a_conversation_left_running_by_a_node_that_lost_its_session_is_streamed_as_failed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;
    let mut stream = server.user.stream().await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![]).await;

    event_of(&mut stream, id, EventKind::Error).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Failed).await;
}

#[tokio::test]
async fn renaming_and_archiving_a_conversation_are_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let mut stream = server.user.stream().await;

    let url = format!("/api/v1/conversations/{id}");
    server.user.json(server.user.patch(&url).json(&json!({"title": "Release notes"}))).await;
    conversation_where(&mut stream, id, |conversation| conversation.title == "Release notes").await;

    server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;
    conversation_where(&mut stream, id, |conversation| conversation.archived_at.is_some()).await;

    server.user.json(server.user.patch(&url).json(&json!({"archived": false}))).await;
    conversation_where(&mut stream, id, |conversation| conversation.archived_at.is_none()).await;
}

#[tokio::test]
async fn changing_mode_and_model_is_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, None).await;
    conversation_where(&mut stream, id, |conversation| conversation.acp_session_id.is_some()).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "plan", "model": "opus"}))).await;
    assert_eq!(status, 204);

    let changed = conversation_where(&mut stream, id, |conversation| conversation.mode.is_some()).await;
    assert_eq!((changed.mode.as_deref(), changed.model.as_deref()), (Some("plan"), Some("opus")));
}

#[tokio::test]
async fn queueing_editing_reordering_and_removing_messages_are_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;
    let mut stream = server.user.stream().await;
    let queue_url = format!("/api/v1/conversations/{id}/queue");

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "second"}))).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "third"}))).await;
    let queued = queue_where(&mut stream, id, |queue| queue.len() == 2).await;
    let (second, third) = (queued[0].id, queued[1].id);

    server.user.json(server.user.patch(&format!("{queue_url}/{second}")).json(&json!({"text": "second, edited"}))).await;
    queue_where(&mut stream, id, |queue| queue[0].text == "second, edited").await;

    server.user.json(server.user.post(&format!("{queue_url}/{third}/send-now"))).await;
    let reordered = queue_where(&mut stream, id, |queue| queue.len() == 2 && queue[0].id == third).await;
    assert_eq!(reordered[1].id, second);

    server.user.json(server.user.delete(&format!("{queue_url}/{second}"))).await;
    queue_where(&mut stream, id, |queue| queue.len() == 1).await;
}

#[tokio::test]
async fn a_queued_message_sent_when_the_turn_ends_is_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, Some("first")).await;
    link.expect_prompt().await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "second"}))).await;
    let mut stream = server.user.stream().await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into() }).await;

    let sent = stream
        .until(|message| match message {
            StreamMessage::Event(event) if event.kind == EventKind::UserMessage && event.payload["text"] == "second" => Some(event),
            _ => None,
        })
        .await;
    assert_eq!(sent.conversation_id, id);
    queue_where(&mut stream, id, |queue| queue.is_empty()).await;
}

#[tokio::test]
async fn conversation_changes_reach_only_the_person_who_owns_it_while_nodes_reach_everyone() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let mut owner_stream = server.user.stream().await;
    let mut other_stream = other.stream().await;

    let id = started_conversation(&server, &mut link, Some("private")).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 1, update: json!({"sessionUpdate": "agent_message_chunk"}) }).await;
    event_of(&mut owner_stream, id, EventKind::AcpUpdate).await;

    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;

    let mut seen = Vec::new();

    loop {
        let message = other_stream.next().await;

        if matches!(&message, StreamMessage::Node(node) if node.draining) {
            break;
        }

        seen.push(message);
    }

    assert!(seen.iter().all(|message| matches!(message, StreamMessage::Node(_) | StreamMessage::Device { .. })), "{seen:?}");
}
