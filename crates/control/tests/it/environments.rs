use crate::common;
use crate::common::{NodeLink, Server};
use crate::decision_providers::{add as add_decision_provider, fake_jev};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::api::{EnvironmentApp, EnvironmentAppKind, EnvironmentCapability, EnvironmentState, EnvironmentVersions, EnvironmentWindow};
use mycelium_proto::link::{BrowserAction, BrowserControl, ControlToNode, EnvironmentInputAction, NodeToControl};
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

fn apps() -> Vec<EnvironmentApp> {
    vec![
        EnvironmentApp { id: "org.gnome.Nautilus".into(), name: "Files".into(), kind: EnvironmentAppKind::FileManager },
        EnvironmentApp { id: "chromium".into(), name: "Chromium".into(), kind: EnvironmentAppKind::Browser },
    ]
}

fn window(id: i64, title: &str, focused: bool) -> EnvironmentWindow {
    EnvironmentWindow { id, app_id: "chromium".into(), title: title.into(), focused }
}

fn capable() -> EnvironmentCapability {
    EnvironmentCapability { available: true, missing: vec![], versions: EnvironmentVersions { compositor: Some("sway 1.10".into()), wayvnc: Some("wayvnc 0.9.1".into()), browser: Some("Chromium 141".into()) }, note: None, running: vec![], apps: apps() }
}

fn incapable() -> EnvironmentCapability {
    EnvironmentCapability { available: false, missing: vec!["sway".into(), "wayvnc".into()], versions: EnvironmentVersions::default(), note: None, running: vec![], apps: vec![] }
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

    let ControlToNode::StartEnvironment { conversation_id, width, height, apps } = fixture.link.next().await else {
        panic!("expected the node to be told to start an environment");
    };
    assert_eq!((conversation_id, width, height, apps), (fixture.conversation_id, 1280, 800, vec!["chromium".to_string()]));

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

async fn start_with(fixture: &Fixture, body: Value) -> (u16, Value) {
    fixture.server.user.json(fixture.server.user.post(&fixture.url("")).json(&body)).await
}

async fn report_windows(fixture: &mut Fixture, windows: Vec<EnvironmentWindow>) {
    fixture.link.send(NodeToControl::EnvironmentWindows { conversation_id: fixture.conversation_id, windows }).await;
}

async fn answer_launch(link: &mut NodeLink, window: Option<i64>, error: Option<String>) -> (String, Option<String>) {
    let ControlToNode::EnvironmentLaunch { request_id, app, file, .. } = link.next().await else {
        panic!("expected the node to be asked to open an app");
    };
    link.send(NodeToControl::EnvironmentLaunched { request_id, window, error }).await;

    (app, file)
}

#[tokio::test]
async fn an_environment_starts_with_the_apps_the_person_ticked() {
    let mut fixture = fixture(Some(capable())).await;

    let (status, _) = start_with(&fixture, json!({"apps": ["org.gnome.Nautilus", "chromium"]})).await;
    let ControlToNode::StartEnvironment { apps, .. } = fixture.link.next().await else {
        panic!("expected the node to be told to start an environment");
    };

    assert_eq!((status, apps), (200, vec!["org.gnome.Nautilus".to_string(), "chromium".to_string()]));
}

#[tokio::test]
async fn ticking_no_apps_starts_an_empty_desktop() {
    let mut fixture = fixture(Some(capable())).await;

    let (status, _) = start_with(&fixture, json!({"apps": []})).await;
    let ControlToNode::StartEnvironment { apps, .. } = fixture.link.next().await else {
        panic!("expected the node to be told to start an environment");
    };

    assert_eq!((status, apps), (200, Vec::<String>::new()));
}

#[tokio::test]
async fn starting_with_an_app_the_node_does_not_offer_says_what_it_offers() {
    let fixture = fixture(Some(capable())).await;

    let (status, refused) = start_with(&fixture, json!({"apps": ["gimp"]})).await;

    assert_eq!(status, 400, "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("laptop does not offer \"gimp\". It offers: org.gnome.Nautilus, chromium."), "{refused}");
}

