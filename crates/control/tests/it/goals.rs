use crate::common;
use crate::common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn working_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    conversation.id
}

async fn put_goal(server: &Server, id: Uuid, body: Value) -> (u16, Value) {
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&body)).await
}

async fn already_delivered(server: &Server, id: Uuid) {
    sqlx::query("delete from queued_prompts where conversation_id = $1 and origin = 'goal'").bind(id).execute(&server.pool).await.unwrap();
    sqlx::query("update goals set delivery_state = 'sent', delivered_at = now() where conversation_id = $1").bind(id).execute(&server.pool).await.unwrap();
}

async fn set_goal(server: &Server, id: Uuid, text: &str, budget: i64) -> (u16, Value) {
    let result = put_goal(server, id, json!({"text": text, "token_budget": budget})).await;
    already_delivered(server, id).await;

    result
}

async fn goal_of(server: &Server, id: Uuid) -> Value {
    let (status, goal) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/goal"))).await;
    assert_eq!(status, 200, "{goal}");

    goal
}

async fn patch_goal(server: &Server, id: Uuid, paused: bool) -> (u16, Value) {
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"paused": paused}))).await
}

async fn end_turn(link: &mut NodeLink, id: Uuid, seq: i64, tokens: i64) {
    let usage = json!({"inputTokens": tokens, "outputTokens": 0, "cachedReadTokens": 0, "cachedWriteTokens": 0});
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq, stop_reason: "end_turn".into(), usage: Some(usage) }).await;
}

async fn user_messages(server: &Server, id: Uuid) -> Vec<String> {
    server.events(id).await.iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].as_str().unwrap().to_string()).collect()
}

fn assert_goal_prompt(text: &str, goal: &str) {
    assert!(text.starts_with(&format!("Work towards this goal: {goal}. When the goal is done, or you cannot go on without ")), "{text}");
    assert!(text.ends_with(", call report_goal and say why."), "{text}");
}

async fn report_goal(server: &Server, link: &NodeLink, id: Uuid, outcome: &str, reason: &str) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "report_goal", "arguments": {"outcome": outcome, "reason": reason}}});
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", id.to_string()).json(&call)).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn say(link: &mut NodeLink, id: Uuid, seq: i64, text: &str) {
    let update = json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}});
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update }).await;
}

async fn nudge_now(server: &Server, id: Uuid) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/goal/nudge"))).await
}

async fn continue_goal(server: &Server, id: Uuid, body: Value) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/goal/continue")).json(&body)).await
}

#[tokio::test]
async fn a_goal_needs_text_and_a_positive_budget() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let (status, body) = set_goal(&server, id, "  ", 1000).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("text_required")));

    let (status, body) = set_goal(&server, id, "Tests pass", 0).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("budget_invalid")));

    assert_eq!(goal_of(&server, id).await, Value::Null);
}

#[tokio::test]
async fn setting_a_goal_on_an_idle_conversation_sends_the_agent_to_work_on_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let (status, goal) = set_goal(&server, id, "Every test passes", 5000).await;

    assert_eq!(status, 200, "{goal}");
    assert_eq!((goal["state"].as_str(), goal["text"].as_str(), goal["token_budget"].as_i64(), goal["tokens_used"].as_i64(), goal["turns"].as_i64()), (Some("active"), Some("Every test passes"), Some(5000), Some(0), Some(0)));
    assert_goal_prompt(&link.expect_prompt().await.1, "Every test passes");
}

#[tokio::test]
async fn tokens_from_the_usage_records_are_counted_and_a_goal_not_met_over_budget_ends_instead_of_continuing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 1000).await;

    end_turn(&mut link, id, 2, 1200).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["tokens_used"].as_i64()), (Some("over_budget"), Some(1200)));
    assert!(goal["reason"].as_str().unwrap().contains("budget"), "{goal}");
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn a_goal_met_on_the_turn_that_used_the_last_of_the_budget_is_met_not_over_budget() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 1000).await;

    report_goal(&server, &link, id, "done", "Every test passes.").await;
    end_turn(&mut link, id, 2, 5000).await;
    link.barrier(id, 3).await;

    assert_eq!(goal_of(&server, id).await["state"], "met");
}

