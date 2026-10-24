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

async fn mirror_aged(server: &Server, node_id: Uuid, path: &str, content: &str, age_seconds: u64) {
    let full = server.data_dir.join("transcripts").join(node_id.to_string()).join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, content).unwrap();
    age(&full, age_seconds);

    sqlx::query("insert into transcripts (node_id, path, bytes) values ($1, $2, $3)").bind(node_id).bind(path).bind(content.len() as i64).execute(&server.pool).await.unwrap();
}

fn age(full: &std::path::Path, seconds: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(full).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(seconds)).unwrap();
}

async fn mirror(server: &Server, node_id: Uuid, path: &str, content: &str) {
    mirror_aged(server, node_id, path, content, 3600).await;
}

async fn start_import(server: &Server, node_id: Uuid, agent: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/imports").json(&json!({"node_id": node_id, "agent": agent}))).await
}

async fn finished_job(server: &Server, node_id: Uuid, agent: &str) -> Value {
    eventually("the import to finish", || async {
        let (_, jobs) = server.user.json(server.user.get("/api/v1/imports/jobs")).await;
        let job = jobs.as_array().unwrap().iter().find(|job| job["node_id"] == json!(node_id) && job["agent"] == agent)?.clone();

        (job["state"] == "finished").then_some(job)
    })
    .await
}

async fn import(server: &Server, node_id: Uuid, agent: &str) -> Value {
    let (status, body) = start_import(server, node_id, agent).await;
    assert_eq!(status, 202, "{body}");

    finished_job(server, node_id, agent).await
}

async fn imported_conversation(server: &Server, session: &str) -> Value {
    let id = sqlx::query_scalar::<_, Uuid>("select id from conversations where acp_session_id = $1").bind(session).fetch_one(&server.pool).await.unwrap();

    server.conversation(id).await
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

fn usage_transcript(session: &str, day: chrono::NaiveDate) -> String {
    let at = |time: &str| format!("{day}T{time}Z");
    let usage = json!({"input_tokens": 100, "output_tokens": 50, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 2000});

    [
        json!({"type": "user", "sessionId": session, "cwd": "/work/app", "timestamp": at("10:00:00"), "message": {"role": "user", "content": "Fix the build"}}),
        json!({"type": "assistant", "sessionId": session, "timestamp": at("10:00:05"), "message": {"id": "m1", "role": "assistant", "model": "claude-opus-5", "content": [{"type": "text", "text": "Looking."}], "usage": usage}}),
        json!({"type": "assistant", "sessionId": session, "timestamp": at("10:00:30"), "message": {"id": "m1", "role": "assistant", "model": "claude-opus-5", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}], "usage": usage}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

fn yesterday() -> chrono::NaiveDate {
    chrono::Utc::now().date_naive() - chrono::Duration::days(1)
}

async fn activity_day(server: &Server, day: chrono::NaiveDate) -> Value {
    let (_, body) = server.user.json(server.user.get("/api/v1/activity?range=7d")).await;

    body["days"].as_array().unwrap().iter().find(|row| row["key"] == day.to_string()).unwrap().clone()
}

#[tokio::test]
async fn importing_a_transcript_with_usage_fills_activity_tokens_time_working_and_an_estimated_cost_on_its_day() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &usage_transcript(CLAUDE_SESSION, yesterday())).await;

    import(&server, link.id, "claude").await;

    let row = activity_day(&server, yesterday()).await;
    assert_eq!((row["turns"].clone(), row["reported_turns"].clone()), (json!(1), json!(1)));
    assert_eq!((row["input_tokens"].clone(), row["output_tokens"].clone(), row["cache_read_tokens"].clone(), row["cache_write_tokens"].clone()), (json!(100), json!(50), json!(2000), json!(1000)));
    assert_eq!(row["working_ms"], 30_000);
    assert_eq!(row["cost_estimated"], true);
    assert!((row["cost_usd"].as_f64().unwrap() - (100.0 * 5.0 + 50.0 * 25.0 + 2000.0 * 0.5 + 1000.0 * 6.25) / 1_000_000.0).abs() < 1e-9);
}

#[tokio::test]
async fn a_restart_fills_the_usage_of_conversations_imported_before_usage_was_read() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &usage_transcript(CLAUDE_SESSION, yesterday())).await;
    import(&server, link.id, "claude").await;
    sqlx::query("update usage_turns set reported = false, input_tokens = 0, output_tokens = 0, cache_read_tokens = 0, cache_write_tokens = 0, working_ms = null, cost_usd = null, cost_estimated = false, day = day - 5").execute(&server.pool).await.unwrap();
    sqlx::query("delete from boot_repairs").execute(&server.pool).await.unwrap();

    let data_dir = server.data_dir.clone();
    let _restarted = Server::start_on(server.database_url.clone(), |config| config.data_dir = data_dir).await;

    let row = eventually("repaired usage", || async {
        let row = activity_day(&server, yesterday()).await;
        (row["input_tokens"] == 100).then_some(row)
    })
    .await;
    assert_eq!((row["turns"].clone(), row["working_ms"].clone(), row["cost_estimated"].clone()), (json!(1), json!(30_000), json!(true)));

    sqlx::query("delete from boot_repairs").execute(&server.pool).await.unwrap();
    let data_dir = server.data_dir.clone();
    let _again = Server::start_on(server.database_url.clone(), |config| config.data_dir = data_dir).await;
    eventually("second repair", || async {
        let count: i64 = sqlx::query_scalar("select count(*) from boot_repairs where name = 'import_usage'").fetch_one(&server.pool).await.unwrap();
        (count == 1).then_some(())
    })
    .await;
    let rows: i64 = sqlx::query_scalar("select count(*) from usage_turns").fetch_one(&server.pool).await.unwrap();
    assert_eq!(rows, 1);
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