#[tokio::test]
async fn the_windows_a_node_reports_are_in_the_environment() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    report_windows(&mut fixture, vec![window(7, "Inbox", true), window(9, "Docs", false)]).await;

    let user = fixture.server.user.clone();
    let url = fixture.url("");
    let environment = common::eventually("the windows", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["windows"].as_array().is_some_and(|windows| windows.len() == 2)).then_some(environment)
    })
    .await;

    assert_eq!(environment["windows"][0], json!({"id": 7, "app_id": "chromium", "title": "Inbox", "focused": true}));
    assert_eq!(environment["agent_window"], Value::Null);
}

#[tokio::test]
async fn the_agent_opening_an_app_focuses_its_window_and_owns_it() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (launched, reply) = tokio::join!(answer_launch(&mut fixture.link, Some(12), None), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_launch", json!({"app": "org.gnome.Nautilus", "file": "/srv/report.pdf"})));
    let (_, environment) = fixture.server.user.json(fixture.server.user.get(&fixture.url(""))).await;
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert_eq!(launched, ("org.gnome.Nautilus".to_string(), Some("/srv/report.pdf".to_string())));
    assert!(text_of(&reply).contains("Opened Files."), "{reply}");
    assert_eq!((environment["agent_window"].clone(), environment["controller"].clone()), (json!(12), json!("agent")));
    assert_eq!(actions[0]["label"], "Opened Files");
}

#[tokio::test]
async fn the_agent_opening_an_unknown_app_is_told_which_apps_are_offered() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_launch", json!({"app": "gimp"})).await;

    assert_eq!(reply["result"]["isError"], true);
    assert!(text_of(&reply).contains("It offers: org.gnome.Nautilus, chromium."), "{reply}");
}

#[tokio::test]
async fn an_agent_window_that_closes_is_no_longer_the_agents() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    tokio::join!(answer_launch(&mut fixture.link, Some(12), None), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_launch", json!({"app": "chromium"})));

    report_windows(&mut fixture, vec![window(3, "Other", true)]).await;

    let user = fixture.server.user.clone();
    let url = fixture.url("");
    common::eventually("the agent window cleared", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["agent_window"].is_null() && environment["windows"].as_array().is_some_and(|windows| windows.len() == 1)).then_some(())
    })
    .await;
}

#[tokio::test]
async fn the_person_opening_an_app_takes_over_without_owning_its_window() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let user = fixture.server.user.clone();
    let url = fixture.url("/apps");

    let (opened, (status, environment)) = tokio::join!(answer_launch(&mut fixture.link, Some(5), None), user.json(user.post(&url).json(&json!({"app": "chromium"}))));

    assert_eq!(opened.0, "chromium");
    assert_eq!((status, environment["controller"].clone(), environment["agent_window"].clone()), (200, json!("person"), Value::Null), "{environment}");
}

#[tokio::test]
async fn the_person_opening_an_unknown_app_is_refused() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (status, refused) = fixture.server.user.json(fixture.server.user.post(&fixture.url("/apps")).json(&json!({"app": "gimp"}))).await;

    assert_eq!(status, 400, "{refused}");
}

#[tokio::test]
async fn the_person_switching_window_asks_the_node_and_takes_over() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    report_windows(&mut fixture, vec![window(7, "Inbox", true), window(9, "Docs", false)]).await;
    let user = fixture.server.user.clone();
    let url = fixture.url("");
    common::eventually("the windows", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["windows"].as_array().is_some_and(|windows| windows.len() == 2)).then_some(())
    })
    .await;
    let focus = fixture.url("/windows/9/focus");

    let (focused, (status, environment)) = tokio::join!(
        async {
            let ControlToNode::EnvironmentFocus { request_id, window, .. } = fixture.link.next().await else {
                panic!("expected the node to be asked to focus a window");
            };
            fixture.link.send(NodeToControl::EnvironmentInputDone { request_id, error: None }).await;

            window
        },
        user.json(user.post(&focus))
    );

    assert_eq!((focused, status, environment["controller"].clone()), (9, 200, json!("person")), "{environment}");
    assert_eq!(environment["windows"][1]["focused"], true);
}