async fn set_goal_with(server: &Server, id: Uuid, limits: Value) -> (u16, Value) {
    let mut body = json!({"text": "Every test passes"});
    body.as_object_mut().unwrap().extend(limits.as_object().unwrap().clone());

    let result = put_goal(server, id, body).await;
    already_delivered(server, id).await;

    result
}

async fn report_windows(link: &mut NodeLink, windows: &[(&str, f64)]) {
    report_windows_for(link, None, windows).await;
}

async fn report_windows_for(link: &mut NodeLink, account: Option<&str>, windows: &[(&str, f64)]) {
    let windows = windows.iter().map(|(name, used)| mycelium_proto::conversation::UsageWindow { name: (*name).into(), used_percent: *used, resets_at: None, rate_per_hour: None, runs_out_at: None }).collect();
    let login = mycelium_proto::conversation::LoginInfo { managed: false, agent: mycelium_proto::conversation::AgentId::Claude, account: account.map(str::to_string), method: "token".into(), state: mycelium_proto::conversation::LoginState::SignedIn, usage_left: None, windows, usage_read_at: Some(chrono::Utc::now()), reset_credits: None };
    link.send(NodeToControl::Logins { logins: vec![login] }).await;
}

async fn goal_reports_window(server: &Server, id: Uuid, five_hour: Value) {
    let user = server.user.clone();

    common::eventually("the window on the goal", || async { (goal_of_with(&user, id).await["five_hour_used"] == five_hour).then_some(()) }).await;
}

async fn goal_of_with(user: &common::Client, id: Uuid) -> Value {
    user.json(user.get(&format!("/api/v1/conversations/{id}/goal"))).await.1
}

#[tokio::test]
async fn only_the_goal_text_is_required_and_a_goal_without_limits_never_runs_out() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let (status, goal) = set_goal_with(&server, id, json!({})).await;
    assert_eq!(status, 200, "{goal}");
    assert!(goal["token_budget"].is_null() && goal["time_limit_seconds"].is_null() && goal["five_hour_limit"].is_null() && goal["weekly_limit"].is_null(), "{goal}");

    end_turn(&mut link, id, 2, 900_000_000).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(goal_of(&server, id).await["state"], "active");
}

#[tokio::test]
async fn limits_are_validated_with_a_message_that_says_what_to_enter() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    for (limits, error, message) in [
        (json!({"time_limit_seconds": 0}), "time_limit_invalid", "Enter a time limit greater than zero."),
        (json!({"five_hour_limit": 0}), "five_hour_limit_invalid", "Enter a 5-hour window limit between 1 and 100 percent."),
        (json!({"five_hour_limit": 101}), "five_hour_limit_invalid", "Enter a 5-hour window limit between 1 and 100 percent."),
        (json!({"weekly_limit": 150}), "weekly_limit_invalid", "Enter a weekly window limit between 1 and 100 percent."),
        (json!({"token_budget": -5}), "budget_invalid", "Enter a token budget greater than zero."),
    ] {
        let (status, body) = set_goal_with(&server, id, limits).await;

        assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (400, Some(error), Some(message)), "{body}");
    }

    assert_eq!(goal_of(&server, id).await, Value::Null);
}

#[tokio::test]
async fn a_goal_not_met_after_its_time_limit_ends_with_that_reason_instead_of_continuing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"time_limit_seconds": 7200})).await;
    sqlx::query("update goals set started_at = now() - interval '3 hours' where conversation_id = $1").bind(id).execute(&server.pool).await.unwrap();

    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "limit_reached");
    assert!(goal["reason"].as_str().unwrap().starts_with("Time limit reached after 2h"), "{goal}");
}

#[tokio::test]
async fn a_goal_within_its_time_limit_keeps_going() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"time_limit_seconds": 7200})).await;

    end_turn(&mut link, id, 2, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
}