const PARENT_SESSION: &str = "99999999-8888-7777-6666-555555555555";

fn parent_transcript_with_two_agents() -> String {
    [
        json!({"type": "user", "sessionId": PARENT_SESSION, "cwd": "/work/app", "timestamp": "2026-10-01T10:00:00Z", "message": {"role": "user", "content": "Review the change"}}),
        json!({"type": "assistant", "sessionId": PARENT_SESSION, "timestamp": "2026-10-01T10:00:02Z", "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Task", "input": {"description": "Check the build", "prompt": "Run the build and report"}},
            {"type": "tool_use", "id": "t2", "name": "Task", "input": {"description": "Check the docs", "prompt": "Read the docs and report"}}
        ]}}),
        json!({"type": "user", "sessionId": PARENT_SESSION, "timestamp": "2026-10-01T10:01:00Z", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "done"}]}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

fn subagent_transcript(agent: &str, prompt: &str, answer: &str, started: &str, finished: &str) -> String {
    [
        json!({"type": "user", "isSidechain": true, "agentId": agent, "sessionId": PARENT_SESSION, "cwd": "/work/app", "timestamp": started, "message": {"role": "user", "content": prompt}}),
        json!({"type": "assistant", "isSidechain": true, "agentId": agent, "sessionId": PARENT_SESSION, "timestamp": finished, "message": {"role": "assistant", "content": [{"type": "text", "text": answer}]}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

fn subagent_path(agent: &str) -> String {
    format!(".claude/projects/-work-app/{PARENT_SESSION}/subagents/agent-{agent}.jsonl")
}

async fn side_chats_of(server: &Server, parent: &str) -> Vec<Value> {
    let mut side_chats = Vec::new();

    for archived in [false, true] {
        let (_, conversations) = server.user.json(server.user.get(&format!("/api/v1/conversations?archived={archived}"))).await;
        side_chats.extend(conversations.as_array().unwrap().iter().filter(|conversation| conversation["parent_id"] == parent).cloned());
    }

    side_chats.sort_by_key(|conversation| conversation["created_at"].as_str().unwrap().to_string());

    side_chats
}

#[tokio::test]
async fn importing_a_transcript_with_two_subagents_creates_two_read_only_side_chats_under_it_in_order() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{PARENT_SESSION}.jsonl"), &parent_transcript_with_two_agents()).await;
    mirror(&server, link.id, &subagent_path("aaa"), &subagent_transcript("aaa", "Read the docs and report", "Docs are current.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z")).await;
    mirror(&server, link.id, &subagent_path("zzz"), &subagent_transcript("zzz", "Run the build and report", "Build passes.", "2026-10-01T10:00:05Z", "2026-10-01T10:00:15Z")).await;

    let (_, before) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&before, link.id, "claude"), (1, 0));

    let job = import(&server, link.id, "claude").await;
    assert_eq!(job["imported"], 3);

    let parent = imported_conversation(&server, PARENT_SESSION).await;
    let side_chats = side_chats_of(&server, parent["id"].as_str().unwrap()).await;
    let titles: Vec<&str> = side_chats.iter().map(|chat| chat["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Check the build", "Check the docs"]);
    assert!(side_chats.iter().all(|chat| chat["side_kind"] == "subagent" && chat["status"] == "done" && chat["cwd"] == "/work/app"));
    assert_eq!(side_chats[0]["created_at"], "2026-10-01T10:00:05Z");
    assert_eq!(side_chats[0]["updated_at"], "2026-10-01T10:00:15Z");

    let id = Uuid::parse_str(side_chats[0]["id"].as_str().unwrap()).unwrap();
    let events = server.events(id).await;
    let kinds: Vec<&str> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["user_message", "acp_update", "turn_end"]);
    assert_eq!(events[0]["payload"]["text"], "Run the build and report");
    assert_eq!(events[1]["payload"]["update"]["content"]["text"], "Build passes.");

    let (_, after) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&after, link.id, "claude"), (1, 1));
    assert_eq!(import(&server, link.id, "claude").await["imported"], 0);
}

