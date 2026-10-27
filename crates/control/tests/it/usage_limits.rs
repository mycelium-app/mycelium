use crate::common;
use chrono::{DateTime, Duration, Utc};
use crate::common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const CONTINUE: &str = "The usage limit has reset. Continue from where you stopped.";

async fn ticking_server() -> Server {
    Server::start_with(|config| config.scheduler_tick = std::time::Duration::from_millis(50)).await
}

fn limit_message(hours_from_now: i64) -> String {
    let resets = Utc::now() + Duration::hours(hours_from_now);

    format!("Internal error: You've hit your session limit · resets {} (UTC)", resets.format("%-I:%M%P"))
}

async fn working_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Build it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    conversation.id
}

async fn hit_limit(server: &Server, link: &mut NodeLink, id: Uuid, message: &str) {
    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 3, message: message.into() }).await;
    common::eventually("the conversation to fail", || async { (server.conversation(id).await["status"] == "failed").then_some(()) }).await;
}

async fn resume_at(server: &Server, id: Uuid) -> Option<DateTime<Utc>> {
    server.conversation(id).await["resume_at"].as_str().map(|at| at.parse().unwrap())
}

async fn make_due(server: &Server, id: Uuid) {
    sqlx::query("update conversations set resume_at = now() - interval '1 minute' where id = $1").bind(id).execute(&server.pool).await.unwrap();
}

async fn choose_automatic_resume(server: &Server) {
    let (status, _) = server.user.json(server.user.put("/api/v1/usage-limit").json(&json!({"on_usage_limit": "resume"}))).await;

    assert_eq!(status, 200);
}

async fn choose_to_be_asked(server: &Server) {
    let (status, _) = server.user.json(server.user.put("/api/v1/usage-limit").json(&json!({"on_usage_limit": "ask"}))).await;

    assert_eq!(status, 200);
}

async fn reply_with_limit(server: &Server, link: &mut NodeLink, id: Uuid, seq: i64, text: &str) {
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: seq + 1, stop_reason: "end_turn".into(), usage: None }).await;
    common::eventually("the conversation to fail", || async { (server.conversation(id).await["status"] == "failed").then_some(()) }).await;
}

async fn schedule(server: &Server, id: Uuid) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/resume"))).await
}

async fn last_error(server: &Server, id: Uuid) -> Value {
    server.events(id).await.into_iter().rev().find(|event| event["kind"] == "error").unwrap()["payload"].clone()
}

#[tokio::test]
async fn a_usage_limit_failure_records_which_window_and_when_it_resets() {
    let server = Server::start().await;
    choose_to_be_asked(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    hit_limit(&server, &mut link, id, &limit_message(2)).await;

    let payload = last_error(&server, id).await;
    assert_eq!(payload["usage_limit"]["window"], "five_hour");
    let resets_at: DateTime<Utc> = payload["usage_limit"]["resets_at"].as_str().unwrap().parse().unwrap();
    assert!(resets_at > Utc::now() + Duration::hours(1), "{payload}");
    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn another_failure_is_not_a_usage_limit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    hit_limit(&server, &mut link, id, "The agent could not start: Method not found").await;

    assert!(last_error(&server, id).await.get("usage_limit").is_none());
}

#[tokio::test]
async fn choosing_to_resume_schedules_it_a_minute_after_the_reset_and_streams_it() {
    let server = Server::start().await;
    choose_to_be_asked(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    let payload = last_error(&server, id).await;
    let resets_at: DateTime<Utc> = payload["usage_limit"]["resets_at"].as_str().unwrap().parse().unwrap();
    let mut stream = server.user.stream().await;

    let (status, _) = schedule(&server, id).await;

    assert_eq!(status, 204);
    assert_eq!(resume_at(&server, id).await, Some(resets_at + Duration::minutes(1)));
    stream.until(|message| matches!(message, StreamMessage::Conversation(conversation) if conversation.id == id && conversation.resume_at.is_some()).then_some(())).await;
}

#[tokio::test]
async fn resuming_without_a_known_reset_time_asks_for_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, "You've hit your usage limit. Try again later.").await;

    let (status, body) = schedule(&server, id).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("no_reset_time")));
    let chosen = Utc::now() + Duration::hours(3);
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/resume")).json(&json!({"at": chosen}))).await;
    assert_eq!(status, 204);
    assert_eq!(resume_at(&server, id).await.map(|at| at.timestamp()), Some(chosen.timestamp()));
}

