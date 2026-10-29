use crate::common;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use base64::Engine;
use crate::common::{NodeLink, Server, eventually, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const GITHUB_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const GITLAB_TOKEN: &str = "glpat-0123456789abcdefghij";

#[derive(Default)]
struct Repository {
    files: HashMap<String, (String, String)>,
    commits: u32,
    refuse_next_write: bool,
}

type Shared = Arc<Mutex<Repository>>;

fn commit_id(number: u32) -> String {
    format!("{number:040x}")
}

fn encode(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn fake_github(shared: Shared) -> Router {
    async fn read(State(shared): State<Shared>, Path((_, _, path)): Path<(String, String, String)>) -> Result<Json<Value>, StatusCode> {
        let repository = shared.lock().unwrap();
        let (text, commit) = repository.files.get(&path).ok_or(StatusCode::NOT_FOUND)?;

        Ok(Json(json!({"content": encode(text), "sha": format!("blob-{commit}")})))
    }

    async fn write(State(shared): State<Shared>, Path((_, _, path)): Path<(String, String, String)>, Json(body): Json<Value>) -> Result<Json<Value>, StatusCode> {
        let mut repository = shared.lock().unwrap();
        let existing = repository.files.get(&path).map(|(_, commit)| format!("blob-{commit}"));

        if repository.refuse_next_write || (existing.is_some() && body["sha"].as_str() != existing.as_deref()) {
            repository.refuse_next_write = false;

            return Err(StatusCode::CONFLICT);
        }

        repository.commits += 1;
        let commit = commit_id(repository.commits);
        let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(body["content"].as_str().unwrap()).unwrap()).unwrap();
        repository.files.insert(path, (text, commit.clone()));

        Ok(Json(json!({"commit": {"sha": commit}})))
    }

    async fn commits(State(shared): State<Shared>, axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>) -> Json<Value> {
        let repository = shared.lock().unwrap();

        Json(json!(query.get("path").and_then(|path| repository.files.get(path)).map(|(_, commit)| vec![json!({"sha": commit})]).unwrap_or_default()))
    }

    Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/repos/{owner}/{name}/contents/{*path}", get(read).put(write))
        .route("/api/v3/repos/{owner}/{name}/commits", get(commits))
        .with_state(shared)
}

fn fake_gitlab(shared: Shared) -> Router {
    async fn project(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        (headers.get("private-token").and_then(|value| value.to_str().ok()) == Some(GITLAB_TOKEN)).then(|| Json(json!({"default_branch": "main"}))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn read(State(shared): State<Shared>, Path((_, path)): Path<(String, String)>) -> Result<Json<Value>, StatusCode> {
        let repository = shared.lock().unwrap();
        let (text, commit) = repository.files.get(&path).ok_or(StatusCode::NOT_FOUND)?;

        Ok(Json(json!({"content": encode(text), "last_commit_id": commit})))
    }

    async fn write(State(shared): State<Shared>, Path((_, path)): Path<(String, String)>, Json(body): Json<Value>) -> StatusCode {
        let mut repository = shared.lock().unwrap();
        let existing = repository.files.get(&path).map(|(_, commit)| commit.clone());

        if existing.is_some() && body["last_commit_id"].as_str() != existing.as_deref() {
            return StatusCode::BAD_REQUEST;
        }

        repository.commits += 1;
        let commit = commit_id(repository.commits);
        repository.files.insert(path, (body["content"].as_str().unwrap().to_string(), commit));

        StatusCode::OK
    }

    Router::new()
        .route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) }))
        .route("/api/v4/groups", get(|| async { Json(json!([])) }))
        .route("/api/v4/projects/{id}", get(project))
        .route("/api/v4/projects/{id}/repository/files/{path}", get(read).put(write).post(write))
        .with_state(shared)
}

async fn connect_forge(server: &Server, kind: &str, base: &str, token: &str) -> Uuid {
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base, "token": token}))).await;
    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
}

async fn github_source(server: &Server, shared: Shared) -> Uuid {
    let base = serve(fake_github(shared)).await;
    let forge_id = connect_forge(server, "github", &base, GITHUB_TOKEN).await;
    let (status, body) = server.user.json(server.user.put("/api/v1/instruction-source").json(&json!({"forge_id": forge_id, "repository": "slop/agents-md"}))).await;
    assert_eq!(status, 200, "{body}");

    forge_id
}

async fn create_group(server: &Server, name: &str) -> Uuid {
    let (status, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": name}))).await;
    assert_eq!(status, 201, "{group}");

    group["id"].as_str().unwrap().parse().unwrap()
}

async fn save(server: &Server, layer: &str, text: &str) -> (u16, Value) {
    server.user.json(server.user.put(&format!("/api/v1/instructions/{layer}")).json(&json!({"text": text}))).await
}

async fn instructions(server: &Server) -> Value {
    let (status, body) = server.user.json(server.user.get("/api/v1/instructions")).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn layer<'a>(document: &'a Value, name: &str) -> &'a Value {
    document["layers"].as_array().unwrap().iter().find(|layer| layer["name"] == name).unwrap()
}

async fn project_in(server: &Server, link: &mut NodeLink, path: &str, group: Option<Uuid>) -> Uuid {
    let user = server.user.clone();
    let node_id = link.id;
    let body = json!({"node_id": node_id, "path": path});
    let asking = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });

    match link.next().await {
        ControlToNode::Git { request_id, command: GitCommand::Register, .. } => link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": format!("/w/{path}")}), error: None }).await,
        other => panic!("expected a register request, got {other:?}"),
    }

    let (status, project) = asking.await.unwrap();
    assert_eq!(status, 201, "{project}");
    let id: Uuid = project["id"].as_str().unwrap().parse().unwrap();

    if group.is_some() {
        let (status, body) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"group_id": group}))).await;
        assert_eq!(status, 200, "{body}");
    }

    id
}

async fn start_in(server: &Server, link: &mut NodeLink, project: Uuid) -> (Uuid, Option<String>) {
    let (status, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "project_id": project}))).await;
    assert_eq!(status, 201, "{conversation}");

    match link.next().await {
        ControlToNode::StartSession { conversation_id, instructions, .. } => (conversation_id, instructions),
        other => panic!("expected start_session, got {other:?}"),
    }
}

async fn group_streamed_for(stream: &mut common::StreamClient, conversation: Uuid, group: Option<Uuid>) -> Option<Uuid> {
    stream
        .until(|message| match message {
            StreamMessage::Conversation(row) if row.id == conversation && row.group_id == group => Some(row.group_id),
            _ => None,
        })
        .await
}

async fn expect_instructions(link: &mut NodeLink) -> (String, Option<String>) {
    match link.next().await {
        ControlToNode::Instructions { revision, global, .. } => (revision, global),
        other => panic!("expected instructions, got {other:?}"),
    }
}

