use crate::common;
use crate::common::{NodeLink, Server, StreamClient};
use mycelium_proto::conversation::{AgentId, Conversation, ConversationStatus, DeviceKind, DeviceState, Event, EventKind, LoginInfo, LoginState, QueuedPrompt};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;
use uuid::Uuid;

const PUBLISHING_ROUTES: &[&str] = &[
    "POST /api/v1/projects/{id}/find-icon",
    "POST /api/v1/projects/{id}/avatar",
    "PATCH /api/v1/projects/{id}",
    "POST /api/v1/projects",
    "POST /api/v1/workflows",
    "POST /api/v1/workflows/{id}/control",
    "POST /api/v1/workflows/{id}/rerun",
    "POST /api/v1/device",
    "POST /api/v1/device/token",
    "POST /api/v1/device/{user_code}/approve",
    "POST /api/v1/device/{user_code}/deny",
    "DELETE /api/v1/nodes/{id}",
    "POST /api/v1/nodes/{id}/drain",
    "POST /api/v1/nodes/{id}/instructions/take-over",
    "PATCH /api/v1/nodes/{id}",
    "POST /api/v1/nodes/{id}/update",
    "DELETE /api/v1/logins/{login_id}",
    "POST /api/v1/conversations",
    "POST /api/v1/chats",
    "POST /api/v1/chats/import",
    "POST /api/v1/conversations/{id}/side",
    "POST /api/v1/conversations/{id}/continue",
    "POST /api/v1/conversations/{id}/move",
    "DELETE /api/v1/conversations/{id}/move",
    "POST /api/v1/conversations/{id}/environment",
    "DELETE /api/v1/conversations/{id}/environment",
    "POST /api/v1/conversations/{id}/environment/take-over",
    "POST /api/v1/conversations/{id}/environment/hand-back",
    "POST /api/v1/conversations/{id}/environment/browser-task/stop",
    "POST /api/v1/conversations/{id}/environment/apps",
    "POST /api/v1/conversations/{id}/environment/windows/{window}/focus",
    "POST /api/v1/nodes/{id}/environment/check",
    "PUT /api/v1/conversations/{id}/goal",
    "PATCH /api/v1/conversations/{id}/goal",
    "DELETE /api/v1/conversations/{id}/goal",
    "POST /api/v1/conversations/{id}/goal/send",
    "POST /api/v1/conversations/{id}/goal/nudge",
    "POST /api/v1/conversations/{id}/goal/continue",
    "POST /api/v1/tasks/{id}/start",
    "POST /api/v1/schedules/{id}/run",
    "POST /api/v1/schedules/{id}/confirm",
    "POST /api/v1/schedules/{id}/decline",
    "POST /api/v1/instruction-proposals/{id}/approve",
    "POST /api/v1/instruction-proposals/{id}/decline",
    "POST /api/v1/conversation-connections",
    "DELETE /api/v1/conversation-connections/{id}",
    "POST /api/v1/group-connections",
    "DELETE /api/v1/group-connections/{id}",
    "POST /api/v1/message-proposals/{id}/connect",
    "POST /api/v1/message-proposals/{id}/connect-groups",
    "POST /api/v1/message-proposals/{id}/approve",
    "POST /api/v1/message-proposals/{id}/decline",
    "POST /api/v1/start-proposals/{id}/approve",
    "POST /api/v1/start-proposals/{id}/connect",
    "POST /api/v1/start-proposals/{id}/decline",
    "POST /api/v1/api-access-requests/{id}/approve",
    "POST /api/v1/api-access-requests/{id}/decline",
    "POST /api/v1/ssh-access-requests/{id}/approve",
    "POST /api/v1/ssh-access-requests/{id}/decline",
    "POST /api/v1/ssh-access-requests/{id}/revoke",
    "POST /api/v1/projects/{id}/issues/{number}/workspace",
    "PATCH /api/v1/conversations/{id}",
    "DELETE /api/v1/conversations/{id}",
    "POST /api/v1/conversations/{id}/close",
    "POST /api/v1/conversations/{id}/opened",
    "POST /api/v1/conversations/{id}/prompt",
    "POST /api/v1/conversations/{id}/retry",
    "POST /api/v1/conversations/{id}/dismiss",
    "POST /api/v1/conversations/{id}/resume",
    "DELETE /api/v1/conversations/{id}/resume",
    "PATCH /api/v1/conversations/{id}/queue/{queue_id}",
    "DELETE /api/v1/conversations/{id}/queue/{queue_id}",
    "POST /api/v1/conversations/{id}/queue/{queue_id}/send-now",
    "POST /api/v1/conversations/{id}/queue/send-all-now",
    "POST /api/v1/conversations/{id}/permission",
    "POST /api/v1/conversations/{id}/responses/{request_id}",
    "POST /api/v1/conversations/{id}/responses/{request_id}/filter",
    "POST /api/v1/conversations/{id}/config",
    "POST /api/v1/imports",
    "DELETE /api/v1/terminals/{terminal_id}",
    "POST /api/v1/mcp",
    "PATCH /api/v1/memories/{id}",
    "DELETE /api/v1/memories/{id}",
    "PUT /api/v1/memories/{id}/bank",
    "POST /api/v1/memory-banks",
    "PATCH /api/v1/memory-banks/{id}",
    "DELETE /api/v1/memory-banks/{id}",
    "PUT /api/v1/walkthroughs/{id}/dismissal",
    "DELETE /api/v1/walkthroughs/{id}/dismissal",
    "POST /api/v1/walkthroughs/{id}/notes",
    "DELETE /api/v1/walkthroughs/{id}/notes/{note_id}",
    "POST /api/v1/walkthroughs/{id}/notes/sent",
    "PUT /api/v1/walkthroughs/{id}/answers/{step}",
    "POST /api/v1/artifacts/{id}/links",
    "DELETE /api/v1/artifacts/{id}/links/{link_id}",
    "POST /api/v1/artifact-access-requests/{id}/approve",
    "POST /api/v1/artifact-access-requests/{id}/decline",
    "POST /api/v1/artifacts",
    "POST /api/v1/artifacts/{id}/versions",
    "POST /internal/artifacts/by-slug/{slug}/versions",
    "PATCH /api/v1/artifacts/{id}",
    "DELETE /api/v1/artifacts/{id}",
    "POST /api/v1/artifacts/{id}/comments",
    "PATCH /api/v1/artifacts/{id}/comments/{comment_id}",
];