#[tokio::test]
async fn switching_to_a_window_that_is_not_open_is_not_found() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (status, _) = fixture.server.user.json(fixture.server.user.post(&fixture.url("/windows/404/focus"))).await;

    assert_eq!(status, 404);
}

async fn environment_lines(fixture: &Fixture) -> Vec<String> {
    let events = fixture.server.user.clone();
    let url = format!("/api/v1/conversations/{}/events", fixture.conversation_id);
    let (_, events) = events.json(events.get(&url)).await;

    events.as_array().unwrap().iter().filter(|event| event["kind"] == "environment").map(|event| event["payload"]["text"].as_str().unwrap().to_string()).collect()
}

async fn next_prompt(link: &mut NodeLink) -> String {
    loop {
        if let ControlToNode::Prompt { text, .. } = link.next().await {
            return text;
        }
    }
}

#[tokio::test]
async fn starting_taking_over_and_handing_back_are_lines_in_the_thread() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    fixture.server.user.json(fixture.server.user.post(&fixture.url("/take-over"))).await;
    fixture.server.user.json(fixture.server.user.post(&fixture.url("/hand-back"))).await;

    assert_eq!(
        environment_lines(&fixture).await,
        [
            "You started the environment on laptop.",
            "You took over the environment. The agent is paused until you hand back.",
            "You handed back the environment.",
        ]
    );
}

#[tokio::test]
async fn taking_over_and_handing_back_are_told_to_the_agent_with_its_next_message() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    fixture.link.start_session(fixture.conversation_id, "acp-1").await;
    let person = fixture.person().await;

    fixture.server.user.json(fixture.server.user.post(&fixture.url("/take-over"))).await;
    fixture.server.user.json(fixture.server.user.post(&fixture.url("/hand-back"))).await;
    fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/conversations/{}/prompt", fixture.conversation_id)).json(&json!({"text": "carry on"}))).await;

    let text = next_prompt(&mut fixture.link).await;

    assert!(text.contains(&format!("{person} took over the environment.")), "{text}");
    assert!(text.contains(&format!("{person} handed the environment back.")), "{text}");
    assert!(text.ends_with("carry on"), "{text}");
}

#[tokio::test]
async fn closing_the_conversation_stops_its_environment() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (status, _) = fixture.server.user.json(fixture.server.user.post(&format!("/api/v1/conversations/{}/close", fixture.conversation_id))).await;
    assert_eq!(status, 204);

    loop {
        if fixture.link.next().await == (ControlToNode::StopEnvironment { conversation_id: fixture.conversation_id }) {
            break;
        }
    }

    let (_, environment) = fixture.server.user.json(fixture.server.user.get(&fixture.url(""))).await;

    assert_eq!((environment["state"].clone(), environment["closed_with_conversation"].clone()), (json!("stopped"), json!(true)), "{environment}");
    assert_eq!(environment_lines(&fixture).await.last().map(String::as_str), Some("The environment stopped when the conversation closed."));
}

#[tokio::test]
async fn starting_an_environment_in_an_archived_conversation_reopens_it() {
    let fixture = fixture(Some(capable())).await;
    let conversation = format!("/api/v1/conversations/{}", fixture.conversation_id);
    fixture.server.user.json(fixture.server.user.patch(&conversation).json(&json!({"archived": true}))).await;

    let (status, _) = fixture.start().await;
    let (_, reopened) = fixture.server.user.json(fixture.server.user.get(&conversation)).await;

    assert_eq!(status, 200);
    assert!(reopened["archived_at"].is_null(), "{reopened}");
}

#[tokio::test]
async fn stopping_by_hand_is_not_recorded_as_closing_with_the_conversation() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let (_, stopped) = fixture.server.user.json(fixture.server.user.delete(&fixture.url(""))).await;

    assert!(stopped["closed_with_conversation"].is_null(), "{stopped}");
}

