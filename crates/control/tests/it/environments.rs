use crate::common;
use crate::common::{NodeLink, Server};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::api::{EnvironmentCapability, EnvironmentState, EnvironmentVersions};
use mycelium_proto::link::{ControlToNode, EnvironmentInputAction, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

const NO_ENVIRONMENT: &str = "No environment is running. Ask the person to start one from the Environment panel.";

struct Fixture {
    server: Server,
    link: NodeLink,
    token: String,
    conversation_id: Uuid,
}

fn capable() -> EnvironmentCapability {
    EnvironmentCapability { available: true, missing: vec![], versions: EnvironmentVersions { compositor: Some("sway 1.10".into()), wayvnc: Some("wayvnc 0.9.1".into()), browser: Some("Chromium 141".into()) }, note: None, running: vec![] }
}

fn incapable() -> EnvironmentCapability {
    EnvironmentCapability { available: false, missing: vec!["sway".into(), "wayvnc".into()], versions: EnvironmentVersions::default(), note: None, running: vec![] }
}

async fn fixture(capability: Option<EnvironmentCapability>) -> Fixture {
    let server = Server::start().await;
    let (mut link, token) = server.online_node_with_token("laptop", env!("CARGO_PKG_VERSION")).await;

    if let Some(capability) = capability {
        let available = capability.available;
        link.send(NodeToControl::EnvironmentCapability { capability }).await;

        let user = server.user.clone();
        common::eventually("the node's environment capability", || async {
            let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

            nodes.as_array().unwrap().iter().any(|node| node["environment"]["available"] == json!(available)).then_some(())
        })
        .await;
    }

    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    Fixture { server, link, token, conversation_id: conversation.id }
}

impl Fixture {
    fn url(&self, tail: &str) -> String {
        format!("/api/v1/conversations/{}/environment{tail}", self.conversation_id)
    }

    async fn start(&self) -> (u16, Value) {
        self.server.user.json(self.server.user.post(&self.url("")).json(&json!({}))).await
    }

    async fn running(&mut self) {
        let (status, started) = self.start().await;
        assert_eq!((status, started["state"].clone()), (200, json!("starting")), "{started}");
        assert!(matches!(self.link.next().await, ControlToNode::StartEnvironment { width: 1280, height: 800, .. }));

        self.link.send(NodeToControl::EnvironmentChanged { conversation_id: self.conversation_id, state: EnvironmentState::Running, failure: None, browser: Some("Chromium 141".into()) }).await;

        let user = self.server.user.clone();
        let url = self.url("");
        common::eventually("the environment running", || async {
            let (_, environment) = user.json(user.get(&url)).await;

            (environment["state"] == "running").then_some(())
        })
        .await;
    }

    async fn person(&self) -> String {
        sqlx::query_scalar::<_, String>("select name from users limit 1").fetch_one(&self.server.pool).await.unwrap()
    }

    async fn stream_socket(&self) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
        let url = format!("ws://127.0.0.1:{}{}", self.server.address.port(), self.url("/stream"));
        let mut request = url.into_client_request().unwrap();

        request.headers_mut().insert("cookie", self.server.user.cookie.parse().unwrap());

        tokio_tungstenite::connect_async(request).await.unwrap().0
    }
}

async fn tool(server: &Server, token: &str, conversation_id: Uuid, name: &str, arguments: Value) -> Value {
    server.agent_mcp(token, conversation_id, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await
}

async fn answer_input(link: &mut NodeLink) -> EnvironmentInputAction {
    let ControlToNode::EnvironmentInput { request_id, action, .. } = link.next().await else {
        panic!("expected the node to be given input");
    };
    link.send(NodeToControl::EnvironmentInputDone { request_id, error: None }).await;

    action
}

fn text_of(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn starting_an_environment_tells_the_node_and_runs_once_the_node_reports_it() {
    let mut fixture = fixture(Some(capable())).await;
    let mut stream = fixture.server.user.stream().await;

    let (status, started) = fixture.start().await;
    assert_eq!((status, started["state"].clone(), started["controller"].clone(), started["node_name"].clone()), (200, json!("starting"), json!("agent"), json!("laptop")), "{started}");

    let ControlToNode::StartEnvironment { conversation_id, width, height } = fixture.link.next().await else {
        panic!("expected the node to be told to start an environment");
    };
    assert_eq!((conversation_id, width, height), (fixture.conversation_id, 1280, 800));

    fixture.link.send(NodeToControl::EnvironmentChanged { conversation_id, state: EnvironmentState::Running, failure: None, browser: Some("Chromium 141".into()) }).await;
    let running = stream.until(|message| match message { StreamMessage::Environment(environment) if environment.state == EnvironmentState::Running => Some(environment), _ => None }).await;
    let (_, read) = fixture.server.user.json(fixture.server.user.get(&fixture.url(""))).await;

    assert_eq!((running.browser.as_deref(), read["state"].clone(), read["browser"].clone()), (Some("Chromium 141"), json!("running"), json!("Chromium 141")));
}

#[tokio::test]
async fn a_failed_start_keeps_what_failed() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.start().await;
    fixture.link.next().await;

    fixture.link.send(NodeToControl::EnvironmentChanged { conversation_id: fixture.conversation_id, state: EnvironmentState::Failed, failure: Some("Chromium did not open a window.".into()), browser: None }).await;

    let user = fixture.server.user.clone();
    let url = fixture.url("");
    let failure = common::eventually("the failure", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["state"] == "failed").then(|| environment["failure"].clone())
    })
    .await;

    assert_eq!(failure, json!("Chromium did not open a window."));
}

