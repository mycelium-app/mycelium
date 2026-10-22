mod common;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use common::{NodeLink, Server, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, GitCredentials, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

const GITHUB_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const GITLAB_TOKEN: &str = "glpat-0123456789abcdefghij";

async fn connect_forge(server: &Server, kind: &str, base: &str, token: &str) -> Uuid {
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base, "token": token}))).await;
    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
}

async fn fake_forge(kind: &str) -> String {
    let router = if kind == "github" {
        Router::new().route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) })).route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
    } else {
        Router::new().route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) })).route("/api/v4/groups", get(|| async { (StatusCode::OK, Json(json!([]))) }))
    };

    serve(router).await
}

async fn answer_next(link: &mut NodeLink, outcome: Result<Value, &str>) -> (String, GitCommand) {
    let (request_id, cwd, command) = match link.next().await {
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

async fn create_project(server: &Server, link: &mut NodeLink, body: Value, outcome: Result<Value, &str>) -> ((u16, Value), (String, GitCommand)) {
    let user = server.user.clone();
    let node_id = link.id;
    let mut body = body;
    body["node_id"] = json!(node_id);
    let asking = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });
    let relayed = answer_next(link, outcome).await;

    (asking.await.unwrap(), relayed)
}

#[tokio::test]
async fn a_github_repository_is_cloned_by_the_node_with_the_forges_token_and_registered_as_a_project() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let ((status, project), (cwd, command)) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "acme-org/acme-backups"}), Ok(json!({"path": "/home/sam/projects/acme-org/acme-backups"}))).await;

    assert_eq!(status, 201, "{project}");
    assert_eq!(cwd, "projects/acme-org/acme-backups");
    assert_eq!(command, GitCommand::Clone { url: format!("{github}/acme-org/acme-backups.git"), credentials: Some(GitCredentials { username: "x-access-token".into(), password: GITHUB_TOKEN.into() }) });
    assert_eq!((project["name"].clone(), project["path"].clone(), project["repository"].clone(), project["forge_id"].clone()), (json!("acme-backups"), json!("/home/sam/projects/acme-org/acme-backups"), json!("acme-org/acme-backups"), json!(forge_id)));

    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(listed, json!([project]));
    assert!(!listed.to_string().contains(GITHUB_TOKEN));
}

#[tokio::test]
async fn a_gitlab_repository_is_cloned_with_the_oauth2_username() {
    let server = Server::start().await;
    let gitlab = fake_forge("gitlab").await;
    let forge_id = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let (_, (cwd, command)) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "slop/tools/backups"}), Ok(json!({"path": "/p/slop/tools/backups"}))).await;

    assert_eq!(cwd, "projects/slop/tools/backups");
    assert_eq!(command, GitCommand::Clone { url: format!("{gitlab}/slop/tools/backups.git"), credentials: Some(GitCredentials { username: "oauth2".into(), password: GITLAB_TOKEN.into() }) });
}

#[tokio::test]
async fn a_clone_that_git_refuses_returns_its_message_and_registers_nothing() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let ((status, error), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "acme-org/missing"}), Err("repository not found")).await;

    assert_eq!((status, error["error"].clone(), error["message"].clone()), (409, json!("git_failed"), json!("repository not found")));
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn an_existing_folder_is_registered_under_the_path_the_node_resolved() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let ((status, project), (cwd, command)) = create_project(&server, &mut link, json!({"path": "  work/app "}), Ok(json!({"path": "/home/sam/work/app", "repository": true}))).await;

    assert_eq!((status, cwd.as_str(), command), (201, "work/app", GitCommand::Register));
    assert_eq!((project["name"].clone(), project["path"].clone(), project["forge_id"].clone(), project["repository"].clone()), (json!("app"), json!("/home/sam/work/app"), Value::Null, Value::Null));

    let ((again, error), _) = create_project(&server, &mut link, json!({"path": "/home/sam/work/app"}), Ok(json!({"path": "/home/sam/work/app", "repository": true}))).await;
    assert_eq!((again, error["error"].clone()), (409, json!("project_exists")));
}

#[tokio::test]
async fn a_project_needs_exactly_one_source_a_valid_repository_and_a_forge_of_your_own() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let link = server.online_node("laptop").await;
    let other = server.second_user("other@example.com").await;
    let post = |user: &common::Client, body: Value| {
        let user = user.clone();

        async move { user.json(user.post("/api/v1/projects").json(&body)).await }
    };

    let (_, nothing) = post(&server.user, json!({"node_id": link.id})).await;
    let (_, both) = post(&server.user, json!({"node_id": link.id, "forge_id": forge_id, "repository": "a/b", "path": "x"})).await;
    let (_, blank) = post(&server.user, json!({"node_id": link.id, "path": "  "})).await;
    let (_, traversal) = post(&server.user, json!({"node_id": link.id, "forge_id": forge_id, "repository": "a/../../b"})).await;
    let (_, option) = post(&server.user, json!({"node_id": link.id, "forge_id": forge_id, "repository": "--upload-pack=x/y"})).await;
    let (stolen, _) = post(&other, json!({"node_id": link.id, "forge_id": forge_id, "repository": "a/b"})).await;

    for error in [&nothing, &both, &blank] {
        assert_eq!(error["error"], "project_source_required");
    }

    for error in [&traversal, &option] {
        assert_eq!(error["error"], "repository_invalid");
    }

    assert_eq!(stolen, 404);
}

