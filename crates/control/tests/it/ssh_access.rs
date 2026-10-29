use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crate::common::{NodeLink, Server, WAIT, eventually};
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::api::{SshAccessRequest, SshAccessState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

type Agent = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Fixture {
    server: Server,
    agent_node: NodeLink,
    target: NodeLink,
    conversation: Uuid,
}

async fn tool(server: &Server, link: &NodeLink, conversation: Uuid, name: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn queued_texts(server: &Server, conversation: Uuid) -> Vec<String> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;

    queue.as_array().unwrap().iter().map(|item| item["text"].as_str().unwrap().to_string()).collect()
}

async fn requests(server: &Server) -> Vec<Value> {
    let (_, requests) = server.user.json(server.user.get("/api/v1/ssh-access-requests")).await;

    requests.as_array().unwrap().clone()
}

async fn fixture() -> Fixture {
    let server = Server::start().await;
    let mut agent_node = server.online_node("agent-node").await;
    let target = server.online_node("target").await;
    let conversation = server.create_conversation(agent_node.id, Some("Debug the target")).await;
    agent_node.expect_start_session().await;
    agent_node.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    agent_node.expect_prompt().await;

    Fixture { server, agent_node, target, conversation: conversation.id }
}

async fn ask(fixture: &Fixture, node: &str, minutes: Option<i64>) -> Value {
    let mut arguments = json!({"node": node, "reason": "Find out why the disk is full"});

    if let Some(minutes) = minutes {
        arguments["minutes"] = json!(minutes);
    }

    tool(&fixture.server, &fixture.agent_node, fixture.conversation, "request_ssh_access", arguments).await
}

async fn approve(server: &Server, id: &str, body: Value) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/ssh-access-requests/{id}/approve")).json(&body)).await
}

async fn approved_token(fixture: &mut Fixture, minutes: Option<i64>) -> (String, String) {
    ask(fixture, "target", minutes).await;
    let id = requests(&fixture.server).await[0]["id"].as_str().unwrap().to_string();
    let (status, body) = approve(&fixture.server, &id, json!({})).await;
    assert_eq!(status, 200, "{body}");

    match fixture.agent_node.next().await {
        ControlToNode::SshToken { token, .. } => (id, token),
        other => panic!("expected ssh_token, got {other:?}"),
    }
}

async fn open(server: &Server, node: &str, token: Option<&str>) -> Result<Agent, tokio_tungstenite::tungstenite::Error> {
    let url = format!("ws://127.0.0.1:{}/api/v1/nodes/{node}/ssh", server.address.port());
    let mut request = url.into_client_request().unwrap();

    if let Some(token) = token {
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
    }

    tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
}

fn refusal(result: Result<Agent, tokio_tungstenite::tungstenite::Error>) -> u16 {
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => response.status().as_u16(),
        Err(other) => panic!("expected an HTTP refusal, got {other:?}"),
        Ok(_) => panic!("expected a refusal, the socket opened"),
    }
}

async fn open_session(fixture: &mut Fixture, token: &str) -> (Agent, Uuid) {
    let agent = open(&fixture.server, &fixture.target.id.to_string(), Some(token)).await.unwrap();
    let session_id = match fixture.target.next().await {
        ControlToNode::SshOpen { session_id } => session_id,
        other => panic!("expected ssh_open, got {other:?}"),
    };

    (agent, session_id)
}

async fn agent_closed(agent: &mut Agent) -> bool {
    loop {
        match tokio::time::timeout(WAIT, agent.next()).await {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => return true,
            Ok(Some(Ok(_))) => {}
            Err(_) => return false,
        }
    }
}