#[tokio::test]
async fn groups_are_created_renamed_and_deleted_with_plain_errors() {
    let server = Server::start().await;
    let id = create_group(&server, "slop").await;

    let (duplicate, body) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "slop"}))).await;
    assert_eq!((duplicate, body["error"].clone()), (409, json!("group_exists")));
    let (empty, body) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "  "}))).await;
    assert_eq!((empty, body["error"].clone()), (400, json!("name_required")));
    let (spaced, body) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "my group"}))).await;
    assert_eq!((spaced, body["error"].clone()), (400, json!("name_invalid")));

    let (renamed, group) = server.user.json(server.user.patch(&format!("/api/v1/groups/{id}")).json(&json!({"name": "work"}))).await;
    assert_eq!((renamed, group["name"].clone()), (200, json!("work")));
    let (_, listed) = server.user.json(server.user.get("/api/v1/groups")).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let (deleted, _) = server.user.json(server.user.delete(&format!("/api/v1/groups/{id}"))).await;
    let (again, _) = server.user.json(server.user.delete(&format!("/api/v1/groups/{id}"))).await;
    assert_eq!((deleted, again), (204, 404));
}

#[tokio::test]
async fn a_project_joins_and_leaves_a_group_and_deleting_the_group_ungroups_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let group = create_group(&server, "slop").await;
    let project = project_in(&server, &mut link, "app", Some(group)).await;

    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(listed[0]["group_id"], json!(group));

    server.user.json(server.user.delete(&format!("/api/v1/groups/{group}"))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(listed[0]["group_id"], Value::Null);

    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.post("/api/v1/groups").json(&json!({"name": "mine"}))).await;
    let (_, theirs) = other.json(other.get("/api/v1/groups")).await;
    let their_group = theirs[0]["id"].clone();
    let (refused, _) = server.user.json(server.user.patch(&format!("/api/v1/projects/{project}")).json(&json!({"group_id": their_group}))).await;
    assert_eq!((status, refused), (201, 404));
}

#[tokio::test]
async fn every_save_is_a_new_version_and_an_unchanged_save_is_not() {
    let server = Server::start().await;
    let group = create_group(&server, "slop").await;

    let (status, first) = save(&server, "global", "# Rules").await;
    assert_eq!(status, 200, "{first}");
    let first_id = first["version"]["id"].as_str().unwrap().to_string();
    assert_eq!(first_id.len(), 40);

    let (_, same) = save(&server, "global", "# Rules").await;
    assert_eq!(same["version"]["id"], json!(first_id));

    let (_, second) = save(&server, "global", "# More rules").await;
    assert_ne!(second["version"]["id"], json!(first_id));

    let (_, layered) = save(&server, &group.to_string(), "Slop only").await;
    let document = instructions(&server).await;
    assert_eq!(document["revision"], layered["version"]["id"]);
    assert_eq!(layer(&document, "Global")["text"], "# More rules");
    assert_eq!(layer(&document, "slop")["text"], "Slop only");
    assert_eq!(layer(&document, "slop")["path"], Value::Null);

    let (unknown, _) = save(&server, &Uuid::new_v4().to_string(), "x").await;
    let (nonsense, _) = save(&server, "nonsense", "x").await;
    assert_eq!((unknown, nonsense), (404, 404));
}

#[tokio::test]
async fn saving_pushes_the_global_file_to_an_online_node_and_the_node_reports_what_it_holds() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (_, saved) = save(&server, "global", "# Rules").await;
    let (revision, global) = expect_instructions(&mut link).await;
    assert_eq!((revision.as_str(), global.as_deref()), (saved["version"]["id"].as_str().unwrap(), Some("# Rules")));

    link.send(NodeToControl::InstructionsHeld { revision: revision.clone(), unmanaged: vec!["Codex: /home/sam/.codex/AGENTS.md".into()] }).await;
    let user = server.user.clone();
    let node = eventually("the node's held version", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().find(|node| node["instructions_revision"] == json!(revision)).cloned()
    })
    .await;
    assert_eq!(node["instructions_unmanaged"], json!(["Codex: /home/sam/.codex/AGENTS.md"]));
}

#[tokio::test]
async fn taking_over_sends_the_current_file_with_permission_to_set_the_persons_own_aside() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    save(&server, "global", "# Rules").await;
    expect_instructions(&mut link).await;
    let node_id = link.id;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/nodes/{node_id}/instructions/take-over"))).await.0 });

    match link.next().await {
        ControlToNode::Instructions { global, take_over, .. } => assert_eq!((global.as_deref(), take_over), (Some("# Rules"), true)),
        other => panic!("expected instructions, got {other:?}"),
    }

    assert_eq!(asking.await.unwrap(), 202);
}

#[tokio::test]
async fn a_node_that_reconnects_is_sent_the_current_file() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "laptop").await;
    save(&server, "global", "# Rules").await;

    let mut link = server.connect_node(id, &token).await;
    link.hello(vec![]).await;

    let (_, global) = expect_instructions(&mut link).await;
    assert_eq!(global.as_deref(), Some("# Rules"));
}

#[tokio::test]
async fn a_conversation_in_a_group_starts_with_the_group_layer_and_others_do_not() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let group = create_group(&server, "slop").await;
    let grouped = project_in(&server, &mut link, "grouped", Some(group)).await;
    let loose = project_in(&server, &mut link, "loose", None).await;
    save(&server, &group.to_string(), "Use British English.").await;
    expect_instructions(&mut link).await;

    set_generated_ui(&server, false).await;
    let (_, with_layer) = start_in(&server, &mut link, grouped).await;
    let (_, without) = start_in(&server, &mut link, loose).await;

    assert_eq!((with_layer.as_deref(), without), (Some("Use British English."), None));
}

#[tokio::test]
async fn sessions_started_before_the_latest_save_are_counted_as_on_an_older_version() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let project = project_in(&server, &mut link, "app", None).await;
    save(&server, "global", "# One").await;
    expect_instructions(&mut link).await;
    let (conversation, _) = start_in(&server, &mut link, project).await;
    link.start_session(conversation, "acp-1").await;

    save(&server, "global", "# Two").await;
    expect_instructions(&mut link).await;

    let user = server.user.clone();
    eventually("one older session", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["sessions_on_older_instructions"] == 1).then_some(())
    })
    .await;
}

