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
async fn a_message_from_the_user_pauses_an_active_goal_and_a_check_answering_later_changes_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    set_goal(&server, id, "Every test passes", 100_000).await;
    end_turn(&mut link, id, 2, 0).await;
    let (check, _) = expect_check(&mut link).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Actually, skip the e2e tests"}))).await;
    assert_eq!(status, 204);
    let goal = goal_of(&server, id).await;
    assert_eq!(goal["state"], "paused");
    assert!(goal["reason"].as_str().unwrap().contains("you sent a message"), "{goal}");
    assert_eq!(link.expect_prompt().await.1, "Actually, skip the e2e tests");

    answer_check(&mut link, check, "{\"verdict\": \"met\", \"reason\": \"Done.\"}").await;
    settled(&mut link, check).await;

    assert_eq!(goal_of(&server, id).await["state"], "paused");
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
