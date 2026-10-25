mod common;

use common::{Client, Server, eventually};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};

const CHROME: &str = "/usr/bin/google-chrome";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

struct Browser {
    url: String,
    _process: Child,
    _profile: TempDir,
}

async fn browser() -> Option<Browser> {
    if !Path::new(CHROME).exists() {
        eprintln!("SKIPPED: {CHROME} is not installed, so there is no browser to take snapshots with.");

        return None;
    }

    let profile = TempDir::new().unwrap();
    let mut process = Command::new(CHROME)
        .args(["--headless=new", "--remote-debugging-port=0", "--no-sandbox", "--disable-gpu", "--no-first-run", "about:blank"])
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(process.stderr.take().unwrap()).lines();
    let url = loop {
        let line = lines.next_line().await.unwrap().expect("the browser exited before it listened");

        if let Some((_, url)) = line.split_once("DevTools listening on ") {
            break url.trim().to_string();
        }
    };

    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

    Some(Browser { url, _process: process, _profile: profile })
}

async fn created(client: &Client, kind: &str, content: &str) -> Value {
    let (status, artifact) = client.json(client.post("/api/v1/artifacts").json(&json!({"kind": kind, "title": "Snapshot", "content": content}))).await;
    assert_eq!(status, 201, "{artifact}");

    artifact
}

async fn thumbnail_version(client: &Client, id: &str) -> Value {
    let (_, artifact) = client.json(client.get(&format!("/api/v1/artifacts/{id}"))).await;

    artifact["thumbnail_version"].clone()
}

async fn snapshotted(client: &Client, id: &str, version: i64) {
    eventually("the snapshot", || async { (thumbnail_version(client, id).await == json!(version)).then_some(()) }).await;
}

#[tokio::test]
async fn an_artifact_without_a_snapshot_has_no_thumbnail() {
    let server = Server::start().await;
    let artifact = created(&server.user, "page", "<h1>Hello</h1>").await;
    let id = artifact["id"].as_str().unwrap();
    let response = server.user.get(&format!("/api/v1/artifacts/{id}/thumbnail")).send().await.unwrap();

    assert_eq!(artifact["thumbnail_version"], Value::Null);
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn nothing_is_attempted_while_no_browser_is_configured() {
    let server = Server::start().await;
    let artifact = created(&server.user, "page", "<h1>Hello</h1>").await;
    let id = artifact["id"].as_str().unwrap();
    let versions: i64 = sqlx::query_scalar("select count(*) from artifact_versions where thumbnail_sha256 is not null").fetch_one(&server.pool).await.unwrap();

    assert_eq!(thumbnail_version(&server.user, id).await, Value::Null);
    assert_eq!(versions, 0);
}

#[tokio::test]
async fn an_unreachable_browser_leaves_the_artifact_working_without_a_snapshot() {
    let server = Server::start_with(|config| config.browser_url = Some("http://127.0.0.1:1".into())).await;
    let artifact = created(&server.user, "page", "<h1>Hello</h1>").await;
    let id = artifact["id"].as_str().unwrap();
    let (status, _) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;

    assert_eq!(status, 200);
    assert_eq!(thumbnail_version(&server.user, id).await, Value::Null);
}

#[tokio::test]
async fn a_page_gets_a_png_snapshot() {
    let Some(browser) = browser().await else {
        return;
    };
    let server = Server::start_with(|config| config.browser_url = Some(browser.url.clone())).await;
    let artifact = created(&server.user, "page", "<html><body style=\"background:#123456\"><h1>Hello</h1></body></html>").await;
    let id = artifact["id"].as_str().unwrap();

    snapshotted(&server.user, id, 1).await;

    let response = server.user.get(&format!("/api/v1/artifacts/{id}/thumbnail")).send().await.unwrap();
    let media_type = response.headers()["content-type"].to_str().unwrap().to_string();
    let bytes = response.bytes().await.unwrap();

    assert_eq!(media_type, "image/png");
    assert_eq!(bytes[..8], PNG_SIGNATURE);
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 2560);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 1600);
}

#[tokio::test]
async fn a_design_gets_a_snapshot_of_its_first_artboard() {
    let Some(browser) = browser().await else {
        return;
    };
    let server = Server::start_with(|config| config.browser_url = Some(browser.url.clone())).await;
    let design = json!({"pages": [{"id": "p1", "name": "Page 1", "items": [{"id": "a1", "type": "artboard", "name": "Home", "x": 0, "y": 0, "w": 400, "h": 300, "html": "<h1>Home</h1>"}]}]}).to_string();
    let artifact = created(&server.user, "design", &design).await;
    let id = artifact["id"].as_str().unwrap();

    snapshotted(&server.user, id, 1).await;

    let bytes = server.user.get(&format!("/api/v1/artifacts/{id}/thumbnail")).send().await.unwrap().bytes().await.unwrap();

    assert_eq!(bytes[..8], PNG_SIGNATURE);
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 800);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 600);
}

#[tokio::test]
async fn a_new_version_moves_the_thumbnail_version_on() {
    let Some(browser) = browser().await else {
        return;
    };
    let server = Server::start_with(|config| config.browser_url = Some(browser.url.clone())).await;
    let artifact = created(&server.user, "page", "<h1>One</h1>").await;
    let id = artifact["id"].as_str().unwrap();

    snapshotted(&server.user, id, 1).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/versions")).json(&json!({"content": "<h1>Two</h1>"}))).await;
    assert_eq!(status, 201);

    snapshotted(&server.user, id, 2).await;
}

#[tokio::test]
async fn a_page_cannot_reach_the_network_while_it_is_snapshotted() {
    let Some(browser) = browser().await else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    let counted = connections.clone();

    tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            counted.fetch_add(1, Ordering::SeqCst);
        }
    });

    let server = Server::start_with(|config| config.browser_url = Some(browser.url.clone())).await;
    let html = format!("<html><body><img src=\"http://127.0.0.1:{port}/pixel.png\"><script>fetch('http://127.0.0.1:{port}/data')</script></body></html>");
    let artifact = created(&server.user, "page", &html).await;
    let id = artifact["id"].as_str().unwrap();

    snapshotted(&server.user, id, 1).await;

    assert_eq!(connections.load(Ordering::SeqCst), 0);
}