#[tokio::test]
async fn an_idle_session_on_an_older_revision_is_reloaded_before_the_next_prompt_and_only_then() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let project = project_in(&server, &mut link, "app", None).await;
    save(&server, "global", "# One").await;
    expect_instructions(&mut link).await;
    let (conversation, _) = start_in(&server, &mut link, project).await;
    link.start_session(conversation, "acp-1").await;
    save(&server, "global", "# Two").await;
    expect_instructions(&mut link).await;

    let prompt_url = format!("/api/v1/conversations/{conversation}/prompt");
    let (status, _) = server.user.json(server.user.post(&prompt_url).json(&json!({"text": "first"}))).await;
    assert_eq!(status, 204);

    match link.next().await {
        ControlToNode::StartSession { reload, resume, .. } => assert_eq!((reload, resume.as_deref()), (true, Some("acp-1"))),
        other => panic!("expected a reload before the prompt, got {other:?}"),
    }

    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "first");
    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation, 2).await;

    let (status, _) = server.user.json(server.user.post(&prompt_url).json(&json!({"text": "second"}))).await;
    assert_eq!(status, 204);
    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "second");
}

#[tokio::test]
async fn memories_remembered_into_the_group_are_recalled_only_by_its_projects() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let group = create_group(&server, "slop").await;
    let grouped = project_in(&server, &mut link, "grouped", Some(group)).await;
    let loose = project_in(&server, &mut link, "loose", None).await;
    let (in_group, _) = start_in(&server, &mut link, grouped).await;
    let (outside, _) = start_in(&server, &mut link, loose).await;
    let token = link.token.clone();

    let call = |conversation: Uuid, tool: &'static str, arguments: Value| {
        let server = &server;
        let token = token.clone();

        async move {
            let anonymous = server.anonymous();
            let request = anonymous
                .post("/api/v1/mcp")
                .header("authorization", format!("Bearer {token}"))
                .header("x-mycelium-conversation", conversation.to_string())
                .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}));
            let (_, body) = anonymous.json(request).await;

            (body["result"]["content"][0]["text"].as_str().unwrap().to_string(), body["result"]["isError"].clone())
        }
    };

    let (stored, _) = call(in_group, "remember", json!({"text": "Deploys go through the slop pipeline", "scope": "group"})).await;
    let (inside, _) = call(in_group, "recall", json!({"query": "pipeline"})).await;
    let (elsewhere, _) = call(outside, "recall", json!({"query": "pipeline"})).await;
    let (ungrouped, is_error) = call(outside, "remember", json!({"text": "Only for a group", "scope": "group"})).await;

    assert!(stored.starts_with("Remembered as"), "{stored}");
    assert!(inside.contains("slop pipeline"), "{inside}");
    assert!(elsewhere.starts_with("No memories match that query.\n1 more match in other groups (slop) left out."), "{elsewhere}");
    assert_eq!(is_error, json!(true), "{ungrouped}");
    let (_, memories) = server.user.json(server.user.get("/api/v1/memories")).await;
    assert_eq!(memories[0]["scopes"], json!([format!("group:{group}")]));
}

#[tokio::test]
async fn pulling_reads_the_global_file_and_each_groups_sibling_file_with_commit_ids() {
    let server = Server::start().await;
    let shared = Shared::default();
    {
        let mut repository = shared.lock().unwrap();
        repository.files.insert("AGENTS.md".into(), ("# From the forge".into(), commit_id(1)));
        repository.files.insert("AGENTS.slop.md".into(), ("Slop layer".into(), commit_id(2)));
        repository.commits = 2;
    }
    let group = create_group(&server, "slop").await;
    create_group(&server, "work").await;
    github_source(&server, shared).await;

    let (status, pulled) = server.user.json(server.user.post("/api/v1/instructions/pull")).await;

    assert_eq!(status, 200, "{pulled}");
    assert_eq!((layer(&pulled, "Global")["version"]["id"].clone(), layer(&pulled, "Global")["text"].clone()), (json!(commit_id(1)), json!("# From the forge")));
    assert_eq!((layer(&pulled, "slop")["version"]["id"].clone(), layer(&pulled, "slop")["path"].clone()), (json!(commit_id(2)), json!("AGENTS.slop.md")));
    assert_eq!(layer(&pulled, "work")["version"], Value::Null);
    assert_eq!(pulled["revision"], json!(commit_id(2)));
    let _ = group;
}

#[tokio::test]
async fn saving_with_a_source_commits_to_the_forge_and_the_version_is_the_commit() {
    let server = Server::start().await;
    let shared = Shared::default();
    github_source(&server, shared.clone()).await;

    let (status, created) = save(&server, "global", "# New file").await;
    assert_eq!(status, 200, "{created}");
    let (_, edited) = save(&server, "global", "# Edited").await;

    assert_eq!(created["version"]["id"], json!(commit_id(1)));
    assert_eq!(edited["version"]["id"], json!(commit_id(2)));
    assert_eq!(shared.lock().unwrap().files["AGENTS.md"].0, "# Edited");
}

#[tokio::test]
async fn a_layer_is_committed_to_a_sibling_file_named_after_the_group() {
    let server = Server::start().await;
    let shared = Shared::default();
    let group = create_group(&server, "homelab").await;
    github_source(&server, shared.clone()).await;

    save(&server, &group.to_string(), "Homelab rules").await;

    assert_eq!(shared.lock().unwrap().files["AGENTS.homelab.md"].0, "Homelab rules");
}

#[tokio::test]
async fn a_refused_commit_leaves_the_text_unsaved_and_asks_for_a_pull() {
    let server = Server::start().await;
    let shared = Shared::default();
    github_source(&server, shared.clone()).await;
    save(&server, "global", "# One").await;
    shared.lock().unwrap().refuse_next_write = true;

    let (status, body) = save(&server, "global", "# Two").await;

    assert_eq!((status, body["error"].clone()), (409, json!("forge_file_changed")));
    assert_eq!(layer(&instructions(&server).await, "Global")["text"], "# One");
}

#[tokio::test]
async fn gitlab_saves_and_pulls_through_the_files_api() {
    let server = Server::start().await;
    let shared = Shared::default();
    let base = serve(fake_gitlab(shared.clone())).await;
    let forge_id = connect_forge(&server, "gitlab", &base, GITLAB_TOKEN).await;
    let (status, _) = server.user.json(server.user.put("/api/v1/instruction-source").json(&json!({"forge_id": forge_id, "repository": "slop/agents-md", "path": "docs/AGENTS.md"}))).await;
    assert_eq!(status, 200);

    let (_, created) = save(&server, "global", "# Created").await;
    let (_, edited) = save(&server, "global", "# Edited").await;
    shared.lock().unwrap().files.insert("docs/AGENTS.md".into(), ("# Changed upstream".into(), commit_id(9)));
    let (_, pulled) = server.user.json(server.user.post("/api/v1/instructions/pull")).await;

    assert_eq!((created["version"]["id"].clone(), edited["version"]["id"].clone()), (json!(commit_id(1)), json!(commit_id(2))));
    assert_eq!((layer(&pulled, "Global")["text"].clone(), layer(&pulled, "Global")["version"]["id"].clone()), (json!("# Changed upstream"), json!(commit_id(9))));
}