#[tokio::test]
async fn a_subagent_without_a_matching_task_call_is_titled_from_its_first_prompt() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{PARENT_SESSION}.jsonl"), &claude_transcript(PARENT_SESSION, "Start", "Started.")).await;
    mirror(&server, link.id, &subagent_path("aaa"), &subagent_transcript("aaa", "Count the files\nin the repo", "Forty.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z")).await;

    import(&server, link.id, "claude").await;

    let parent = imported_conversation(&server, PARENT_SESSION).await;
    let side_chats = side_chats_of(&server, parent["id"].as_str().unwrap()).await;
    assert_eq!(side_chats[0]["title"], "Count the files");
}

#[tokio::test]
async fn a_subagent_whose_parent_is_not_mirrored_is_neither_counted_nor_imported() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &subagent_path("aaa"), &subagent_transcript("aaa", "Orphan", "Alone.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z")).await;

    let (_, statuses) = server.user.json(server.user.get("/api/v1/imports")).await;

    assert_eq!(status_of(&statuses, link.id, "claude"), (0, 0));
}

#[tokio::test]
async fn a_file_whose_upload_has_not_reached_its_full_size_is_recorded_as_still_uploading() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let path = format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl");
    let content = claude_transcript(CLAUDE_SESSION, "Half sent", "Partial.");
    mirror(&server, link.id, &path, &content).await;
    sqlx::query("update transcripts set size_bytes = $3 where node_id = $1 and path = $2").bind(link.id).bind(&path).bind(content.len() as i64 + 500).execute(&server.pool).await.unwrap();

    let job = import(&server, link.id, "claude").await;

    assert_eq!((job["imported"].clone(), job["failed"][0]["path"].clone(), job["failed"][0]["reason"].clone()), (json!(0), json!(path), json!("Still uploading.")));

    sqlx::query("update transcripts set size_bytes = bytes where node_id = $1 and path = $2").bind(link.id).bind(&path).execute(&server.pool).await.unwrap();

    assert_eq!(import(&server, link.id, "claude").await["imported"], 1);
}