#[tokio::test]
async fn a_goal_stops_when_the_five_hour_window_of_its_account_reaches_the_limit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"five_hour_limit": 80})).await;
    report_windows(&mut link, &[("five_hour", 82.4), ("seven_day", 10.0)]).await;
    goal_reports_window(&server, id, json!(82.4)).await;

    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "limit_reached");
    assert_eq!(goal["reason"], "5-hour window at 82%, over your limit of 80%. Set the goal again to carry on.");
}

#[tokio::test]
async fn a_conversation_without_a_recorded_account_reads_the_usage_of_its_nodes_signed_in_login() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"weekly_limit": 85})).await;

    report_windows_for(&mut link, Some("work@example.com"), &[("five_hour", 27.0), ("seven_day", 65.0)]).await;

    let user = server.user.clone();
    common::eventually("the weekly reading from the node's login", || async { (goal_of_with(&user, id).await["weekly_used"] == json!(65.0)).then_some(()) }).await;
}

#[tokio::test]
async fn a_goal_stops_when_the_weekly_window_reaches_its_limit_but_not_while_it_is_below() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"weekly_limit": 50, "five_hour_limit": 90})).await;
    report_windows(&mut link, &[("five_hour", 89.0), ("seven_day", 49.0)]).await;
    goal_reports_window(&server, id, json!(89.0)).await;

    end_turn(&mut link, id, 2, 0).await;
    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    link.barrier(id, 3).await;

    report_windows(&mut link, &[("five_hour", 89.0), ("seven_day", 50.0)]).await;
    let user = server.user.clone();
    common::eventually("the new weekly reading", || async { (goal_of_with(&user, id).await["weekly_used"] == json!(50.0)).then_some(()) }).await;
    end_turn(&mut link, id, 4, 0).await;
    link.barrier(id, 5).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["reason"].as_str()), (Some("limit_reached"), Some("Weekly window at 50%, over your limit of 50%. Set the goal again to carry on.")));
}

#[tokio::test]
async fn unknown_usage_never_stops_a_goal_with_a_window_limit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"five_hour_limit": 10})).await;

    end_turn(&mut link, id, 2, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["five_hour_limit"].as_i64(), goal["five_hour_used"].is_null()), (Some("active"), Some(10), true));
}

#[tokio::test]
async fn a_paused_goal_is_not_nudged_when_the_turn_ends() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let (status, goal) = patch_goal(&server, id, true).await;
    assert_eq!((status, goal["state"].as_str()), (200, Some("paused")));
    end_turn(&mut link, id, 2, 400).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["tokens_used"].as_i64()), (Some("paused"), Some(400)));
    let (_, conversations) = server.user.json(server.user.get("/api/v1/conversations")).await;
    assert_eq!(conversations.as_array().unwrap().len(), 1);
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn resuming_a_goal_on_an_idle_conversation_nudges_straight_away() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    patch_goal(&server, id, true).await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let (status, goal) = patch_goal(&server, id, false).await;
    assert_eq!((status, goal["state"].as_str()), (200, Some("active")));

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
}

#[tokio::test]
async fn an_ended_goal_cannot_be_paused_or_resumed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    report_goal(&server, &link, id, "done", "Every test passes.").await;

    let (status, body) = patch_goal(&server, id, false).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("goal_ended")));
}

#[tokio::test]
async fn stopping_a_turn_pauses_the_goal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 2, stop_reason: "cancelled".into(), usage: None }).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "paused");
    assert!(goal["reason"].as_str().unwrap().contains("stopped"), "{goal}");
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
    let report = server.events(id).await.into_iter().find(|event| event["kind"] == "goal_report").unwrap();
    assert_eq!(report["payload"]["outcome"], "stopped");
}

#[tokio::test]
async fn clearing_a_goal_removes_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}/goal"))).await;

    assert_eq!(status, 204);
    assert_eq!(goal_of(&server, id).await, Value::Null);
}