#[tokio::test]
async fn the_source_is_checked_and_can_be_cleared() {
    let server = Server::start().await;
    let shared = Shared::default();
    let forge_id = github_source(&server, shared).await;

    let (invalid_repository, body) = server.user.json(server.user.put("/api/v1/instruction-source").json(&json!({"forge_id": forge_id, "repository": "agents-md"}))).await;
    assert_eq!((invalid_repository, body["error"].clone()), (400, json!("repository_invalid")));
    let (invalid_path, body) = server.user.json(server.user.put("/api/v1/instruction-source").json(&json!({"forge_id": forge_id, "repository": "slop/agents-md", "path": "../AGENTS.md"}))).await;
    assert_eq!((invalid_path, body["error"].clone()), (400, json!("path_invalid")));
    let (unknown_forge, _) = server.user.json(server.user.put("/api/v1/instruction-source").json(&json!({"forge_id": Uuid::new_v4(), "repository": "slop/agents-md"}))).await;
    assert_eq!(unknown_forge, 404);

    assert_eq!(instructions(&server).await["source"]["path"], "AGENTS.md");
    let (cleared, _) = server.user.json(server.user.delete("/api/v1/instruction-source")).await;
    let (pull, body) = server.user.json(server.user.post("/api/v1/instructions/pull")).await;
    assert_eq!((cleared, pull, body["error"].clone()), (204, 409, json!("no_source")));
}

async fn claim(server: &Server, group: Uuid, prefix: &str, node: Option<Uuid>) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/groups/{group}/folders")).json(&json!({"prefix": prefix, "node_id": node}))).await
}

async fn start_in_folder(server: &Server, link: &mut NodeLink, cwd: &str) -> (Uuid, Option<String>) {
    let (status, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": cwd}))).await;
    assert_eq!(status, 201, "{conversation}");

    loop {
        match link.next().await {
            ControlToNode::StartSession { conversation_id, instructions, .. } => return (conversation_id, instructions),
            ControlToNode::Instructions { .. } => continue,
            other => panic!("expected start_session, got {other:?}"),
        }
    }
}

async fn tool(server: &Server, token: &str, conversation: Uuid, name: &str, arguments: Value) -> (String, Value) {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {token}"))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    let (_, body) = anonymous.json(request).await;

    (body["result"]["content"][0]["text"].as_str().unwrap().to_string(), body["result"]["isError"].clone())
}

#[tokio::test]
async fn a_group_claims_folders_and_says_plainly_when_one_is_wrong_or_taken() {
    let server = Server::start().await;
    let slop = create_group(&server, "slop").await;
    let work = create_group(&server, "work").await;

    let (created, folder) = claim(&server, slop, "/home/claude/projects/", None).await;
    let (relative, relative_body) = claim(&server, slop, "projects", None).await;
    let (dotted, _) = claim(&server, slop, "/home/../etc", None).await;
    let (taken, taken_body) = claim(&server, work, "/home/claude/projects", None).await;
    let (unknown_node, _) = claim(&server, work, "/srv", Some(Uuid::new_v4())).await;

    assert_eq!((created, folder["prefix"].clone()), (201, json!("/home/claude/projects")));
    assert_eq!((relative, relative_body["error"].clone()), (400, json!("folder_invalid")));
    assert_eq!(dotted, 400);
    assert_eq!((taken, taken_body["error"].clone()), (409, json!("folder_claimed")));
    assert_eq!(unknown_node, 404);

    let (_, groups) = server.user.json(server.user.get("/api/v1/groups")).await;
    assert_eq!(groups[0]["folders"][0]["prefix"], "/home/claude/projects");

    let folder_id = folder["id"].as_str().unwrap();
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/groups/{slop}/folders/{folder_id}"))).await;
    let (again, _) = server.user.json(server.user.delete(&format!("/api/v1/groups/{slop}/folders/{folder_id}"))).await;
    assert_eq!((removed, again), (204, 404));
}

#[tokio::test]
async fn a_conversation_without_a_project_joins_the_group_with_the_longest_matching_folder() {
    let server = Server::start().await;
    set_generated_ui(&server, false).await;
    let mut link = server.online_node("laptop").await;
    let slop = create_group(&server, "slop").await;
    let mycelium = create_group(&server, "mycelium").await;
    claim(&server, slop, "/projects", None).await;
    claim(&server, mycelium, "/projects/mycelium", None).await;
    save(&server, &slop.to_string(), "Slop rules.").await;
    save(&server, &mycelium.to_string(), "Mycelium rules.").await;
    expect_instructions(&mut link).await;
    expect_instructions(&mut link).await;

    let (_, in_slop) = start_in_folder(&server, &mut link, "/projects/other/src").await;
    let (_, in_mycelium) = start_in_folder(&server, &mut link, "/projects/mycelium/crates").await;
    let (_, sibling) = start_in_folder(&server, &mut link, "/projects2/app").await;

    assert_eq!((in_slop.as_deref(), in_mycelium.as_deref(), sibling), (Some("Slop rules."), Some("Mycelium rules."), None));

    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations")).await;
    let groups: Vec<(String, Value)> = listed.as_array().unwrap().iter().map(|conversation| (conversation["cwd"].as_str().unwrap().to_string(), conversation["group_id"].clone())).collect();
    assert!(groups.contains(&("/projects/other/src".to_string(), json!(slop))), "{groups:?}");
    assert!(groups.contains(&("/projects/mycelium/crates".to_string(), json!(mycelium))), "{groups:?}");
    assert!(groups.contains(&("/projects2/app".to_string(), Value::Null)), "{groups:?}");
}

#[tokio::test]
async fn an_imported_conversation_is_in_the_group_that_claims_its_folder_without_a_new_import() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let slop = create_group(&server, "slop").await;
    sqlx::query("insert into conversations (owner, node_id, agent, cwd, title, status, imported_from, archived_at) select created_by, $1, 'claude', '/projects/app', 'Old work', 'done', '/x.jsonl', now() from nodes where id = $1")
        .bind(link.id)
        .execute(&server.pool)
        .await
        .unwrap();

    let (_, before) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;
    claim(&server, slop, "/projects", None).await;
    let (_, after) = server.user.json(server.user.get("/api/v1/conversations?archived=true")).await;

    assert_eq!((before[0]["group_id"].clone(), after[0]["group_id"].clone()), (Value::Null, json!(slop)));
}