#[tokio::test]
async fn the_screen_stream_is_view_only_until_the_person_takes_over() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let mut socket = fixture.stream_socket().await;
    let ControlToNode::OpenEnvironmentStream { stream_id, .. } = fixture.link.next().await else {
        panic!("expected the node to be asked to open the stream");
    };
    let encoding = base64::engine::general_purpose::STANDARD;
    let key_press = vec![4u8, 1, 0, 0, 0, 0, 0, 97];
    let refresh = vec![3u8, 1, 0, 0, 0, 0, 0, 100, 0, 100];

    socket.send(Message::Binary([b"RFB 003.008\n".to_vec(), vec![1, 1]].concat().into())).await.unwrap();
    assert!(matches!(fixture.link.next().await, ControlToNode::EnvironmentStreamInput { .. }));

    socket.send(Message::Binary([key_press.clone(), refresh.clone()].concat().into())).await.unwrap();
    assert_eq!(fixture.link.next().await, ControlToNode::EnvironmentStreamInput { stream_id, data: encoding.encode(&refresh) });

    fixture.server.user.json(fixture.server.user.post(&fixture.url("/take-over"))).await;
    socket.send(Message::Binary(key_press.clone().into())).await.unwrap();

    assert_eq!(fixture.link.next().await, ControlToNode::EnvironmentStreamInput { stream_id, data: encoding.encode(&key_press) });
}

fn send_button_box() -> Value {
    json!({"present": {"noul": 0.97}, "target": {"box": {"top": 0.1, "bottom": 0.2, "left": 0.5, "right": 0.55}, "confidence": 0.9}})
}

fn step(operation: &str, target: &str) -> Value {
    json!({"operation": {"choice": operation}, "target": {"choice": target}})
}

async fn answer_screenshot(link: &mut NodeLink, png: &str) {
    let ControlToNode::EnvironmentScreenshot { request_id, .. } = link.next().await else {
        panic!("expected the node to be asked for a screenshot");
    };
    link.send(NodeToControl::EnvironmentScreenshotTaken { request_id, png: Some(png.to_string()), error: None }).await;
}

async fn flights_page(link: &mut NodeLink, acted: &mut Vec<BrowserAction>) {
    loop {
        match link.next().await {
            ControlToNode::EnvironmentBrowserRead { request_id, .. } => {
                let controls = vec![BrowserControl { index: 1, role: "textbox".into(), name: "From".into(), value: String::new() }, BrowserControl { index: 2, role: "button".into(), name: "Search flights".into(), value: String::new() }];
                link.send(NodeToControl::EnvironmentBrowserControls { request_id, page: "Flights".into(), controls, error: None }).await;
            }
            ControlToNode::EnvironmentBrowserAct { request_id, action, .. } => {
                acted.push(action);
                link.send(NodeToControl::EnvironmentBrowserActed { request_id, error: None }).await;
            }
            other => panic!("unexpected message for the browser: {other:?}"),
        }
    }
}

async fn environment_of(fixture: &Fixture) -> Value {
    fixture.server.user.json(fixture.server.user.get(&fixture.url(""))).await.1
}

#[tokio::test]
async fn clicking_a_described_target_finds_it_on_a_screenshot_and_clicks_its_centre() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (base, jev) = fake_jev().await;
    add_decision_provider(&fixture.server, &base, "OpenJev", json!(["browser"])).await;
    jev.scripted.lock().unwrap().push_back(send_button_box());
    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG fake");
    let link = &mut fixture.link;

    let (action, reply) = tokio::join!(
        async {
            answer_screenshot(link, &png).await;
            answer_input(link).await
        },
        tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"target": "the Send button"}))
    );
    let asked = jev.asked.lock().unwrap().last().cloned().unwrap();
    let environment = environment_of(&fixture).await;

    assert_eq!(action, EnvironmentInputAction::Click { x: 672, y: 120, button: None });
    assert!(text_of(&reply).starts_with("Clicked “the Send button”. Found at 672, 120 by OpenJev in "), "{reply}");
    assert_eq!((asked["questions"]["target"]["type"].clone(), asked["assets"]["screen"]["data_base64"].clone()), (json!("box"), json!(png)));
    assert_eq!(environment["target"], json!({"x": 672, "y": 120, "label": "the Send button"}));
}

