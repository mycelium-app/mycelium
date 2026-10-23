mod common;

use common::{Server, eventually};
use mycelium_proto::conversation::{Conversation, ConversationStatus};
use mycelium_proto::link::NodeToControl;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const CLAUDE_SESSION: &str = "11111111-2222-3333-4444-555555555555";

fn claude_transcript(session: &str, prompt: &str, answer: &str) -> String {
    [
        json!({"type": "user", "sessionId": session, "cwd": "/work/app", "timestamp": "2026-10-01T10:00:00Z", "message": {"role": "user", "content": prompt}}),
        json!({"type": "assistant", "sessionId": session, "timestamp": "2026-10-01T10:00:05Z", "message": {"role": "assistant", "content": [{"type": "text", "text": answer}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}}),
        json!({"type": "user", "sessionId": session, "timestamp": "2026-10-01T10:00:09Z", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

async fn mirror(server: &Server, node_id: Uuid, path: &str, content: &str) {
    let full = server.data_dir.join("transcripts").join(node_id.to_string()).join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, content).unwrap();

    sqlx::query("insert into transcripts (node_id, path, bytes) values ($1, $2, $3)").bind(node_id).bind(path).bind(content.len() as i64).execute(&server.pool).await.unwrap();
}

async fn import(server: &Server, node_id: Uuid, agent: &str) -> Value {
    let (status, body) = server.user.json(server.user.post("/api/v1/imports").json(&json!({"node_id": node_id, "agent": agent}))).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn search(server: &Server, query: &str) -> Vec<Value> {
    let (status, body) = server.user.json(server.user.get("/api/v1/search").query(&[("q", query)])).await;
    assert_eq!(status, 200, "{body}");

    body.as_array().unwrap().clone()
}

fn status_of(statuses: &Value, node_id: Uuid, agent: &str) -> (i64, i64) {
    let row = statuses.as_array().unwrap().iter().find(|row| row["node_id"] == json!(node_id) && row["agent"] == agent).unwrap();

    (row["found"].as_i64().unwrap(), row["imported"].as_i64().unwrap())
}

#[tokio::test]
async fn importing_a_transcript_counts_its_turns_in_activity_without_usage() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &claude_transcript(CLAUDE_SESSION, "Fix the build", "Looking at it.")).await;

    import(&server, link.id, "claude").await;

    let (_, body) = server.user.json(server.user.get("/api/v1/activity?range=30d")).await;
    assert_eq!(body["totals"]["turns"], 1);
    assert_eq!(body["totals"]["reported_turns"], 0);
    assert_eq!(body["unreported_agents"], json!(["claude"]));
}

#[tokio::test]
async fn importing_a_claude_transcript_creates_a_done_conversation_with_events_and_streams_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &claude_transcript(CLAUDE_SESSION, "Fix the build\nplease", "Looking at it.")).await;

    let result = import(&server, link.id, "claude").await;
    assert_eq!(result["imported"], 1);

    let streamed: Conversation = stream
        .until(|message| match message {
            StreamMessage::Conversation(conversation) if conversation.title == "Fix the build" => Some(conversation),
            _ => None,
        })
        .await;
    assert_eq!(streamed.status, ConversationStatus::Done);
    assert_eq!(streamed.acp_session_id.as_deref(), Some(CLAUDE_SESSION));
    assert_eq!(streamed.cwd, "/work/app");
    assert_eq!(streamed.created_at.to_rfc3339(), "2026-10-01T10:00:00+00:00");
    assert_eq!(streamed.updated_at.to_rfc3339(), "2026-10-01T10:00:09+00:00");
    assert_eq!(streamed.archived_at.map(|at| at.to_rfc3339()).as_deref(), Some("2026-10-01T10:00:09+00:00"));

    let events = server.events(streamed.id).await;
    let kinds: Vec<&str> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["user_message", "acp_update", "acp_update", "acp_update", "turn_end"]);
    assert_eq!(events[0]["payload"]["text"], "Fix the build\nplease");
    assert_eq!(events[1]["payload"]["update"]["content"]["text"], "Looking at it.");
    assert_eq!(events[2]["payload"]["update"]["toolCallId"], "t1");
    assert_eq!(events[3]["payload"]["update"]["sessionUpdate"], "tool_call_update");
    assert_eq!(events[4]["payload"]["stop_reason"], "end_turn");
}

#[tokio::test]
async fn importing_twice_adds_nothing_and_the_counts_follow() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "One", "Reply one.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("bbbb", "Two", "Reply two.")).await;

    let (_, before) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&before, link.id, "claude"), (2, 0));
    assert_eq!(status_of(&before, link.id, "codex"), (0, 0));

    assert_eq!(import(&server, link.id, "claude").await["imported"], 2);
    assert_eq!(import(&server, link.id, "claude").await["imported"], 0);

    let (_, after) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&after, link.id, "claude"), (2, 2));
}

