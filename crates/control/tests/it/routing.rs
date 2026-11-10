use crate::common;
use crate::common::{NodeLink, Server};
use crate::continue_elsewhere::{expect_prompt, limit_message, next_command, node_with_accounts, with_usage};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn create_routed(server: &Server, node_id: Uuid, routing: &str) -> Value {
    let (status, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "cwd": "/work/app", "prompt": "Build it", "routing": routing}))).await;
    assert_eq!(status, 201, "{created}");

    created
}

async fn started_on(link: &mut NodeLink) -> Option<String> {
    match next_command(link).await {
        ControlToNode::StartSession { account, .. } => account,
        other => panic!("expected start_session, got {other:?}"),
    }
}

async fn running(server: &Server, link: &mut NodeLink, routing: &str) -> Uuid {
    let created = create_routed(server, link.id, routing).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

    next_command(link).await;
    link.start_session(id, "acp-1").await;
    expect_prompt(link).await;

    id
}

async fn finish_turn(server: &Server, link: &mut NodeLink, id: Uuid, seq: i64) {
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq, stop_reason: "end_turn".into(), usage: None }).await;
    common::eventually("the turn to end", || async { (server.conversation(id).await["status"] == "review").then_some(()) }).await;
}

async fn save_any_model(server: &Server, entries: Value) {
    let (status, body) = server.user.json(server.user.put("/api/v1/agent-settings").json(&json!({"default_modes": {}, "any_model": entries}))).await;
    assert_eq!(status, 200, "{body}");
}

async fn report(server: &Server, link: &mut NodeLink, accounts: Vec<mycelium_proto::conversation::LoginInfo>, account: &str, left: i64) {
    link.send(NodeToControl::Logins { logins: accounts }).await;

    let user = server.user.clone();
    common::eventually("the usage to be recorded", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;
        let found = nodes[0]["logins"].as_array().unwrap().iter().any(|login| login["account"] == account && login["usage_left"] == json!(left));

        found.then_some(())
    })
    .await;
}

#[tokio::test]
async fn claude_code_any_account_starts_on_the_account_with_most_usage_left() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 20), with_usage("work@example.org", 70)]).await;

    let created = create_routed(&server, link.id, "claude_any").await;

    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
    assert_eq!((created["account"].clone(), created["routing"].clone()), (json!("work@example.org"), json!("claude_any")));
}

#[tokio::test]
async fn starting_on_any_account_with_nobody_signed_in_says_how_to_fix_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "routing": "claude_any"}))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("no_routing_choice")));
    assert!(body["message"].as_str().unwrap().starts_with("No Claude Code account is signed in on this node."), "{body}");
}

#[tokio::test]
async fn a_usage_limit_moves_a_routed_conversation_to_the_next_best_account_and_records_why() {
    let server = Server::start().await;
    server.user.json(server.user.put("/api/v1/usage-limit").json(&json!({"on_usage_limit": "ask"}))).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90), with_usage("work@example.org", 40), with_usage("spare@example.org", 10)]).await;
    let id = running(&server, &mut link, "claude_any").await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: limit_message() }).await;

    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
    assert_eq!(expect_prompt(&mut link).await, "The usage limit was reached on sam@example.com. Continue from where you stopped.");

    let events = server.events(id).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").expect("a continued event")["payload"].clone();
    assert_eq!((continued["reason"].clone(), continued["window"].clone(), continued["account"].clone()), (json!("usage_limit"), json!("five_hour"), json!("work@example.org")));
    assert_eq!((continued["from"]["agent"].clone(), continued["from"]["account"].clone()), (json!("claude"), json!("sam@example.com")));
    assert_eq!(server.conversation(id).await["routing"], "claude_any");
}

#[tokio::test]
async fn an_account_that_signs_out_is_left_at_the_next_turn() {
    let server = Server::start().await;
    let accounts = vec![with_usage("sam@example.com", 90), with_usage("work@example.org", 40)];
    let mut link = node_with_accounts(&server, "v0.11.0", accounts).await;
    let id = running(&server, &mut link, "claude_any").await;
    finish_turn(&server, &mut link, id, 2).await;
    let signed_out = mycelium_proto::conversation::LoginInfo { managed: false, state: mycelium_proto::conversation::LoginState::SignedOut, ..with_usage("sam@example.com", 90) };
    link.send(NodeToControl::Logins { logins: vec![signed_out, with_usage("work@example.org", 40)] }).await;
    common::eventually("sam to be signed out", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
        let signed_out = nodes[0]["logins"].as_array().unwrap().iter().any(|login| login["account"] == "sam@example.com" && login["state"] == "signed_out");

        signed_out.then_some(())
    })
    .await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "More"}))).await;

    assert_eq!(status, 204);
    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
    let events = server.events(id).await;
    let reason = events.iter().rev().find(|event| event["kind"] == "continued").unwrap()["payload"]["reason"].clone();
    assert_eq!(reason, "signed_out");
}