#[tokio::test]
async fn a_target_the_model_cannot_find_fails_without_clicking() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (base, jev) = fake_jev().await;
    add_decision_provider(&fixture.server, &base, "OpenJev", json!(["browser"])).await;
    jev.scripted.lock().unwrap().push_back(json!({"present": {"noul": 0.04}, "target": {"box": {"top": 0.4, "bottom": 0.6, "left": 0.4, "right": 0.6}, "confidence": 0.8}}));
    let link = &mut fixture.link;

    let (_, reply) = tokio::join!(answer_screenshot(link, "cG5n"), tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"target": "the Send button"})));
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert_eq!((text_of(&reply), reply["result"]["isError"].clone(), actions), ("Could not find the Send button on screen".to_string(), json!(true), json!([])));
}

#[tokio::test]
async fn a_target_with_no_browser_provider_tells_the_agent_to_click_by_position() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_click", json!({"target": "the Send button"})).await;

    assert!(text_of(&reply).contains("click with x and y instead"), "{reply}");
    assert_eq!(reply["result"]["isError"], true);
}

#[tokio::test]
async fn a_browser_task_picks_each_step_acts_by_number_and_ends_done() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (base, jev) = fake_jev().await;
    add_decision_provider(&fixture.server, &base, "OpenJev", json!(["browser"])).await;
    jev.scripted.lock().unwrap().extend([step("click", "2"), step("done", "1")]);
    let mut acted = Vec::new();

    let reply = tokio::select! {
        reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "browser_task", json!({"goal": "Search for flights"})) => reply,
        _ = flights_page(&mut fixture.link, &mut acted) => unreachable!(),
    };
    let environment = environment_of(&fixture).await;
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;
    let asked = jev.asked.lock().unwrap().last().cloned().unwrap();

    assert!(text_of(&reply).starts_with("Browser task done after 1 steps in "), "{reply}");
    assert_eq!(acted, [BrowserAction::Click { index: 2 }]);
    assert_eq!((environment["browser_task"]["state"].clone(), environment["browser_task"]["provider"].clone(), environment["browser_task"]["goal"].clone()), (json!("done"), json!("OpenJev"), json!("Search for flights")));
    assert_eq!(environment["browser_task"]["steps"][0]["label"], "Click: Search flights");
    assert!(environment["browser_task"]["result"].as_str().unwrap().contains("Flights"), "{environment}");
    assert!(asked["state"]["controls"].as_str().unwrap().contains("2 | button | Search flights"), "{asked}");
    assert!(asked["questions"]["target"]["criteria"]["2"].as_str().unwrap().contains("Search flights"), "{asked}");
    assert_eq!(actions[1]["label"], "Clicked “Search flights”");
}

#[tokio::test]
async fn a_browser_task_that_needs_the_person_ends_blocked_with_why() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (base, jev) = fake_jev().await;
    add_decision_provider(&fixture.server, &base, "OpenJev", json!(["browser"])).await;
    jev.scripted.lock().unwrap().push_back(step("blocked", "1"));
    let mut acted = Vec::new();

    let reply = tokio::select! {
        reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "browser_task", json!({"goal": "Search for flights"})) => reply,
        _ = flights_page(&mut fixture.link, &mut acted) => unreachable!(),
    };
    let environment = environment_of(&fixture).await;

    assert!(text_of(&reply).contains("Browser task blocked after 0 steps. Why: "), "{reply}");
    assert_eq!(environment["browser_task"]["state"], "blocked");
    assert!(environment["browser_task"]["why"].is_string(), "{environment}");
    assert!(acted.is_empty());
}

#[tokio::test]
async fn a_browser_task_with_no_browser_provider_points_the_agent_at_screenshots_and_clicks() {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "browser_task", json!({"goal": "Search for flights"})).await;

    assert!(text_of(&reply).contains("environment_screenshot and environment_click"), "{reply}");
    assert_eq!(reply["result"]["isError"], true);
}