#[tokio::test]
async fn a_subagent_transcript_streamed_live_becomes_a_side_chat_that_grows_with_the_file() {
    use base64::Engine;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(parent.id, PARENT_SESSION).await;
    eventually("session id stored", || async { (server.conversation(parent.id).await["acp_session_id"] == PARENT_SESSION).then_some(()) }).await;
    let mut stream = server.user.stream().await;
    let path = subagent_path("aaa");
    let first = subagent_transcript("aaa", "Read the docs and report", "Looking.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z") + "\n";
    let encode = |text: &str| base64::engine::general_purpose::STANDARD.encode(text);

    link.send(NodeToControl::Transcript { path: path.clone(), offset: 0, data: encode(&first), size: Some(first.len() as i64) }).await;

    let created: Conversation = stream
        .until(|message| match message {
            StreamMessage::Conversation(conversation) if conversation.parent_id == Some(parent.id) => Some(conversation),
            _ => None,
        })
        .await;
    assert_eq!((created.title.as_str(), created.archived_at), ("Read the docs and report", None));
    assert_eq!(server.events(created.id).await.len(), 3);

    let more = json!({"type": "assistant", "isSidechain": true, "agentId": "aaa", "sessionId": PARENT_SESSION, "timestamp": "2026-10-01T10:00:40Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "Docs are current."}]}}).to_string() + "\n";
    link.send(NodeToControl::Transcript { path: path.clone(), offset: first.len() as i64, data: encode(&more), size: Some((first.len() + more.len()) as i64) }).await;

    eventually("the side chat to carry the new answer", || async { (server.events(created.id).await.len() == 4).then_some(()) }).await;
    assert_eq!(side_chats_of(&server, &parent.id.to_string()).await.len(), 1);
}

#[tokio::test]
async fn a_workflow_agents_side_chat_is_titled_by_its_workflow_label_and_carries_its_agent_id() {
    use base64::Engine;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(parent.id, PARENT_SESSION).await;
    eventually("session id stored", || async { (server.conversation(parent.id).await["acp_session_id"] == PARENT_SESSION).then_some(()) }).await;
    link.send(NodeToControl::Workflow { conversation_id: parent.id, run_id: "run-1".into(), line: json!({"type": "started", "key": "k1", "agentId": "aaa", "label": "write:docs", "phase": "Write"}) }).await;
    eventually("the workflow line", || async { server.events(parent.id).await.iter().any(|event| event["kind"] == "workflow").then_some(()) }).await;
    let transcript = subagent_transcript("aaa", "[Workflow harness] long preamble", "Done.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z") + "\n";
    let data = base64::engine::general_purpose::STANDARD.encode(&transcript);

    link.send(NodeToControl::Transcript { path: subagent_path("aaa"), offset: 0, data, size: Some(transcript.len() as i64) }).await;
    let side_chat = eventually("the side chat", || async { side_chats_of(&server, &parent.id.to_string()).await.into_iter().next() }).await;

    assert_eq!((side_chat["title"].clone(), side_chat["agent_id"].clone()), (json!("write:docs"), json!("aaa")));
}

#[tokio::test]
async fn a_live_subagents_token_usage_is_counted_in_its_parent_conversations_total() {
    use base64::Engine;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(parent.id, PARENT_SESSION).await;
    eventually("session id stored", || async { (server.conversation(parent.id).await["acp_session_id"] == PARENT_SESSION).then_some(()) }).await;
    let transcript = [
        json!({"type": "user", "isSidechain": true, "agentId": "aaa", "sessionId": PARENT_SESSION, "cwd": "/work/app", "timestamp": "2026-10-01T10:00:20Z", "message": {"role": "user", "content": "Read the docs and report"}}),
        json!({"type": "assistant", "isSidechain": true, "agentId": "aaa", "sessionId": PARENT_SESSION, "timestamp": "2026-10-01T10:00:30Z", "message": {"id": "msg_1", "role": "assistant", "model": "claude-sonnet-5-5", "content": [{"type": "text", "text": "Docs are current."}], "usage": {"input_tokens": 100, "output_tokens": 20, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let data = base64::engine::general_purpose::STANDARD.encode(&transcript);

    link.send(NodeToControl::Transcript { path: subagent_path("aaa"), offset: 0, data, size: Some(transcript.len() as i64) }).await;
    eventually("the subagent's side chat", || async { (side_chats_of(&server, &parent.id.to_string()).await.len() == 1).then_some(()) }).await;

    let (_, rows) = server.user.json(server.user.get("/api/v1/activity/conversations")).await;
    let rows = rows.as_array().unwrap();

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0]["key"].clone(), rows[0]["input_tokens"].clone(), rows[0]["output_tokens"].clone()), (json!(parent.id.to_string()), json!(100), json!(20)));
    assert_eq!(rows[0]["subagent_tokens"], json!(120));
}

#[tokio::test]
async fn a_subagent_transcript_delivered_in_two_chunks_keeps_its_first_events_and_stays_searchable_once() {
    use base64::Engine;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(parent.id, PARENT_SESSION).await;
    eventually("session id stored", || async { (server.conversation(parent.id).await["acp_session_id"] == PARENT_SESSION).then_some(()) }).await;
    let path = subagent_path("aaa");
    let first = subagent_transcript("aaa", "Audit the zeppelin hangar", "Looking.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z") + "\n";
    let encode = |text: &str| base64::engine::general_purpose::STANDARD.encode(text);

    link.send(NodeToControl::Transcript { path: path.clone(), offset: 0, data: encode(&first), size: Some(first.len() as i64) }).await;

    let side_chat = eventually("the side chat to appear", || async { side_chats_of(&server, &parent.id.to_string()).await.into_iter().next() }).await;
    let id = Uuid::parse_str(side_chat["id"].as_str().unwrap()).unwrap();
    let before = server.events(id).await;

    let more = json!({"type": "assistant", "isSidechain": true, "agentId": "aaa", "sessionId": PARENT_SESSION, "timestamp": "2026-10-01T10:00:40Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "Docs are current."}]}}).to_string() + "\n";
    link.send(NodeToControl::Transcript { path: path.clone(), offset: first.len() as i64, data: encode(&more), size: Some((first.len() + more.len()) as i64) }).await;

    let after = eventually("the side chat to carry the new answer", || async {
        let events = server.events(id).await;

        (events.len() > before.len()).then_some(events)
    })
    .await;
    assert_eq!(after[..before.len()], before[..]);
    assert_eq!(after.iter().map(|event| event["seq"].clone()).collect::<Vec<_>>(), (1..=after.len()).map(|seq| json!(seq)).collect::<Vec<_>>());
    assert_eq!(search(&server, "zeppelin").await.len(), 1);
}

#[tokio::test]
async fn a_live_subagent_chunk_before_the_upload_is_complete_creates_nothing_yet() {
    use base64::Engine;

    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let parent = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(parent.id, PARENT_SESSION).await;
    eventually("session id stored", || async { (server.conversation(parent.id).await["acp_session_id"] == PARENT_SESSION).then_some(()) }).await;
    let content = subagent_transcript("aaa", "Read the docs and report", "Looking.", "2026-10-01T10:00:20Z", "2026-10-01T10:00:30Z") + "\n";

    link.send(NodeToControl::Transcript { path: subagent_path("aaa"), offset: 0, data: base64::engine::general_purpose::STANDARD.encode(&content), size: Some(content.len() as i64 + 100) }).await;
    link.next().await;

    assert!(side_chats_of(&server, &parent.id.to_string()).await.is_empty());
}

#[tokio::test]
async fn a_subagent_chat_refuses_prompts() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{PARENT_SESSION}.jsonl"), &parent_transcript_with_two_agents()).await;
    mirror(&server, link.id, &subagent_path("aaa"), &subagent_transcript("aaa", "Run the build and report", "Build passes.", "2026-10-01T10:00:05Z", "2026-10-01T10:00:15Z")).await;
    import(&server, link.id, "claude").await;
    let parent = imported_conversation(&server, PARENT_SESSION).await;
    let side_chats = side_chats_of(&server, parent["id"].as_str().unwrap()).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{}/prompt", side_chats[0]["id"].as_str().unwrap())).json(&json!({"text": "Carry on"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("read_only")));
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
async fn a_transcript_the_control_plane_no_longer_has_is_recorded_as_failed() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    sqlx::query("insert into transcripts (node_id, path, bytes) values ($1, $2, 10)").bind(link.id).bind(".claude/projects/-work-app/gone.jsonl").execute(&server.pool).await.unwrap();

    let result = import(&server, link.id, "claude").await;

    assert_eq!(result["imported"], 0);
    assert_eq!(result["failed"], json!([{"path": ".claude/projects/-work-app/gone.jsonl", "reason": "The control plane no longer has this file."}]));
}

#[tokio::test]
async fn an_imported_conversation_joins_the_project_whose_folder_holds_its_directory() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let user = server.user.clone();
    let node_id = link.id;
    let registering = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&json!({"node_id": node_id, "path": "/work"}))).await });
    let (request_id, ..) = match link.next().await {
        mycelium_proto::link::ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": "/work", "repository": false}), error: None }).await;
    let (status, project) = registering.await.unwrap();
    assert_eq!(status, 201, "{project}");
    mirror(&server, link.id, &format!(".claude/projects/-work-app/{CLAUDE_SESSION}.jsonl"), &claude_transcript(CLAUDE_SESSION, "Fix the build", "Looking at it.")).await;

    import(&server, link.id, "claude").await;

    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    let imported = listed.as_array().unwrap().iter().find(|conversation| conversation["title"] == "Fix the build").unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(imported["project_id"], project["id"]);
}

#[tokio::test]
async fn starting_an_import_answers_at_once_with_the_job_and_the_number_of_files() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("aaaa", "One", "Reply one.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("bbbb", "Two", "Reply two.")).await;

    let (status, started) = start_import(&server, link.id, "claude").await;
    assert_eq!(status, 202, "{started}");
    assert_eq!((started["total"].as_i64(), started["agent"].as_str()), (Some(2), Some("claude")));
    assert!(started["id"].is_string());

    let finished = finished_job(&server, link.id, "claude").await;
    assert_eq!(finished["id"], started["id"]);
    assert_eq!((finished["done"].as_i64(), finished["imported"].as_i64(), finished["failed"].as_array().unwrap().len()), (Some(2), Some(2), 0));
}

#[tokio::test]
async fn a_second_start_while_a_job_runs_returns_the_running_job() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let running = sqlx::query_scalar::<_, Uuid>("insert into import_jobs (owner, node_id, agent, total) values ($1, $2, 'claude', 7) returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let (status, again) = start_import(&server, link.id, "claude").await;

    assert_eq!(status, 202, "{again}");
    assert_eq!((again["id"].as_str(), again["total"].as_i64(), again["state"].as_str()), (Some(running.to_string().as_str()), Some(7), Some("running")));
}

#[tokio::test]
async fn a_file_that_fails_is_recorded_with_a_reason_and_the_job_carries_on() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", "this is not a transcript\n").await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("bbbb", "Two", "Reply two.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/cccc.jsonl", &claude_transcript("cccc", "Three", "Reply three.")).await;

    let job = import(&server, link.id, "claude").await;

    assert_eq!((job["done"].as_i64(), job["imported"].as_i64()), (Some(3), Some(2)));
    assert_eq!(job["failed"], json!([{"path": ".claude/projects/-work-app/aaaa.jsonl", "reason": "There is no conversation in this file."}]));
}