#[tokio::test]
async fn a_conversation_that_has_not_failed_cannot_be_scheduled_to_resume() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    let (status, body) = schedule(&server, id).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("not_failed")));
}

#[tokio::test]
async fn cancelling_a_pending_resume_clears_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/conversations/{id}/resume"))).await;

    assert_eq!(status, 204);
    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn a_due_resume_retries_the_session_and_sends_continue() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;

    make_due(&server, id).await;

    match link.next().await {
        ControlToNode::StartSession { conversation_id, resume, .. } => assert_eq!((conversation_id, resume.as_deref()), (id, Some("acp-1"))),
        other => panic!("expected start_session, got {other:?}"),
    }
    assert_eq!(link.expect_prompt().await.1, CONTINUE);
    assert_eq!(resume_at(&server, id).await, None);
    let messages: Vec<Value> = server.events(id).await.into_iter().filter(|event| event["kind"] == "user_message").collect();
    assert_eq!(messages.last().unwrap()["payload"]["text"], CONTINUE);
}

#[tokio::test]
async fn a_due_resume_sends_what_was_queued_instead_of_continue() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    sqlx::query("insert into queued_prompts (conversation_id, position, text) values ($1, 1, 'waiting')").bind(id).execute(&server.pool).await.unwrap();
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;

    make_due(&server, id).await;

    link.expect_start_session().await;
    assert_eq!(link.expect_prompt().await.1, "waiting");
}

#[tokio::test]
async fn a_resume_that_is_not_due_yet_waits() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    assert!(resume_at(&server, id).await.is_some());
    assert_eq!(server.conversation(id).await["status"], "failed");
}

#[tokio::test]
async fn a_resume_whose_node_is_offline_tries_again_later_without_using_up_an_attempt() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;
    drop(link);
    common::eventually("the node to go offline", || async { (server.conversation(id).await["status"] == "failed").then_some(()) }).await;

    make_due(&server, id).await;

    common::eventually("the resume to be put back", || async { resume_at(&server, id).await.filter(|at| *at > Utc::now()) }).await;
    let attempts = sqlx::query_scalar::<_, i32>("select limit_resumes from conversations where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn a_workflow_paused_conversation_whose_node_is_offline_when_due_still_resumes_later() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "failed", "key": "k1", "agentId": "aaa", "reason": limit_message(2)}) }).await;
    common::eventually("the conversation to be paused", || async { resume_at(&server, id).await }).await;
    assert_ne!(server.conversation(id).await["status"], "failed");
    drop(link);
    common::eventually("the node to go offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    make_due(&server, id).await;

    common::eventually("the pause to be put back", || async { resume_at(&server, id).await.filter(|at| *at > Utc::now()) }).await;
}

#[tokio::test]
async fn the_default_is_to_resume_automatically_and_the_choice_is_kept() {
    let server = Server::start().await;

    let (_, initial) = server.user.json(server.user.get("/api/v1/usage-limit")).await;
    choose_to_be_asked(&server).await;
    let (_, chosen) = server.user.json(server.user.get("/api/v1/usage-limit")).await;

    assert_eq!(initial["on_usage_limit"], "resume");
    assert_eq!(chosen["on_usage_limit"], "ask");
}

#[tokio::test]
async fn the_first_limit_reply_stops_the_conversation_with_a_card_and_resumes_by_default() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    reply_with_limit(&server, &mut link, id, 3, &limit_message(2)).await;

    let payload = last_error(&server, id).await;
    assert_eq!(payload["usage_limit"]["window"], "five_hour");
    let resumes = common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;
    assert!(resumes > Utc::now() + Duration::hours(1));
}

#[tokio::test]
async fn a_limit_reply_streamed_in_several_chunks_still_stops_the_conversation_with_a_card() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let message = limit_message(2);
    let (head, tail) = message.split_at(message.len() - 6);

    for (offset, text) in [head, tail].into_iter().enumerate() {
        link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 3 + offset as i64, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}) }).await;
    }
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 5, stop_reason: "end_turn".into(), usage: None }).await;

    common::eventually("the conversation to fail", || async { (server.conversation(id).await["status"] == "failed").then_some(()) }).await;
    assert_eq!(last_error(&server, id).await["usage_limit"]["window"], "five_hour");
}

