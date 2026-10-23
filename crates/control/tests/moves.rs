mod common;

use axum::Json;
use axum::Router;
use axum::routing::get;
use base64::Engine;
use common::{NodeLink, Server, eventually, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, ListeningPort, NodeToControl, ProcessUsage, ResourceSample};
use serde_json::{Value, json};
use uuid::Uuid;

const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const SESSION: &str = "acp-1";
const SOURCE_PATH: &str = "/w/acme-org/app";
const TARGET_PATH: &str = "/b/acme-org/app";
const TRANSCRIPT: &str = ".claude/projects/-w-acme-org-app/acp-1.jsonl";

struct Moving {
    server: Server,
    source: NodeLink,
    target: NodeLink,
    forge_id: Uuid,
    conversation: Uuid,
}

async fn next_command(link: &mut NodeLink) -> ControlToNode {
    loop {
        let message = link.next().await;

        if !matches!(message, ControlToNode::TranscriptAck { .. } | ControlToNode::Watch { .. }) {
            return message;
        }
    }
}

async fn answer_next(link: &mut NodeLink, outcome: Result<Value, &str>) -> (String, GitCommand) {
    let (request_id, cwd, command) = match next_command(link).await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };
    let message = match outcome {
        Ok(output) => NodeToControl::GitResult { request_id, ok: true, output, error: None },
        Err(error) => NodeToControl::GitResult { request_id, ok: false, output: Value::Null, error: Some(error.to_string()) },
    };

    link.send(message).await;

    (cwd, command)
}

async fn create_project(server: &Server, link: &mut NodeLink, forge_id: Uuid, path: &str) -> Value {
    let user = server.user.clone();
    let body = json!({"node_id": link.id, "forge_id": forge_id, "repository": "acme-org/app"});
    let asking = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });
    answer_next(link, Ok(json!({"path": path}))).await;
    let (status, project) = asking.await.unwrap();
    assert_eq!(status, 201, "{project}");

    project
}

async fn start(target_has_project: bool, working: bool) -> Moving {
    let server = Server::start().await;
    let forge = serve(Router::new().route("/api/v3/user", get(|| async { Json(json!({"login": "sam"})) })).route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))).await;
    let (status, connected) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": "github", "base_url": forge, "token": TOKEN}))).await;
    assert_eq!(status, 201, "{connected}");
    let forge_id: Uuid = connected["id"].as_str().unwrap().parse().unwrap();
    let mut source = server.online_node("laptop").await;
    let mut target = server.online_node("build").await;
    let project = create_project(&server, &mut source, forge_id, SOURCE_PATH).await;

    if target_has_project {
        create_project(&server, &mut target, forge_id, TARGET_PATH).await;
    }

    let (status, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": source.id, "agent": "claude", "project_id": project["id"], "prompt": "go"}))).await;
    assert_eq!(status, 201, "{conversation}");
    let conversation: Uuid = conversation["id"].as_str().unwrap().parse().unwrap();
    source.expect_start_session().await;
    source.start_session(conversation, SESSION).await;
    source.expect_prompt().await;

    let user = server.user.clone();
    eventually("the session id", || async { (user.json(user.get(&format!("/api/v1/conversations/{conversation}"))).await.1["acp_session_id"] == json!(SESSION)).then_some(()) }).await;

    source.send(NodeToControl::AcpUpdate { conversation_id: conversation, seq: 1, update: json!({"sessionUpdate": "agent_message_chunk"}) }).await;

    if !working {
        source.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
        source.barrier(conversation, 3).await;
    }

    Moving { server, source, target, forge_id, conversation }
}

fn snapshot() -> Value {
    let engine = base64::engine::general_purpose::STANDARD;

    json!({
        "branch": "feature/moves",
        "head": "0123456789abcdef0123456789abcdef01234567",
        "changed_files": 3,
        "patch": engine.encode("diff --git a/a.txt b/a.txt\n"),
        "bundle": engine.encode("bundle bytes"),
        "transcript": {"path": TRANSCRIPT, "bytes": 11},
    })
}

