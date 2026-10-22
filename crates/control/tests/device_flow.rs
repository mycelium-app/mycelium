mod common;

use common::Server;
use serde_json::{Value, json};

async fn start_device(server: &Server, kind: &str, label: &str) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/device").json(&json!({"kind": kind, "label": label, "hostname": "box", "os": "linux", "arch": "x86_64"}))).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn poll(server: &Server, started: &Value) -> (u16, Value) {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/device/token").json(&json!({"device_code": started["device_code"]}))).await
}

#[tokio::test]
async fn a_node_joins_through_the_device_code_flow_and_the_credential_works_once() {
    let server = Server::start().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();

    let bytes = user_code.as_bytes();
    assert_eq!(bytes.len(), 9);
    assert_eq!(bytes[4], b'-');
    assert!(started["verification_uri_complete"].as_str().unwrap().ends_with(&format!("/approve?code={user_code}")));
    assert_eq!(started["expires_in"], 600);

    let (status, pending) = poll(&server, &started).await;
    assert_eq!((status, pending["error"].as_str()), (400, Some("authorization_pending")));

    let (status, request) = server.user.json(server.user.get(&format!("/api/v1/device/{user_code}"))).await;
    assert_eq!(status, 200);
    assert_eq!(request["label"], "laptop");
    assert_eq!(request["kind"], "node");

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/device/{}/approve", user_code.to_lowercase()))).await;
    assert_eq!(status, 204);

    let (status, token) = poll(&server, &started).await;
    assert_eq!(status, 200);
    assert_eq!(token["kind"], "node");
    assert!(token["token"].as_str().unwrap().starts_with("myc_node_"));

    let (status, again) = poll(&server, &started).await;
    assert_eq!((status, again["error"].as_str()), (400, Some("expired_token")));

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let node = &nodes.as_array().unwrap()[0];
    assert_eq!(node["name"], "laptop");
    assert_eq!(node["online"], false);

    let id = token["id"].as_str().unwrap().parse().unwrap();
    let mut link = server.connect_node(id, token["token"].as_str().unwrap()).await;
    link.hello(vec![]).await;
}

#[tokio::test]
async fn approving_needs_a_signed_in_person_and_a_code_can_only_be_approved_once() {
    let server = Server::start().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();
    let anonymous = server.anonymous();

    let (status, body) = anonymous.json(anonymous.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    assert_eq!(status, 401);
    assert_eq!(body["error"], "unauthorised");

    let (first, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    let (second, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;

    assert_eq!((first, second), (204, 404));
}

#[tokio::test]
async fn a_denied_code_answers_access_denied() {
    let server = Server::start().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/deny"))).await;
    assert_eq!(status, 204);

    let (status, body) = poll(&server, &started).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("access_denied")));
}

#[tokio::test]
async fn an_expired_code_cannot_be_approved_or_exchanged() {
    let server = Server::start().await;
    let started = start_device(&server, "node", "laptop").await;
    let user_code = started["user_code"].as_str().unwrap();

    sqlx::query("update device_codes set expires_at = now() - interval '1 minute'").execute(&server.pool).await.unwrap();

    let (status, _) = server.user.json(server.user.get(&format!("/api/v1/device/{user_code}"))).await;
    assert_eq!(status, 404);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
    assert_eq!(status, 404);

    let (status, body) = poll(&server, &started).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("expired_token")));
}

#[tokio::test]
async fn an_unknown_device_code_is_reported_as_expired() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, body) = anonymous.json(anonymous.post("/api/v1/device/token").json(&json!({"device_code": "nope"}))).await;

    assert_eq!((status, body["error"].as_str()), (400, Some("expired_token")));
}

#[tokio::test]
async fn a_desktop_client_gets_a_client_token_that_authenticates_as_the_approver() {
    let server = Server::start().await;
    let (id, token) = server.join("client", "desktop").await;
    assert!(token.starts_with("myc_client_"));

    let anonymous = server.anonymous();
    let (status, me) = anonymous.json(anonymous.get("/api/v1/me").header("authorization", format!("Bearer {token}"))).await;

    assert_eq!(status, 200);
    assert_eq!(me["email"], "dev@example.com");

    let (_, own) = server.user.json(server.user.get("/api/v1/me")).await;
    assert_eq!(me["id"], own["id"]);

    let stored = sqlx::query_scalar::<_, String>("select label from client_tokens where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(stored, "desktop");
}

#[tokio::test]
async fn tokens_are_stored_only_as_hashes() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "laptop").await;

    let stored = sqlx::query_scalar::<_, Vec<u8>>("select token_hash from nodes where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();

    assert_eq!(stored.len(), 32);
    assert!(!String::from_utf8_lossy(&stored).contains(token.trim_start_matches("myc_node_")));
    assert_eq!(stored, common::hash(token.trim_start_matches("myc_node_")));
}

#[tokio::test]
async fn nodes_joining_with_the_same_label_get_distinct_names() {
    let server = Server::start().await;

    server.join("node", "laptop").await;
    server.join("node", "laptop").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let names: Vec<&str> = nodes.as_array().unwrap().iter().map(|node| node["name"].as_str().unwrap()).collect();

    assert_eq!(names, vec!["laptop", "laptop-2"]);
}

#[tokio::test]
async fn a_device_request_without_any_name_is_rejected_with_a_fix() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, body) = anonymous.json(anonymous.post("/api/v1/device").json(&json!({"kind": "node", "label": " ", "hostname": "", "os": "linux", "arch": "x86_64"}))).await;

    assert_eq!(status, 400);
    assert_eq!(body["message"], "Enter a name for this device.");
}