const SILENT_ROUTES: &[&str] = &[
    "PUT /api/v1/agent-settings",
    "PUT /api/v1/agent-settings/account-mode",
    "PUT /api/v1/account-prices",
    "POST /auth/logout",
    "DELETE /api/v1/sessions/{id}",
    "POST /api/v1/nodes/{id}/logins",
    "POST /api/v1/nodes/{id}/watch",
    "POST /api/v1/nodes/{id}/processes/{pid}/stop",
    "POST /api/v1/logins/{login_id}/input",
    "POST /api/v1/conversations/{id}/cancel",
    "POST /api/v1/conversations/{id}/terminal",
    "POST /api/v1/conversations/{id}/attachments",
    "POST /api/v1/transcriptions",
    "POST /api/v1/speech",
    "PUT /api/v1/privacy",
    "POST /api/v1/conversations/{id}/git/hunk",
    "POST /api/v1/conversations/{id}/git/file",
    "POST /api/v1/conversations/{id}/git/stage",
    "POST /api/v1/conversations/{id}/git/unstage",
    "POST /api/v1/conversations/{id}/git/stage-all",
    "POST /api/v1/conversations/{id}/git/commit",
    "POST /api/v1/conversations/{id}/git/fetch",
    "POST /api/v1/conversations/{id}/git/pull",
    "POST /api/v1/conversations/{id}/git/push",
    "POST /api/v1/conversations/{id}/git/changes-rows",
    "POST /api/v1/conversations/{id}/git/town/sync",
    "POST /api/v1/conversations/{id}/git/town/propose",
    "POST /api/v1/conversations/{id}/git/undo",
    "POST /api/v1/conversations/{id}/git/switch",
    "POST /api/v1/terminals/{terminal_id}/input",
    "POST /api/v1/terminals/{terminal_id}/resize",
    "POST /api/v1/forges",
    "DELETE /api/v1/forges/{id}",
    "POST /api/v1/model-providers",
    "POST /api/v1/model-providers/{id}/refresh",
    "PATCH /api/v1/model-providers/{id}",
    "PUT /api/v1/model-providers/{id}/favourites",
    "DELETE /api/v1/model-providers/{id}",
    "POST /api/v1/chat-tool-servers",
    "POST /api/v1/chat-tool-servers/oauth",
    "PATCH /api/v1/chat-tool-servers/{id}",
    "POST /api/v1/chat-tool-servers/{id}/refresh",
    "PUT /api/v1/chat-tool-servers/{id}/groups",
    "POST /api/v1/chat-tool-servers/{id}/lastfm-sign-in",
    "DELETE /api/v1/chat-tool-servers/{id}/lastfm-sign-in",
    "POST /api/v1/chat-tool-servers/{id}/freeagent-sign-in",
    "DELETE /api/v1/chat-tool-servers/{id}/freeagent-sign-in",
    "DELETE /api/v1/chat-tool-servers/{id}",
    "POST /api/v1/tasks",
    "POST /api/v1/tasks/{id}/approve",
    "POST /api/v1/tasks/{id}/decline",
    "POST /api/v1/tasks/{id}/answer",
    "POST /api/v1/tasks/{id}/done",
    "DELETE /api/v1/tasks/{id}",
    "POST /api/v1/schedules",
    "PATCH /api/v1/schedules/{id}",
    "PUT /api/v1/me/time-zone",
    "DELETE /api/v1/schedules/{id}",
    "POST /api/v1/assistants",
    "PATCH /api/v1/assistants/{id}",
    "POST /api/v1/assistant-changes/{id}/approve",
    "POST /api/v1/assistant-changes/{id}/decline",
    "POST /api/v1/assistant-changes/{id}/undo",
    "POST /api/v1/assistants/{id}/actions",
    "PUT /api/v1/assistants/{id}/actions/{action_id}",
    "DELETE /api/v1/assistants/{id}/actions/{action_id}",
    "POST /api/v1/assistants/{id}/actions/{action_id}/run",
    "POST /api/v1/assistants/{id}/actions/{action_id}/reset",
    "POST /api/v1/assistants/{id}/action-proposals/{proposal_id}/approve",
    "POST /api/v1/assistants/{id}/action-proposals/{proposal_id}/decline",
    "POST /api/v1/assistants/{id}/prompt",
    "POST /api/v1/assistants/{id}/instructions/reset",
    "POST /api/v1/assistants/{id}/clear",
    "PUT /api/v1/assistants/{id}/files/{*path}",
    "DELETE /api/v1/assistants/{id}/files/{*path}",
    "PUT /api/v1/assistants/{id}/backup",
    "DELETE /api/v1/assistants/{id}/backup",
    "POST /api/v1/assistants/{id}/backup/pull",
    "PUT /api/v1/assistants/{id}/secrets/{name}",
    "DELETE /api/v1/assistants/{id}/secrets/{name}",
    "POST /api/v1/groups",
    "PATCH /api/v1/groups/{id}",
    "DELETE /api/v1/groups/{id}",
    "POST /api/v1/groups/{id}/folders",
    "PUT /api/v1/groups/{id}/instructions",
    "DELETE /api/v1/groups/{id}/folders/{folder_id}",
    "PUT /api/v1/instructions/{layer}",
    "PUT /api/v1/extensions",
    "PUT /api/v1/experiments",
    "PUT /api/v1/usage-limit",
    "PATCH /api/v1/reflections/{id}",
    "POST /api/v1/reflections/{id}/redraft",
    "PUT /api/v1/skills/hidden",
    "DELETE /api/v1/skills/hidden",
    "POST /api/v1/exercises/propose",
    "POST /api/v1/exercises/{id}/start",
    "POST /api/v1/exercises/{id}/not-now",
    "POST /api/v1/exercises/{id}/tests",
    "POST /api/v1/exercises/{id}/review",
    "POST /api/v1/exercises/{id}/done",
    "PUT /api/v1/wellness/settings",
    "POST /api/v1/wellness/heartbeat",
    "POST /api/v1/wellness/breaks",
    "DELETE /api/v1/wellness/breaks/current",
    "PUT /api/v1/wellness/breaks/{id}/note",
    "POST /api/v1/wellness/holds",
    "PUT /api/v1/wellness/puzzles/{puzzle}/today",
    "DELETE /api/v1/wellness/puzzles/{puzzle}/today",
    "POST /api/v1/wellness/puzzles/wordle/guesses",
    "POST /api/v1/wellness/puzzles/connections/guesses",
    "POST /api/v1/usage/resets",
    "POST /api/v1/instructions/pull",
    "PUT /api/v1/instruction-source",
    "DELETE /api/v1/instruction-source",
    "PUT /api/v1/walkthroughs/{id}/reads/{step}",
    "PUT /api/v1/artifacts/{id}/data/{key}",
    "POST /api/v1/typst/compile",
    "PUT /internal/artifacts/by-slug/{slug}/data/{key}",
    "POST /api/v1/chat-tool-servers/probe",
    "POST /api/v1/chat-tool-servers/test",
    "POST /api/v1/chat-tool-servers/caldav-test",
    "POST /api/v1/chat-tool-servers/coolify-test",
    "POST /api/v1/chat-tool-servers/tandoor-test",
    "PUT /api/v1/chat-tool-servers/{id}/coolify-target",
    "DELETE /api/v1/apps/{uuid}",
    "PUT /api/v1/apps/{uuid}/envs/{key}",
    "DELETE /api/v1/apps/{uuid}/envs/{key}",
    "POST /api/v1/apps/{uuid}/redeploy",
    "POST /api/v1/apps/{uuid}/start",
    "POST /api/v1/apps/{uuid}/stop",
    "POST /api/v1/decision-providers",
    "POST /api/v1/decision-providers/test",
    "PATCH /api/v1/decision-providers/{id}",
    "DELETE /api/v1/decision-providers/{id}",
    "PUT /api/v1/groups/{id}/hours",
    "POST /api/v1/groups/{id}/open",
    "DELETE /api/v1/groups/{id}/open",
    "POST /api/v1/nodes/{id}/ssh-tokens",
    "POST /api/v1/forges/{id}/mcp",
    "POST /api/v1/conversations/{id}/environment/stream-ticket",
];