async fn mirror_transcript(moving: &mut Moving) {
    let engine = base64::engine::general_purpose::STANDARD;
    moving.source.send(NodeToControl::Transcript { path: TRANSCRIPT.into(), offset: 0, data: engine.encode("one\ntwo\nxyz") }).await;
}

async fn request_move(moving: &Moving, fork: bool) -> (u16, Value) {
    moving.server.user.json(moving.server.user.post(&format!("/api/v1/conversations/{}/move", moving.conversation)).json(&json!({"node_id": moving.target.id, "fork": fork}))).await
}

async fn progress(moving: &Moving, wanted: &str) -> Value {
    let user = moving.server.user.clone();
    let path = format!("/api/v1/conversations/{}/move", moving.conversation);
    let wanted = wanted.to_string();

    eventually("the move to reach its state", || async {
        let (_, current) = user.json(user.get(&path)).await;

        (current["state"] == json!(wanted)).then_some(current)
    })
    .await
}

async fn sessions_of(moving: &Moving, node_id: Uuid) -> Vec<Value> {
    let (_, nodes) = moving.server.user.json(moving.server.user.get("/api/v1/nodes")).await;

    nodes.as_array().unwrap().iter().find(|node| node["id"] == json!(node_id)).unwrap()["sessions"].as_array().unwrap().clone()
}

fn sample(conversation: Uuid) -> ResourceSample {
    let port = |port: u16, owner: Option<Uuid>, cwd: Option<&str>| ListeningPort { port, address: "127.0.0.1".into(), pid: Some(7), process: Some("vite".into()), conversation_id: owner, cwd: cwd.map(str::to_string) };
    let process = |name: &str, agent: bool, cwd: Option<&str>| ProcessUsage { pid: 9, name: name.into(), command: name.into(), cpu_percent: 1.0, memory_bytes: 1, agent, conversation_id: None, cwd: cwd.map(str::to_string) };

    ResourceSample {
        at_ms: 1,
        available: true,
        cpu_percent: 1.0,
        cores: 4,
        load: [0.0; 3],
        memory_total_bytes: 1,
        memory_available_bytes: 1,
        swap_total_bytes: 0,
        swap_free_bytes: 0,
        disks: vec![],
        processes: vec![process("node", false, Some("/w/acme-org/app/web")), process("claude", true, Some(SOURCE_PATH)), process("postgres", false, Some("/var/lib"))],
        ports: vec![port(5173, Some(conversation), Some(SOURCE_PATH)), port(3000, None, Some("/w/acme-org/app/web")), port(22, None, None)],
    }
}

#[tokio::test]
async fn the_plan_names_what_goes_with_the_conversation_and_what_stays_on_its_node() {
    let mut moving = start(true, false).await;
    moving.source.send(NodeToControl::Resources { sample: sample(moving.conversation) }).await;
    let user = moving.server.user.clone();
    let path = format!("/api/v1/conversations/{}/move-plan?node_id={}", moving.conversation, moving.target.id);
    let asking = tokio::spawn(async move { user.json(user.get(&path)).await });

    let (cwd, command) = answer_next(&mut moving.source, Ok(json!({"branch": "feature/moves", "head": "abc", "changed_files": 3}))).await;
    let (status, plan) = asking.await.unwrap();

    assert_eq!((cwd.as_str(), command), (SOURCE_PATH, GitCommand::MovePlan));
    assert_eq!(status, 200, "{plan}");
    assert_eq!(
        plan,
        json!({"movable": true, "branch": "feature/moves", "changed_files": 3, "project_on_target": true, "clones_project": false, "agent_session": true, "stays_ports": [3000, 5173], "stays_processes": ["node"]})
    );
}

#[tokio::test]
async fn the_plan_says_the_project_will_be_cloned_when_the_other_node_lacks_it() {
    let mut moving = start(false, false).await;
    moving.source.send(NodeToControl::Resources { sample: sample(moving.conversation) }).await;
    let user = moving.server.user.clone();
    let path = format!("/api/v1/conversations/{}/move-plan?node_id={}", moving.conversation, moving.target.id);
    let asking = tokio::spawn(async move { user.json(user.get(&path)).await });

    answer_next(&mut moving.source, Ok(json!({"branch": "main", "head": "abc", "changed_files": 0}))).await;
    let (_, plan) = asking.await.unwrap();

    assert_eq!((plan["project_on_target"].clone(), plan["clones_project"].clone(), plan["movable"].clone()), (json!(false), json!(true), json!(true)));
}

