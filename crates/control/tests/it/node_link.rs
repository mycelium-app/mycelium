use crate::common;
use crate::common::{NodeLink, Server};
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn the_link_refuses_a_missing_or_wrong_node_credential() {
    let server = Server::start().await;

    assert!(server.try_connect_node("myc_node_not-a-real-token").await.is_err());
    assert!(server.try_connect_node("not-even-prefixed").await.is_err());
}

#[tokio::test]
async fn hello_records_the_agents_and_the_node_is_online_only_while_linked() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let node = &nodes.as_array().unwrap()[0];

    assert_eq!(node["online"], true);
    assert_eq!(node["version"], "0.1.0");
    assert_eq!(node["agents"][0]["agent"], "claude");
    assert_eq!(node["agents"][0]["available"], true);

    drop(link);

    let user = server.user.clone();
    common::eventually("the node to go offline", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;
}

#[tokio::test]
async fn a_node_going_online_and_offline_is_streamed_to_clients() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let link = server.online_node("laptop").await;

    let online = stream.until(|message| match message {
        StreamMessage::Node(node) if node.online && !node.agents.is_empty() => Some(node),
        _ => None,
    })
    .await;
    assert_eq!(online.name, "laptop");

    drop(link);

    let offline = stream.until(|message| match message {
        StreamMessage::Node(node) if !node.online => Some(node),
        _ => None,
    })
    .await;
    assert_eq!(offline.id, online.id);
}

#[tokio::test]
async fn acp_updates_are_stored_acknowledged_and_deduplicated_by_seq_across_reconnects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 4, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"text": "hi"}}) }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 4));

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 4, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"text": "hi"}}) }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 4));

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 6, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"text": "there"}}) }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 6));

    let events = server.events(conversation.id).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "acp_update");
    assert_eq!(events[0]["payload"]["content"]["text"], "hi");
    assert_eq!(events[1]["payload"]["content"]["text"], "there");

    let (id, token) = (link.id, link.token.clone());
    drop(link);

    let mut reconnected = server.connect_node(id, &token).await;
    reconnected.hello(vec![(conversation.id, 3)]).await;
    assert_eq!(reconnected.expect_ack().await, (conversation.id, 6));

    reconnected.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 5, update: json!({"sessionUpdate": "replayed"}) }).await;
    reconnected.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 6, update: json!({"sessionUpdate": "replayed again"}) }).await;
    reconnected.barrier(conversation.id, 7).await;

    let events = server.events(conversation.id).await;
    let seqs: Vec<i64> = events.iter().map(|event| event["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4], "{events:?}");
    assert_eq!(events[2]["payload"]["sessionUpdate"], "replayed");
}

fn chunk(kind: &str, text: &str) -> serde_json::Value {
    json!({"sessionId": "acp-1", "update": {"sessionUpdate": kind, "content": {"type": "text", "text": text}}})
}

#[tokio::test]
async fn a_burst_of_message_chunks_is_stored_as_one_event_and_acknowledged_in_full() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    for (seq, text) in [(1, "Hel"), (2, "lo, "), (3, "world")] {
        link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq, update: chunk("agent_message_chunk", text) }).await;
    }

    assert_eq!(link.expect_ack().await, (conversation.id, 3));

    let events = server.events(conversation.id).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["payload"]["update"]["content"]["text"], "Hello, world");
}

#[tokio::test]
async fn thought_chunks_are_not_merged_into_message_chunks() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 1, update: chunk("agent_thought_chunk", "think ") }).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 2, update: chunk("agent_thought_chunk", "more") }).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 3, update: chunk("agent_message_chunk", "say") }).await;

    assert_eq!(link.expect_ack().await, (conversation.id, 3));

    let events = server.events(conversation.id).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["payload"]["update"]["content"]["text"], "think more");
    assert_eq!(events[1]["payload"]["update"]["content"]["text"], "say");
}

#[tokio::test]
async fn an_identical_repeated_tool_call_update_is_stored_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    let progress = |status: &str| json!({"sessionId": "acp-1", "update": {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": status, "content": [{"type": "text", "text": "running"}]}});

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 1, update: progress("in_progress") }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 1));

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 2, update: progress("in_progress") }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 2));

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 3, update: progress("completed") }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 3));

    let events = server.events(conversation.id).await;
    let statuses: Vec<_> = events.iter().map(|event| event["payload"]["update"]["status"].clone()).collect();
    assert_eq!(statuses, vec![json!("in_progress"), json!("completed")]);
}