async fn interrupt_a_browser_task(interruption: &str) -> (String, Value, mycelium_proto::api::Environment) {
    let mut fixture = fixture(Some(capable())).await;
    fixture.running().await;
    let (base, jev) = fake_jev().await;
    add_decision_provider(&fixture.server, &base, "OpenJev", json!(["browser"])).await;
    jev.scripted.lock().unwrap().extend((0..5).map(|_| step("wait", "1")));
    let mut stream = fixture.server.user.stream().await;
    let mut acted = Vec::new();
    let interrupt = fixture.url(interruption);
    let environment_url = fixture.url("");
    let server = &fixture.server;
    let token = fixture.token.clone();
    let conversation_id = fixture.conversation_id;

    let reply = tokio::select! {
        reply = async {
            let (reply, _) = tokio::join!(
                tool(server, &token, conversation_id, "browser_task", json!({"goal": "Search for flights"})),
                async {
                    common::eventually("the browser task running", || async {
                        let (_, environment) = server.user.json(server.user.get(&environment_url)).await;

                        (environment["browser_task"]["state"] == "running").then_some(())
                    })
                    .await;
                    server.user.json(server.user.post(&interrupt)).await;
                }
            );

            reply
        } => reply,
        _ = flights_page(&mut fixture.link, &mut acted) => unreachable!(),
    };
    let published = stream.until(|message| match message { StreamMessage::Environment(environment) if environment.browser_task.as_ref().is_some_and(|task| task.state == mycelium_proto::api::BrowserTaskState::Stopped) => Some(environment), _ => None }).await;

    (text_of(&reply), environment_of(&fixture).await, published)
}

#[tokio::test]
async fn taking_over_stops_a_browser_task_and_tells_the_agent_who_did() {
    let (reply, environment, published) = interrupt_a_browser_task("/take-over").await;

    assert!(reply.contains("Browser task stopped when") && reply.contains("took over, after "), "{reply}");
    assert_eq!((environment["browser_task"]["state"].clone(), environment["browser_task"]["stopped_by"].clone()), (json!("stopped"), json!("took_over")));
    assert_eq!(published.browser_task.unwrap().stopped_by.as_deref(), Some("took_over"));
}

#[tokio::test]
async fn the_stop_task_route_ends_a_browser_task_without_taking_over() {
    let (reply, environment, published) = interrupt_a_browser_task("/browser-task/stop").await;

    assert!(reply.contains("stopped it, after "), "{reply}");
    assert_eq!((environment["browser_task"]["stopped_by"].clone(), environment["controller"].clone()), (json!("stop"), json!("agent")));
    assert_eq!(published.browser_task.unwrap().stopped_by.as_deref(), Some("stop"));
}

async fn make_chat(fixture: &Fixture) {
    sqlx::query("update conversations set agent = 'chat' where id = $1").bind(fixture.conversation_id).execute(&fixture.server.pool).await.unwrap();
}

async fn second_capable_node(fixture: &Fixture, name: &str) -> NodeLink {
    let mut link = fixture.server.online_node(name).await;
    let id = link.id;

    link.send(NodeToControl::EnvironmentCapability { capability: capable() }).await;

    let user = fixture.server.user.clone();
    common::eventually("the second node's environment capability", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(id) && node["environment"]["available"] == json!(true)).then_some(())
    })
    .await;

    link
}

async fn stop_environment(fixture: &Fixture) {
    fixture.server.user.json(fixture.server.user.delete(&fixture.url(""))).await;
}

#[tokio::test]
async fn a_chat_starts_its_environment_on_the_only_node_that_can_run_one() {
    let mut fixture = fixture(Some(capable())).await;
    make_chat(&fixture).await;

    let (status, started) = fixture.start().await;

    assert_eq!((status, started["node_name"].clone()), (200, json!("laptop")), "{started}");
    assert!(matches!(fixture.link.next().await, ControlToNode::StartEnvironment { .. }));
}