#[tokio::test]
async fn a_folder_that_is_not_a_forge_repository_cannot_be_moved_and_the_plan_says_why() {
    let server = Server::start().await;
    let mut source = server.online_node("laptop").await;
    let target = server.online_node("build").await;
    let conversation = server.create_conversation(source.id, None).await;
    source.expect_start_session().await;

    let (status, plan) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/move-plan?node_id={}", conversation.id, target.id))).await;

    assert_eq!(status, 200, "{plan}");
    assert_eq!(plan["movable"], false);
    assert!(plan["reason"].as_str().unwrap().contains("not a forge repository"), "{plan}");
}

#[tokio::test]
async fn a_plan_that_the_source_node_cannot_make_is_unmovable_with_the_nodes_reason() {
    let mut moving = start(true, false).await;
    moving.source.send(NodeToControl::Resources { sample: sample(moving.conversation) }).await;
    let user = moving.server.user.clone();
    let path = format!("/api/v1/conversations/{}/move-plan?node_id={}", moving.conversation, moving.target.id);
    let asking = tokio::spawn(async move { user.json(user.get(&path)).await });

    answer_next(&mut moving.source, Err("This folder is not on a branch. Check one out on this node, then try again.")).await;
    let (_, plan) = asking.await.unwrap();

    assert_eq!((plan["movable"].clone(), plan["reason"].clone()), (json!(false), json!("This folder is not on a branch. Check one out on this node, then try again.")));
}

#[tokio::test]
async fn an_idle_conversation_moves_with_its_branch_unsaved_work_and_transcript_and_continues_on_the_other_node() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;

    let (status, accepted) = request_move(&moving, false).await;
    assert_eq!((status, accepted["state"].clone()), (202, json!("moving")), "{accepted}");

    let (cwd, command) = answer_next(&mut moving.source, Ok(snapshot())).await;
    assert_eq!((cwd.as_str(), command), (SOURCE_PATH, GitCommand::Snapshot { session_id: Some(SESSION.into()) }));

    let (cwd, command) = answer_next(&mut moving.target, Ok(json!({"branch": "feature/moves", "applied_files": 1}))).await;
    let engine = base64::engine::general_purpose::STANDARD;
    match command {
        GitCommand::Apply { branch, head, patch, bundle, credentials } => {
            assert_eq!((cwd.as_str(), branch.as_str(), head.as_str()), (TARGET_PATH, "feature/moves", "0123456789abcdef0123456789abcdef01234567"));
            assert_eq!((patch, bundle), (engine.encode("diff --git a/a.txt b/a.txt\n"), Some(engine.encode("bundle bytes"))));
            assert_eq!(credentials.map(|credentials| credentials.username).as_deref(), Some("x-access-token"));
        }
        other => panic!("expected apply, got {other:?}"),
    }

    match next_command(&mut moving.target).await {
        ControlToNode::RestoreTranscript { path, offset, data } => {
            assert_eq!((path.as_str(), offset, data), (".claude/projects/-b-acme-org-app/acp-1.jsonl", 0, engine.encode("one\ntwo\nxyz")));
        }
        other => panic!("expected the transcript, got {other:?}"),
    }
    assert_eq!(next_command(&mut moving.target).await, ControlToNode::ContinueSequence { conversation_id: moving.conversation, after: 3 });
    match next_command(&mut moving.target).await {
        ControlToNode::StartSession { conversation_id, cwd, resume, fork, .. } => assert_eq!((conversation_id, cwd.as_str(), resume.as_deref(), fork), (moving.conversation, TARGET_PATH, Some(SESSION), false)),
        other => panic!("expected start_session, got {other:?}"),
    }

    moving.target.start_session(moving.conversation, SESSION).await;
    let done = progress(&moving, "done").await;

    assert_eq!(done["result_id"], json!(moving.conversation));
    assert_eq!(next_command(&mut moving.source).await, ControlToNode::CloseSession { conversation_id: moving.conversation });
    let conversation = moving.server.conversation(moving.conversation).await;
    assert_eq!((conversation["node_id"].clone(), conversation["cwd"].clone()), (json!(moving.target.id), json!(TARGET_PATH)));

    moving.target.send(NodeToControl::AcpUpdate { conversation_id: moving.conversation, seq: 4, update: json!({"sessionUpdate": "agent_message_chunk"}) }).await;
    let server = &moving.server;
    let conversation_id = moving.conversation;
    eventually("an event from the new node", || async { (server.events(conversation_id).await.len() >= 5).then_some(()) }).await;
    assert!(!sessions_of(&moving, moving.source.id).await.contains(&json!(moving.conversation)));
    assert!(sessions_of(&moving, moving.target.id).await.contains(&json!(moving.conversation)));
}