#[tokio::test]
async fn a_turn_end_after_a_burst_is_stored_after_the_merged_chunks() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 1, update: chunk("agent_message_chunk", "one ") }).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 2, update: chunk("agent_message_chunk", "two") }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;

    assert_eq!(link.expect_ack().await, (conversation.id, 2));
    assert_eq!(link.expect_ack().await, (conversation.id, 3));

    let events = server.events(conversation.id).await;
    let kinds: Vec<_> = events.iter().map(|event| event["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, vec!["acp_update", "turn_end"]);
    assert_eq!(events[0]["payload"]["update"]["content"]["text"], "one two");
}

#[tokio::test]
async fn a_nul_in_a_node_update_is_stored_as_a_replacement_character() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 1, update: json!({"sessionUpdate": "tool_call_update", "content": [{"text": "a\u{0}b"}], "raw\u{0}Key": "x"}) }).await;
    assert_eq!(link.expect_ack().await, (conversation.id, 1));

    let events = server.events(conversation.id).await;
    assert_eq!(events[0]["payload"]["content"][0]["text"], "a\u{FFFD}b");
    assert_eq!(events[0]["payload"]["raw\u{FFFD}Key"], "x");
}

#[tokio::test]
async fn a_node_cannot_write_into_another_nodes_conversation() {
    let server = Server::start().await;
    let mut owner = server.online_node("laptop").await;
    let mut intruder = server.online_node("server").await;
    let conversation = server.create_conversation(owner.id, None).await;
    owner.expect_start_session().await;

    intruder.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq: 1, update: json!({"sessionUpdate": "forged"}) }).await;
    intruder.send(NodeToControl::SessionStarted { conversation_id: conversation.id, acp_session_id: "forged".into(), modes: None, models: None, config_options: None }).await;

    let mut stream = server.user.stream().await;
    intruder.send(NodeToControl::Logins { logins: vec![LoginInfo { agent: AgentId::Claude, account: None, method: "token".into(), state: LoginState::SignedOut, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }] }).await;
    stream.until(|message| matches!(message, StreamMessage::Node(node) if node.id == intruder.id && !node.logins.is_empty()).then_some(())).await;

    assert!(server.events(conversation.id).await.is_empty());
    assert_eq!(server.conversation(conversation.id).await["acp_session_id"], json!(null));
}

#[tokio::test]
async fn revoking_a_node_tells_it_to_stop_and_voids_its_credential() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/nodes/{}", link.id))).await;
    assert_eq!(status, 204);

    assert_eq!(link.next().await, ControlToNode::Revoked);
    assert!(link.closed().await);
    assert!(server.try_connect_node(&link.token).await.is_err());

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert!(nodes.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_second_connection_from_the_same_node_replaces_the_first() {
    let server = Server::start().await;
    let mut first = server.online_node("laptop").await;
    let mut second = server.connect_node(first.id, &first.token.clone()).await;
    second.hello(vec![]).await;

    assert!(first.closed().await);

    let user = server.user.clone();
    common::eventually("the second connection to be online", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == true).then_some(())
    })
    .await;

    let conversation = server.create_conversation(second.id, None).await;
    assert_eq!(second.expect_start_session().await, conversation.id);
}

#[tokio::test]
async fn a_draining_node_starts_no_new_conversations_and_an_offline_node_cannot_either() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;
    assert_eq!(status, 204);

    let request = json!({"node_id": link.id, "agent": "claude", "cwd": "/work"});
    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&request)).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("node_draining")));

    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": false}))).await;
    let id = link.id;
    drop(link);

    let user = server.user.clone();
    common::eventually("the node to go offline", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": id, "agent": "claude", "cwd": "/work"}))).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("node_offline")));
}