#[tokio::test]
async fn a_folder_claimed_for_one_node_leaves_the_same_folder_on_another_node_alone() {
    let server = Server::start().await;
    set_generated_ui(&server, false).await;
    let mut laptop = server.online_node("laptop").await;
    let mut desktop = server.online_node("desktop").await;
    let slop = create_group(&server, "slop").await;
    claim(&server, slop, "/projects", Some(laptop.id)).await;
    save(&server, &slop.to_string(), "Slop rules.").await;
    expect_instructions(&mut laptop).await;
    expect_instructions(&mut desktop).await;

    let (_, on_laptop) = start_in_folder(&server, &mut laptop, "/projects/app").await;
    let (_, on_desktop) = start_in_folder(&server, &mut desktop, "/projects/app").await;

    assert_eq!((on_laptop.as_deref(), on_desktop), (Some("Slop rules."), None));
}

#[tokio::test]
async fn a_projects_own_group_wins_over_a_folder_claim() {
    let server = Server::start().await;
    set_generated_ui(&server, false).await;
    let mut link = server.online_node("laptop").await;
    let work = create_group(&server, "work").await;
    let slop = create_group(&server, "slop").await;
    claim(&server, slop, "/w", None).await;
    let project = project_in(&server, &mut link, "app", Some(work)).await;
    save(&server, &work.to_string(), "Work rules.").await;
    expect_instructions(&mut link).await;

    let (_, instructions) = start_in(&server, &mut link, project).await;

    assert_eq!(instructions.as_deref(), Some("Work rules."));
}

#[tokio::test]
async fn changing_a_projects_group_regroups_its_conversations_and_tells_open_clients() {
    let server = Server::start().await;
    set_generated_ui(&server, false).await;
    let mut link = server.online_node("laptop").await;
    let work = create_group(&server, "work").await;
    let slop = create_group(&server, "slop").await;
    claim(&server, slop, "/w", None).await;
    let project = project_in(&server, &mut link, "app", None).await;
    let (conversation, _) = start_in(&server, &mut link, project).await;
    let mut stream = server.user.stream().await;
    let url = format!("/api/v1/projects/{project}");

    server.user.json(server.user.patch(&url).json(&json!({"group_id": work}))).await;
    let grouped = group_streamed_for(&mut stream, conversation, Some(work)).await;
    server.user.json(server.user.patch(&url).json(&json!({"group_id": null}))).await;
    let released = group_streamed_for(&mut stream, conversation, Some(slop)).await;

    assert_eq!((grouped, released), (Some(work), Some(slop)));
}

#[tokio::test]
async fn remember_defaults_to_the_sessions_group_and_global_scope_is_explicit() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let slop = create_group(&server, "slop").await;
    claim(&server, slop, "/projects", None).await;
    let (grouped, _) = start_in_folder(&server, &mut link, "/projects/app").await;
    let (loose, _) = start_in_folder(&server, &mut link, "/elsewhere").await;
    let token = link.token.clone();

    let (default, _) = tool(&server, &token, grouped, "remember", json!({"text": "Pipelines publish on tags"})).await;
    let (explicit, _) = tool(&server, &token, grouped, "remember", json!({"text": "The host has 8 cores", "scope": "global"})).await;
    let (ungrouped, _) = tool(&server, &token, loose, "remember", json!({"text": "Everything is UTC"})).await;
    let (refused, is_error) = tool(&server, &token, loose, "remember", json!({"text": "Nope", "scope": "group"})).await;

    assert!(default.ends_with("shared with the slop group."), "{default}");
    assert!(explicit.ends_with("shared with every conversation."), "{explicit}");
    assert!(ungrouped.ends_with("shared with every conversation."), "{ungrouped}");
    assert_eq!(is_error, json!(true), "{refused}");
    let (_, memories) = server.user.json(server.user.get("/api/v1/memories")).await;
    let scopes: Vec<(String, Value)> = memories.as_array().unwrap().iter().map(|memory| (memory["text"].as_str().unwrap().to_string(), memory["scopes"].clone())).collect();
    assert!(scopes.contains(&("Pipelines publish on tags".to_string(), json!([format!("group:{slop}")]))), "{scopes:?}");
    assert!(scopes.contains(&("The host has 8 cores".to_string(), json!(["global"]))), "{scopes:?}");
    assert!(scopes.contains(&("Everything is UTC".to_string(), json!(["global"]))), "{scopes:?}");
}

#[tokio::test]
async fn recall_names_what_other_groups_hold_and_groups_reaches_into_them() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let slop = create_group(&server, "slop").await;
    let work = create_group(&server, "work").await;
    claim(&server, slop, "/projects", None).await;
    claim(&server, work, "/job", None).await;
    let (in_slop, _) = start_in_folder(&server, &mut link, "/projects/app").await;
    let (in_work, _) = start_in_folder(&server, &mut link, "/job/app").await;
    let token = link.token.clone();
    tool(&server, &token, in_work, "remember", json!({"text": "Keep the immich ui version in step with backups"})).await;
    tool(&server, &token, in_work, "remember", json!({"text": "The immich ui package needs a pinned release"})).await;
    tool(&server, &token, in_slop, "remember", json!({"text": "Slop uses an immich mirror", "scope": "global"})).await;

    let (default, _) = tool(&server, &token, in_slop, "recall", json!({"query": "immich"})).await;
    let (named, _) = tool(&server, &token, in_slop, "recall", json!({"query": "immich", "groups": ["Work"]})).await;
    let (everywhere, _) = tool(&server, &token, in_slop, "recall", json!({"query": "immich", "groups": "all"})).await;
    let (own, _) = tool(&server, &token, in_work, "recall", json!({"query": "immich"})).await;
    let (unknown, is_error) = tool(&server, &token, in_slop, "recall", json!({"query": "immich", "groups": ["nobody"]})).await;

    assert!(default.contains("Slop uses an immich mirror") && !default.contains("pinned release"), "{default}");
    assert!(default.contains("2 more matches in other groups (work) left out"), "{default}");
    assert!(named.contains("pinned release") && !named.contains("left out"), "{named}");
    assert!(everywhere.contains("pinned release") && !everywhere.contains("left out"), "{everywhere}");
    assert!(own.contains("pinned release") && !own.contains("left out"), "{own}");
    assert_eq!(is_error, json!(true));
    assert_eq!(unknown, "No group is called nobody. Use a group name, or groups: \"all\".");
}

async fn set_extensions(server: &Server, generated_ui: bool, rich_markdown: bool, mycelium_tools: bool) -> (u16, Value) {
    server.user.json(server.user.put("/api/v1/extensions").json(&json!({"generated_ui": generated_ui, "rich_markdown": rich_markdown, "mycelium_tools": mycelium_tools}))).await
}

async fn set_generated_ui(server: &Server, on: bool) -> (u16, Value) {
    set_extensions(server, on, false, false).await
}

