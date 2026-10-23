mod common;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use common::{Client, Server, eventually, serve};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";

#[derive(Default)]
struct Forge {
    listings: AtomicUsize,
    failing: AtomicBool,
    repository: Mutex<String>,
}

type Fake = Arc<Forge>;

async fn fake_github() -> (String, Fake) {
    async fn repositories(State(forge): State<Fake>) -> Result<Json<Value>, StatusCode> {
        forge.listings.fetch_add(1, Ordering::SeqCst);

        if forge.failing.load(Ordering::SeqCst) {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        Ok(Json(json!([{"full_name": forge.repository.lock().unwrap().clone(), "private": false}])))
    }

    async fn create() -> StatusCode {
        StatusCode::CREATED
    }

    let forge: Fake = Arc::new(Forge { repository: Mutex::new("samexample/before".into()), ..Forge::default() });
    let router = Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/user/repos", get(repositories).post(create))
        .route("/api/v3/orgs/{org}/repos", post(create))
        .with_state(forge.clone());

    (serve(router).await, forge)
}

async fn connect(client: &Client, base: &str) -> Uuid {
    let (status, forge) = client.json(client.post("/api/v1/forges").json(&json!({"kind": "github", "base_url": base, "token": TOKEN}))).await;
    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
}

async fn listed(client: &Client, query: &str) -> Vec<String> {
    let (status, body) = client.json(client.get(&format!("/api/v1/repositories{query}"))).await;
    assert_eq!(status, 200, "{body}");

    body["repositories"].as_array().unwrap().iter().map(|repository| repository["full_name"].as_str().unwrap().to_string()).collect()
}

fn rename(forge: &Fake, name: &str) {
    *forge.repository.lock().unwrap() = name.to_string();
}

fn listings(forge: &Fake) -> usize {
    forge.listings.load(Ordering::SeqCst)
}

#[tokio::test]
async fn a_second_identical_read_within_the_lifetime_does_not_reach_the_forge() {
    let server = Server::start().await;
    let (base, forge) = fake_github().await;
    connect(&server.user, &base).await;

    assert_eq!(listed(&server.user, "").await, vec!["samexample/before"]);
    rename(&forge, "samexample/after");

    assert_eq!(listed(&server.user, "").await, vec!["samexample/before"]);
    assert_eq!(listings(&forge), 1);
}

#[tokio::test]
async fn an_expired_entry_is_served_at_once_and_refreshed_for_the_next_read() {
    let server = Server::start_with(|config| config.forge_cache_ttl = Duration::ZERO).await;
    let (base, forge) = fake_github().await;
    connect(&server.user, &base).await;

    listed(&server.user, "").await;
    rename(&forge, "samexample/after");

    assert_eq!(listed(&server.user, "").await, vec!["samexample/before"], "the stale list is returned without waiting for the forge");

    eventually("the background refresh to land", || async { (listed(&server.user, "").await == vec!["samexample/after"]).then_some(()) }).await;
}

#[tokio::test]
async fn a_refresh_request_bypasses_the_cache_and_replaces_the_entry() {
    let server = Server::start().await;
    let (base, forge) = fake_github().await;
    connect(&server.user, &base).await;

    listed(&server.user, "").await;
    rename(&forge, "samexample/after");

    assert_eq!(listed(&server.user, "?refresh=true").await, vec!["samexample/after"]);
    assert_eq!(listed(&server.user, "").await, vec!["samexample/after"]);
    assert_eq!(listings(&forge), 2);
}

#[tokio::test]
async fn creating_a_repository_on_the_forge_drops_what_was_cached_for_it() {
    let server = Server::start().await;
    let (base, forge) = fake_github().await;
    let forge_id = connect(&server.user, &base).await;
    let mut link = server.online_node("laptop").await;

    listed(&server.user, "").await;
    rename(&forge, "samexample/after");

    let user = server.user.clone();
    let body = json!({"node_id": link.id, "forge_id": forge_id, "repository": "samexample/new", "new_repository": true});
    let creating = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });
    let (request_id, ..) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": "/p/new"}), error: None }).await;
    let (status, project) = creating.await.unwrap();
    assert_eq!(status, 201, "{project}");

    assert_eq!(listed(&server.user, "").await, vec!["samexample/after"]);
}

#[tokio::test]
async fn two_users_never_share_cached_replies() {
    let server = Server::start().await;
    let (base, forge) = fake_github().await;
    let other = server.second_user("someone@example.com").await;
    connect(&server.user, &base).await;
    connect(&other, &base).await;

    listed(&server.user, "").await;
    listed(&other, "").await;

    assert_eq!(listings(&forge), 2);
}

#[tokio::test]
async fn an_error_reply_is_not_cached() {
    let server = Server::start().await;
    let (base, forge) = fake_github().await;
    connect(&server.user, &base).await;
    forge.failing.store(true, Ordering::SeqCst);

    assert!(listed(&server.user, "").await.is_empty());

    forge.failing.store(false, Ordering::SeqCst);

    assert_eq!(listed(&server.user, "").await, vec!["samexample/before"]);
    assert_eq!(listings(&forge), 2);
}