#[tokio::test]
async fn an_agent_the_node_reported_as_missing_cannot_be_started() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "laptop").await;
    let mut link = server.connect_node(id, &token).await;
    link.send(NodeToControl::Hello {
        version: "0.1.0".into(),
        hostname: "laptop".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        agents: vec![mycelium_proto::conversation::AgentInfo { agent: AgentId::Codex, version: None, available: false, update_available: None }],
        sessions: vec![],
        kubernetes: None,
    })
    .await;

    let user = server.user.clone();
    common::eventually("the hello", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (!nodes[0]["agents"].as_array().unwrap().is_empty()).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": id, "agent": "codex", "cwd": "/work"}))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("agent_unavailable")));
}

fn cluster(conversation_id: Uuid, state: mycelium_proto::api::PodState) -> mycelium_proto::api::KubernetesNode {
    mycelium_proto::api::KubernetesNode {
        namespace: "mycelium".into(),
        image: "ghcr.io/mycelium-app/mycelium-agent:latest".into(),
        credentials_secret: "mycelium-agent-credentials".into(),
        workspace_persistent: false,
        pods: vec![mycelium_proto::api::SessionPod { conversation_id, name: "mycelium-session-7f3c2a91".into(), state }],
    }
}

#[tokio::test]
async fn a_node_reporting_kubernetes_facts_shows_them_in_the_node_api() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "lab-cluster").await;
    let conversation_id = Uuid::from_u128(7);
    let mut link = server.connect_node(id, &token).await;
    link.send(NodeToControl::Hello { version: "0.1.0".into(), hostname: "lab-cluster".into(), os: "linux".into(), arch: "x86_64".into(), agents: vec![], sessions: vec![], kubernetes: Some(cluster(conversation_id, mycelium_proto::api::PodState::Running)) }).await;

    let user = server.user.clone();
    let kubernetes = common::eventually("the kubernetes facts", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes[0]["kubernetes"].is_object().then(|| nodes[0]["kubernetes"].clone())
    })
    .await;

    assert_eq!(kubernetes["namespace"], "mycelium");
    assert_eq!(kubernetes["credentials_secret"], "mycelium-agent-credentials");
    assert_eq!(kubernetes["workspace_persistent"], false);
    assert_eq!(kubernetes["pods"], json!([{"conversation_id": conversation_id, "name": "mycelium-session-7f3c2a91", "state": "running"}]));
}

#[tokio::test]
async fn a_session_pod_changing_state_updates_the_node_api_without_a_reconnect() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "lab-cluster").await;
    let conversation_id = Uuid::from_u128(7);
    let mut link = server.connect_node(id, &token).await;
    link.send(NodeToControl::Hello { version: "0.1.0".into(), hostname: "lab-cluster".into(), os: "linux".into(), arch: "x86_64".into(), agents: vec![], sessions: vec![], kubernetes: Some(cluster(conversation_id, mycelium_proto::api::PodState::Running)) }).await;

    link.send(NodeToControl::NodeStatus { kubernetes: Some(cluster(conversation_id, mycelium_proto::api::PodState::Exited)) }).await;

    let user = server.user.clone();
    let state = common::eventually("the pod state change", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["kubernetes"]["pods"][0]["state"] == "exited").then(|| nodes[0]["kubernetes"]["pods"][0]["state"].clone())
    })
    .await;

    assert_eq!(state, "exited");
}

#[tokio::test]
async fn a_node_that_does_not_report_kubernetes_has_no_kubernetes_facts() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

    assert!(nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(link.id)));
    assert!(nodes[0].get("kubernetes").is_none());
}

#[tokio::test]
async fn an_agent_sign_in_is_started_on_the_node_and_its_output_streams_to_clients() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "token"}))).await;
    assert_eq!(status, 200);
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();

    assert_eq!(link.next().await, ControlToNode::StartLogin { login_id, agent: AgentId::Claude, method: "token".into(), account: None });

    link.send(NodeToControl::LoginOutput { login_id, data: "Open https://example.com/login".into() }).await;
    let data = stream.until(|message| match message {
        StreamMessage::LoginOutput { data, .. } => Some(data),
        _ => None,
    })
    .await;
    assert_eq!(data, "Open https://example.com/login");

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/logins/{login_id}/input")).json(&json!({"text": "abc123\n"}))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::LoginInput { login_id, text: "abc123\n".into() });

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { agent: AgentId::Claude, account: Some("me@example.com".into()), method: "token".into(), state: LoginState::SignedIn, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }] }).await;
    link.send(NodeToControl::LoginFinished { login_id, ok: true }).await;
    stream.until(|message| matches!(message, StreamMessage::LoginFinished { ok: true, .. }).then_some(())).await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["logins"][0]["state"], "signed_in");
    assert_eq!(nodes[0]["logins"][0]["account"], "me@example.com");

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/logins/{login_id}/input")).json(&json!({"text": "late"}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn stopping_a_sign_in_tells_the_node() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (_, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "codex", "method": "device"}))).await;
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();
    link.next().await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/logins/{login_id}"))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::StopLogin { login_id });
}

