mod common;

use common::Server;
use serde_json::{Value, json};

async fn history(server: &Server, query: &str) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/usage/history{query}"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

#[tokio::test]
async fn imported_orchestrator_readings_become_windows_and_are_marked_as_from_orchestrator() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let reading = |at: i64, five: f64| json!({"at": at, "five": five, "seven": five / 4.0, "five_resets": 1_790_000_000, "seven_resets": 1_790_400_000});
    let (status, imported) = server
        .user
        .json(server.user.post("/api/v1/usage/history/import").json(&json!({"agent": "claude", "account": "me", "node_id": link.id, "coarse": [reading(1_789_990_000, 10.0), reading(1_789_995_000, 40.0)]})))
        .await;

    assert_eq!(status, 200, "{imported}");
    assert_eq!(imported["imported"], 2);
    let body = history(&server, "?range=all&account=claude:me").await;
    let account = &body["accounts"][0];
    assert_eq!(account["windows"].as_array().unwrap().len(), 1);
    assert_eq!(account["windows"][0]["peak_percent"], 40.0);
    assert!(account["orchestrator_until"].is_string());
}

#[tokio::test]
async fn an_unknown_range_is_refused() {
    let server = Server::start().await;
    let (status, _) = server.user.json(server.user.get("/api/v1/usage/history?range=forever")).await;

    assert_eq!(status, 400);
}
