mod common;

use common::Server;
use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::json;

fn login(account: &str, state: LoginState) -> LoginInfo {
    LoginInfo { agent: AgentId::Claude, account: Some(account.into()), method: "oauth_token".into(), state, usage_left: None, windows: vec![], usage_read_at: None, reset_credits: None }
}

async fn node_with_accounts(server: &Server, accounts: Vec<LoginInfo>) -> common::NodeLink {
    let mut link = server.online_node("laptop").await;
    link.send(NodeToControl::Logins { logins: accounts }).await;

    let user = server.user.clone();
    let id = link.id;
    common::eventually("the accounts", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;
        let node = nodes.as_array().unwrap().iter().find(|node| node["id"] == json!(id)).unwrap().clone();

        (!node["logins"].as_array().unwrap().is_empty()).then_some(())
    })
    .await;

    link
}

#[tokio::test]
async fn a_node_lists_every_account_signed_in_to_one_agent_and_forgets_the_ones_it_stops_reporting() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let accounts: Vec<&str> = nodes[0]["logins"].as_array().unwrap().iter().map(|login| login["account"].as_str().unwrap()).collect();
    assert_eq!(accounts, vec!["sam@example.com", "work@acme.example"]);

    link.send(NodeToControl::Logins { logins: vec![login("work@acme.example", LoginState::SignedIn)] }).await;

    let user = server.user.clone();
    common::eventually("the dropped account", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["logins"].as_array().unwrap().len() == 1).then_some(())
    })
    .await;
}

#[tokio::test]
async fn the_usage_left_an_account_reports_is_shown_on_the_node_and_replaced_by_the_next_report() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![LoginInfo { usage_left: Some(18), ..login("sam@example.com", LoginState::SignedIn) }, login("work@acme.example", LoginState::SignedIn)]).await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let usage = |nodes: &serde_json::Value| nodes[0]["logins"].as_array().unwrap().iter().map(|login| login["usage_left"].clone()).collect::<Vec<_>>();
    assert_eq!(usage(&nodes), vec![json!(18), serde_json::Value::Null]);

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { usage_left: Some(60), ..login("sam@example.com", LoginState::SignedIn) }] }).await;

    let user = server.user.clone();
    common::eventually("the refreshed usage", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["logins"][0]["usage_left"] == json!(60)).then_some(())
    })
    .await;
}

fn window(name: &str, used: f64, resets_in_hours: i64) -> mycelium_proto::conversation::UsageWindow {
    mycelium_proto::conversation::UsageWindow { name: name.into(), used_percent: used, resets_at: Some(chrono::Utc::now() + chrono::Duration::hours(resets_in_hours)), rate_per_hour: None, runs_out_at: None }
}

async fn report_usage(link: &mut common::NodeLink, hours_ago: i64, windows: Vec<mycelium_proto::conversation::UsageWindow>) {
    let read_at = chrono::Utc::now() - chrono::Duration::hours(hours_ago);
    link.send(NodeToControl::Logins { logins: vec![LoginInfo { windows, usage_read_at: Some(read_at), ..login("sam@example.com", LoginState::SignedIn) }] }).await;
}

async fn first_login(server: &Server, settled: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let user = server.user.clone();

    common::eventually("the reported usage", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;
        let login = nodes[0]["logins"][0].clone();

        settled(&login).then_some(login)
    })
    .await
}

#[tokio::test]
async fn each_usage_window_an_account_reports_is_shown_with_its_percent_reset_and_when_it_was_read() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    report_usage(&mut link, 0, vec![window("five_hour", 64.0, 2), window("seven_day", 31.0, 100), window("seven_day_opus", 5.0, 100)]).await;
    let login = first_login(&server, |login| login["windows"].as_array().is_some_and(|windows| windows.len() == 3)).await;

    let names: Vec<_> = login["windows"].as_array().unwrap().iter().map(|window| (window["name"].as_str().unwrap().to_string(), window["used_percent"].as_f64().unwrap())).collect();
    assert_eq!(names, vec![("five_hour".to_string(), 64.0), ("seven_day".to_string(), 31.0), ("seven_day_opus".to_string(), 5.0)]);
    assert!(login["windows"][0]["resets_at"].is_string());
    assert!(login["usage_read_at"].is_string());
}