#[tokio::test]
async fn every_workflow_line_becomes_an_event_with_the_line_as_a_json_object() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    for step in 1..=3 {
        link.send(NodeToControl::Workflow { conversation_id: conversation.id, run_id: "run-1".into(), line: json!({"type": "started", "key": format!("agent-{step}")}) }).await;
    }
    link.barrier(conversation.id, 1).await;

    let events = server.events(conversation.id).await;
    let workflow: Vec<_> = events.iter().filter(|event| event["kind"] == "workflow").collect();
    assert_eq!(workflow.len(), 3);
    assert_eq!(workflow[2]["payload"]["run_id"], "run-1");
    assert_eq!(workflow[2]["payload"]["line"], json!({"type": "started", "key": "agent-3"}));
    assert!(workflow[2]["payload"]["at"].as_str().is_some_and(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok()));
}

#[tokio::test]
async fn an_ended_workflow_line_keeps_its_state_and_reason_in_the_event() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let ended = json!({"type": "ended", "state": "stopped", "ended_at": "2026-10-27T00:42:10.000Z", "reason": "Workflow was stopped", "last_line_at": null});

    link.send(NodeToControl::Workflow { conversation_id: conversation.id, run_id: "run-1".into(), line: ended.clone() }).await;
    link.barrier(conversation.id, 1).await;

    let events = server.events(conversation.id).await;
    let workflow: Vec<_> = events.iter().filter(|event| event["kind"] == "workflow").collect();
    assert_eq!(workflow.len(), 1);
    assert_eq!(workflow[0]["payload"]["line"], ended);
}

#[tokio::test]
async fn transcript_chunks_are_copied_under_the_node_and_acknowledged() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let path = "projects/-work-app/session.jsonl";

    link.send(NodeToControl::Transcript { path: path.into(), offset: 0, data: base64_of(b"{\"a\":1}\n"), size: None }).await;
    assert_eq!(link.next().await, ControlToNode::TranscriptAck { path: path.into(), bytes: 8 });

    link.send(NodeToControl::Transcript { path: path.into(), offset: 8, data: base64_of(b"{\"b\":2}\n"), size: None }).await;
    assert_eq!(link.next().await, ControlToNode::TranscriptAck { path: path.into(), bytes: 16 });

    let stored = std::fs::read(server.data_dir.join("transcripts").join(link.id.to_string()).join(path)).unwrap();
    assert_eq!(stored, b"{\"a\":1}\n{\"b\":2}\n");

    let recorded = sqlx::query_scalar::<_, i64>("select bytes from transcripts where node_id = $1 and path = $2").bind(link.id).bind(path).fetch_one(&server.pool).await.unwrap();
    assert_eq!(recorded, 16);
}

#[tokio::test]
async fn a_transcript_chunk_past_the_end_is_answered_with_what_the_control_plane_holds() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    link.send(NodeToControl::Transcript { path: "a.jsonl".into(), offset: 0, data: base64_of(b"one\n"), size: None }).await;
    link.next().await;

    link.send(NodeToControl::Transcript { path: "a.jsonl".into(), offset: 100, data: base64_of(b"gap\n"), size: None }).await;
    assert_eq!(link.next().await, ControlToNode::TranscriptAck { path: "a.jsonl".into(), bytes: 4 });

    link.send(NodeToControl::Transcript { path: "a.jsonl".into(), offset: 0, data: base64_of(b"rewritten\n"), size: None }).await;
    assert_eq!(link.next().await, ControlToNode::TranscriptAck { path: "a.jsonl".into(), bytes: 10 });

    let stored = std::fs::read(server.data_dir.join("transcripts").join(link.id.to_string()).join("a.jsonl")).unwrap();
    assert_eq!(stored, b"rewritten\n");
}