fn routes_that_change_state() -> BTreeSet<String> {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let mut routes = BTreeSet::new();

    for line in source.lines().filter(|line| line.contains(".route(\"")) {
        let path = line.split('"').nth(1).unwrap();

        for (needle, method) in [("post(", "POST"), ("patch(", "PATCH"), ("delete(", "DELETE"), ("put(", "PUT")] {
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

#[tokio::test]
async fn a_new_chat_is_streamed_as_a_conversation_without_a_node() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let provider = sqlx::query_scalar::<_, Uuid>("insert into model_providers (owner, kind, name, base_url, models, default_model) values ($1, 'openai_compatible', 'Ollama', 'http://127.0.0.1:9/v1', '{llama3.2}', 'llama3.2') returning id")
        .bind(owner)
        .fetch_one(&server.pool)
        .await
        .unwrap();

    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider}))).await;
    let streamed = stream.until(|message| match message {
        StreamMessage::Conversation(conversation) if conversation.id.to_string() == chat["id"].as_str().unwrap_or_default() => Some(conversation),
        _ => None,
    })
    .await;

    assert_eq!(status, 201, "{chat}");
    assert_eq!((streamed.agent, streamed.provider_id, streamed.model), (mycelium_proto::conversation::ConversationAgent::Chat, Some(provider), Some("llama3.2".to_string())));
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

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { agent: AgentId::Claude, account: Some("me@example.com".into()), method: "token".into(), state: LoginState::SignedIn, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }] }).await;
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
async fn starting_a_task_streams_its_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let (_, task) = server.user.json(server.user.post("/api/v1/tasks").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "title": "Tidy"}))).await;

    let (_, started) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    let id: Uuid = started["id"].as_str().unwrap().parse().unwrap();

    let streamed = conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;
    assert_eq!(streamed.title, "Tidy");

    link.expect_start_session().await;
}