#[tokio::test]
async fn an_unknown_usage_report_preserves_the_last_reading_without_preserving_sign_in_state() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    report_usage(&mut link, 0, vec![window("five_hour", 42.0, 2)]).await;
    let before = first_login(&server, |login| login["usage_read_at"].is_string()).await;

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { method: "refresh".into(), ..login("sam@example.com", LoginState::SignedOut) }] }).await;
    let after = first_login(&server, |login| login["method"] == "refresh").await;

    assert_eq!(after["state"], "signed_out");
    assert_eq!(after["windows"], before["windows"]);
    assert_eq!(after["usage_read_at"], before["usage_read_at"]);
}

#[tokio::test]
async fn a_reconnected_account_recovers_usage_but_a_missing_account_stays_removed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    report_usage(&mut link, 0, vec![window("five_hour", 42.0, 2)]).await;
    let before = first_login(&server, |login| login["usage_read_at"].is_string()).await;
    link.send(NodeToControl::Logins { logins: vec![] }).await;
    common::eventually("the removed account", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes[0]["logins"].as_array().unwrap().is_empty().then_some(())
    }).await;
    let id = link.id;
    let token = link.token.clone();
    drop(link);
    let mut reconnected = server.connect_node(id, &token).await;
    reconnected.hello(vec![]).await;
    reconnected.send(NodeToControl::Logins { logins: vec![login("sam@example.com", LoginState::SignedIn)] }).await;
    let after = first_login(&server, |login| login["account"] == "sam@example.com").await;

    assert_eq!(after["windows"], before["windows"]);
    assert_eq!(after["usage_read_at"], before["usage_read_at"]);
}

#[tokio::test]
async fn an_older_usage_report_does_not_replace_a_newer_control_plane_reading() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    report_usage(&mut link, 0, vec![window("five_hour", 42.0, 2)]).await;
    let before = first_login(&server, |login| login["usage_read_at"].is_string()).await;
    link.send(NodeToControl::Logins { logins: vec![LoginInfo { method: "older".into(), windows: vec![window("five_hour", 10.0, 2)], usage_read_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)), ..login("sam@example.com", LoginState::SignedIn) }] }).await;
    let after = first_login(&server, |login| login["method"] == "older").await;

    assert_eq!(after["windows"][0]["used_percent"], before["windows"][0]["used_percent"]);
    assert_eq!(after["windows"][0]["resets_at"], before["windows"][0]["resets_at"]);
    assert_eq!(after["usage_read_at"], before["usage_read_at"]);
}

#[tokio::test]
async fn the_rate_of_use_and_when_the_window_runs_out_come_from_the_readings_since_it_last_reset() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    report_usage(&mut link, 9 * 24, vec![window("five_hour", 0.0, 2)]).await;
    report_usage(&mut link, 30, vec![window("five_hour", 90.0, 2)]).await;
    report_usage(&mut link, 3, vec![window("five_hour", 10.0, 2)]).await;
    report_usage(&mut link, 2, vec![window("five_hour", 20.0, 2)]).await;
    report_usage(&mut link, 0, vec![window("five_hour", 40.0, 10)]).await;
    let login = first_login(&server, |login| login["windows"][0]["used_percent"] == json!(40.0)).await;

    let five_hour = &login["windows"][0];
    assert!((five_hour["rate_per_hour"].as_f64().unwrap() - 10.0).abs() < 0.1, "{five_hour}");
    let runs_out: chrono::DateTime<chrono::Utc> = five_hour["runs_out_at"].as_str().unwrap().parse().unwrap();
    assert!(((runs_out - chrono::Utc::now()).num_minutes() - 360).abs() < 5, "{five_hour}");
}

#[tokio::test]
async fn no_projection_is_given_when_the_window_resets_before_it_would_run_out_or_use_is_not_rising() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    report_usage(&mut link, 3, vec![window("five_hour", 10.0, 1), window("seven_day", 50.0, 100)]).await;
    report_usage(&mut link, 0, vec![window("five_hour", 40.0, 1), window("seven_day", 50.0, 100)]).await;
    let login = first_login(&server, |login| login["windows"][0]["used_percent"] == json!(40.0)).await;

    assert!(login["windows"][0]["rate_per_hour"].is_number());
    assert!(login["windows"][0]["runs_out_at"].is_null());
    assert!(login["windows"][1]["rate_per_hour"].is_null());
    assert!(login["windows"][1]["runs_out_at"].is_null());
}