#[tokio::test]
async fn setting_a_goal_again_starts_it_over_with_the_new_budget() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 1000).await;
    end_turn(&mut link, id, 2, 1500).await;
    link.barrier(id, 3).await;
    assert_eq!(goal_of(&server, id).await["state"], "over_budget");

    let (_, goal) = set_goal(&server, id, "Every test passes", 50_000).await;

    assert_eq!((goal["state"].as_str(), goal["tokens_used"].as_i64(), goal["turns"].as_i64()), (Some("active"), Some(0), Some(0)));
}

#[tokio::test]
async fn a_goal_on_a_conversation_that_is_not_yours_is_not_found() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": "x", "token_budget": 10}))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_running_goal_counts_reported_codex_tokens_before_the_turn_ends_without_counting_replayed_samples_twice() {
    use base64::Engine;
    for folder in [".codex/sessions", ".local/state/mycelium/accounts/codex/account-test/sessions"] {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let session = Uuid::new_v4();
    sqlx::query("update conversations set acp_session_id = $2 where id = $1").bind(id).bind(session.to_string()).execute(&server.pool).await.unwrap();
    let path = format!("{folder}/2026/10/24/rollout-2026-10-24T10-00-00-{session}.jsonl");
    let sample = |at: chrono::DateTime<chrono::Utc>, total: i64, last: i64| format!("{}\n", json!({"timestamp":at.to_rfc3339(),"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":total},"last_token_usage":{"total_tokens":last}}}}));
    let old = sample(chrono::Utc::now() - chrono::Duration::minutes(2), 1000, 1000);
    set_goal(&server, id, "Track the running work", 100_000).await;
    let live = sample(chrono::Utc::now(), 1300, 300);
    let text = format!("{old}{live}{live}");
    link.send(NodeToControl::Transcript { path: path.clone(), offset: 0, data: base64::engine::general_purpose::STANDARD.encode(&text), size: Some(text.len() as i64) }).await;
    link.barrier(id, 20).await;
    let goal = goal_of(&server, id).await;
    assert_eq!(goal["tokens_used"], 300, "{goal}");
    assert_eq!(goal["turn_active"], true, "{goal}");
    assert_eq!(goal["tokens_known"], true, "{goal}");

    link.send(NodeToControl::Transcript { path, offset: 0, data: base64::engine::general_purpose::STANDARD.encode(&text), size: Some(text.len() as i64) }).await;
    link.barrier(id, 21).await;
    assert_eq!(goal_of(&server, id).await["tokens_used"], 300);
    }
}

#[tokio::test]
async fn fresh_account_usage_is_published_to_an_existing_goal_without_waiting_for_turn_end() {
    use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState, UsageWindow};
    use mycelium_proto::stream::StreamMessage;
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Track usage", 100_000).await;
    let mut stream = server.user.stream().await;
    link.send(NodeToControl::Logins { logins: vec![LoginInfo { managed: false, agent: AgentId::Claude, account: None, method: "token".into(), state: LoginState::SignedIn, usage_left: Some(58), windows: vec![UsageWindow { name: "five_hour".into(), used_percent: 42.0, resets_at: None, rate_per_hour: None, runs_out_at: None }], usage_read_at: Some(chrono::Utc::now()), reset_credits: None }] }).await;
    let updated = stream.until(|message| match message { StreamMessage::Goal(goal) if goal.conversation_id == id && goal.five_hour_used == Some(42.0) => Some(goal), _ => None }).await;
    assert_eq!(updated.five_hour_used, Some(42.0));
}

