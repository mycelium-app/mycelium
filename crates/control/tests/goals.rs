mod common;

use common::{NodeLink, Server};
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

async fn set_goal(server: &Server, id: Uuid, text: &str, budget: i64) -> (u16, Value) {
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": text, "token_budget": budget}))).await
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

async fn expect_check(link: &mut NodeLink) -> (Uuid, String) {
    let check = link.expect_start_session().await;
    link.start_session(check, "acp-check").await;
    let (_, prompt) = link.expect_prompt().await;

    (check, prompt)
}

async fn answer_check(link: &mut NodeLink, check: Uuid, answer: &str) {
    let update = json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": answer}}});
    link.send(NodeToControl::AcpUpdate { conversation_id: check, seq: 1, update }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: check, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
}

async fn settled(link: &mut NodeLink, check: Uuid) {
    link.barrier(check, 3).await;
}

async fn user_messages(server: &Server, id: Uuid) -> Vec<String> {
    server.events(id).await.iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].as_str().unwrap().to_string()).collect()
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
    assert_eq!(link.expect_prompt().await.1, "Work towards this goal: Every test passes");
}

#[tokio::test]
async fn when_a_turn_ends_a_hidden_check_session_of_the_same_agent_judges_the_goal_and_not_met_sends_the_reason_back() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 300).await;
    let (check, prompt) = expect_check(&mut link).await;

    assert!(prompt.contains("Goal: Every test passes"), "{prompt}");
    assert!(prompt.contains("\"verdict\""), "{prompt}");
    let hidden = server.conversation(check).await;
    assert_eq!((hidden["side_kind"].as_str(), hidden["parent_id"].clone(), hidden["agent"].as_str(), hidden["cwd"].as_str()), (Some("check"), json!(id), Some("claude"), Some("/work/app")));
    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations")).await;
    assert!(listed.as_array().unwrap().iter().all(|conversation| conversation["id"] != json!(check)));

    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"Two e2e tests still fail.\"}").await;

    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "Continue towards the goal: Every test passes\n\nTwo e2e tests still fail.");
    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["turns"].as_i64(), goal["reason"].as_str()), (Some("active"), Some(1), Some("Two e2e tests still fail.")));
}

#[tokio::test]
async fn the_check_session_is_archived_once_it_has_answered() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"ok\"}").await;

    match link.next().await {
        ControlToNode::CloseSession { conversation_id } => assert_eq!(conversation_id, check),
        other => panic!("expected close_session, got {other:?}"),
    }
    assert!(!server.conversation(check).await["archived_at"].is_null());
    let (_, archived) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    assert!(archived.as_array().unwrap().iter().all(|conversation| conversation["id"] != json!(check)));
}

#[tokio::test]
async fn a_met_verdict_ends_the_goal_with_the_checks_reason_and_sends_nothing_more() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"All 214 tests pass.\"}").await;
    settled(&mut link, check).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["reason"].as_str()), (Some("met"), Some("All 214 tests pass.")));
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn a_stuck_verdict_ends_the_goal_with_the_checks_reason() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"stuck\", \"reason\": \"The CI secret is missing.\"}").await;
    settled(&mut link, check).await;

    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["reason"].as_str()), (Some("stuck"), Some("The CI secret is missing.")));
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn a_verdict_wrapped_in_prose_or_a_code_fence_is_still_read() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "Here is my answer:\n```json\n{\"verdict\": \"MET\", \"reason\": \"Done.\"}\n```").await;
    settled(&mut link, check).await;

    assert_eq!(goal_of(&server, id).await["state"], "met");
}

#[tokio::test]
async fn an_answer_that_is_not_the_requested_json_counts_as_stuck() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "I think it is probably done.").await;
    settled(&mut link, check).await;

    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "stuck");
    assert!(goal["reason"].as_str().unwrap().contains("did not answer"), "{goal}");
    assert_eq!(user_messages(&server, id).await, ["Build it"]);
}

#[tokio::test]
async fn an_unknown_verdict_counts_as_stuck() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"maybe\", \"reason\": \"?\"}").await;
    settled(&mut link, check).await;

    assert_eq!(goal_of(&server, id).await["state"], "stuck");
}

#[tokio::test]
async fn tokens_from_the_usage_records_are_counted_and_a_goal_not_met_over_budget_ends_instead_of_continuing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 1000).await;

    end_turn(&mut link, id, 2, 1200).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;
    settled(&mut link, check).await;

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

    end_turn(&mut link, id, 2, 5000).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"Done.\"}").await;
    settled(&mut link, check).await;

    assert_eq!(goal_of(&server, id).await["state"], "met");
}