#[tokio::test]
async fn starting_a_side_chat_streams_its_conversation_with_its_parent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let mut stream = server.user.stream().await;

    let (_, side) = server.user.json(server.user.post(&format!("/api/v1/conversations/{}/side", parent.id)).json(&json!({"kind": "side"}))).await;
    let id: Uuid = side["id"].as_str().unwrap().parse().unwrap();

    let streamed = conversation_where(&mut stream, id, |conversation| conversation.parent_id == Some(parent.id)).await;
    assert_eq!(streamed.title, "Side chat");

    link.expect_start_session().await;
}

#[tokio::test]
async fn running_a_schedule_streams_its_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let (_, schedule) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "prompt": "Tidy", "cron": "0 6 * * 1", "each_run": "new"}))).await;

    let (_, run) = server.user.json(server.user.post(&format!("/api/v1/schedules/{}/run", schedule["id"].as_str().unwrap()))).await;
    let id: Uuid = run["conversation_id"].as_str().unwrap().parse().unwrap();

    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

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
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "result"}) }).await;
    event_of(&mut stream, id, EventKind::Workflow).await;

    link.send(NodeToControl::PermissionRequest { conversation_id: id, seq: 2, request_id: "req-1".into(), request: json!({"options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}) }).await;
    event_of(&mut stream, id, EventKind::PermissionRequest).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Input).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/permission")).json(&json!({"request_id": "req-1", "option_id": "allow"}))).await;
    assert_eq!(status, 204);
    event_of(&mut stream, id, EventKind::PermissionResolved).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
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
async fn deleting_an_archived_conversation_is_streamed_as_its_removal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let mut stream = server.user.stream().await;
    let url = format!("/api/v1/conversations/{id}");
    server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;

    server.user.json(server.user.delete(&url)).await;

    let removed = stream.until(|message| match message {
        StreamMessage::ConversationRemoved { conversation_id } => Some(conversation_id),
        _ => None,
    })
    .await;
    assert_eq!(removed, id);
}

async fn announce_title(link: &mut NodeLink, id: Uuid, seq: i64, title: &str) {
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update: json!({"sessionId": "acp-1", "update": {"sessionUpdate": "session_info_update", "title": title}}) }).await;
}

#[tokio::test]
async fn a_title_the_agent_predicts_is_streamed_for_an_unrenamed_conversation_and_not_for_a_renamed_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    let mut stream = server.user.stream().await;

    announce_title(&mut link, id, 1, "Fix the restore backoff").await;
    conversation_where(&mut stream, id, |conversation| conversation.title == "Fix the restore backoff" && !conversation.title_user_set).await;

    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"title": "Restore work"}))).await;
    conversation_where(&mut stream, id, |conversation| conversation.title == "Restore work" && conversation.title_user_set).await;
    announce_title(&mut link, id, 2, "Share the restore strings").await;
    let streamed = conversation_where(&mut stream, id, |conversation| conversation.title_user_set).await;
    assert_eq!(streamed.title, "Restore work");
}

#[tokio::test]
async fn retrying_a_failed_conversation_streams_it_as_no_longer_failed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "limit".into() }).await;
    let mut stream = server.user.stream().await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Failed).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Done).await;
}

#[tokio::test]
async fn dismissing_a_failed_conversation_settles_it_as_done_and_keeps_its_history() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "limit".into() }).await;
    let mut stream = server.user.stream().await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Failed).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/dismiss"))).await;

    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Done).await;
    let (status, _) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/events"))).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_turn_that_ends_with_workflow_agents_still_out_stays_working_until_the_last_one_reports() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "agentId": "a"}) }).await;
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "agentId": "b"}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;
    event_of(&mut stream, id, EventKind::TurnEnd).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "result", "agentId": "a"}) }).await;
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "failed", "agentId": "b"}) }).await;

    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Review).await;
}