#[tokio::test]
async fn a_cached_window_that_has_reset_is_visible_but_is_not_used_by_a_goal() {
    use mycelium_proto::conversation::{AgentId, LoginInfo, LoginState, UsageWindow};
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let read_at = chrono::Utc::now() - chrono::Duration::hours(2);
    link.send(NodeToControl::Logins { logins: vec![LoginInfo { managed: false,
        agent: AgentId::Claude,
        account: None,
        method: "token".into(),
        state: LoginState::SignedIn,
        usage_left: Some(0),
        windows: vec![UsageWindow { name: "five_hour".into(), used_percent: 100.0, resets_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)), rate_per_hour: None, runs_out_at: None }],
        usage_read_at: Some(read_at),
        reset_credits: None,
    }] }).await;
    link.barrier(id, 20).await;
    set_goal(&server, id, "Use current limits", 100_000).await;
    let goal = goal_of(&server, id).await;
    assert!(goal["five_hour_used"].is_null(), "{goal}");
    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["logins"][0]["windows"][0]["used_percent"], 100.0);
    let cached_at: chrono::DateTime<chrono::Utc> = nodes[0]["logins"][0]["usage_read_at"].as_str().unwrap().parse().unwrap();
    assert_eq!(cached_at.timestamp(), read_at.timestamp());
}

#[tokio::test]
async fn a_goal_set_during_a_turn_waits_for_the_turn_to_end_and_then_reaches_the_agent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let (status, goal) = put_goal(&server, id, json!({"text": "Release 0.13"})).await;

    assert_eq!(status, 200, "{goal}");
    assert_eq!(goal["delivery"], "pending");
    assert!(!user_messages(&server, id).await.iter().any(|text| text.contains("Release 0.13")));

    end_turn(&mut link, id, 2, 0).await;

    assert_goal_prompt(&link.expect_prompt().await.1, "Release 0.13");
    let goal = goal_of(&server, id).await;
    assert_eq!(goal["delivery"], "sent");
    assert!(goal["delivered_at"].is_string(), "{goal}");
    let sent = server.events(id).await.into_iter().filter(|event| event["kind"] == "user_message").last().unwrap();
    assert_eq!((sent["payload"]["origin"].as_str(), sent["payload"]["sent_at"].is_string(), sent["payload"]["nudge"].as_i64(), sent["payload"]["why"].as_str()), (Some("goal"), true, Some(0), Some("you set the goal")));
}

#[tokio::test]
async fn a_goal_set_while_the_agent_is_idle_is_marked_sent_at_once() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let (_, goal) = put_goal(&server, id, json!({"text": "Release 0.13"})).await;

    assert_goal_prompt(&link.expect_prompt().await.1, "Release 0.13");
    let delivered = common::eventually("the goal to be marked sent", || async { (goal_of(&server, id).await["delivery"] == "sent").then_some(()) }).await;
    assert_eq!((goal["state"].as_str(), delivered), (Some("active"), ()));
}

#[tokio::test]
async fn a_goal_that_cannot_be_sent_because_the_node_is_offline_says_so_and_can_be_sent_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;
    let (node_id, token) = (link.id, link.token.clone());
    drop(link);
    common::eventually("the node to go offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|candidate| candidate["id"] == json!(node_id) && candidate["online"] == false).then_some(())
    })
    .await;

    let (_, goal) = put_goal(&server, id, json!({"text": "Release 0.13"})).await;

    assert_eq!(goal["delivery"], "failed");
    assert_eq!(goal["delivery_reason"], "Mycelium could not send the goal to Claude Code because laptop did not answer. Check that laptop is online, then send the goal again.");

    let mut returned = server.connect_node(node_id, &token).await;
    returned.hello(vec![(id, 3)]).await;
    common::eventually("the node to come back online", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|candidate| candidate["id"] == json!(node_id) && candidate["online"] == true).then_some(())
    })
    .await;
    let (status, again) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/goal/send"))).await;

    assert_eq!(status, 200, "{again}");
    loop {
        if let ControlToNode::Prompt { text, .. } = returned.next().await {
            assert_goal_prompt(&text, "Release 0.13");
            break;
        }
    }
    assert_eq!(goal_of(&server, id).await["delivery"], "sent");
}

#[tokio::test]
async fn a_goal_that_reached_the_agent_cannot_be_sent_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Release 0.13", 1000).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/goal/send"))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("goal_delivered")));
}