#[tokio::test]
async fn any_model_moves_down_the_list_at_a_limit_and_stays_there_when_usage_returns() {
    let server = Server::start().await;
    save_any_model(&server, json!([{"agent": "claude", "account": "sam@example.com"}, {"agent": "claude"}])).await;
    let accounts = vec![with_usage("sam@example.com", 60), with_usage("work@example.org", 50)];
    let mut link = node_with_accounts(&server, "v0.11.0", accounts).await;
    let created = create_routed(&server, link.id, "any_model").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert_eq!(started_on(&mut link).await.as_deref(), Some("sam@example.com"));
    link.start_session(id, "acp-1").await;
    expect_prompt(&mut link).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: limit_message() }).await;

    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
    expect_prompt(&mut link).await;
    finish_turn(&server, &mut link, id, 4).await;
    report(&server, &mut link, vec![with_usage("sam@example.com", 80), with_usage("work@example.org", 50)], "sam@example.com", 80).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "More"}))).await;

    assert!(matches!(next_command(&mut link).await, ControlToNode::Prompt { .. }));
    let events = server.events(id).await;
    let moves: Vec<Value> = events.iter().filter(|event| event["kind"] == "continued").map(|event| event["payload"]["reason"].clone()).collect();
    assert_eq!(moves, vec![json!("usage_limit")]);
}

#[tokio::test]
async fn any_model_at_a_limit_moves_to_the_first_favourite_with_usage_even_one_above() {
    let server = Server::start().await;
    save_any_model(&server, json!([{"agent": "claude", "account": "sam@example.com"}, {"agent": "claude", "account": "work@example.org"}, {"agent": "claude", "account": "spare@example.net"}])).await;
    let accounts = vec![with_usage("sam@example.com", 0), with_usage("work@example.org", 50), with_usage("spare@example.net", 90)];
    let mut link = node_with_accounts(&server, "v0.11.0", accounts).await;
    let created = create_routed(&server, link.id, "any_model").await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
    link.start_session(id, "acp-1").await;
    expect_prompt(&mut link).await;
    report(&server, &mut link, vec![with_usage("sam@example.com", 80), with_usage("work@example.org", 50), with_usage("spare@example.net", 90)], "sam@example.com", 80).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: limit_message() }).await;

    assert!(matches!(next_command(&mut link).await, ControlToNode::CloseSession { .. }));
    assert_eq!(started_on(&mut link).await.as_deref(), Some("sam@example.com"));
}

async fn running_on_codex_before_claude(server: &Server, claude: mycelium_proto::conversation::LoginInfo) -> (NodeLink, Uuid) {
    save_any_model(server, json!([{"agent": "codex"}, {"agent": "claude"}])).await;
    let codex = mycelium_proto::conversation::LoginInfo { agent: mycelium_proto::conversation::AgentId::Codex, ..with_usage("alex@example.com", 70) };
    let mut link = node_with_accounts(server, "v0.11.0", vec![codex, claude]).await;
    let (status, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "codex", "cwd": "/work/app", "prompt": "Build it", "routing": "any_model"}))).await;
    assert_eq!(status, 201, "{created}");
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert_eq!(started_on(&mut link).await.as_deref(), Some("alex@example.com"));
    link.start_session(id, "codex-1").await;
    expect_prompt(&mut link).await;

    (link, id)
}

async fn expect_carried_on_with_claude(server: &Server, link: &mut NodeLink, id: Uuid) {
    assert!(matches!(next_command(link).await, ControlToNode::CloseSession { .. }));
    assert_eq!(started_on(link).await.as_deref(), Some("work@example.org"));
    link.start_session(id, "acp-2").await;
    assert!(expect_prompt(link).await.ends_with("The usage limit was reached on alex@example.com. Continue from where you stopped."));
    assert_eq!(server.conversation(id).await["status"], "working");
}

