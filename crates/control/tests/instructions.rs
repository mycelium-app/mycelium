mod common;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use base64::Engine;
use common::{NodeLink, Server, eventually, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
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

async fn expect_instructions(link: &mut NodeLink) -> (String, Option<String>) {
    match link.next().await {
        ControlToNode::Instructions { revision, global } => (revision, global),
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
    assert_eq!(elsewhere, "No memories match that query.");
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