#[tokio::test]
async fn transcript_paths_that_escape_the_nodes_folder_are_rejected() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let escape = server.data_dir.join("transcripts").join("escape.txt");

    for path in ["../escape.txt", "../../escape.txt", "/tmp/absolute-escape.txt", "a/../../escape.txt"] {
        link.send(NodeToControl::Transcript { path: path.into(), offset: 0, data: base64_of(b"owned"), size: None }).await;
    }

    link.send(NodeToControl::Transcript { path: "ok.jsonl".into(), offset: 0, data: base64_of(b"fine"), size: None }).await;

    assert_eq!(link.next().await, ControlToNode::TranscriptAck { path: "ok.jsonl".into(), bytes: 4 });
    assert!(!escape.exists());
    assert!(!std::path::Path::new("/tmp/absolute-escape.txt").exists());

    let rows = sqlx::query_scalar::<_, i64>("select count(*) from transcripts").fetch_one(&server.pool).await.unwrap();
    assert_eq!(rows, 1);
}

fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine;

    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tokio::test]
async fn sign_in_output_reaches_only_the_person_who_started_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let mut owner_stream = server.user.stream().await;
    let mut other_stream = other.stream().await;

    let (_, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "token"}))).await;
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();
    link.next().await;

    link.send(NodeToControl::LoginOutput { login_id, data: "code ABCD-1234".into() }).await;
    link.send(NodeToControl::LoginFinished { login_id, ok: true }).await;

    owner_stream.until(|message| matches!(message, StreamMessage::LoginFinished { ok: true, .. }).then_some(())).await;
    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;
    other_stream
        .until(|message| match message {
            StreamMessage::LoginOutput { .. } | StreamMessage::LoginFinished { .. } => panic!("another person received a sign-in message: {message:?}"),
            StreamMessage::Node(_) => Some(()),
            _ => None,
        })
        .await;
}

#[tokio::test]
async fn another_person_cannot_write_to_or_stop_a_sign_in() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;

    let (_, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "token"}))).await;
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();
    link.next().await;

    let (status, _) = other.json(other.post(&format!("/api/v1/logins/{login_id}/input")).json(&json!({"text": "secret"}))).await;
    assert_eq!(status, 404);
    let (status, _) = other.json(other.delete(&format!("/api/v1/logins/{login_id}"))).await;
    assert_eq!(status, 404);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/logins/{login_id}/input")).json(&json!({"text": "mine"}))).await;
    assert_eq!(status, 204);
    assert_eq!(link.next().await, ControlToNode::LoginInput { login_id, text: "mine".into() });
}

#[tokio::test]
async fn a_node_cannot_publish_output_for_another_nodes_sign_in() {
    let server = Server::start().await;
    let first = server.online_node("laptop").await;
    let mut intruder = server.online_node("desktop").await;
    let mut stream = server.user.stream().await;

    let (_, started) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", first.id)).json(&json!({"agent": "claude", "method": "token"}))).await;
    let login_id: Uuid = started["login_id"].as_str().unwrap().parse().unwrap();

    intruder.send(NodeToControl::LoginOutput { login_id, data: "spoofed".into() }).await;
    intruder.send(NodeToControl::LoginFinished { login_id, ok: true }).await;
    intruder.send(NodeToControl::Logins { logins: vec![] }).await;

    stream
        .until(|message| match message {
            StreamMessage::LoginOutput { .. } | StreamMessage::LoginFinished { .. } => panic!("the stream carried a spoofed sign-in message: {message:?}"),
            StreamMessage::Node(node) if node.id == intruder.id => Some(()),
            _ => None,
        })
        .await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/logins/{login_id}/input")).json(&json!({"text": "still there"}))).await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn revoking_a_node_tells_clients_to_drop_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/nodes/{}", link.id))).await;
    assert_eq!(status, 204);

    let removed = stream.until(|message| match message {
        StreamMessage::NodeRemoved { node_id } => Some(node_id),
        _ => None,
    })
    .await;
    assert_eq!(removed, link.id);
}