#[tokio::test]
async fn a_limit_reply_after_a_long_run_of_earlier_text_still_gets_the_card() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let drafted = format!("Drafting them now. {}", "The first section covers the plan in detail. ".repeat(12));

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 3, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": drafted}}}) }).await;
    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 4, update: json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Write", "kind": "edit", "status": "completed"}}) }).await;
    reply_with_limit(&server, &mut link, id, 5, &limit_message(2)).await;

    assert_eq!(last_error(&server, id).await["usage_limit"]["window"], "five_hour");
    let resumes = common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;
    assert!(resumes > Utc::now() + Duration::hours(1));
}

#[tokio::test]
async fn a_limit_reply_on_its_own_line_after_text_in_the_same_message_gets_the_card() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let drafted = format!("{}\n\n", "The first section covers the plan in detail. ".repeat(12));

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 3, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": drafted}}}) }).await;
    reply_with_limit(&server, &mut link, id, 4, &limit_message(2)).await;

    assert_eq!(last_error(&server, id).await["usage_limit"]["window"], "five_hour");
}

#[tokio::test]
async fn a_limit_reply_waits_for_the_person_when_they_chose_to_be_asked() {
    let server = Server::start().await;
    choose_to_be_asked(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    reply_with_limit(&server, &mut link, id, 3, &limit_message(2)).await;

    assert_eq!(last_error(&server, id).await["usage_limit"]["window"], "five_hour");
    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn a_long_answer_that_mentions_a_limit_is_not_a_usage_limit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let essay = format!("{} Your session limit is shown in the settings. ", "The report is ready.".repeat(40));

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 3, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": essay}}}) }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 4, stop_reason: "end_turn".into(), usage: None }).await;

    common::eventually("the turn to end", || async { (server.conversation(id).await["status"] == "review").then_some(()) }).await;
    assert!(server.events(id).await.iter().all(|event| event["kind"] != "error"));
}

#[tokio::test]
async fn a_second_limit_reply_before_the_reset_keeps_one_automatic_resume() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    reply_with_limit(&server, &mut link, id, 3, &limit_message(2)).await;
    common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "cont."}))).await;
    link.expect_start_session().await;
    link.start_session(id, "s2").await;
    link.expect_prompt().await;

    reply_with_limit(&server, &mut link, id, 6, &limit_message(2)).await;

    common::eventually("the resume to be scheduled again", || async { resume_at(&server, id).await }).await;
}

#[tokio::test]
async fn a_workflow_agent_that_hits_the_limit_pauses_its_conversation_until_the_reset() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    let failed = json!({"type": "failed", "key": "k1", "agentId": "aaa", "reason": limit_message(2)});

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: failed }).await;

    let resumes = common::eventually("the conversation to be paused", || async { resume_at(&server, id).await }).await;
    assert!(resumes > Utc::now() + Duration::hours(1));
    let window = common::eventually("the workflow event to carry the limit", || async {
        let events = server.events(id).await;
        events.iter().rev().find(|event| event["kind"] == "workflow").map(|event| event["payload"]["usage_limit"]["window"].clone())
    })
    .await;
    assert_eq!(window, "five_hour");
}

#[tokio::test]
async fn a_paused_workflow_resumes_its_conversation_when_the_limit_resets() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "failed", "key": "k1", "agentId": "aaa", "reason": limit_message(2)}) }).await;
    common::eventually("the conversation to be paused", || async { resume_at(&server, id).await }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 5, stop_reason: "end_turn".into(), usage: None }).await;
    common::eventually("the turn to end", || async { (server.conversation(id).await["status"] == "review").then_some(()) }).await;

    make_due(&server, id).await;

    assert_eq!(link.expect_prompt().await.1, CONTINUE);
}

