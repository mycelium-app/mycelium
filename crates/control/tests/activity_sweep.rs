mod common;

use base64::Engine;
use common::{NodeLink, Server};
use mycelium_proto::link::NodeToControl;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

async fn sweeping_server() -> Server {
    Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await
}

async fn prompted_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("run it")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    conversation.id
}

async fn end_turn(link: &mut NodeLink, id: Uuid, seq: i64) {
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq, stop_reason: "end_turn".into(), usage: None }).await;
}

async fn wait_for(server: &Server, id: Uuid, status: &str, agent_active: bool) {
    common::eventually("the conversation to reach the expected state", || async {
        let conversation = server.conversation(id).await;

        (conversation["status"] == status && conversation["agent_active"] == agent_active).then_some(())
    })
    .await;
}

async fn start_agent(link: &mut NodeLink, id: Uuid, agent: &str) {
    link.send(NodeToControl::Workflow { conversation_id: id, run_id: "run-1".into(), line: json!({"type": "started", "agentId": agent}) }).await;
}

async fn mirror_agent_transcript(server: &Server, link: &mut NodeLink, agent: &str) {
    let path = format!(".claude/projects/-work-app/acp-1/subagents/agent-{agent}.jsonl");
    let data = base64::engine::general_purpose::STANDARD.encode("{}\n");
    link.send(NodeToControl::Transcript { path: path.clone(), offset: 0, data, size: Some(3) }).await;

    common::eventually("the transcript to be stored", || async {
        sqlx::query_scalar::<_, bool>("select exists (select 1 from transcripts where path = $1)").bind(&path).fetch_one(&server.pool).await.unwrap().then_some(())
    })
    .await;
}

async fn age_workflow_events(server: &Server, id: Uuid) {
    sqlx::query("update events set created_at = now() - interval '20 minutes' where conversation_id = $1 and kind = 'workflow'").bind(id).execute(&server.pool).await.unwrap();
}

async fn age_transcripts(server: &Server, agent: &str) {
    sqlx::query("update transcripts set updated_at = now() - interval '20 minutes' where path like $1").bind(format!("%/agent-{agent}.jsonl")).execute(&server.pool).await.unwrap();
}

async fn age_agent_activity(server: &Server, id: Uuid) {
    sqlx::query("update conversations set last_activity_at = now() - interval '1 minute' where id = $1").bind(id).execute(&server.pool).await.unwrap();
}

#[tokio::test]
async fn the_agent_is_active_from_a_prompt_until_its_turn_ends() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;

    wait_for(&server, id, "working", true).await;

    end_turn(&mut link, id, 1).await;

    wait_for(&server, id, "review", false).await;
}

#[tokio::test]
async fn agent_output_after_a_turn_has_ended_makes_the_conversation_working_until_it_goes_quiet() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    end_turn(&mut link, id, 1).await;
    wait_for(&server, id, "review", false).await;
    sqlx::query("update events set created_at = now() - interval '10 seconds' where conversation_id = $1 and kind = 'turn_end'").bind(id).execute(&server.pool).await.unwrap();

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 2, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Background task finished."}}}) }).await;

    wait_for(&server, id, "working", true).await;

    age_agent_activity(&server, id).await;

    wait_for(&server, id, "review", false).await;
}

#[tokio::test]
async fn output_trailing_just_after_a_turn_ends_leaves_the_conversation_in_review() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    end_turn(&mut link, id, 1).await;
    wait_for(&server, id, "review", false).await;

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 2, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "late"}}}) }).await;
    common::eventually("the trailing chunk to be stored", || async { server.events(id).await.iter().any(|event| event["payload"]["update"]["content"]["text"] == "late").then_some(()) }).await;

    let conversation = server.conversation(id).await;
    assert_eq!((conversation["status"].clone(), conversation["agent_active"].clone()), (json!("review"), json!(false)));
}

#[tokio::test]
async fn a_workflow_agent_without_recent_news_no_longer_keeps_the_conversation_working() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let quiet = prompted_conversation(&server, &mut link).await;
    start_agent(&mut link, quiet, "quiet").await;
    end_turn(&mut link, quiet, 1).await;
    wait_for(&server, quiet, "working", false).await;

    let live = prompted_conversation(&server, &mut link).await;
    start_agent(&mut link, live, "live").await;
    mirror_agent_transcript(&server, &mut link, "live").await;
    end_turn(&mut link, live, 1).await;
    wait_for(&server, live, "working", false).await;

    age_workflow_events(&server, quiet).await;
    age_workflow_events(&server, live).await;

    wait_for(&server, quiet, "review", false).await;

    assert_eq!(server.conversation(live).await["status"], "working");
}

#[tokio::test]
async fn a_workflow_agent_whose_transcript_went_quiet_no_longer_keeps_the_conversation_working() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    start_agent(&mut link, id, "stalled").await;
    mirror_agent_transcript(&server, &mut link, "stalled").await;
    end_turn(&mut link, id, 1).await;
    wait_for(&server, id, "working", false).await;

    age_transcripts(&server, "stalled").await;

    wait_for(&server, id, "review", false).await;
}

#[tokio::test]
async fn a_cancelled_turn_stays_stopped_even_with_a_live_workflow_agent() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    start_agent(&mut link, id, "live").await;
    link.send(NodeToControl::TurnEnd { conversation_id: id, seq: 1, stop_reason: "cancelled".into(), usage: None }).await;

    wait_for(&server, id, "review", false).await;
}

#[tokio::test]
async fn a_failed_session_clears_agent_active() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    wait_for(&server, id, "working", true).await;

    link.send(NodeToControl::SessionFailed { conversation_id: id, seq: 1, message: "boom".into() }).await;

    common::eventually("agent_active to clear", || async { (server.conversation(id).await["agent_active"] == false).then_some(()) }).await;
}

#[tokio::test]
async fn archiving_a_conversation_clears_agent_active() {
    let server = sweeping_server().await;
    let mut link = server.online_node("laptop").await;
    let id = prompted_conversation(&server, &mut link).await;
    wait_for(&server, id, "working", true).await;

    let url = format!("/api/v1/conversations/{id}");
    server.user.json(server.user.patch(&url).json(&json!({"archived": true}))).await;

    assert_eq!(server.conversation(id).await["agent_active"], false);
}