#[tokio::test]
async fn a_conversation_starts_as_the_account_it_was_created_with() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;

    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": "work@acme.example"}))).await;

    assert_eq!(status, 201, "{body}");
    assert_eq!(body["account"], "work@acme.example");

    match link.next().await {
        ControlToNode::StartSession { account, .. } => assert_eq!(account.as_deref(), Some("work@acme.example")),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn an_account_that_is_not_signed_in_on_the_node_is_refused_with_a_reason() {
    let server = Server::start().await;
    let link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("side@example.com", LoginState::SignedOut)]).await;

    for account in ["side@example.com", "nobody@example.com"] {
        let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": account}))).await;

        assert_eq!(status, 409);
        assert_eq!(body["error"], "account_not_signed_in");
        assert_eq!(body["message"], format!("{account} is not signed in on this node. Sign in to it there, or choose another account."));
    }
}

#[tokio::test]
async fn the_account_can_change_until_the_conversation_has_a_session_and_is_then_locked() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;
    let created = server.create_conversation(link.id, None).await;
    let conversation_id = link.expect_start_session().await;
    let path = format!("/api/v1/conversations/{}", created.id);

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["account"], "work@acme.example");

    link.start_session(conversation_id, "s1").await;
    let user = server.user.clone();
    common::eventually("the session", || async { (!user.json(user.get(&path)).await.1["acp_session_id"].is_null()).then_some(()) }).await;

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"account": "sam@example.com"}))).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "account_locked");

    let (status, _) = server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_sign_in_can_name_the_account_it_is_for() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/logins", link.id)).json(&json!({"agent": "claude", "method": "oauth_token", "account": "  work@acme.example "}))).await;
    assert_eq!(status, 200);

    match link.next().await {
        ControlToNode::StartLogin { account, .. } => assert_eq!(account.as_deref(), Some("work@acme.example")),
        other => panic!("expected start_login, got {other:?}"),
    }
}

#[tokio::test]
async fn the_default_account_can_be_chosen_explicitly_until_the_conversation_has_a_session() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, vec![login("sam@example.com", LoginState::SignedIn), login("work@acme.example", LoginState::SignedIn)]).await;
    let created = server.create_conversation(link.id, None).await;
    let conversation_id = link.expect_start_session().await;
    let path = format!("/api/v1/conversations/{}", created.id);

    server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"use_default_account": true}))).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["account"].is_null());

    server.user.json(server.user.patch(&path).json(&json!({"account": "work@acme.example"}))).await;
    link.start_session(conversation_id, "s1").await;
    let user = server.user.clone();
    common::eventually("the session", || async { (!user.json(user.get(&path)).await.1["acp_session_id"].is_null()).then_some(()) }).await;

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"use_default_account": true}))).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "account_locked");
}

#[tokio::test]
async fn banked_resets_a_node_reports_appear_on_its_login() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let credits = mycelium_proto::conversation::ResetCredits {
        available_count: 2,
        credits: Some(vec![mycelium_proto::conversation::ResetCredit { id: "a".into(), expires_at: Some(chrono::Utc::now() + chrono::Duration::days(14)), title: Some("Bonus reset".into()) }]),
    };

    link.send(NodeToControl::Logins { logins: vec![LoginInfo { reset_credits: Some(credits), ..login("sam@example.com", LoginState::SignedIn) }] }).await;
    let reported = first_login(&server, |login| login["reset_credits"].is_object()).await;

    assert_eq!(reported["reset_credits"]["available_count"], json!(2));
    assert_eq!(reported["reset_credits"]["credits"][0]["title"], json!("Bonus reset"));
    assert!(reported["reset_credits"]["credits"][0]["expires_at"].is_string());

    link.send(NodeToControl::Logins { logins: vec![login("sam@example.com", LoginState::SignedIn)] }).await;
    let user = server.user.clone();
    common::eventually("the spent resets to clear", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes[0]["logins"][0].get("reset_credits").is_none().then_some(())
    })
    .await;
}
