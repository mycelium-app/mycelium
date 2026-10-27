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

#[tokio::test]
async fn a_node_that_finds_orchestrator_usage_imports_it_and_later_readings_keep_it() {
    use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState, UsageWindow};
    use mycelium_proto::link::NodeToControl;
    use mycelium_proto::usage_history::OrchestratorReading;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let old = |at: i64, five: f64| OrchestratorReading { at, five, seven: five / 4.0, five_resets: Some(1_790_000_000), seven_resets: None };
    link.send(NodeToControl::OrchestratorUsage { agent: AgentId::Claude, account: Some("me".into()), fine: vec![old(1_700_000_000, 10.0)], coarse: vec![old(1_700_003_000, 40.0)] }).await;

    let fresh = LoginInfo {
        agent: AgentId::Claude,
        account: Some("me".into()),
        method: "token".into(),
        state: LoginState::SignedIn,
        usage_left: None,
        windows: vec![UsageWindow { name: "five_hour".into(), used_percent: 20.0, resets_at: None, rate_per_hour: None, runs_out_at: None }],
        usage_read_at: Some(chrono::Utc::now()),
        reset_credits: None,
    };
    link.send(NodeToControl::Logins { logins: vec![fresh] }).await;

    let account = common::eventually("the import and the fresh reading to be stored", || async {
        let body = history(&server, "?range=all&account=claude:me").await;
        let account = body["accounts"][0].clone();
        let has_both = account["orchestrator_until"].is_string() && account["readings"].as_array().is_some_and(|readings| readings.len() >= 3);

        has_both.then_some(account)
    })
    .await;

    assert_eq!(account["windows"][0]["peak_percent"], 40.0);
}