#[tokio::test]
async fn stopping_a_turn_while_a_workflow_agent_is_out_leaves_the_conversation_stopped_and_the_next_message_is_sent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "agentId": "a"}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "cancelled".into(), usage: None }).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Review).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "carry on"}))).await;

    assert_eq!(link.expect_prompt().await.1, "carry on");
}

#[tokio::test]
async fn a_message_is_sent_after_a_turn_ends_even_while_a_workflow_agent_never_reports_back() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "agentId": "a"}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;
    event_of(&mut stream, id, EventKind::TurnEnd).await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "next"}))).await;

    assert_eq!(link.expect_prompt().await.1, "next");
}

#[tokio::test]
async fn closing_a_working_conversation_streams_it_as_done() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    let id = started_conversation(&server, &mut link, Some("run it")).await;
    link.expect_prompt().await;
    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Working).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/close"))).await;

    conversation_where(&mut stream, id, |conversation| conversation.status == ConversationStatus::Done).await;
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

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

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

#[tokio::test]
async fn a_prompt_to_an_idle_conversation_is_streamed_as_sent_without_first_showing_as_queued() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link, None).await;
    link.barrier(id, 1).await;
    let mut stream = server.user.stream().await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "hello"}))).await;

    let mut queued_first = false;
    stream
        .until(|message| match message {
            StreamMessage::Queue { conversation_id, queue } if conversation_id == id && !queue.is_empty() => {
                queued_first = true;
                None
            }
            StreamMessage::Event(event) if event.conversation_id == id && event.kind == EventKind::UserMessage => Some(()),
            _ => None,
        })
        .await;
    assert!(!queued_first);
}

#[tokio::test]
async fn waiting_to_move_and_cancelling_the_wait_are_streamed() {
    let server = Server::start().await;
    let mut source = server.online_node("laptop").await;
    let target = server.online_node("build").await;
    let conversation = server.create_conversation(source.id, Some("go")).await;
    source.expect_start_session().await;
    let mut stream = server.user.stream().await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{}/move", conversation.id)).json(&json!({"node_id": target.id}))).await;
    assert_eq!(status, 202, "{body}");
    let waiting = stream
        .until(|message| match message {
            StreamMessage::Move(progress) => Some(progress),
            _ => None,
        })
        .await;
    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{}/move", conversation.id))).await;
    assert_eq!(status, 204);
    let cancelled = stream
        .until(|message| match message {
            StreamMessage::Move(progress) => Some(progress),
            _ => None,
        })
        .await;

    assert_eq!((waiting.conversation_id, waiting.node_id, waiting.state), (conversation.id, target.id, mycelium_proto::api::MoveState::Waiting));
    assert_eq!(cancelled.state, mycelium_proto::api::MoveState::Cancelled);
}

#[tokio::test]
async fn setting_pausing_and_clearing_a_goal_is_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let mut stream = server.user.stream().await;
    let url = format!("/api/v1/conversations/{}/goal", conversation.id);

    server.user.json(server.user.put(&url).json(&json!({"text": "Tests pass", "token_budget": 100}))).await;
    let set = stream.until(|message| match message { StreamMessage::Goal(goal) => Some(goal), _ => None }).await;
    assert_eq!((set.text.as_str(), set.state), ("Tests pass", mycelium_proto::api::GoalState::Active));

    server.user.json(server.user.patch(&url).json(&json!({"paused": true}))).await;
    let paused = stream.until(|message| match message { StreamMessage::Goal(goal) if goal.state == mycelium_proto::api::GoalState::Paused => Some(goal), _ => None }).await;
    assert_eq!(paused.text, "Tests pass");

    server.user.json(server.user.delete(&url)).await;
    let cleared = stream.until(|message| match message { StreamMessage::GoalCleared { conversation_id } => Some(conversation_id), _ => None }).await;
    assert_eq!(cleared, conversation.id);
}

#[tokio::test]
async fn sending_a_failed_goal_again_is_streamed_as_pending() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let url = format!("/api/v1/conversations/{}/goal", conversation.id);
    server.user.json(server.user.put(&url).json(&json!({"text": "Tests pass", "token_budget": 100}))).await;
    sqlx::query("update goals set delivery_state = 'failed', delivery_reason = 'laptop did not answer' where conversation_id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    let mut stream = server.user.stream().await;

    let (status, body) = server.user.json(server.user.post(&format!("{url}/send"))).await;
    let resent = stream.until(|message| match message { StreamMessage::Goal(goal) => Some(goal), _ => None }).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!((resent.conversation_id, resent.delivery), (conversation.id, mycelium_proto::api::GoalDelivery::Pending));
}

#[tokio::test]
async fn continuing_a_blocked_goal_is_streamed_as_active() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let url = format!("/api/v1/conversations/{}/goal", conversation.id);
    server.user.json(server.user.put(&url).json(&json!({"text": "Tests pass"}))).await;
    sqlx::query("update goals set state = 'stuck', reason = 'The CI secret is missing.' where conversation_id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    let mut stream = server.user.stream().await;

    let (status, body) = server.user.json(server.user.post(&format!("{url}/continue")).json(&json!({"note": "It is in place."}))).await;
    let continued = stream.until(|message| match message { StreamMessage::Goal(goal) if goal.state == mycelium_proto::api::GoalState::Active => Some(goal), _ => None }).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(continued.conversation_id, conversation.id);
}