#[tokio::test]
async fn a_message_from_the_user_is_delivered_without_pausing_the_goal_and_the_check_after_that_turn_carries_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Actually, skip the e2e tests"}))).await;
    assert_eq!(status, 204);
    assert_eq!(goal_of(&server, id).await["state"], "active");
    assert_eq!(link.expect_prompt().await.1, "Actually, skip the e2e tests");

    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"Judged before the message.\"}").await;
    settled(&mut link, check).await;
    assert_eq!(goal_of(&server, id).await["state"], "active");

    end_turn(&mut link, id, 4, 0).await;
    let (check, prompt) = expect_check(&mut link).await;
    assert!(prompt.contains("Goal: Every test passes"), "{prompt}");
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"Skipped e2e, unit tests remain.\"}").await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nSkipped e2e, unit tests remain.");
    assert_eq!(goal_of(&server, id).await["state"], "active");
}

async fn set_goal_with(server: &Server, id: Uuid, limits: Value) -> (u16, Value) {
    let mut body = json!({"text": "Every test passes"});
    body.as_object_mut().unwrap().extend(limits.as_object().unwrap().clone());

    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&body)).await
}

async fn report_windows(link: &mut NodeLink, windows: &[(&str, f64)]) {
    report_windows_for(link, None, windows).await;
}

async fn report_windows_for(link: &mut NodeLink, account: Option<&str>, windows: &[(&str, f64)]) {
    let windows = windows.iter().map(|(name, used)| mycelium_proto::conversation::UsageWindow { name: (*name).into(), used_percent: *used, resets_at: None, rate_per_hour: None, runs_out_at: None }).collect();
    let login = mycelium_proto::conversation::LoginInfo { agent: mycelium_proto::conversation::AgentId::Claude, account: account.map(str::to_string), method: "token".into(), state: mycelium_proto::conversation::LoginState::SignedIn, usage_left: None, windows, usage_read_at: Some(chrono::Utc::now()) };
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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nMore to do.");
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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;
    settled(&mut link, check).await;

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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nMore to do.");
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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;
    settled(&mut link, check).await;

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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"One.\"}").await;
    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nOne.");
    settled(&mut link, check).await;

    report_windows(&mut link, &[("five_hour", 89.0), ("seven_day", 50.0)]).await;
    let user = server.user.clone();
    common::eventually("the new weekly reading", || async { (goal_of_with(&user, id).await["weekly_used"] == json!(50.0)).then_some(()) }).await;
    end_turn(&mut link, id, 4, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"Two.\"}").await;
    settled(&mut link, check).await;

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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More to do.\"}").await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nMore to do.");
    let goal = goal_of(&server, id).await;
    assert_eq!((goal["state"].as_str(), goal["five_hour_limit"].as_i64(), goal["five_hour_used"].is_null()), (Some("active"), Some(10), true));
}

#[tokio::test]
async fn a_paused_goal_does_not_run_a_check_when_the_turn_ends() {
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
}

#[tokio::test]
async fn resuming_a_goal_on_an_idle_conversation_runs_a_check_straight_away() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    patch_goal(&server, id, true).await;
    end_turn(&mut link, id, 2, 0).await;
    link.barrier(id, 3).await;

    let (status, goal) = patch_goal(&server, id, false).await;
    assert_eq!((status, goal["state"].as_str()), (200, Some("active")));
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"Keep going.\"}").await;

    assert_eq!(link.expect_prompt().await.1, "Continue towards the goal: Every test passes\n\nKeep going.");
}

#[tokio::test]
async fn an_ended_goal_cannot_be_paused_or_resumed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"Done.\"}").await;
    settled(&mut link, check).await;

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
    let (check, _) = expect_check(&mut link).await;
    answer_check(&mut link, check, "{\"verdict\": \"continue\", \"reason\": \"More.\"}").await;
    settled(&mut link, check).await;
    assert_eq!(goal_of(&server, id).await["state"], "over_budget");

    let (_, goal) = set_goal(&server, id, "Every test passes", 50_000).await;

    assert_eq!((goal["state"].as_str(), goal["tokens_used"].as_i64(), goal["turns"].as_i64()), (Some("active"), Some(0), Some(0)));
}

#[tokio::test]
async fn a_check_that_fails_to_start_a_session_ends_the_goal_as_stuck() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;

    end_turn(&mut link, id, 2, 0).await;
    let check = link.expect_start_session().await;
    link.send(NodeToControl::SessionFailed { conversation_id: check, seq: 1, message: "agent crashed".into() }).await;
    link.barrier(id, 3).await;

    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "stuck");
    assert!(goal["reason"].as_str().unwrap().contains("check could not run"), "{goal}");
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