#[tokio::test]
async fn the_project_is_cloned_on_the_other_node_when_it_is_missing_there() {
    let mut moving = start(false, false).await;
    mirror_transcript(&mut moving).await;
    request_move(&moving, false).await;

    answer_next(&mut moving.source, Ok(snapshot())).await;
    let (cwd, command) = answer_next(&mut moving.target, Ok(json!({"path": TARGET_PATH}))).await;

    assert_eq!(cwd, "projects/acme-org/app");
    assert!(matches!(command, GitCommand::Clone { ref url, .. } if url.ends_with("/acme-org/app.git")));
    let (cwd, command) = answer_next(&mut moving.target, Ok(json!({"branch": "feature/moves", "applied_files": 1}))).await;
    assert_eq!(cwd, TARGET_PATH);
    assert!(matches!(command, GitCommand::Apply { .. }));

    let (_, projects) = moving.server.user.json(moving.server.user.get("/api/v1/projects")).await;
    let on_target = projects.as_array().unwrap().iter().find(|project| project["node_id"] == json!(moving.target.id)).unwrap();
    assert_eq!((on_target["path"].clone(), on_target["repository"].clone(), on_target["forge_id"].clone()), (json!(TARGET_PATH), json!("acme-org/app"), json!(moving.forge_id)));
}