#[tokio::test]
async fn nudging_a_waiting_goal_now_is_streamed_with_the_wait_cleared() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation.id, 3).await;
    let url = format!("/api/v1/conversations/{}/goal", conversation.id);
    server.user.json(server.user.put(&url).json(&json!({"text": "Tests pass"}))).await;
    sqlx::query("update goals set delivery_state = 'sent', wait_reason = 'repeated_outcome', next_nudge_at = now() + interval '2 minutes', repeats = 2 where conversation_id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set status = 'review' where id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    let mut stream = server.user.stream().await;

    let (status, body) = server.user.json(server.user.post(&format!("{url}/nudge"))).await;
    let nudged = stream.until(|message| match message { StreamMessage::Goal(goal) if goal.nudges == 1 && goal.wait_reason.is_none() => Some(goal), _ => None }).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(nudged.conversation_id, conversation.id);
}

#[tokio::test]
async fn a_stored_walkthrough_is_streamed() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let mut stream = server.user.stream().await;
    let anonymous = server.anonymous();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": {"steps": [{"title": "One", "key": true, "files": [{"path": "a.rs", "diff": "+a"}]}]}}});

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", conversation.id.to_string()).json(&call)).await;

    let walkthrough = stream.until(|message| match message { StreamMessage::Walkthrough(walkthrough) => Some(walkthrough), _ => None }).await;
    assert_eq!((walkthrough.conversation_id, walkthrough.steps[0].title.as_str(), walkthrough.steps[0].key), (conversation.id, "One", true));
}

#[tokio::test]
async fn a_walkthrough_note_added_and_removed_on_one_client_reaches_another() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let mut writer = server.user.stream().await;
    let mut reader = server.user.stream().await;
    let anonymous = server.anonymous();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": {"steps": [{"title": "One", "key": true, "files": [{"path": "a.rs", "diff": "+a"}]}]}}});

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", conversation.id.to_string()).json(&call)).await;

    let walkthrough = writer.until(|message| match message { StreamMessage::Walkthrough(walkthrough) => Some(walkthrough), _ => None }).await;
    let url = format!("/api/v1/walkthroughs/{}/notes", walkthrough.id);
    let (_, added) = server.user.json(server.user.post(&url).json(&json!({"step": 0, "comment": "Check this"}))).await;
    let note_id = added["id"].as_str().unwrap().to_string();

    let notes = reader.until(|message| match message { StreamMessage::WalkthroughNotes { conversation_id, notes, .. } if conversation_id == conversation.id => Some(notes), _ => None }).await;
    assert_eq!(notes.iter().map(|note| note.comment.as_str()).collect::<Vec<_>>(), ["Check this"]);

    server.user.json(server.user.delete(&format!("{url}/{note_id}"))).await;

    let remaining = reader.until(|message| match message { StreamMessage::WalkthroughNotes { notes, .. } if notes.is_empty() => Some(notes), _ => None }).await;
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn proposing_confirming_and_declining_a_schedule_are_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;
    let mut stream = server.user.stream().await;
    propose(&server, &link, conversation.id, json!({"prompt": "Go", "preset": "daily at 09:00"})).await;

    let proposed = stream.until(|message| match message {
        StreamMessage::Schedule(schedule) => Some(schedule),
        _ => None,
    })
    .await;
    assert!(proposed.proposed);

    server.user.json(server.user.post(&format!("/api/v1/schedules/{}/confirm", proposed.id))).await;
    let confirmed = stream.until(|message| match message {
        StreamMessage::Schedule(schedule) if !schedule.proposed => Some(schedule),
        _ => None,
    })
    .await;
    assert_eq!(confirmed.id, proposed.id);

    propose(&server, &link, conversation.id, json!({"prompt": "Again", "preset": "daily at 10:00"})).await;
    let second = stream.until(|message| match message {
        StreamMessage::Schedule(schedule) if schedule.proposed => Some(schedule),
        _ => None,
    })
    .await;
    server.user.json(server.user.post(&format!("/api/v1/schedules/{}/decline", second.id))).await;

    let removed = stream.until(|message| match message {
        StreamMessage::ScheduleRemoved { schedule_id } => Some(schedule_id),
        _ => None,
    })
    .await;
    assert_eq!(removed, second.id);
}

async fn propose(server: &Server, link: &NodeLink, conversation: Uuid, arguments: Value) {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "propose_schedule", "arguments": arguments}}));
    anonymous.json(request).await;
}