#[tokio::test]
async fn time_held_at_a_usage_limit_does_not_count_towards_the_time_limit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal_with(&server, id, json!({"time_limit_seconds": 7200})).await;
    sqlx::query("update goals set started_at = now() - interval '3 hours', held_seconds = 7200 where conversation_id = $1").bind(id).execute(&server.pool).await.unwrap();

    end_turn(&mut link, id, 2, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(goal_of(&server, id).await["state"], "active");
}

async fn last_user_message(server: &Server, id: Uuid) -> Value {
    server.events(id).await.into_iter().filter(|event| event["kind"] == "user_message").last().unwrap()["payload"].clone()
}

#[tokio::test]
async fn a_turn_that_ends_without_a_report_is_nudged_and_the_nudge_says_why() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    say(&mut link, id, 2, "Two tests still fail.").await;
    end_turn(&mut link, id, 3, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    let nudge = last_user_message(&server, id).await;
    assert_eq!((nudge["origin"].as_str(), nudge["nudge"].as_i64()), (Some("goal"), Some(1)));
    assert!(nudge["why"].as_str().unwrap().starts_with("the turn ended without a report"), "{nudge}");
    assert_eq!(goal_of(&server, id).await["nudges"], 1);
}

#[tokio::test]
async fn reporting_the_goal_done_ends_the_loop_and_nothing_more_is_sent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let reply = report_goal(&server, &link, id, "done", "All 214 tests pass.").await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    assert_ne!(reply["result"]["isError"], true, "{reply}");
    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["reason"].as_str()), (Some("met"), Some("All 214 tests pass.")));
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
    let report = server.events(id).await.into_iter().find(|event| event["kind"] == "goal_report").unwrap();
    assert_eq!((report["payload"]["outcome"].as_str(), report["payload"]["reason"].as_str()), (Some("done"), Some("All 214 tests pass.")));
}

#[tokio::test]
async fn reporting_the_goal_blocked_ends_the_loop_and_waits_for_the_person() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    report_goal(&server, &link, id, "blocked", "The CI secret is missing.").await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["reason"].as_str()), (Some("stuck"), Some("The CI secret is missing.")));
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn a_report_without_an_active_goal_is_refused() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let reply = report_goal(&server, &link, id, "done", "Finished.").await;

    assert_eq!(reply["result"]["isError"], true, "{reply}");
}

#[tokio::test]
async fn a_report_must_say_why() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let reply = report_goal(&server, &link, id, "done", "  ").await;

    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(goal_of(&server, id).await["state"], "active");
}

#[tokio::test]
async fn a_turn_that_ends_the_same_way_twice_backs_off_and_the_third_stops_the_goal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    say(&mut link, id, 2, "Two tests still fail.").await;
    end_turn(&mut link, id, 3, 0).await;
    link.expect_prompt().await;

    say(&mut link, id, 5, "Two tests still fail.").await;
    end_turn(&mut link, id, 6, 0).await;
    link.barrier(id, 7).await;

    let waiting = goal_of(&server, id).await;
    assert_eq!((waiting["state"].as_str(), waiting["wait_reason"].as_str(), waiting["repeats"].as_i64()), (Some("active"), Some("repeated_outcome"), Some(2)));
    assert!(waiting["next_nudge_at"].is_string(), "{waiting}");
    assert_eq!(user_messages(&server, id).await.len(), 2);

    let (status, nudged) = nudge_now(&server, id).await;
    assert_eq!(status, 200, "{nudged}");
    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(last_user_message(&server, id).await["why"], "you chose to nudge now");

    say(&mut link, id, 8, "Two tests still fail.").await;
    end_turn(&mut link, id, 9, 0).await;
    link.barrier(id, 10).await;

    let stopped = goal_of(&server, id).await;
    assert_eq!(stopped["state"], "stalled");
    assert_eq!(stopped["reason"], "Stopped after 3 turns ended the same way: Two tests still fail");
    assert_eq!(user_messages(&server, id).await.len(), 3);
}