#[tokio::test]
async fn a_request_names_a_node_and_waits_for_the_person() {
    let fixture = fixture().await;
    let mut stream = fixture.server.user.stream().await;

    let result = ask(&fixture, "target", None).await;

    let listed = requests(&fixture.server).await;
    assert_eq!(listed.len(), 1);
    assert_eq!((result["isError"].clone(), text(&result)), (json!(false), format!("Waiting for the person to approve SSH access to target (request {}).", listed[0]["id"].as_str().unwrap())));
    assert_eq!((listed[0]["state"].clone(), listed[0]["node_name"].clone(), listed[0]["minutes"].clone()), (json!("requested"), json!("target"), json!(30)));

    let streamed: SshAccessRequest = stream
        .until(|message| match message {
            StreamMessage::SshAccessRequest(request) => Some(request),
            _ => None,
        })
        .await;
    assert_eq!(streamed.state, SshAccessState::Requested);
}

#[tokio::test]
async fn a_bad_duration_an_unknown_node_or_no_reason_is_refused_and_records_nothing() {
    let fixture = fixture().await;

    for arguments in [json!({"node": "target", "minutes": 4, "reason": "r"}), json!({"node": "target", "minutes": 241, "reason": "r"}), json!({"node": "nowhere", "reason": "r"}), json!({"node": "target", "reason": " "}), json!({"node": " ", "reason": "r"})] {
        let result = tool(&fixture.server, &fixture.agent_node, fixture.conversation, "request_ssh_access", arguments.clone()).await;

        assert_eq!(result["isError"], true, "{arguments}: {result}");
        assert!(text(&result).starts_with("Nothing was requested."), "{arguments}: {result}");
    }

    assert_eq!(requests(&fixture.server).await.len(), 0);
}

#[tokio::test]
async fn approving_sends_a_token_to_the_conversations_node_and_the_agent_is_told_the_command_never_the_token() {
    let mut fixture = fixture().await;
    let (id, token) = approved_token(&mut fixture, None).await;
    assert!(token.starts_with("myc_ssh_"));

    let path = format!("/state/sessions/{}/ssh-token-{id}", fixture.conversation);
    fixture.agent_node.send(NodeToControl::SshTokenSaved { request_id: id.parse().unwrap(), path: Some(path.clone()) }).await;
    let told = eventually("the agent being told", || async { queued_texts(&fixture.server, fixture.conversation).await.into_iter().find(|text| text.contains(&path)) }).await;

    assert!(told.contains(&format!("mycelium ssh-proxy {} --token-file {path}", fixture.target.id)), "{told}");
    assert!(!told.contains(token.trim_start_matches("myc_ssh_")), "the token leaked");
    assert!(!serde_json::to_string(&requests(&fixture.server).await).unwrap().contains(token.trim_start_matches("myc_ssh_")), "the token leaked");
}

#[tokio::test]
async fn when_the_node_cannot_save_the_token_the_grant_is_revoked_and_the_agent_is_told() {
    let mut fixture = fixture().await;
    let (id, token) = approved_token(&mut fixture, None).await;

    fixture.agent_node.send(NodeToControl::SshTokenSaved { request_id: id.parse().unwrap(), path: None }).await;

    eventually("the agent being told", || async { queued_texts(&fixture.server, fixture.conversation).await.into_iter().find(|text| text.contains("could not save")) }).await;
    assert_eq!(refusal(open(&fixture.server, &fixture.target.id.to_string(), Some(&token)).await), 401);
}

#[tokio::test]
async fn approving_with_fewer_minutes_shortens_the_grant_and_more_than_requested_is_refused() {
    let mut fixture = fixture().await;
    ask(&fixture, "target", Some(60)).await;
    let id = requests(&fixture.server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = approve(&fixture.server, &id, json!({"minutes": 61})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("minutes_out_of_range")));
    assert_eq!(requests(&fixture.server).await[0]["state"], "requested");

    let (status, body) = approve(&fixture.server, &id, json!({"minutes": 15})).await;
    assert_eq!((status, body["state"].clone(), body["minutes"].clone()), (200, json!("approved"), json!(15)), "{body}");
    fixture.agent_node.next().await;

    let minutes = sqlx::query_scalar::<_, f64>("select (extract(epoch from expires_at - now()) / 60)::float8 from ssh_access_requests").fetch_one(&fixture.server.pool).await.unwrap();
    assert!((14.0..=15.0).contains(&minutes), "{minutes}");
}