#[tokio::test]
async fn a_file_still_being_written_waits_and_a_later_import_picks_it_up() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let path = ".claude/projects/-work-app/live.jsonl";
    mirror_aged(&server, link.id, path, &claude_transcript("live", "Still going", "Working."), 5).await;

    let job = import(&server, link.id, "claude").await;
    assert_eq!((job["waiting"].as_i64(), job["imported"].as_i64()), (Some(1), Some(0)));

    let (_, statuses) = server.user.json(server.user.get("/api/v1/imports")).await;
    assert_eq!(status_of(&statuses, link.id, "claude"), (1, 0));

    age(&server.data_dir.join("transcripts").join(link.id.to_string()).join(path), 3600);

    let later = import(&server, link.id, "claude").await;
    assert_eq!((later["waiting"].as_i64(), later["imported"].as_i64()), (Some(0), Some(1)));
}

#[tokio::test]
async fn a_file_that_shares_the_session_of_an_imported_one_is_skipped_not_failed() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    mirror(&server, link.id, ".claude/projects/-work-app/aaaa.jsonl", &claude_transcript("shared", "One", "Reply one.")).await;
    mirror(&server, link.id, ".claude/projects/-work-app/bbbb.jsonl", &claude_transcript("shared", "One", "Reply one.")).await;

    let job = import(&server, link.id, "claude").await;

    assert_eq!((job["imported"].as_i64(), job["skipped"].as_i64(), job["failed"].as_array().unwrap().len()), (Some(1), Some(1), 0));
}