#[tokio::test]
async fn a_changed_outcome_starts_the_count_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    say(&mut link, id, 2, "Two tests still fail.").await;
    end_turn(&mut link, id, 3, 0).await;
    link.expect_prompt().await;
    say(&mut link, id, 5, "One test still fails.").await;
    end_turn(&mut link, id, 6, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(goal_of(&server, id).await["repeats"], 1);
}

#[tokio::test]
async fn the_repeat_limit_is_set_with_the_goal_and_a_stopped_goal_resumes_with_a_nudge() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let (status, body) = put_goal(&server, id, json!({"text": "Every test passes", "repeat_limit": 1})).await;
    assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (400, Some("repeat_limit_invalid"), Some("Enter how many turns, as a whole number like 3, or leave it empty.")));

    let (status, goal) = put_goal(&server, id, json!({"text": "Every test passes", "repeat_limit": 2})).await;
    already_delivered(&server, id).await;
    assert_eq!((status, goal["repeat_limit"].as_i64()), (200, Some(2)));

    say(&mut link, id, 2, "Stuck on the same test.").await;
    end_turn(&mut link, id, 3, 0).await;
    link.expect_prompt().await;
    say(&mut link, id, 5, "Stuck on the same test.").await;
    end_turn(&mut link, id, 6, 0).await;
    link.barrier(id, 7).await;
    assert_eq!(goal_of(&server, id).await["state"], "stalled");

    let (status, resumed) = patch_goal(&server, id, false).await;

    assert_eq!((status, resumed["state"].as_str(), resumed["repeats"].as_i64()), (200, Some("active"), Some(0)));
    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(last_user_message(&server, id).await["why"], "you resumed the goal");
}

#[tokio::test]
async fn nudge_now_is_refused_when_mycelium_is_not_waiting() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let (status, body) = nudge_now(&server, id).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("goal_not_waiting")));
}

#[tokio::test]
async fn an_open_question_holds_the_goal_until_it_is_answered_and_then_the_loop_carries_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    let anonymous = server.anonymous();
    let ask = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ask_question", "arguments": {"question": "Which database?"}}});
    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", id.to_string()).json(&ask)).await;

    say(&mut link, id, 2, "Waiting for your answer.").await;
    end_turn(&mut link, id, 3, 0).await;
    link.barrier(id, 4).await;

    let held = goal_of(&server, id).await;
    assert_eq!((held["state"].as_str(), held["wait_reason"].as_str()), (Some("active"), Some("question_waiting")));
    assert!(held["held_since"].is_string(), "{held}");
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
    let (status, body) = nudge_now(&server, id).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("goal_not_waiting")));

    let task: Uuid = sqlx::query_scalar("select id from tasks where source_id = $1 and status = 'question'").bind(id).fetch_one(&server.pool).await.unwrap();
    let (status, answered) = server.user.json(server.user.post(&format!("/api/v1/tasks/{task}/answer")).json(&json!({"answer": "Postgres"}))).await;
    assert_eq!(status, 200, "{answered}");
    assert_eq!(link.expect_prompt().await.1, "Answer to your question \"Which database?\": Postgres");

    say(&mut link, id, 5, "Switched to Postgres.").await;
    end_turn(&mut link, id, 6, 0).await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes");
    assert_eq!(goal_of(&server, id).await["held_since"], Value::Null);
}

#[tokio::test]
async fn continuing_a_blocked_goal_sends_the_note_with_the_next_nudge() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    report_goal(&server, &link, id, "blocked", "The CI secret is missing.").await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let (status, goal) = continue_goal(&server, id, json!({"note": "The secret is in place."})).await;

    assert_eq!((status, goal["state"].as_str()), (200, Some("active")));
    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nThe secret is in place.");
    assert_eq!(last_user_message(&server, id).await["why"], "you chose to continue the goal");
}

#[tokio::test]
async fn only_a_blocked_goal_can_be_continued() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    let (status, body) = continue_goal(&server, id, json!({})).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("goal_not_blocked")));
}