#[tokio::test]
async fn any_model_carries_on_the_interrupted_turn_on_claude_when_the_codex_workspace_runs_out_of_credits() {
    let server = Server::start().await;
    let (mut link, id) = running_on_codex_before_claude(&server, with_usage("work@example.org", 60)).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "Your workspace is out of credits. Add credits to keep going.".into() }).await;

    expect_carried_on_with_claude(&server, &mut link, id).await;
}

#[tokio::test]
async fn exhausted_credits_with_every_other_account_at_its_limit_resume_when_one_resets() {
    let server = Server::start_with(|config| config.scheduler_tick = std::time::Duration::from_millis(50)).await;
    let resets = chrono::Utc::now() + chrono::Duration::hours(2);
    let spent = mycelium_proto::conversation::UsageWindow { name: "five_hour".into(), used_percent: 100.0, resets_at: Some(resets), rate_per_hour: None, runs_out_at: None };
    let claude = mycelium_proto::conversation::LoginInfo { windows: vec![spent], ..with_usage("work@example.org", 0) };
    let (mut link, id) = running_on_codex_before_claude(&server, claude).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: "Your workspace is out of credits. Add credits to keep going.".into() }).await;

    let resume_at = common::eventually("a resume to be scheduled", || async { server.conversation(id).await["resume_at"].as_str().map(|at| at.parse::<chrono::DateTime<chrono::Utc>>().unwrap()) }).await;
    assert_eq!(server.conversation(id).await["status"], "failed");
    assert!(resume_at > resets && resume_at <= resets + chrono::Duration::minutes(2), "{resume_at} against {resets}");

    let codex = mycelium_proto::conversation::LoginInfo { agent: mycelium_proto::conversation::AgentId::Codex, ..with_usage("alex@example.com", 0) };
    report(&server, &mut link, vec![codex, with_usage("work@example.org", 80)], "work@example.org", 80).await;
    sqlx::query("update conversations set resume_at = now() - interval '1 second' where id = $1").bind(id).execute(&server.pool).await.unwrap();

    common::next_matching(&mut link, |message| matches!(message, ControlToNode::StartSession { account: Some(account), .. } if account == "work@example.org").then_some(())).await;
    link.start_session(id, "acp-2").await;
    let carried_on = common::next_matching(&mut link, |message| match message {
        ControlToNode::Prompt { text, .. } => Some(text.clone()),
        _ => None,
    })
    .await;
    assert!(carried_on.ends_with("Continue from where you stopped."), "{carried_on}");
}

#[tokio::test]
async fn continuing_on_a_named_account_pins_the_conversation() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90), with_usage("work@example.org", 40)]).await;
    let id = running(&server, &mut link, "claude_any").await;
    finish_turn(&server, &mut link, id, 2).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/continue")).json(&json!({"agent": "claude", "account": "work@example.org"}))).await;

    assert_eq!(status, 200, "{body}");
    assert!(body["routing"].is_null(), "{body}");
    assert!(server.conversation(id).await["routing"].is_null());
}

#[tokio::test]
async fn choosing_a_routing_account_on_a_started_conversation_moves_it_and_keeps_routing() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 20), with_usage("work@example.org", 80)]).await;
    let created = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": "sam@example.com", "prompt": "Build it"}))).await.1;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    next_command(&mut link).await;
    link.start_session(id, "acp-1").await;
    expect_prompt(&mut link).await;
    finish_turn(&server, &mut link, id, 2).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/continue")).json(&json!({"agent": "claude", "routing": "claude_any"}))).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!((body["routing"].clone(), body["account"].clone()), (json!("claude_any"), json!("work@example.org")));
}

#[tokio::test]
async fn a_schedule_on_any_account_starts_each_run_on_the_best_account() {
    let server = Server::start_with(|config| config.scheduler_tick = std::time::Duration::from_millis(50)).await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 20), with_usage("work@example.org", 80)]).await;

    let (status, schedule) = server
        .user
        .json(server.user.post("/api/v1/schedules").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "prompt": "Update dependencies", "cron": "* * * * *", "each_run": "new", "routing": "claude_any"})))
        .await;
    assert_eq!(status, 201, "{schedule}");
    assert_eq!(schedule["routing"], "claude_any");
    sqlx::query("update schedules set next_run_at = now() - interval '1 minute'").execute(&server.pool).await.unwrap();

    assert_eq!(started_on(&mut link).await.as_deref(), Some("work@example.org"));
}