#[tokio::test]
async fn nul_characters_and_unreadable_lines_do_not_stop_a_transcript_importing() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let lines = [
        json!({"type": "user", "sessionId": "nul", "cwd": "/work/a\u{0}pp", "timestamp": "2026-10-01T10:00:00Z", "message": {"role": "user", "content": "Run\u{0} it"}}).to_string(),
        "{ not json".to_string(),
        json!({"type": "assistant", "sessionId": "nul", "timestamp": "2026-10-01T10:00:05Z", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}}).to_string(),
        json!({"type": "user", "sessionId": "nul", "timestamp": "2026-10-01T10:00:09Z", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "a\u{0}b"}]}}).to_string(),
    ];
    mirror(&server, link.id, ".claude/projects/-work-app/nul.jsonl", &lines.join("\n")).await;

    let job = import(&server, link.id, "claude").await;
    assert_eq!((job["imported"].as_i64(), job["failed"].as_array().unwrap().len()), (Some(1), 0), "{job}");

    let imported = &imported_conversation(&server, "nul").await;
    assert_eq!(imported["cwd"], "/work/a\u{FFFD}pp");
    assert_eq!(imported["title"], "Run\u{FFFD} it");

    let events = server.events(imported["id"].as_str().unwrap().parse().unwrap()).await;
    assert_eq!(events[2]["payload"]["update"]["content"][0]["content"]["text"], "a\u{FFFD}b");
}