#[tokio::test]
async fn an_environment_is_not_started_on_a_node_that_cannot_run_one() {
    let fixture = fixture(Some(incapable())).await;

    let (status, body) = fixture.start().await;

    assert_eq!((status, body["error"].clone()), (409, json!("environment_unavailable")), "{body}");
    assert!(body["message"].as_str().unwrap().starts_with("laptop cannot run an environment"), "{body}");
    assert_eq!(fixture.server.user.get(&fixture.url("")).send().await.unwrap().status().as_u16(), 404);
}

#[tokio::test]
async fn an_environment_is_not_started_before_the_node_has_reported_what_it_can_do() {
    let fixture = fixture(None).await;

    let (status, body) = fixture.start().await;

    assert_eq!((status, body["error"].clone()), (409, json!("environment_unavailable")), "{body}");
}

#[tokio::test]
async fn asking_a_node_to_check_again_returns_what_it_then_reports() {
    let fixture = fixture(Some(incapable())).await;
    let mut link = fixture.link;
    let url = format!("/api/v1/nodes/{}/environment/check", link.id);
    let user = fixture.server.user.clone();

    let (checked, ()) = tokio::join!(user.json(user.post(&url)), async {
        assert_eq!(link.next().await, ControlToNode::CheckEnvironment);
        link.send(NodeToControl::EnvironmentCapability { capability: capable() }).await;
    });

    assert_eq!((checked.0, checked.1["available"].clone()), (200, json!(true)), "{}", checked.1);
}

#[tokio::test]
async fn a_tool_with_no_environment_says_how_to_get_one() {
    let fixture = fixture(Some(capable())).await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"x": 10, "y": 20, "label": "Password"})).await;

    assert_eq!(text_of(&reply), NO_ENVIRONMENT);
    assert_eq!(reply["result"]["isError"], true);
}

#[tokio::test]
async fn click_and_type_need_a_label_the_person_can_read() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let click = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"x": 10, "y": 20, "label": "  "})).await;
    let typed = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_type", json!({"text": "hello", "into": ""})).await;

    assert!(click["error"]["message"].as_str().unwrap().contains("label"), "{click}");
    assert!(typed["error"]["message"].as_str().unwrap().contains("into"), "{typed}");
}

#[tokio::test]
async fn while_the_person_has_control_every_tool_waits_and_nothing_reaches_the_node() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let person = fixture.person().await;

    let (status, taken) = fixture.server.user.json(fixture.server.user.post(&fixture.url("/take-over"))).await;
    assert_eq!((status, taken["controller"].clone()), (200, json!("person")), "{taken}");

    let waiting = format!("{person} has taken over the environment; wait until they hand back");
    let click = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"x": 1, "y": 1, "label": "While away"})).await;
    let shot = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_screenshot", json!({})).await;
    assert_eq!((text_of(&click), text_of(&shot)), (waiting.clone(), waiting));

    fixture.server.user.json(fixture.server.user.post(&fixture.url("/hand-back"))).await;
    let (_, click) = tokio::join!(answer_input(&mut fixture.link), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"x": 2, "y": 3, "label": "After"})));

    assert!(text_of(&click).contains("Clicked “After”."), "{click}");
}