#[tokio::test]
async fn an_approved_token_carries_bytes_both_ways_and_the_session_is_recorded() {
    let mut fixture = fixture().await;
    let (_, token) = approved_token(&mut fixture, None).await;

    let (mut agent, session_id) = open_session(&mut fixture, &token).await;

    agent.send(Message::Binary(b"SSH-2.0-test".to_vec().into())).await.unwrap();
    assert_eq!(fixture.target.next().await, ControlToNode::SshData { session_id, data: STANDARD.encode(b"SSH-2.0-test") });

    fixture.target.send(NodeToControl::SshData { session_id, data: STANDARD.encode(b"SSH-2.0-node") }).await;
    let received = tokio::time::timeout(WAIT, agent.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(received, Message::Binary(b"SSH-2.0-node".to_vec().into()));

    let recorded = sqlx::query_as::<_, (Uuid, Uuid, bool)>("select conversation_id, node_id, ended_at is null from ssh_sessions where id = $1 and owner = (select id from users limit 1)").bind(session_id).fetch_one(&fixture.server.pool).await.unwrap();
    assert_eq!(recorded, (fixture.conversation, fixture.target.id, true));
}

#[tokio::test]
async fn a_missing_unknown_expired_or_misdirected_token_and_an_offline_target_are_refused() {
    let mut fixture = fixture().await;
    let (id, token) = approved_token(&mut fixture, None).await;
    let target = fixture.target.id.to_string();

    assert_eq!(refusal(open(&fixture.server, &target, None).await), 401);
    assert_eq!(refusal(open(&fixture.server, &target, Some("myc_ssh_unknown")).await), 401);
    assert_eq!(refusal(open(&fixture.server, &fixture.agent_node.id.to_string(), Some(&token)).await), 403);

    drop(fixture.target);
    eventually("the target going offline", || async {
        let (_, nodes) = fixture.server.user.json(fixture.server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(target) && node["online"] == false).then_some(())
    })
    .await;
    assert_eq!(refusal(open(&fixture.server, &target, Some(&token)).await), 409);

    sqlx::query("update ssh_access_requests set expires_at = now() - interval '1 minute' where id = $1::uuid").bind(&id).execute(&fixture.server.pool).await.unwrap();
    assert_eq!(refusal(open(&fixture.server, &target, Some(&token)).await), 401);
}

#[tokio::test]
async fn revoking_cuts_an_open_session_blocks_new_ones_and_tells_the_agent() {
    let mut fixture = fixture().await;
    let (id, token) = approved_token(&mut fixture, None).await;
    let (mut agent, session_id) = open_session(&mut fixture, &token).await;

    let (status, body) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/ssh-access-requests/{id}/revoke"))).await;

    assert_eq!((status, body["state"].clone()), (200, json!("revoked")), "{body}");
    assert_eq!(fixture.target.next().await, ControlToNode::SshClose { session_id });
    assert!(agent_closed(&mut agent).await);
    assert_eq!(refusal(open(&fixture.server, &fixture.target.id.to_string(), Some(&token)).await), 401);
    assert!(queued_texts(&fixture.server, fixture.conversation).await.iter().any(|text| text.contains("revoked") && text.contains("SSH access")));

    let (status, body) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/ssh-access-requests/{id}/revoke"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("request_not_approved")));
}

#[tokio::test]
async fn a_session_ends_when_the_grant_expires() {
    let mut fixture = fixture().await;
    let (id, token) = approved_token(&mut fixture, None).await;
    sqlx::query("update ssh_access_requests set expires_at = now() + interval '1 second' where id = $1::uuid").bind(&id).execute(&fixture.server.pool).await.unwrap();
    let (mut agent, session_id) = open_session(&mut fixture, &token).await;

    assert!(agent_closed(&mut agent).await);
    assert_eq!(fixture.target.next().await, ControlToNode::SshClose { session_id });
    eventually("the session being marked ended", || async { sqlx::query_scalar::<_, bool>("select ended_at is not null from ssh_sessions where id = $1").bind(session_id).fetch_one(&fixture.server.pool).await.unwrap().then_some(()) }).await;
}

#[tokio::test]
async fn the_node_closing_a_session_closes_the_agent_without_an_ssh_close_back() {
    let mut fixture = fixture().await;
    let (_, token) = approved_token(&mut fixture, None).await;
    let (mut agent, session_id) = open_session(&mut fixture, &token).await;

    fixture.target.send(NodeToControl::SshClosed { session_id }).await;

    assert!(agent_closed(&mut agent).await);
    eventually("the session being marked ended", || async { sqlx::query_scalar::<_, bool>("select ended_at is not null from ssh_sessions where id = $1").bind(session_id).fetch_one(&fixture.server.pool).await.unwrap().then_some(()) }).await;
}

#[tokio::test]
async fn declining_tells_the_agent_and_mints_nothing() {
    let fixture = fixture().await;
    ask(&fixture, "target", None).await;
    let id = requests(&fixture.server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/ssh-access-requests/{id}/decline"))).await;

    assert_eq!((status, body["state"].clone()), (200, json!("declined")), "{body}");
    assert!(queued_texts(&fixture.server, fixture.conversation).await.iter().any(|text| text.contains("declined") && text.contains("SSH access to target")));
    assert_eq!(sqlx::query_scalar::<_, i64>("select count(*) from ssh_access_requests where token_hash is not null").fetch_one(&fixture.server.pool).await.unwrap(), 0);

    let (status, body) = approve(&fixture.server, &id, json!({})).await;
    assert_eq!((status, body["error"].clone()), (409, json!("request_not_requested")));
}

#[tokio::test]
async fn a_person_mints_a_short_token_for_their_node_and_it_opens_a_session_without_a_conversation() {
    let mut fixture = fixture().await;
    let (status, minted) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/nodes/{}/ssh-tokens", fixture.target.id))).await;
    assert_eq!(status, 200, "{minted}");
    let token = minted["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("myc_ssh_"));

    let (mut person, session_id) = open_session(&mut fixture, &token).await;
    person.send(Message::Binary(b"SSH-2.0-zed".to_vec().into())).await.unwrap();

    assert_eq!(fixture.target.next().await, ControlToNode::SshData { session_id, data: STANDARD.encode(b"SSH-2.0-zed") });
    assert_eq!(refusal(open(&fixture.server, &fixture.agent_node.id.to_string(), Some(&token)).await), 403);
}

#[tokio::test]
async fn a_token_is_not_minted_for_a_node_another_person_registered_or_by_a_scoped_token() {
    let fixture = fixture().await;
    let other = fixture.server.second_user("other@example.com").await;
    let path = format!("/api/v1/nodes/{}/ssh-tokens", fixture.target.id);
    let secret = Uuid::new_v4().to_string();
    sqlx::query("insert into client_tokens (user_id, label, token_hash, scopes) values ((select id from users where email = 'dev@example.com'), 'Test', $1, $2)").bind(crate::common::hash(&secret)).bind(vec!["nodes:read".to_string()]).execute(&fixture.server.pool).await.unwrap();

    let (by_another_person, _) = other.json(other.post(&path)).await;
    let anonymous = fixture.server.anonymous();
    let (by_a_scoped_token, _) = anonymous.json(anonymous.post(&path).header("authorization", format!("Bearer myc_client_{secret}"))).await;

    assert_eq!((by_another_person, by_a_scoped_token), (404, 403));
}

#[tokio::test]
async fn a_token_for_a_node_that_does_not_exist_is_not_minted() {
    let fixture = fixture().await;
    let (status, _) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/nodes/{}/ssh-tokens", Uuid::new_v4()))).await;

    assert_eq!(status, 404);
}