#[tokio::test]
async fn generated_ui_is_on_by_default_and_a_new_session_gets_the_schema_with_the_workspace_action() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (_, extensions) = server.user.json(server.user.get("/api/v1/extensions")).await;
    let (_, instructions) = start_in_folder(&server, &mut link, "/anywhere").await;
    let prompt = instructions.unwrap();

    assert_eq!(extensions, json!({"generated_ui": true, "rich_markdown": true, "mycelium_tools": true}));
    assert!(prompt.starts_with("When an interface helps"), "{prompt}");
    assert!(prompt.contains("mycelium-ui") && prompt.contains("\"type\":\"bar_chart\"") && prompt.contains("\"type\":\"progress\"") && prompt.contains("\"type\":\"open_file\""), "{prompt}");
}

#[tokio::test]
async fn the_schema_follows_the_group_layer_and_turning_the_switch_off_leaves_only_the_group_layer() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let slop = create_group(&server, "slop").await;
    claim(&server, slop, "/projects", None).await;
    save(&server, &slop.to_string(), "Use British English.").await;
    expect_instructions(&mut link).await;

    let (_, on) = start_in_folder(&server, &mut link, "/projects/app").await;
    let (status, saved) = set_generated_ui(&server, false).await;
    let (_, off) = start_in_folder(&server, &mut link, "/projects/app").await;
    let (_, off_without_group) = start_in_folder(&server, &mut link, "/anywhere").await;
    let (_, extensions) = server.user.json(server.user.get("/api/v1/extensions")).await;

    assert!(on.unwrap().starts_with("Use British English.\n\nWhen an interface helps"));
    assert_eq!((status, saved, extensions), (200, json!({"generated_ui": false, "rich_markdown": false, "mycelium_tools": false}), json!({"generated_ui": false, "rich_markdown": false, "mycelium_tools": false})));
    assert_eq!((off.as_deref(), off_without_group), (Some("Use British English."), None));
}

#[tokio::test]
async fn rich_markdown_adds_its_note_to_a_new_session_only_while_the_switch_is_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    set_extensions(&server, false, true, false).await;
    let (_, on) = start_in_folder(&server, &mut link, "/anywhere").await;
    set_extensions(&server, true, false, false).await;
    let (_, off) = start_in_folder(&server, &mut link, "/anywhere").await;
    set_extensions(&server, false, true, false).await;
    let (_, only_markdown) = start_in_folder(&server, &mut link, "/anywhere").await;

    let on = on.unwrap();
    assert!(on.contains("[!WARNING]") && on.contains("^x^") && on.contains("Raw HTML is not rendered."), "{on}");
    assert!(!off.unwrap().contains("[!WARNING]"));
    assert!(only_markdown.unwrap().starts_with("Mycelium renders these Markdown extensions"));
}

#[tokio::test]
async fn mycelium_tools_note_lists_every_tool_the_mcp_server_offers_only_while_the_switch_is_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, token) = (0, link.token.clone());

    let (_, on) = start_in_folder(&server, &mut link, "/anywhere").await;
    let (_, listed) = {
        let anonymous = server.anonymous();
        let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
        anonymous.json(request).await
    };
    set_extensions(&server, true, true, false).await;
    let (_, off) = start_in_folder(&server, &mut link, "/anywhere").await;

    let on = on.unwrap();
    for tool in listed["result"]["tools"].as_array().unwrap() {
        assert!(on.contains(&format!("mcp__mycelium__{}: ", tool["name"].as_str().unwrap())), "{on}");
    }
    assert!(on.contains("not skills") && on.contains("memory features are off on purpose"), "{on}");
    assert!(!off.unwrap().contains("mcp__mycelium__"));
}

async fn conversation_with_session(server: &Server, link: &mut NodeLink, cwd: &str) -> Uuid {
    let (conversation, _) = start_in_folder(server, link, cwd).await;
    link.start_session(conversation, "acp").await;
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation}/prompt")).json(&json!({"text": "go"}))).await;
    assert_eq!(status, 204);
    link.expect_prompt().await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 2, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation, 3).await;

    conversation
}

async fn propose(server: &Server, token: &str, conversation: Uuid, arguments: Value) -> (String, Value) {
    tool(server, token, conversation, "propose_instructions_edit", arguments).await
}

async fn proposals(server: &Server) -> Value {
    let (status, body) = server.user.json(server.user.get("/api/v1/instruction-proposals")).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn revision_in(text: &str) -> String {
    text.lines().find_map(|line| line.strip_prefix("Revision: ")).unwrap().to_string()
}

#[tokio::test]
async fn an_agent_reads_a_layer_and_proposes_an_edit_that_waits_and_is_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    save(&server, "global", "# Rules\nBe brief.").await;
    expect_instructions(&mut link).await;
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    let mut stream = server.user.stream().await;

    let (read, is_error) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
    let revision = revision_in(&read);
    let (answer, proposed_error) = propose(&server, &token, conversation, json!({"text": "# Rules\nBe brief.\nNever commit unasked.", "base_revision": revision, "reason": "\"Do not commit unless I say so\""})).await;

    assert_eq!((is_error, read.starts_with("Layer: Global\nRevision: ")), (json!(false), true), "{read}");
    assert!(read.ends_with("# Rules\nBe brief."), "{read}");
    assert_eq!(proposed_error, json!(false), "{answer}");
    let streamed = stream.until(|message| match message {
        StreamMessage::InstructionProposal(proposal) => Some(proposal),
        _ => None,
    })
    .await;
    assert_eq!((streamed.layer_name.as_str(), streamed.text.as_str(), streamed.base_text.as_str(), streamed.out_of_date), ("Global", "# Rules\nBe brief.\nNever commit unasked.", "# Rules\nBe brief.", false));
    assert_eq!(streamed.conversation_id, Some(conversation));
    assert_eq!(layer(&instructions(&server).await, "Global")["text"], "# Rules\nBe brief.");
    let listed = proposals(&server).await;
    assert_eq!((listed[0]["state"].clone(), listed[0]["reason"].clone()), (json!("proposed"), json!("\"Do not commit unless I say so\"")));
}