#[tokio::test]
async fn after_a_hand_back_a_click_reaches_the_node_is_logged_and_warns_that_the_screen_may_have_changed() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    fixture.server.user.json(fixture.server.user.post(&fixture.url("/take-over"))).await;
    fixture.server.user.json(fixture.server.user.post(&fixture.url("/hand-back"))).await;

    let (action, click) = tokio::join!(answer_input(&mut fixture.link), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"x": 40, "y": 50, "label": "Password", "button": "right"})));
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;
    let (_, environment) = fixture.server.user.json(fixture.server.user.get(&fixture.url(""))).await;
    let labels: Vec<&str> = actions.as_array().unwrap().iter().map(|action| action["label"].as_str().unwrap()).collect();

    assert_eq!(action, EnvironmentInputAction::Click { x: 40, y: 50, button: Some("right".into()) });
    assert!(text_of(&click).contains("Take a screenshot before relying on it."), "{click}");
    assert_eq!(labels, ["Clicking “Password”", "You handed back", "You took over"]);
    assert_eq!(environment["target"], json!({"x": 40, "y": 50, "label": "Password"}));
}

#[tokio::test]
async fn typing_scrolling_and_keys_are_logged_without_the_typed_text() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (typed, _) = tokio::join!(answer_input(&mut fixture.link), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_type", json!({"text": "hunter2", "into": "Email"})));
    let (scrolled, _) = tokio::join!(answer_input(&mut fixture.link), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_scroll", json!({"direction": "down"})));
    let (pressed, _) = tokio::join!(answer_input(&mut fixture.link), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_key", json!({"keys": "Ctrl+L"})));
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert_eq!(typed, EnvironmentInputAction::Type { text: "hunter2".into() });
    assert_eq!(scrolled, EnvironmentInputAction::Scroll { direction: "down".into(), amount: 3, x: None, y: None });
    assert_eq!(pressed, EnvironmentInputAction::Key { keys: "Ctrl+L".into() });
    assert_eq!(actions.to_string().contains("hunter2"), false);
    assert_eq!(actions[0]["label"], "Pressing Ctrl+L");
    assert_eq!(actions[1]["label"], "Scrolling down");
    assert_eq!(actions[2]["label"], "Typed into “Email”");
}

#[tokio::test]
async fn a_screenshot_comes_back_as_an_image() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG fake");

    let asked_for = fixture.conversation_id;
    let link = &mut fixture.link;

    let (_, reply) = tokio::join!(
        async {
            let ControlToNode::EnvironmentScreenshot { request_id, conversation_id } = link.next().await else {
                panic!("expected the node to be asked for a screenshot");
            };
            assert_eq!(conversation_id, asked_for);
            link.send(NodeToControl::EnvironmentScreenshotTaken { request_id, png: Some(png.clone()), error: None }).await;
        },
        tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_screenshot", json!({}))
    );
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert_eq!(reply["result"]["content"][0], json!({"type": "image", "data": png, "mimeType": "image/png"}));
    assert_eq!(actions[0]["label"], "Took a screenshot");
}

#[tokio::test]
async fn an_error_from_the_node_reaches_the_agent_and_is_not_logged() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let link = &mut fixture.link;

    let (_, reply) = tokio::join!(
        async {
            let ControlToNode::EnvironmentInput { request_id, .. } = link.next().await else {
                panic!("expected input");
            };
            link.send(NodeToControl::EnvironmentInputDone { request_id, error: Some("The browser has no window to click in.".into()) }).await;
        },
        tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_key", json!({"keys": "Enter"}))
    );
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert_eq!((text_of(&reply), reply["result"]["isError"].clone(), actions), ("The browser has no window to click in.".to_string(), json!(true), json!([])));
}

#[tokio::test]
async fn stopping_tells_the_node_records_when_and_leaves_tools_with_nothing_to_drive() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (status, stopped) = fixture.server.user.json(fixture.server.user.delete(&fixture.url(""))).await;
    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_key", json!({"keys": "Enter"})).await;

    assert_eq!((status, stopped["state"].clone()), (200, json!("stopped")), "{stopped}");
    assert!(stopped["stopped_at"].is_string(), "{stopped}");
    assert_eq!(fixture.link.next().await, ControlToNode::StopEnvironment { conversation_id: fixture.conversation_id });
    assert_eq!(text_of(&reply), NO_ENVIRONMENT);
}

#[tokio::test]
async fn another_person_cannot_see_or_drive_the_environment() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let other = fixture.server.second_user("other@example.com").await;

    let (read, _) = other.json(other.get(&fixture.url(""))).await;
    let (take, _) = other.json(other.post(&fixture.url("/take-over"))).await;
    let (stop, _) = other.json(other.delete(&fixture.url(""))).await;

    assert_eq!((read, take, stop), (404, 404, 404));
}