#[tokio::test]
async fn a_remembered_memory_and_its_edit_and_deletion_reach_every_stream_client() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let mut stream = server.user.stream().await;
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {token}"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "remember", "arguments": {"text": "Staging listens on 5433"}}}));
    anonymous.json(request).await;

    let remembered = stream.until(|message| match message {
        StreamMessage::Memory(memory) => Some(memory),
        _ => None,
    })
    .await;
    assert_eq!((remembered.text.as_str(), remembered.scopes.as_slice()), ("Staging listens on 5433", ["global".to_string()].as_slice()));

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/memories/{}", remembered.id)).json(&json!({"text": "Staging listens on 5434"}))).await;
    let edited = stream.until(|message| match message {
        StreamMessage::Memory(memory) => Some(memory),
        _ => None,
    })
    .await;
    assert_eq!((status, edited.id, edited.text.as_str()), (200, remembered.id, "Staging listens on 5434"));

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/memories/{}", remembered.id))).await;
    let removed = stream.until(|message| match message {
        StreamMessage::MemoryRemoved { memory_id } => Some(memory_id),
        _ => None,
    })
    .await;
    assert_eq!((status, removed), (204, remembered.id));
}

#[tokio::test]
async fn memory_banks_and_moving_a_memory_into_one_reach_the_stream() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let mut stream = server.user.stream().await;
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {token}"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "remember", "arguments": {"text": "Staging listens on 5433"}}}));
    anonymous.json(request).await;
    let remembered = stream.until(|message| match message {
        StreamMessage::Memory(memory) => Some(memory),
        _ => None,
    })
    .await;

    let (status, bank) = server.user.json(server.user.post("/api/v1/memory-banks").json(&json!({"name": "Health", "model_class": "private"}))).await;
    let created = stream.until(|message| match message {
        StreamMessage::MemoryBank(bank) => Some(bank),
        _ => None,
    })
    .await;
    assert_eq!((status, created.name.as_str()), (201, "Health"));

    let (status, _) = server.user.json(server.user.put(&format!("/api/v1/memories/{}/bank", remembered.id)).json(&json!({"bank_id": bank["id"]}))).await;
    let moved = stream.until(|message| match message {
        StreamMessage::Memory(memory) => Some(memory),
        _ => None,
    })
    .await;
    assert_eq!((status, moved.id, moved.bank_id), (200, remembered.id, Some(created.id)));
}

#[tokio::test]
async fn a_version_saved_by_the_artifact_host_is_streamed_to_its_owner() {
    let server = Server::start_with(|config| config.artifact_host_token = Some("host-token".into())).await;
    let mut stream = server.user.stream().await;
    let user = &server.user;
    let (_, artifact) = user.json(user.post("/api/v1/artifacts").json(&json!({"kind": "file", "title": "Report", "content": "a", "media_type": "application/pdf"}))).await;
    sqlx::query("update users set subject = 'oidc:reader'").execute(&server.pool).await.unwrap();
    let anonymous = server.anonymous();
    let slug = artifact["slug"].as_str().unwrap();

    anonymous.json(anonymous.post(&format!("/internal/artifacts/by-slug/{slug}/versions")).query(&[("subject", "reader")]).header("authorization", "Bearer host-token").body("b")).await;
    let versioned = stream.until(|message| match message {
        StreamMessage::Artifact(artifact) if artifact.latest_version == 2 => Some(artifact),
        _ => None,
    })
    .await;

    assert_eq!(versioned.title, "Report");
}

#[tokio::test]
async fn an_artifact_and_its_comments_are_streamed_to_its_owner_as_they_change() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let user = &server.user;

    let (_, artifact) = user.json(user.post("/api/v1/artifacts").json(&json!({"kind": "page", "title": "Board", "content": "a"}))).await;
    let id = artifact["id"].as_str().unwrap().to_string();
    let created = stream.until(|message| match message {
        StreamMessage::Artifact(artifact) if artifact.id.to_string() == id => Some(artifact),
        _ => None,
    })
    .await;

    user.json(user.post(&format!("/api/v1/artifacts/{id}/versions")).json(&json!({"content": "b"}))).await;
    let versioned = stream.until(|message| match message {
        StreamMessage::Artifact(artifact) if artifact.latest_version == 2 => Some(artifact),
        _ => None,
    })
    .await;

    user.json(user.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"title": "Roadmap", "public": true}))).await;
    let renamed = stream.until(|message| match message {
        StreamMessage::Artifact(artifact) if artifact.title == "Roadmap" => Some(artifact),
        _ => None,
    })
    .await;

    let (_, comment) = user.json(user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "Looks good"}))).await;
    let commented = stream.until(|message| match message {
        StreamMessage::ArtifactComment(comment) => Some(comment),
        _ => None,
    })
    .await;

    user.delete(&format!("/api/v1/artifacts/{id}")).send().await.unwrap();
    let removed = stream.until(|message| match message {
        StreamMessage::Artifact(artifact) if artifact.deleted => Some(artifact),
        _ => None,
    })
    .await;

    assert_eq!((created.latest_version, created.deleted), (1, false));
    assert_eq!(versioned.versions.len(), 2);
    assert!(renamed.public);
    assert_eq!((commented.body.as_str(), commented.id.to_string()), ("Looks good", comment["id"].as_str().unwrap().to_string()));
    assert_eq!(removed.id.to_string(), id);
}