#[tokio::test]
async fn a_proposal_defaults_to_the_conversations_group_and_global_must_be_asked_for() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let group = create_group(&server, "slop").await;
    claim(&server, group, "/slop", None).await;
    let conversation = conversation_with_session(&server, &mut link, "/slop/app").await;

    let (read, _) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
    let (global_read, _) = tool(&server, &token, conversation, "get_instructions", json!({"layer": "GLOBAL"})).await;
    propose(&server, &token, conversation, json!({"text": "Group rule", "base_revision": "none", "reason": "because"})).await;
    propose(&server, &token, conversation, json!({"text": "Everywhere rule", "base_revision": "none", "reason": "because", "layer": "global"})).await;
    let (unknown, is_error) = propose(&server, &token, conversation, json!({"text": "x", "base_revision": "none", "reason": "because", "layer": "nope"})).await;

    assert!(read.starts_with("Layer: slop\nRevision: none"), "{read}");
    assert!(global_read.starts_with("Layer: Global\nRevision: none"), "{global_read}");
    let listed = proposals(&server).await;
    let names: Vec<(&str, &str)> = listed.as_array().unwrap().iter().map(|proposal| (proposal["layer_name"].as_str().unwrap(), proposal["text"].as_str().unwrap())).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&("slop", "Group rule")) && names.contains(&("Global", "Everywhere rule")), "{names:?}");
    assert_eq!(is_error, json!(true), "{unknown}");
    assert!(unknown.contains("No instructions layer is called nope"), "{unknown}");
}

#[tokio::test]
async fn a_proposal_on_a_stale_base_or_without_a_reason_is_refused_with_what_to_do() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    save(&server, "global", "# One").await;
    expect_instructions(&mut link).await;
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    let (read, _) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
    let stale = revision_in(&read);
    save(&server, "global", "# Two").await;

    let (refused, is_error) = propose(&server, &token, conversation, json!({"text": "# Three", "base_revision": stale, "reason": "because"})).await;
    let (no_reason, no_reason_error) = propose(&server, &token, conversation, json!({"text": "# Three", "base_revision": stale, "reason": "  "})).await;
    let (unchanged, unchanged_error) = {
        let (current, _) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
        propose(&server, &token, conversation, json!({"text": "# Two", "base_revision": revision_in(&current), "reason": "because"})).await
    };

    assert_eq!(is_error, json!(true));
    assert!(refused.starts_with("Nothing was proposed.") && refused.contains("Read the current text with get_instructions"), "{refused}");
    assert_eq!((no_reason_error, unchanged_error), (json!(true), json!(true)), "{no_reason} {unchanged}");
    assert!(no_reason.contains("quoting the person's own words") && unchanged.contains("same as the current"), "{no_reason} {unchanged}");
    assert_eq!(proposals(&server).await, json!([]));
}

#[tokio::test]
async fn approving_saves_the_layer_like_the_editor_and_reaches_nodes() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    save(&server, "global", "# One").await;
    expect_instructions(&mut link).await;
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    let (read, _) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
    propose(&server, &token, conversation, json!({"text": "# One\nNew rule", "base_revision": revision_in(&read), "reason": "because"})).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let before = instructions(&server).await["revision"].clone();

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;

    assert_eq!(status, 200, "{approved}");
    assert_eq!((approved["state"].clone(), approved["out_of_date"].clone()), (json!("approved"), json!(false)));
    let after = instructions(&server).await;
    assert_eq!(layer(&after, "Global")["text"], "# One\nNew rule");
    assert_ne!(after["revision"], before);
    let (revision, global) = expect_instructions(&mut link).await;
    assert_eq!((json!(revision), global.as_deref()), (after["revision"].clone(), Some("# One\nNew rule")));
    let (again, body) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;
    assert_eq!((again, body["error"].clone()), (409, json!("proposal_not_proposed")));
}

#[tokio::test]
async fn approving_with_a_source_commits_to_the_forge() {
    let server = Server::start().await;
    let shared = Shared::default();
    github_source(&server, shared.clone()).await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    propose(&server, &token, conversation, json!({"text": "# From a proposal", "base_revision": "none", "reason": "because"})).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;

    assert_eq!(status, 200, "{approved}");
    assert_eq!(shared.lock().unwrap().files["AGENTS.md"].0, "# From a proposal");
    assert_eq!(layer(&instructions(&server).await, "Global")["version"]["id"], json!(commit_id(1)));
}

#[tokio::test]
async fn a_proposal_goes_out_of_date_when_the_layer_is_saved_and_cannot_be_approved() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    save(&server, "global", "# One").await;
    expect_instructions(&mut link).await;
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    let (read, _) = tool(&server, &token, conversation, "get_instructions", json!({})).await;
    propose(&server, &token, conversation, json!({"text": "# One\nRule", "base_revision": revision_in(&read), "reason": "because"})).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let mut stream = server.user.stream().await;

    save(&server, "global", "# Edited by hand").await;

    let streamed = stream.until(|message| match message {
        StreamMessage::InstructionProposal(proposal) => Some(proposal),
        _ => None,
    })
    .await;
    assert!(streamed.out_of_date);
    assert_eq!(proposals(&server).await[0]["out_of_date"], json!(true));
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("proposal_out_of_date")));
    assert_eq!(layer(&instructions(&server).await, "Global")["text"], "# Edited by hand");
}

#[tokio::test]
async fn declining_records_the_note_and_tells_the_agent_and_redo_asks_it_to_propose_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    propose(&server, &token, conversation, json!({"text": "First", "base_revision": "none", "reason": "because"})).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, declined) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/decline")).json(&json!({"note": "Too broad"}))).await;

    assert_eq!(status, 200, "{declined}");
    assert_eq!((declined["state"].clone(), declined["note"].clone()), (json!("declined"), json!("Too broad")));
    let (_, message) = link.expect_prompt().await;
    assert!(message.contains("declined the edit") && message.contains("Global") && message.ends_with("My note: Too broad"), "{message}");
    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 4, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation, 5).await;
    assert_eq!(layer(&instructions(&server).await, "Global")["text"], "");
    let (again, body) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/decline")).json(&json!({}))).await;
    assert_eq!((again, body["error"].clone()), (409, json!("proposal_not_proposed")));

    propose(&server, &token, conversation, json!({"text": "Second", "base_revision": "none", "reason": "because"})).await;
    let second = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let (_, redone) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{second}/decline")).json(&json!({"redo": true}))).await;
    let (_, redo_message) = link.expect_prompt().await;
    assert_eq!(redone["state"], json!("declined"));
    assert!(redo_message.contains("propose it again on the current text"), "{redo_message}");
}

#[tokio::test]
async fn declining_without_telling_the_agent_sends_it_no_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    propose(&server, &token, conversation, json!({"text": "First", "base_revision": "none", "reason": "because"})).await;
    let first = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, declined) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{first}/decline")).json(&json!({"notify": false}))).await;
    propose(&server, &token, conversation, json!({"text": "Second", "base_revision": "none", "reason": "because"})).await;
    let second = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{second}/decline")).json(&json!({"redo": true}))).await;
    let (_, message) = link.expect_prompt().await;

    assert_eq!((status, declined["state"].clone()), (200, json!("declined")));
    assert!(message.contains("propose it again on the current text"), "the first prompt the agent saw was {message}");
}