#[tokio::test]
async fn subagent_journals_and_sessions_that_already_have_a_conversation_are_not_imported() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "existing-session").await;
    eventually("session id stored", || async { (server.conversation(conversation.id).await["acp_session_id"] == "existing-session").then_some(()) }).await;

    mirror(&server, link.id, ".claude/projects/-work-app/existing-session.jsonl", &claude_transcript("existing-session", "Already here", "Yes.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/parent/subagents/workflows/run-1/journal.jsonl", &claude_transcript("parent", "Journal", "Entry.")).await;

    assert_eq!(import(&server, link.id, "claude").await["imported"], 0);

    let (_, statuses) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&statuses, link.id, "claude"), (1, 1));
}

#[tokio::test]
async fn importing_a_codex_rollout_reads_the_session_meta_and_response_items() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let rollout = [
        json!({"timestamp": "2026-10-02T09:00:00Z", "type": "session_meta", "payload": {"id": "0199aaaa-bbbb-cccc-dddd-eeeeffff0000", "cwd": "/work/codex-app"}}),
        json!({"timestamp": "2026-10-02T09:00:01Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Add a changelog"}]}}),
        json!({"timestamp": "2026-10-02T09:00:02Z", "type": "response_item", "payload": {"type": "function_call", "name": "shell", "call_id": "c1", "arguments": "{}"}}),
        json!({"timestamp": "2026-10-02T09:00:03Z", "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Changelog added."}]}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    mirror(&server, link.id, ".codex/sessions/2026/10/02/rollout-2026-10-02T09-00-00-0199aaaa-bbbb-cccc-dddd-eeeeffff0000.jsonl", &rollout).await;

    assert_eq!(import(&server, link.id, "codex").await["imported"], 1);

    let (_, conversations) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    let imported = &conversations.as_array().unwrap()[0];
    assert_eq!((imported["agent"].as_str(), imported["cwd"].as_str(), imported["title"].as_str()), (Some("codex"), Some("/work/codex-app"), Some("Add a changelog")));
    assert_eq!(imported["acp_session_id"], "0199aaaa-bbbb-cccc-dddd-eeeeffff0000");
}

#[tokio::test]
async fn importing_for_an_unknown_node_is_not_found() {
    let server = Server::start().await;

    let (status, _) = server.user.json(server.user.post("/api/v1/imports").json(&json!({"node_id": Uuid::new_v4(), "agent": "claude"}))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn search_finds_imported_conversations_with_a_highlighted_snippet() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "Rotate the signing keys", "The keystore is rotated and the pipeline passes.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("bbbb", "Tidy the readme", "Done tidying.")).await;
    import(&server, link.id, "claude").await;

    let hits = search(&server, "keystore").await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["title"], "Rotate the signing keys");
    assert_eq!(hits[0]["node_id"], json!(link.id));
    assert!(hits[0]["snippet"].as_str().unwrap().contains("**keystore**"), "{}", hits[0]);
}

#[tokio::test]
async fn search_only_returns_the_users_own_conversations() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "Rotate the signing keys", "Rotated.")).await;
    import(&server, link.id, "claude").await;
    let other = server.second_user("other@example.com").await;

    let (status, hits) = other.json(other.get("/api/v1/search").query(&[("q", "signing")])).await;

    assert_eq!((status, hits.as_array().unwrap().len()), (200, 0));
}

#[tokio::test]
async fn an_empty_search_says_what_to_enter() {
    let server = Server::start().await;

    for path in ["/api/v1/search", "/api/v1/search?q=%20%20"] {
        let (status, body) = server.user.json(server.user.get(path)).await;

        assert_eq!(status, 400);
        assert_eq!(body["message"], "Enter something to search for.");
    }
}

#[tokio::test]
async fn live_conversations_are_indexed_by_user_message_and_by_assistant_turn() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Plan the zebra migration")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    for (seq, text) in [(1, "The quagga "), (2, "table is next.")] {
        link.send(NodeToControl::AcpUpdate { conversation_id: conversation.id, seq, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}) }).await;
    }

    link.send(NodeToControl::TurnEnd { conversation_id: conversation.id, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation.id, 4).await;

    let by_prompt = search(&server, "zebra").await;
    let by_answer = search(&server, "quagga table").await;

    assert_eq!(by_prompt[0]["conversation_id"], json!(conversation.id));
    assert_eq!(by_answer[0]["conversation_id"], json!(conversation.id));
    assert!(by_answer[0]["snippet"].as_str().unwrap().contains("**quagga**"), "{}", by_answer[0]);
}

#[tokio::test]
async fn indexing_reports_counts_per_node() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "One", "Reply one.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("bbbb", "Two", "Reply two.")).await;
    import(&server, link.id, "claude").await;

    let (status, body) = server.user.json(server.user.get("/api/v1/indexing")).await;

    assert_eq!(status, 200);
    assert_eq!((body["conversations"].as_i64(), body["entries"].as_i64()), (Some(2), Some(4)));
    assert_eq!(body["nodes"], json!([{"node_id": link.id, "transcripts": 2, "imported": 2, "conversations": 2}]));
}