#[tokio::test]
async fn a_node_that_disconnects_stops_its_environments() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let url = fixture.url("");
    let user = fixture.server.user.clone();

    drop(fixture.link);

    let environment = common::eventually("the environment stopping", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["state"] == "stopped").then_some(environment)
    })
    .await;

    assert!(environment["stopped_at"].is_string(), "{environment}");
}

#[tokio::test]
async fn the_screen_stream_is_relayed_both_ways_and_closes_with_either_side() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let mut socket = fixture.stream_socket().await;

    let ControlToNode::OpenEnvironmentStream { stream_id, conversation_id } = fixture.link.next().await else {
        panic!("expected the node to be asked to open the stream");
    };
    assert_eq!(conversation_id, fixture.conversation_id);

    let encoding = base64::engine::general_purpose::STANDARD;
    fixture.link.send(NodeToControl::EnvironmentStreamData { stream_id, data: encoding.encode(b"RFB 003.008\n") }).await;
    let Some(Ok(Message::Binary(greeting))) = socket.next().await else {
        panic!("expected the greeting as a binary frame");
    };
    assert_eq!(greeting.as_ref(), b"RFB 003.008\n");

    socket.send(Message::Binary(b"RFB 003.008\n".to_vec().into())).await.unwrap();
    assert_eq!(fixture.link.next().await, ControlToNode::EnvironmentStreamInput { stream_id, data: encoding.encode(b"RFB 003.008\n") });

    socket.close(None).await.unwrap();
    assert_eq!(fixture.link.next().await, ControlToNode::CloseEnvironmentStream { stream_id });
}

#[tokio::test]
async fn the_screen_stream_closes_when_the_node_closes_it() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let mut socket = fixture.stream_socket().await;
    let ControlToNode::OpenEnvironmentStream { stream_id, .. } = fixture.link.next().await else {
        panic!("expected the node to be asked to open the stream");
    };

    fixture.link.send(NodeToControl::EnvironmentStreamClosed { stream_id }).await;

    loop {
        match socket.next().await {
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            Some(Ok(_)) => {}
        }
    }
}

#[tokio::test]
async fn the_screen_stream_needs_a_signed_in_person_and_a_running_environment() {
    let fixture = fixture(Some(capable())).await;
    let url = format!("ws://127.0.0.1:{}{}", fixture.server.address.port(), fixture.url("/stream"));

    let anonymous = tokio_tungstenite::connect_async(url.clone().into_client_request().unwrap()).await;
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("cookie", fixture.server.user.cookie.parse().unwrap());
    let not_running = tokio_tungstenite::connect_async(request).await;

    assert!(anonymous.is_err());
    assert!(not_running.is_err());
}

#[tokio::test]
async fn a_live_view_ticket_opens_the_stream_once_and_a_token_in_the_address_does_not() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (status, issued) = fixture.server.user.json(fixture.server.user.post(&fixture.url("/stream-ticket"))).await;
    assert_eq!(status, 200, "{issued}");
    let ticket = issued["ticket"].as_str().unwrap().to_string();
    let address = |query: &str| format!("ws://127.0.0.1:{}{}?{query}", fixture.server.address.port(), fixture.url("/stream"));

    let first = tokio_tungstenite::connect_async(address(&format!("ticket={ticket}")).into_client_request().unwrap()).await;
    let again = tokio_tungstenite::connect_async(address(&format!("ticket={ticket}")).into_client_request().unwrap()).await;
    let token = tokio_tungstenite::connect_async(address("token=anything").into_client_request().unwrap()).await;

    assert!(first.is_ok());
    assert!(matches!(fixture.link.next().await, ControlToNode::OpenEnvironmentStream { .. }));
    assert!(again.is_err());
    assert!(token.is_err());
}

#[tokio::test]
async fn the_novnc_files_are_served_without_signing_in() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let page = anonymous.get("/novnc/embed.html").send().await.unwrap();
    let script = anonymous.get("/novnc/core/rfb.js").send().await.unwrap();
    let missing = anonymous.get("/novnc/core/nothing.js").send().await.unwrap();

    assert_eq!((page.status().as_u16(), script.status().as_u16(), missing.status().as_u16()), (200, 200, 404));
    assert_eq!(script.headers()["content-type"], "text/javascript");
}