#[tokio::test]
async fn a_workflow_failure_that_is_not_a_usage_limit_leaves_the_conversation_alone() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "failed", "key": "k1", "agentId": "aaa", "reason": "Stopped responding"}) }).await;
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "key": "k2", "agentId": "bbb"}) }).await;

    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn with_automatic_resume_chosen_a_usage_limit_schedules_itself() {
    let server = Server::start().await;
    choose_automatic_resume(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;

    hit_limit(&server, &mut link, id, &limit_message(2)).await;

    common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;
}

#[tokio::test]
async fn a_limit_that_keeps_coming_back_stops_scheduling_itself_after_three_resumes() {
    let server = Server::start().await;
    choose_automatic_resume(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    sqlx::query("update conversations set limit_resumes = 3 where id = $1").bind(id).execute(&server.pool).await.unwrap();

    hit_limit(&server, &mut link, id, &limit_message(2)).await;

    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn a_finished_turn_resets_the_count_of_automatic_resumes() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    sqlx::query("update conversations set limit_resumes = 2 where id = $1").bind(id).execute(&server.pool).await.unwrap();

    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;

    common::eventually("the count to be reset", || async { (sqlx::query_scalar::<_, i32>("select limit_resumes from conversations where id = $1").bind(id).fetch_one(&server.pool).await.unwrap() == 0).then_some(()) }).await;
}

#[tokio::test]
async fn retrying_now_cancels_the_pending_resume() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    schedule(&server, id).await;

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/retry"))).await;

    assert_eq!(resume_at(&server, id).await, None);
}

#[tokio::test]
async fn a_goal_is_held_when_its_usage_limit_will_resume_automatically() {
    let server = Server::start().await;
    choose_automatic_resume(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": "Every test passes", "token_budget": 100000}))).await;

    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;

    let (_, goal) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/goal"))).await;
    assert_eq!(goal["state"], "active");
    assert!(goal["held_since"].is_string(), "{goal}");
    assert!(goal["resumes_at"].is_string(), "{goal}");
}

#[tokio::test]
async fn a_held_goal_carries_on_when_the_limit_resets_and_keeps_its_counts() {
    let server = ticking_server().await;
    choose_automatic_resume(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": "Every test passes", "token_budget": 100000, "time_limit_seconds": 7200}))).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    common::eventually("the resume to be scheduled", || async { resume_at(&server, id).await }).await;
    sqlx::query("update goals set held_since = now() - interval '1 hour' where conversation_id = $1").bind(id).execute(&server.pool).await.unwrap();
    let started: Value = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/goal"))).await.1;

    make_due(&server, id).await;

    loop {
        if let ControlToNode::Prompt { text, .. } = link.next().await {
            if text.contains("towards this goal") || text.starts_with("Continue towards the goal") {
                assert!(text.ends_with("Every test passes"), "{text}");
                break;
            }
        }
    }
    let goal = common::eventually("the hold to be released", || async {
        let (_, goal) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/goal"))).await;

        goal["held_since"].is_null().then_some(goal)
    })
    .await;
    assert_eq!((goal["state"].as_str(), goal["started_at"].clone(), goal["token_budget"].as_i64()), (Some("active"), started["started_at"].clone(), Some(100000)));
    assert!(goal["held_seconds"].as_i64().unwrap() >= 3600, "{goal}");
}

#[tokio::test]
async fn a_goal_is_held_on_a_usage_limit_when_the_person_chose_to_be_asked() {
    let server = Server::start().await;
    choose_to_be_asked(&server).await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    server.user.json(server.user.put(&format!("/api/v1/conversations/{id}/goal")).json(&json!({"text": "Every test passes", "token_budget": 100000}))).await;

    hit_limit(&server, &mut link, id, &limit_message(2)).await;

    let (_, goal) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/goal"))).await;
    assert_eq!(goal["state"], "active");
    assert!(goal["held_since"].is_string(), "{goal}");
}

#[tokio::test]
async fn another_person_cannot_schedule_or_cancel_a_resume() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = working_conversation(&server, &mut link).await;
    hit_limit(&server, &mut link, id, &limit_message(2)).await;
    let other = server.second_user("other@example.com").await;

    let (scheduled, _) = other.json(other.post(&format!("/api/v1/conversations/{id}/resume"))).await;
    let (cancelled, _) = other.json(other.delete(&format!("/api/v1/conversations/{id}/resume"))).await;

    assert_eq!((scheduled, cancelled), (404, 404));
}