#[tokio::test]
async fn a_newer_proposal_from_the_same_conversation_supersedes_the_waiting_one() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;

    propose(&server, &token, conversation, json!({"text": "First", "base_revision": "none", "reason": "because"})).await;
    propose(&server, &token, conversation, json!({"text": "Second", "base_revision": "none", "reason": "because"})).await;

    let listed = proposals(&server).await;
    assert_eq!((listed[0]["text"].clone(), listed[0]["state"].clone()), (json!("Second"), json!("proposed")));
    assert_eq!((listed[1]["text"].clone(), listed[1]["state"].clone()), (json!("First"), json!("superseded")));
}

#[tokio::test]
async fn another_persons_proposal_is_not_listed_or_decided() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let token = link.token.clone();
    let conversation = conversation_with_session(&server, &mut link, "/anywhere").await;
    propose(&server, &token, conversation, json!({"text": "First", "base_revision": "none", "reason": "because"})).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let other = server.second_user("other@example.com").await;

    let (listed_status, listed) = other.json(other.get("/api/v1/instruction-proposals")).await;
    let (approve_status, _) = other.json(other.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;

    assert_eq!((listed_status, listed), (200, json!([])));
    assert_eq!(approve_status, 404);
}

#[tokio::test]
async fn saving_group_instructions_is_a_new_version_the_instructions_page_also_sees() {
    let server = Server::start().await;
    let group = create_group(&server, "slop").await;
    let path = format!("/api/v1/groups/{group}/instructions");

    let (_, saved) = server.user.json(server.user.put(&path).json(&json!({"text": "Slop only"}))).await;
    let (_, read) = server.user.json(server.user.get(&path)).await;
    assert_eq!((read["text"].clone(), read["version"]["id"].clone()), (json!("Slop only"), saved["version"]["id"].clone()));
    assert_eq!(layer(&instructions(&server).await, "slop")["version"]["id"], saved["version"]["id"]);

    let (_, same) = server.user.json(server.user.put(&path).json(&json!({"text": "Slop only"}))).await;
    assert_eq!(same["version"]["id"], saved["version"]["id"]);
}

#[tokio::test]
async fn group_instructions_reach_a_new_conversation_in_the_group() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let group = create_group(&server, "slop").await;
    let grouped = project_in(&server, &mut link, "grouped", Some(group)).await;

    let (status, saved) = server.user.json(server.user.put(&format!("/api/v1/groups/{group}/instructions")).json(&json!({"text": "Use British English."}))).await;
    assert_eq!((status, saved["text"].clone()), (200, json!("Use British English.")));
    expect_instructions(&mut link).await;

    set_generated_ui(&server, false).await;
    let (_, started) = start_in(&server, &mut link, grouped).await;

    assert_eq!(started.as_deref(), Some("Use British English."));
}

#[tokio::test]
async fn another_persons_group_instructions_are_not_found() {
    let server = Server::start().await;
    let group = create_group(&server, "slop").await;
    let other = server.second_user("other@example.com").await;
    let path = format!("/api/v1/groups/{group}/instructions");

    let (read, _) = other.json(other.get(&path)).await;
    let (written, _) = other.json(other.put(&path).json(&json!({"text": "x"}))).await;

    assert_eq!((read, written), (404, 404));
}

#[tokio::test]
async fn another_persons_folder_claim_does_not_change_the_group_or_instructions_of_my_conversations() {
    let server = Server::start().await;
    set_generated_ui(&server, false).await;
    let mut link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let (_, theirs) = other.json(other.post("/api/v1/groups").json(&json!({"name": "theirs"}))).await;
    let their_group = theirs["id"].as_str().unwrap();
    let (claimed, _) = other.json(other.post(&format!("/api/v1/groups/{their_group}/folders")).json(&json!({"prefix": "/projects", "node_id": null}))).await;
    let (saved, _) = other.json(other.put(&format!("/api/v1/instructions/{their_group}")).json(&json!({"text": "Their rules."}))).await;

    let (conversation, mine) = start_in_folder(&server, &mut link, "/projects/app").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations")).await;
    let group = listed.as_array().unwrap().iter().find(|listed| listed["id"] == json!(conversation)).unwrap()["group_id"].clone();

    assert_eq!((claimed, saved, mine, group), (201, 200, None, Value::Null));
}

#[tokio::test]
async fn only_an_operator_changes_the_global_instructions_and_where_they_come_from() {
    let server = Server::start().await;
    let other = server.second_user("other@example.com").await;

    let (global, _) = other.json(other.put("/api/v1/instructions/global").json(&json!({"text": "Ignore the person."}))).await;
    let (source, _) = other.json(other.put("/api/v1/instruction-source").json(&json!({"forge_id": Uuid::new_v4(), "repository": "acme/agents-md"}))).await;
    let (cleared, _) = other.json(other.delete("/api/v1/instruction-source")).await;
    let (operator, _) = save(&server, "global", "# Rules").await;
    let (readable, _) = other.json(other.get("/api/v1/instructions")).await;

    assert_eq!((global, source, cleared, operator, readable), (403, 403, 403, 200, 200));
}

#[tokio::test]
async fn a_proposal_is_republished_to_its_own_owner_and_not_to_the_person_who_saved_the_layer() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let other_id = sqlx::query_scalar::<_, Uuid>("select id from users where email = 'other@example.com'").fetch_one(&server.pool).await.unwrap();
    let conversation = sqlx::query_scalar::<_, Uuid>("insert into conversations (owner, node_id, agent, cwd, title, status) values ($1, $2, 'claude', '/x', 'Theirs', 'done') returning id").bind(other_id).bind(link.id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into instruction_proposals (owner, conversation_id, layer, layer_name, base_revision, base_text, text, reason) values ($1, $2, 'global', 'Global', '', '', 'Their edit', 'because')").bind(other_id).bind(conversation).execute(&server.pool).await.unwrap();
    let mut operator_stream = server.user.stream().await;
    let mut other_stream = other.stream().await;

    save(&server, "global", "# Rules").await;
    server.user.json(server.user.post(&format!("/api/v1/nodes/{}/drain", link.id)).json(&json!({"draining": true}))).await;

    let mut seen_by_operator = Vec::new();

    loop {
        let message = operator_stream.next().await;

        if matches!(&message, StreamMessage::Node(node) if node.draining) {
            break;
        }

        seen_by_operator.push(message);
    }

    let republished = other_stream.until(|message| match message {
        StreamMessage::InstructionProposal(proposal) => Some(proposal.text),
        _ => None,
    })
    .await;

    assert_eq!(republished, "Their edit");
    assert!(!seen_by_operator.iter().any(|message| matches!(message, StreamMessage::InstructionProposal(_))), "{seen_by_operator:?}");
}
