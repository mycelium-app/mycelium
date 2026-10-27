use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use crate::common::{Server, eventually, serve};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Published {
    release: Mutex<String>,
    claude: Mutex<String>,
    failing: AtomicBool,
    requests: AtomicUsize,
}

type Shared = Arc<Published>;

async fn release(State(published): State<Shared>) -> impl IntoResponse {
    published.requests.fetch_add(1, Ordering::SeqCst);

    if published.failing.load(Ordering::SeqCst) {
        return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
    }

    (StatusCode::OK, format!("{}\n", published.release.lock().unwrap()))
}

async fn claude(State(published): State<Shared>) -> impl IntoResponse {
    if published.failing.load(Ordering::SeqCst) {
        return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
    }

    (StatusCode::OK, json!({"name": "@agentclientprotocol/claude-agent-acp", "version": *published.claude.lock().unwrap()}).to_string())
}

async fn publisher(release_tag: &str, claude_version: &str) -> (Shared, String) {
    let published = Arc::new(Published { release: Mutex::new(release_tag.into()), claude: Mutex::new(claude_version.into()), failing: AtomicBool::new(false), requests: AtomicUsize::new(0) });
    let router = Router::new().route("/mycelium/latest", get(release)).route("/registry/@agentclientprotocol/claude-agent-acp/latest", get(claude)).with_state(published.clone());

    (published, serve(router).await)
}

async fn server_checking(base: &str, interval: Duration) -> Server {
    let base = base.to_string();

    Server::start_with(move |config| {
        config.release_url = format!("{base}/mycelium/latest");
        config.npm_registry_url = format!("{base}/registry");
        config.version_check_interval = interval;
    })
    .await
}

async fn first_node(server: &Server) -> Value {
    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

    nodes[0].clone()
}

#[tokio::test]
async fn a_node_and_an_agent_behind_the_latest_release_are_listed_with_the_newer_version() {
    let (_, base) = publisher("v0.1.16", "2.2.0").await;
    let server = server_checking(&base, Duration::from_secs(3600)).await;
    server.online_node("laptop").await;

    let node = eventually("the newer versions", || async {
        let node = first_node(&server).await;

        node["update_available"].is_string().then_some(node)
    })
    .await;

    assert_eq!(node["update_available"], "v0.1.16");
    assert_eq!(node["agents"][0]["update_available"], "2.2.0");
}

#[tokio::test]
async fn a_node_and_an_agent_that_are_not_behind_list_no_newer_version() {
    let (published, base) = publisher("v0.1.0", "2.1.308").await;
    let server = server_checking(&base, Duration::from_secs(3600)).await;
    server.online_node("laptop").await;

    eventually("the release check", || async { (published.requests.load(Ordering::SeqCst) > 0).then_some(()) }).await;
    let node = first_node(&server).await;

    assert!(node.get("update_available").is_none());
    assert!(node["agents"][0].get("update_available").is_none());
}

#[tokio::test]
async fn a_failing_version_source_keeps_the_last_known_version_and_the_nodes_still_list() {
    let (published, base) = publisher("v0.1.16", "2.2.0").await;
    let server = server_checking(&base, Duration::from_millis(50)).await;
    server.online_node("laptop").await;
    eventually("the newer versions", || async { first_node(&server).await["update_available"].is_string().then_some(()) }).await;

    published.failing.store(true, Ordering::SeqCst);
    let before = published.requests.load(Ordering::SeqCst);
    eventually("a failed check", || async { (published.requests.load(Ordering::SeqCst) > before + 1).then_some(()) }).await;

    let (status, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

    assert_eq!(status, 200);
    assert_eq!(nodes[0]["update_available"], "v0.1.16");
    assert_eq!(nodes[0]["agents"][0]["update_available"], "2.2.0");
}

#[tokio::test]
async fn a_newer_release_published_later_is_streamed_to_clients() {
    let (published, base) = publisher("v0.1.0", "2.1.308").await;
    let server = server_checking(&base, Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    *published.release.lock().unwrap() = "v0.2.0".into();

    let node = stream
        .until(|message| match message {
            mycelium_proto::stream::StreamMessage::Node(node) if node.id == link.id && node.update_available.is_some() => Some(node),
            _ => None,
        })
        .await;

    assert_eq!(node.update_available.as_deref(), Some("v0.2.0"));
}