#[tokio::test]
async fn a_project_cannot_be_set_up_on_an_offline_or_unknown_node() {
    let server = Server::start().await;
    let (offline_id, _) = server.join("node", "asleep").await;

    let (offline, error) = server.user.json(server.user.post("/api/v1/projects").json(&json!({"node_id": offline_id, "path": "x"}))).await;
    let (unknown, _) = server.user.json(server.user.post("/api/v1/projects").json(&json!({"node_id": Uuid::new_v4(), "path": "x"}))).await;

    assert_eq!((offline, error["error"].clone(), unknown), (409, json!("node_offline"), 404));
}

#[tokio::test]
async fn projects_are_listed_only_for_the_person_who_set_them_up() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let other = server.second_user("other@example.com").await;

    let (_, theirs) = other.json(other.get("/api/v1/projects")).await;

    assert_eq!(theirs, json!([]));
}

#[tokio::test]
async fn a_conversation_started_from_a_project_runs_in_the_projects_folder() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, (_, _)) = create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    let project_id = listed[0]["id"].clone();

    let (status, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "project_id": project_id}))).await;

    assert_eq!(status, 201, "{conversation}");
    assert_eq!(conversation["cwd"], "/w/app");
    match link.next().await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, "/w/app"),
        other => panic!("expected start_session, got {other:?}"),
    }
}

#[tokio::test]
async fn a_conversation_from_a_project_needs_the_node_it_lives_on_and_a_project_of_your_own() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let elsewhere = server.online_node("server").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    let project_id = listed[0]["id"].clone();
    let other = server.second_user("other@example.com").await;

    let (wrong_node, error) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": elsewhere.id, "agent": "claude", "project_id": project_id}))).await;
    let (not_theirs, _) = other.json(other.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "project_id": project_id}))).await;
    let (no_folder, missing) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude"}))).await;

    assert_eq!((wrong_node, error["error"].clone(), not_theirs), (400, json!("project_not_on_node"), 404));
    assert_eq!((no_folder, missing["error"].clone()), (400, json!("cwd_required")));
}

async fn project_on(server: &Server, link: &mut NodeLink) -> Value {
    let (_, _) = create_project(server, link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;

    listed[0].clone()
}

#[tokio::test]
async fn a_conversation_can_start_in_a_new_worktree_the_node_creates_on_a_new_branch() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let project = project_on(&server, &mut link).await;
    let user = server.user.clone();
    let node_id = link.id;
    let starting = tokio::spawn(async move { user.json(user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "project_id": project["id"], "branch": " feature/login "}))).await });

    let (cwd, command) = answer_next(&mut link, Ok(json!({"path": "/w/app-worktrees/feature-login", "branch": "feature/login"}))).await;
    let (status, conversation) = starting.await.unwrap();

    assert_eq!((cwd.as_str(), command), ("/w/app", GitCommand::AddWorktree { branch: "feature/login".into() }));
    assert_eq!((status, conversation["cwd"].clone()), (201, json!("/w/app-worktrees/feature-login")));
    match link.next().await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, "/w/app-worktrees/feature-login"),
        other => panic!("expected start_session, got {other:?}"),
    }

    let (_, worktrees) = server.user.json(server.user.get("/api/v1/worktrees")).await;
    assert_eq!((worktrees[0]["branch"].clone(), worktrees[0]["path"].clone(), worktrees[0]["issue_number"].clone()), (json!("feature/login"), json!("/w/app-worktrees/feature-login"), Value::Null));
    let other = server.second_user("other@example.com").await;
    let (_, theirs) = other.json(other.get("/api/v1/worktrees")).await;
    assert_eq!(theirs, json!([]));
}

#[tokio::test]
async fn a_worktree_that_git_refuses_starts_no_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let project = project_on(&server, &mut link).await;
    let user = server.user.clone();
    let node_id = link.id;
    let starting = tokio::spawn(async move { user.json(user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "project_id": project["id"], "branch": "main"}))).await });

    answer_next(&mut link, Err("a branch named 'main' already exists")).await;
    let (status, error) = starting.await.unwrap();

    assert_eq!((status, error["error"].clone(), error["message"].clone()), (409, json!("git_failed"), json!("a branch named 'main' already exists")));
    let (_, conversations) = server.user.json(server.user.get("/api/v1/conversations")).await;
    let (_, worktrees) = server.user.json(server.user.get("/api/v1/worktrees")).await;
    assert_eq!((conversations, worktrees), (json!([]), json!([])));
}

#[tokio::test]
async fn a_branch_needs_a_project_and_a_name() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let project = project_on(&server, &mut link).await;

    let (_, without_project) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app", "branch": "x"}))).await;
    let (_, blank) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "project_id": project["id"], "branch": "  "}))).await;

    assert_eq!((without_project["error"].clone(), blank["error"].clone()), (json!("project_required"), json!("branch_required")));
}