#[tokio::test]
async fn a_conversation_in_a_subfolder_of_the_project_continues_in_the_same_subfolder() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;
    sqlx::query("update conversations set cwd = $2 where id = $1").bind(moving.conversation).bind(format!("{SOURCE_PATH}/web")).execute(&moving.server.pool).await.unwrap();
    request_move(&moving, false).await;

    answer_next(&mut moving.source, Ok(snapshot())).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 0}))).await;
    next_command(&mut moving.target).await;
    next_command(&mut moving.target).await;

    match next_command(&mut moving.target).await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, format!("{TARGET_PATH}/web")),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn a_move_requested_mid_turn_waits_for_the_turn_to_end() {
    let mut moving = start(true, true).await;
    mirror_transcript(&mut moving).await;
    let mut stream = moving.server.user.stream().await;

    let (status, accepted) = request_move(&moving, false).await;
    assert_eq!((status, accepted["state"].clone()), (202, json!("waiting")), "{accepted}");
    stream
        .until(|message| match message {
            mycelium_proto::stream::StreamMessage::Move(progress) if progress.state == mycelium_proto::api::MoveState::Waiting => Some(()),
            _ => None,
        })
        .await;

    moving.source.send(NodeToControl::TurnEnd { conversation_id: moving.conversation, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;

    let (_, command) = answer_next(&mut moving.source, Ok(snapshot())).await;
    assert!(matches!(command, GitCommand::Snapshot { .. }));
    assert_eq!(progress(&moving, "moving").await["state"], "moving");
}

#[tokio::test]
async fn a_waiting_move_can_be_cancelled_and_then_nothing_moves_when_the_turn_ends() {
    let mut moving = start(true, true).await;
    request_move(&moving, false).await;

    let (status, _) = moving.server.user.json(moving.server.user.delete(&format!("/api/v1/conversations/{}/move", moving.conversation))).await;
    assert_eq!(status, 204);
    assert_eq!(progress(&moving, "cancelled").await["state"], "cancelled");

    moving.source.send(NodeToControl::TurnEnd { conversation_id: moving.conversation, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
    moving.source.barrier(moving.conversation, 3).await;
    let conversation = moving.server.conversation(moving.conversation).await;
    assert_eq!(conversation["node_id"], json!(moving.source.id));
    let (status, error) = moving.server.user.json(moving.server.user.delete(&format!("/api/v1/conversations/{}/move", moving.conversation))).await;
    assert_eq!((status, error["error"].clone()), (409, json!("move_not_waiting")));
}

#[tokio::test]
async fn a_move_the_other_node_refuses_leaves_the_conversation_on_its_node_and_says_how_to_retry() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;
    request_move(&moving, false).await;

    answer_next(&mut moving.source, Ok(snapshot())).await;
    answer_next(&mut moving.target, Err("The project folder on this node has uncommitted changes. Commit or discard them there, then try again.")).await;
    let failed = progress(&moving, "failed").await;

    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("uncommitted changes") && message.contains("still on laptop") && message.contains("Choose Move to try again"), "{message}");
    let conversation = moving.server.conversation(moving.conversation).await;
    assert_eq!((conversation["node_id"].clone(), conversation["cwd"].clone()), (json!(moving.source.id), json!(SOURCE_PATH)));
}

#[tokio::test]
async fn an_agent_that_cannot_resume_on_the_other_node_puts_the_conversation_back_untouched() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;
    let events_before = moving.server.events(moving.conversation).await;
    request_move(&moving, false).await;

    answer_next(&mut moving.source, Ok(snapshot())).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 1}))).await;
    next_command(&mut moving.target).await;
    next_command(&mut moving.target).await;
    next_command(&mut moving.target).await;
    moving.target.send(NodeToControl::SessionFailed { conversation_id: moving.conversation, seq: 4, message: "The agent could not resume the session: no such session".into() }).await;
    assert_eq!(next_command(&mut moving.target).await, ControlToNode::CloseSession { conversation_id: moving.conversation });
    answer_next(&mut moving.target, Ok(json!({}))).await;
    let failed = progress(&moving, "failed").await;

    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("could not resume on build") && message.contains("no such session") && message.contains("Choose Move to try again"), "{message}");
    let conversation = moving.server.conversation(moving.conversation).await;
    assert_eq!((conversation["node_id"].clone(), conversation["cwd"].clone(), conversation["status"].clone()), (json!(moving.source.id), json!(SOURCE_PATH), json!("review")));
    assert_eq!(moving.server.events(moving.conversation).await, events_before);
}

#[tokio::test]
async fn an_agent_that_cannot_resume_on_the_other_node_has_the_applied_work_undone_there() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;
    request_move(&moving, false).await;

    answer_next(&mut moving.source, Ok(snapshot())).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 1, "previous": "main", "branch_before": null}))).await;
    next_command(&mut moving.target).await;
    next_command(&mut moving.target).await;
    next_command(&mut moving.target).await;
    moving.target.send(NodeToControl::SessionFailed { conversation_id: moving.conversation, seq: 4, message: "The agent could not resume the session: no such session".into() }).await;
    assert_eq!(next_command(&mut moving.target).await, ControlToNode::CloseSession { conversation_id: moving.conversation });

    let (cwd, command) = answer_next(&mut moving.target, Ok(json!({}))).await;
    assert_eq!((cwd.as_str(), command), (TARGET_PATH, GitCommand::Undo { branch: "feature/moves".into(), previous: "main".into(), branch_before: None }));
    let failed = progress(&moving, "failed").await;

    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("Nothing has changed") && message.contains("still on laptop"), "{message}");
}

#[tokio::test]
async fn a_transcript_that_cannot_be_copied_has_the_applied_work_undone_on_the_other_node() {
    let mut moving = start(true, false).await;
    request_move(&moving, false).await;

    let mut lost = snapshot();
    lost["transcript"] = Value::Null;
    answer_next(&mut moving.source, Ok(lost)).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 1, "previous": "main", "branch_before": "0123456789abcdef0123456789abcdef01234500"}))).await;

    let (cwd, command) = answer_next(&mut moving.target, Ok(json!({}))).await;
    assert_eq!((cwd.as_str(), command), (TARGET_PATH, GitCommand::Undo { branch: "feature/moves".into(), previous: "main".into(), branch_before: Some("0123456789abcdef0123456789abcdef01234500".into()) }));
    let failed = progress(&moving, "failed").await;

    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("was not found on its node") && message.contains("Nothing has changed"), "{message}");
}

