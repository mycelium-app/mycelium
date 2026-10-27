use crate::common;
use crate::common::Server;
use serde_json::{Value, json};

async fn history(server: &Server, query: &str) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/usage/history{query}"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

#[tokio::test]
async fn orchestrator_readings_and_imported_conversation_turns_backfill_windows_and_tokens_per_point() {
    use mycelium_proto::conversation::AgentId;
    use mycelium_proto::link::NodeToControl;
    use mycelium_proto::usage_history::OrchestratorReading;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Old work")).await;
    sqlx::query("update conversations set account = 'me' where id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    let at = |at: i64, five: f64| OrchestratorReading { at, five, seven: five / 4.0, five_resets: Some(1_700_010_000), seven_resets: Some(1_700_400_000) };
    link.send(NodeToControl::OrchestratorUsage { agent: AgentId::Claude, account: Some("me".into()), fine: vec![], coarse: vec![at(1_700_000_000, 10.0), at(1_700_004_000, 50.0)] }).await;
    sqlx::query(
        "insert into usage_turns (conversation_id, owner, node_id, agent, ended_at, day, input_tokens, output_tokens)
         select id, owner, node_id, 'claude', to_timestamp(1700002000), date '2023-11-14', 3000, 1000 from conversations where id = $1",
    )
    .bind(conversation.id)
    .execute(&server.pool)
    .await
    .unwrap();

    let account = common::eventually("the imported readings to appear as a window", || async {
        let body = history(&server, "?range=all&account=claude:me").await;
        let account = body["accounts"][0].clone();

        account["windows"].as_array().is_some_and(|windows| !windows.is_empty()).then_some(account)
    })
    .await;

    assert_eq!(account["windows"][0]["peak_percent"], 50.0);
    assert_eq!(account["windows"][0]["tokens"], 4000);
    assert_eq!(account["tokens_per_point"]["five_hour"], 100.0);
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