#[tokio::test]
async fn indexing_counts_only_the_users_own_conversations() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "One", "Reply one.")).await;
    import(&server, link.id, "claude").await;
    let other = server.second_user("other@example.com").await;

    let (status, body) = other.json(other.get("/api/v1/indexing")).await;

    assert_eq!(status, 200);
    assert_eq!((body["conversations"].as_i64(), body["entries"].as_i64()), (Some(0), Some(0)));
}

#[tokio::test]
async fn conversations_that_predate_the_search_index_are_backfilled_by_the_migration() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "Plan the okapi rollout", "The tapir stage comes first.")).await;
    import(&server, link.id, "claude").await;
    sqlx::query("delete from search_entries").execute(&server.pool).await.unwrap();
    assert_eq!(search(&server, "okapi").await.len(), 0);

    sqlx::raw_sql(include_str!("../migrations/0006_backfill_search_entries.sql")).execute(&server.pool).await.unwrap();

    assert_eq!(search(&server, "okapi").await.len(), 1);
    assert_eq!(search(&server, "tapir").await.len(), 1);
}

#[tokio::test]
async fn conversations_imported_before_archiving_are_archived_by_the_migration_and_others_are_left_alone() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "Plan the okapi rollout", "The tapir stage comes first.")).await;
    import(&server, link.id, "claude").await;
    let live = server.create_conversation(link.id, None).await;
    sqlx::query("update conversations set archived_at = null").execute(&server.pool).await.unwrap();

    sqlx::raw_sql(include_str!("../migrations/0007_archive_imports.sql")).execute(&server.pool).await.unwrap();

    let archived: Vec<(String, bool)> = sqlx::query_as("select title, archived_at is not null from conversations order by title").fetch_all(&server.pool).await.unwrap();
    assert_eq!(archived, vec![(live.title, false), ("Plan the okapi rollout".to_string(), true)]);
}

#[tokio::test]
async fn imports_continued_in_mycelium_are_restored_unless_archived_after_the_upgrade() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    for (session, title) in [("aaaa", "Continued"), ("bbbb", "Untouched"), ("cccc", "Archived later")] {
        mirror(&server, link.id, &format!(".claude/projects/-work-app/{session}.jsonl"), &claude_transcript(session, title, "Reply.")).await;
    }
    import(&server, link.id, "claude").await;
    sqlx::query("insert into events (conversation_id, seq, kind, payload, node_seq) select id, 100, 'turn_end', '{}', 1 from conversations where title in ('Continued', 'Archived later')").execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set archived_at = (select installed_on from _sqlx_migrations where version = 7) - interval '1 hour'").execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set archived_at = now() + interval '1 hour' where title = 'Archived later'").execute(&server.pool).await.unwrap();

    sqlx::raw_sql(include_str!("../migrations/0011_restore_continued_imports.sql")).execute(&server.pool).await.unwrap();

    let archived: Vec<(String, bool)> = sqlx::query_as("select title, archived_at is not null from conversations order by title").fetch_all(&server.pool).await.unwrap();
    assert_eq!(archived, vec![("Archived later".to_string(), true), ("Continued".to_string(), false), ("Untouched".to_string(), true)]);
}

#[tokio::test]
async fn a_restart_puts_back_the_last_activity_dates_that_archiving_overwrote() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &claude_transcript(CLAUDE_SESSION, "Fix the build", "Looking at it.")).await;
    import(&server, link.id, "claude").await;
    sqlx::query("update conversations set updated_at = now()").execute(&server.pool).await.unwrap();
    sqlx::query("delete from boot_repairs").execute(&server.pool).await.unwrap();

    let data_dir = server.data_dir.clone();
    let _restarted = Server::start_on(server.database_url.clone(), |config| config.data_dir = data_dir).await;

    let restored = eventually("restored date", || async {
        let at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("select updated_at from conversations").fetch_one(&server.pool).await.unwrap();
        (at.to_rfc3339() == "2026-10-01T10:00:09+00:00").then_some(at)
    })
    .await;
    assert_eq!(restored.to_rfc3339(), "2026-10-01T10:00:09+00:00");
}

#[tokio::test]
async fn unreadable_transcripts_are_counted_as_skipped() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    sqlx::query("insert into transcripts (node_id, path, bytes) values ($1, $2, 10)").bind(link.id).bind(".claude/projects/-work-app/gone.jsonl").execute(&server.pool).await.unwrap();

    let result = import(&server, link.id, "claude").await;

    assert_eq!((result["imported"].as_i64(), result["skipped"].as_i64()), (Some(0), Some(1)));
}