#[tokio::test]
async fn resetting_voice_instructions_and_opening_a_screen_reach_the_stream() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (_, voice) = server.user.json(server.user.get("/api/v1/assistants/builtin/voice")).await;
    let id = voice["id"].as_str().unwrap().to_string();
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/AGENTS.md")).header("content-type", "text/plain").body("Only talk about tea.")).await;
    let mut stream = server.user.stream().await;
    let (status, reset) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/instructions/reset"))).await;
    assert_eq!(status, 200, "{reset}");

    let published = stream.until(|message| match message {
        StreamMessage::Assistant(assistant) if assistant.builtin_key.as_deref() == Some("voice") => Some(assistant),
        _ => None,
    })
    .await;
    assert_eq!(published.id.to_string(), id);

    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {token}"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "open_screen", "arguments": {"route": "/settings/privacy"}}}));
    anonymous.json(request).await;

    let route = stream.until(|message| match message {
        StreamMessage::OpenScreen { route } => Some(route),
        _ => None,
    })
    .await;
    assert_eq!(route, "/settings/privacy");
}

#[tokio::test]
async fn an_environment_is_streamed_as_it_starts_changes_hands_and_stops() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    link.send(NodeToControl::EnvironmentCapability { capability: mycelium_proto::api::EnvironmentCapability { available: true, missing: vec![], versions: Default::default(), note: None, running: vec![], apps: vec![] } }).await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let url = format!("/api/v1/conversations/{}/environment", conversation.id);
    let node_with_environment = stream.until(|message| match message { StreamMessage::Node(node) if node.environment.is_some() => Some(node), _ => None }).await;

    let (status, started) = server.user.json(server.user.post(&url).json(&json!({}))).await;
    let starting = stream.until(|message| match message { StreamMessage::Environment(environment) => Some(environment), _ => None }).await;
    link.send(NodeToControl::EnvironmentChanged { conversation_id: conversation.id, state: mycelium_proto::api::EnvironmentState::Running, failure: None, browser: None }).await;
    stream.until(|message| match message { StreamMessage::Environment(environment) if environment.state == mycelium_proto::api::EnvironmentState::Running => Some(()), _ => None }).await;
    server.user.json(server.user.post(&format!("{url}/take-over"))).await;
    let taken = stream.until(|message| match message { StreamMessage::Environment(environment) if environment.controller == mycelium_proto::api::EnvironmentController::Person => Some(environment), _ => None }).await;
    link.send(NodeToControl::EnvironmentWindows { conversation_id: conversation.id, windows: vec![mycelium_proto::api::EnvironmentWindow { id: 4, app_id: "chromium".into(), title: "Inbox".into(), focused: true }] }).await;
    let windowed = stream.until(|message| match message { StreamMessage::Environment(environment) if !environment.windows.is_empty() => Some(environment), _ => None }).await;
    let focus = server.user.post(&format!("{url}/windows/4/focus"));
    let (_, (_, focused)) = tokio::join!(
        async {
            let request_id = loop {
                if let ControlToNode::EnvironmentFocus { request_id, .. } = link.next().await {
                    break request_id;
                }
            };
            link.send(NodeToControl::EnvironmentInputDone { request_id, error: None }).await;
        },
        server.user.json(focus)
    );
    assert_eq!(focused["windows"][0]["id"], 4);
    server.user.json(server.user.post(&format!("{url}/hand-back"))).await;
    let handed = stream.until(|message| match message { StreamMessage::Environment(environment) if environment.controller == mycelium_proto::api::EnvironmentController::Agent && environment.state == mycelium_proto::api::EnvironmentState::Running => Some(environment), _ => None }).await;
    server.user.json(server.user.delete(&url)).await;
    let stopped = stream.until(|message| match message { StreamMessage::Environment(environment) if environment.state == mycelium_proto::api::EnvironmentState::Stopped => Some(environment), _ => None }).await;

    assert_eq!(status, 200, "{started}");
    assert_eq!(node_with_environment.id, link.id);
    assert_eq!(starting.state, mycelium_proto::api::EnvironmentState::Starting);
    assert_eq!((taken.conversation_id, handed.conversation_id), (conversation.id, conversation.id));
    assert_eq!(windowed.windows[0].title, "Inbox");
    assert!(stopped.stopped_at.is_some());
}

#[tokio::test]
async fn re_running_a_workflow_streams_the_new_workflow() {
    let server = Server::start().await;
    let (_, created) = server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title": "Review", "steps": [{"key": "a", "title": "A", "gate": true}]}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    server.user.json(server.user.post(&format!("/api/v1/workflows/{id}/control")).json(&json!({"action": "stop"}))).await;
    let mut stream = server.user.stream().await;

    let (status, rerun) = server.user.json(server.user.post(&format!("/api/v1/workflows/{id}/rerun"))).await;
    let streamed = stream.until(|message| match message {
        StreamMessage::Workflow { workflow } if workflow.rerun_of.map(|origin| origin.to_string()) == Some(id.clone()) => Some(workflow),
        _ => None,
    })
    .await;

    assert_eq!((status, streamed.id.to_string()), (200, rerun["id"].as_str().unwrap().to_string()), "{rerun}");
}