#[tokio::test]
async fn a_long_transcript_imports_with_contiguous_events_and_searchable_text() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let mut lines = Vec::new();

    for turn in 0..400 {
        lines.push(json!({"type": "user", "sessionId": "long", "cwd": "/work/app", "timestamp": "2026-10-01T10:00:00Z", "message": {"role": "user", "content": format!("question {turn} about zebracrossing")}}).to_string());
        lines.push(json!({"type": "assistant", "sessionId": "long", "timestamp": "2026-10-01T10:00:05Z", "message": {"role": "assistant", "content": [{"type": "text", "text": format!("answer {turn} mentions aardvarks")}]}}).to_string());
    }

    mirror(&server, link.id, ".claude/projects/-work-app/long.jsonl", &lines.join("\n")).await;
    import(&server, link.id, "claude").await;

    let conversation = imported_conversation(&server, "long").await;
    let id: Uuid = conversation["id"].as_str().unwrap().parse().unwrap();
    let rows = sqlx::query_as::<_, (i64, String, Value)>("select seq, kind, payload from events where conversation_id = $1 order by seq").bind(id).fetch_all(&server.pool).await.unwrap();

    assert_eq!(rows.iter().map(|row| row.0).collect::<Vec<i64>>(), (1..=1200).collect::<Vec<i64>>());
    assert_eq!(rows[0].2["text"], "question 0 about zebracrossing");
    assert_eq!(rows[2].1, "turn_end");
    assert_eq!(search(&server, "zebracrossing").await.len(), 1);
    assert_eq!(search(&server, "aardvarks").await.len(), 1);
}

#[tokio::test]
#[ignore]
async fn measure_a_fifty_megabyte_transcript_and_a_hundred_small_ones() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let output = "x".repeat(10_000);
    let mut big = Vec::new();

    for turn in 0..5000 {
        big.push(json!({"type": "user", "sessionId": "big", "cwd": "/work/app", "timestamp": "2026-10-01T10:00:00Z", "message": {"role": "user", "content": format!("prompt {turn}")}}).to_string());
        big.push(json!({"type": "assistant", "sessionId": "big", "timestamp": "2026-10-01T10:00:05Z", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "Bash", "input": {}}]}}).to_string());
        big.push(json!({"type": "user", "sessionId": "big", "timestamp": "2026-10-01T10:00:09Z", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": output}]}}).to_string());
    }

    mirror(&server, link.id, ".claude/projects/-work-app/big.jsonl", &big.join("\n")).await;

    for index in 0..100 {
        let session = format!("small-{index}");
        mirror(&server, link.id, &format!(".claude/projects/-work-app/{session}.jsonl"), &claude_transcript(&session, "Hi", "Hello.")).await;
    }

    let started = std::time::Instant::now();
    let job = import(&server, link.id, "claude").await;

    println!("imported {} of {} in {:?}", job["imported"], job["total"], started.elapsed());
    assert_eq!(job["imported"], 101);
}