async fn conversation_awaiting_prompt_acknowledgement(server: &Server) -> (NodeLink, Uuid, Uuid) {
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("hello")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let (prompt_id, _) = link.expect_prompt().await;

    (link, conversation.id, prompt_id)
}

async fn reconnect_reporting_live(server: &Server, link: NodeLink, conversation_id: Uuid) -> NodeLink {
    let (id, token) = (link.id, link.token.clone());
    drop(link);

    let mut returned = server.connect_node(id, &token).await;
    returned.hello(vec![(conversation_id, 0)]).await;

    returned
}

#[tokio::test]
async fn a_prompt_the_node_never_acknowledged_is_sent_again_when_it_reports_the_session_live() {
    let server = Server::start().await;
    let (link, conversation_id, prompt_id) = conversation_awaiting_prompt_acknowledgement(&server).await;

    let mut returned = reconnect_reporting_live(&server, link, conversation_id).await;

    match returned.next().await {
        ControlToNode::Prompt { conversation_id: resent_conversation, prompt_id: resent, text, .. } => {
            assert_eq!((resent_conversation, resent, text.as_str()), (conversation_id, prompt_id, "hello"));
        }
        other => panic!("expected the prompt again, got {other:?}"),
    }
}

#[tokio::test]
async fn an_acknowledged_prompt_is_not_sent_again() {
    let server = Server::start().await;
    let (mut link, conversation_id, prompt_id) = conversation_awaiting_prompt_acknowledgement(&server).await;
    link.send(NodeToControl::PromptReceived { conversation_id, prompt_id }).await;
    link.barrier(conversation_id, 1).await;

    let mut returned = reconnect_reporting_live(&server, link, conversation_id).await;
    returned.send(NodeToControl::AcpUpdate { conversation_id, seq: 2, update: json!({"sessionUpdate": "barrier"}) }).await;

    loop {
        match returned.raw().await {
            ControlToNode::Prompt { .. } => panic!("an acknowledged prompt was sent again"),
            ControlToNode::Ack { seq, .. } if seq >= 2 => break,
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_prompt_acknowledged_by_another_node_does_not_count() {
    let server = Server::start().await;
    let (link, conversation_id, prompt_id) = conversation_awaiting_prompt_acknowledgement(&server).await;
    let mut intruder = server.online_node("server").await;
    let own = server.create_conversation(intruder.id, None).await;
    intruder.expect_start_session().await;
    intruder.start_session(own.id, "acp-2").await;
    intruder.send(NodeToControl::PromptReceived { conversation_id, prompt_id }).await;
    intruder.barrier(own.id, 1).await;

    let mut returned = reconnect_reporting_live(&server, link, conversation_id).await;

    assert!(matches!(returned.next().await, ControlToNode::Prompt { .. }));
}

#[tokio::test]
async fn a_second_connection_with_the_same_credential_closes_the_first_and_says_why() {
    let server = Server::start().await;
    let mut first = server.online_node("laptop").await;
    let mut second = server.connect_node(first.id, &first.token.clone()).await;
    second.hello(vec![]).await;

    assert_eq!(first.close_code().await, Some(mycelium_proto::link::REPLACED_CLOSE_CODE));
}

#[tokio::test]
async fn a_reconnect_acknowledges_each_conversation_at_its_own_position() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let first = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(first.id, "acp-1").await;
    let second = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(second.id, "acp-2").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: first.id, seq: 4, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"text": "a"}}) }).await;
    assert_eq!(link.expect_ack().await, (first.id, 4));
    link.send(NodeToControl::AcpUpdate { conversation_id: second.id, seq: 9, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"text": "b"}}) }).await;
    assert_eq!(link.expect_ack().await, (second.id, 9));

    let (id, token) = (link.id, link.token.clone());
    drop(link);

    let mut reconnected = server.connect_node(id, &token).await;
    reconnected.hello(vec![]).await;
    let mut acks = vec![reconnected.expect_ack().await, reconnected.expect_ack().await];
    acks.sort();

    let mut expected = vec![(first.id, 4), (second.id, 9)];
    expected.sort();
    assert_eq!(acks, expected);
}