#[tokio::test]
async fn when_the_undo_fails_the_message_says_what_was_left_on_the_other_node() {
    let mut moving = start(true, false).await;
    request_move(&moving, false).await;

    let mut lost = snapshot();
    lost["transcript"] = Value::Null;
    answer_next(&mut moving.source, Ok(lost)).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 1, "previous": "main", "branch_before": null}))).await;
    answer_next(&mut moving.target, Err("git could not reset")).await;
    let failed = progress(&moving, "failed").await;

    let message = failed["message"].as_str().unwrap();
    assert!(!message.contains("Nothing has changed"), "{message}");
    assert!(message.contains("build") && message.contains(TARGET_PATH) && message.contains("feature/moves") && message.contains("git reset --hard") && message.contains("git clean -fd") && message.contains("git checkout main"), "{message}");
}

#[tokio::test]
async fn forking_starts_a_new_conversation_on_the_other_node_and_leaves_the_original_where_it_is() {
    let mut moving = start(true, false).await;
    mirror_transcript(&mut moving).await;
    let (_, original) = moving.server.user.json(moving.server.user.get(&format!("/api/v1/conversations/{}", moving.conversation))).await;

    let (status, accepted) = request_move(&moving, true).await;
    assert_eq!((status, accepted["fork"].clone()), (202, json!(true)), "{accepted}");
    answer_next(&mut moving.source, Ok(snapshot())).await;
    answer_next(&mut moving.target, Ok(json!({"applied_files": 1}))).await;
    next_command(&mut moving.target).await;

    let forked = match next_command(&mut moving.target).await {
        ControlToNode::StartSession { conversation_id, cwd, resume, fork, .. } => {
            assert_eq!((cwd.as_str(), resume.as_deref(), fork), (TARGET_PATH, Some(SESSION), true));
            conversation_id
        }
        other => panic!("expected start_session, got {other:?}"),
    };
    moving.target.start_session(forked, "acp-2").await;
    let done = progress(&moving, "done").await;

    assert_eq!(done["result_id"], json!(forked));
    assert_ne!(forked, moving.conversation);
    let fork = moving.server.conversation(forked).await;
    assert_eq!((fork["node_id"].clone(), fork["title"].clone(), fork["acp_session_id"].clone(), fork["archived_at"].clone()), (json!(moving.target.id), json!(format!("{} (fork)", original["title"].as_str().unwrap())), json!("acp-2"), Value::Null));
    let original_now = moving.server.conversation(moving.conversation).await;
    assert_eq!((original_now["node_id"].clone(), original_now["cwd"].clone()), (json!(moving.source.id), json!(SOURCE_PATH)));
    assert!(sessions_of(&moving, moving.source.id).await.contains(&json!(moving.conversation)));
    assert!(sessions_of(&moving, moving.target.id).await.contains(&json!(forked)));
}

#[tokio::test]
async fn a_conversation_cannot_move_to_its_own_node_to_a_busy_move_or_to_an_offline_node() {
    let mut moving = start(true, false).await;

    let (status, same) = moving.server.user.json(moving.server.user.post(&format!("/api/v1/conversations/{}/move", moving.conversation)).json(&json!({"node_id": moving.source.id}))).await;
    assert_eq!((status, same["error"].clone()), (409, json!("same_node")));

    mirror_transcript(&mut moving).await;
    request_move(&moving, false).await;
    let (status, again) = request_move(&moving, false).await;
    assert_eq!((status, again["error"].clone()), (409, json!("move_in_progress")));

    let offline = Uuid::new_v4();
    let (status, unknown) = moving.server.user.json(moving.server.user.post(&format!("/api/v1/conversations/{}/move", moving.conversation)).json(&json!({"node_id": offline}))).await;
    assert_eq!((status, unknown["error"].clone()), (404, json!("not_found")));
}