#[tokio::test]
async fn a_chat_starts_on_the_node_used_last_and_an_explicit_node_wins() {
    let mut fixture = fixture(Some(capable())).await;
    let mut desktop = second_capable_node(&fixture, "desktop").await;
    make_chat(&fixture).await;

    let (_, first) = fixture.server.user.json(fixture.server.user.post(&fixture.url("")).json(&json!({"node_id": desktop.id}))).await;
    assert_eq!(first["node_name"], "desktop", "{first}");
    assert!(matches!(desktop.next().await, ControlToNode::StartEnvironment { .. }));
    stop_environment(&fixture).await;
    assert!(matches!(desktop.next().await, ControlToNode::StopEnvironment { .. }));

    let (_, again) = fixture.start().await;
    assert_eq!(again["node_name"], "desktop", "{again}");
    assert!(matches!(desktop.next().await, ControlToNode::StartEnvironment { .. }));
    stop_environment(&fixture).await;

    let (_, explicit) = fixture.server.user.json(fixture.server.user.post(&fixture.url("")).json(&json!({"node_id": fixture.link.id}))).await;
    assert_eq!(explicit["node_name"], "laptop", "{explicit}");
}

#[tokio::test]
async fn a_chat_with_no_node_that_can_run_an_environment_is_told_to_start_one() {
    let fixture = fixture(Some(incapable())).await;
    make_chat(&fixture).await;

    let (status, body) = fixture.start().await;

    assert_eq!((status, body["error"].clone(), body["message"].clone()), (409, json!("environment_no_node"), json!("No node that can run an environment is online. Start one from Nodes.")), "{body}");
}

#[tokio::test]
async fn the_agent_starting_an_environment_in_a_chat_is_recorded_as_the_agent() {
    let mut fixture = fixture(Some(capable())).await;
    make_chat(&fixture).await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_start", json!({})).await;
    let again = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_start", json!({})).await;
    let (_, actions) = fixture.server.user.json(fixture.server.user.get(&fixture.url("/actions"))).await;

    assert!(matches!(fixture.link.next().await, ControlToNode::StartEnvironment { .. }));
    assert_eq!((text_of(&reply).starts_with("Started the environment on laptop."), reply["result"]["isError"].clone()), (true, json!(false)), "{reply}");
    assert_eq!(text_of(&again), "The environment is already running on laptop.");
    assert_eq!(environment_lines(&fixture).await, ["The agent started the environment on laptop."]);
    assert_eq!(actions[0]["label"], "The agent started the environment");
}

#[tokio::test]
async fn the_agent_is_told_when_no_node_can_run_an_environment() {
    let fixture = fixture(Some(incapable())).await;
    make_chat(&fixture).await;

    let reply = tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_start", json!({})).await;

    assert_eq!((text_of(&reply), reply["result"]["isError"].clone()), ("No node that can run an environment is online. Start one from Nodes.".to_string(), json!(true)));
}

#[tokio::test]
async fn a_chats_screenshot_is_taken_on_the_node_the_environment_runs_on() {
    let fixture = fixture(Some(capable())).await;
    let mut desktop = second_capable_node(&fixture, "desktop").await;
    make_chat(&fixture).await;
    fixture.server.user.json(fixture.server.user.post(&fixture.url("")).json(&json!({"node_id": desktop.id}))).await;
    assert!(matches!(desktop.next().await, ControlToNode::StartEnvironment { .. }));
    desktop.send(NodeToControl::EnvironmentChanged { conversation_id: fixture.conversation_id, state: EnvironmentState::Running, failure: None, browser: None }).await;

    let user = fixture.server.user.clone();
    let url = fixture.url("");
    common::eventually("the environment running", || async {
        let (_, environment) = user.json(user.get(&url)).await;

        (environment["state"] == "running").then_some(())
    })
    .await;

    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG fake");
    let (_, reply) = tokio::join!(
        async {
            let ControlToNode::EnvironmentScreenshot { request_id, .. } = desktop.next().await else {
                panic!("expected the desktop node to be asked for a screenshot");
            };
            desktop.send(NodeToControl::EnvironmentScreenshotTaken { request_id, png: Some(png.clone()), error: None }).await;
        },
        tool(&fixture.server, &fixture.token, fixture.conversation_id, "environment_screenshot", json!({}))
    );

    assert_eq!(reply["result"]["content"][0], json!({"type": "image", "data": png, "mimeType": "image/png"}));
}
