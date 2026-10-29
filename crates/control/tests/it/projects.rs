use crate::common;
use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use crate::common::{NodeLink, Server, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, GitCredentials, NodeToControl};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

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
        Router::new()
            .route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) }))
            .route("/api/v4/groups", get(|| async { (StatusCode::OK, Json(json!([]))) }))
            .route("/api/v4/projects/{project}", get(|Path(project): Path<String>| async move { Json(json!({"ssh_url_to_repo": format!("ssh://git@ssh.forge.test:2222/{project}.git")})) }))
    };

    serve(router).await
}

async fn fake_creating_forge(kind: &str, seen: Seen) -> String {
    async fn github_user(State(seen): State<Seen>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        seen.lock().unwrap().push(("POST /user/repos".into(), body));

        (StatusCode::CREATED, Json(json!({})))
    }

    async fn github_organisation(State(seen): State<Seen>, Path(organisation): Path<String>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        seen.lock().unwrap().push((format!("POST /orgs/{organisation}/repos"), body.clone()));

        if body["name"] == "taken" {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"message": "name already exists on this account"})));
        }

        (StatusCode::CREATED, Json(json!({})))
    }

    async fn gitlab_namespace(Path(path): Path<String>) -> Json<Value> {
        Json(json!({"id": if path == "slop/tools" { 77 } else { 5 }}))
    }

    async fn gitlab_project(State(seen): State<Seen>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        seen.lock().unwrap().push(("POST /projects".into(), body));

        (StatusCode::CREATED, Json(json!({})))
    }

    let router = if kind == "github" {
        Router::new()
            .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
            .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
            .route("/api/v3/user/repos", post(github_user))
            .route("/api/v3/orgs/{organisation}/repos", post(github_organisation))
    } else {
        Router::new()
            .route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) }))
            .route("/api/v4/groups", get(|| async { Json(json!([])) }))
            .route("/api/v4/namespaces/{path}", get(gitlab_namespace))
            .route("/api/v4/projects", post(gitlab_project))
    };

    serve(router.with_state(seen)).await
}

async fn answer_next(link: &mut NodeLink, outcome: Result<Value, &str>) -> (String, GitCommand) {
    let (request_id, cwd, command) = loop {
        match link.next().await {
            ControlToNode::Git { request_id, cwd, command } => break (request_id, cwd, command),
            ControlToNode::StartSession { .. } => continue,
            other => panic!("expected git, got {other:?}"),
        }
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
    let (_, blank) = post(&server.user, json!({"node_id": link.id, "path": "  "})).await;
    let (_, traversal) = post(&server.user, json!({"node_id": link.id, "forge_id": forge_id, "repository": "a/../../b"})).await;
    let (_, option) = post(&server.user, json!({"node_id": link.id, "forge_id": forge_id, "repository": "--upload-pack=x/y"})).await;
    let (stolen, _) = post(&other, json!({"node_id": link.id, "forge_id": forge_id, "repository": "a/b"})).await;

    for error in [&nothing, &blank] {
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
async fn a_conversation_started_in_a_folder_inside_a_project_belongs_to_that_project() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;

    let (_, inside) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app/crates"}))).await;
    let (_, outside) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/application"}))).await;

    assert_eq!(inside["project_id"], listed[0]["id"]);
    assert_eq!(outside["project_id"], Value::Null);
}

#[tokio::test]
async fn a_conversation_started_in_a_worktree_folder_of_a_project_belongs_to_that_project() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;

    let (_, worktree) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app-worktrees/feature-login"}))).await;
    let (_, sibling) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app-worktreesx/other"}))).await;

    assert_eq!(worktree["project_id"], listed[0]["id"]);
    assert_eq!(sibling["project_id"], Value::Null);
}

#[tokio::test]
async fn a_conversation_in_a_worktree_folder_joins_a_project_registered_afterwards() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, before) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app-worktrees/feature-login"}))).await;
    assert_eq!(before["project_id"], Value::Null);

    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    let (_, after) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}", before["id"].as_str().unwrap()))).await;

    assert_eq!(after["project_id"], listed[0]["id"]);
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

#[tokio::test]
async fn a_new_github_repository_is_created_on_your_account_or_an_organisation_then_cloned_as_the_project() {
    let server = Server::start().await;
    let seen = Seen::default();
    let github = fake_creating_forge("github", seen.clone()).await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let ((own, mine), (_, clone)) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "samexample/restore-tool", "new_repository": true}), Ok(json!({"path": "/p/samexample/restore-tool"}))).await;
    let ((shared, theirs), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "acme-org/restore-tool", "new_repository": true}), Ok(json!({"path": "/p/acme-org/restore-tool"}))).await;

    assert_eq!((own, mine["repository"].clone(), shared, theirs["repository"].clone()), (201, json!("samexample/restore-tool"), 201, json!("acme-org/restore-tool")));
    assert!(matches!(clone, GitCommand::Clone { url, .. } if url == format!("{github}/samexample/restore-tool.git")));
    let created = seen.lock().unwrap().clone();
    assert_eq!(created[0], ("POST /user/repos".to_string(), json!({"name": "restore-tool", "private": true, "auto_init": true})));
    assert_eq!(created[1].0, "POST /orgs/acme-org/repos");
}

#[tokio::test]
async fn a_new_gitlab_repository_is_created_in_its_namespace_then_cloned_as_the_project() {
    let server = Server::start().await;
    let seen = Seen::default();
    let gitlab = fake_creating_forge("gitlab", seen.clone()).await;
    let forge_id = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let ((status, project), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "slop/tools/restore-tool", "new_repository": true}), Ok(json!({"path": "/p/slop/tools/restore-tool"}))).await;

    assert_eq!((status, project["repository"].clone()), (201, json!("slop/tools/restore-tool")));
    let created = seen.lock().unwrap().clone();
    assert_eq!(created[0], ("POST /projects".to_string(), json!({"name": "restore-tool", "path": "restore-tool", "namespace_id": 77, "visibility": "private", "initialize_with_readme": true})));
}

#[tokio::test]
async fn a_new_repository_the_forge_refuses_is_reported_and_nothing_is_cloned_or_registered() {
    let server = Server::start().await;
    let github = fake_creating_forge("github", Seen::default()).await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let link = server.online_node("laptop").await;

    let (status, error) = server.user.json(server.user.post("/api/v1/projects").json(&json!({"node_id": link.id, "forge_id": forge_id, "repository": "acme-org/taken", "new_repository": true}))).await;

    assert_eq!((status, error["error"].clone()), (400, json!("forge_repository_refused")));
    assert!(error["message"].as_str().unwrap().starts_with("GitHub did not create that repository"));
    let (_, projects) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(projects, json!([]));
}

async fn conversation_in(server: &Server, node_id: Uuid, cwd: &str) -> Uuid {
    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "cwd": cwd}))).await;
    assert_eq!(status, 201, "{body}");

    body["id"].as_str().unwrap().parse().unwrap()
}

async fn project_of(server: &Server, conversation_id: Uuid) -> Value {
    server.conversation(conversation_id).await["project_id"].clone()
}

#[tokio::test]
async fn an_existing_folder_whose_remote_is_the_repository_becomes_the_project_over_ssh_or_https() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut link = server.online_node("laptop").await;
    let ssh = "git@127.0.0.1:acme-org/acme-backups.git";

    let ((status, project), (cwd, command)) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "acme-org/acme-backups", "path": "work/backups"}), Ok(json!({"path": "/home/sam/work/backups", "repository": true, "remotes": [ssh]}))).await;

    assert_eq!((status, cwd.as_str(), command), (201, "work/backups", GitCommand::Register));
    assert_eq!((project["name"].clone(), project["path"].clone(), project["repository"].clone(), project["forge_id"].clone()), (json!("acme-backups"), json!("/home/sam/work/backups"), json!("acme-org/acme-backups"), json!(forge_id)));

    let ((other, _), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "acme-org/acme-backups", "path": "/second"}), Ok(json!({"path": "/second", "repository": true, "remotes": [format!("{github}/acme-org/acme-backups")]}))).await;
    assert_eq!(other, 201);
}

#[tokio::test]
async fn an_existing_folder_cloned_over_the_forges_own_ssh_host_becomes_the_project() {
    let server = Server::start().await;
    let gitlab = fake_forge("gitlab").await;
    let forge_id = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let mut link = server.online_node("laptop").await;

    let ((status, _), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "sam/nix", "path": "/nix"}), Ok(json!({"path": "/nix", "repository": true, "remotes": ["ssh://git@ssh.forge.test:2222/sam/nix.git"]}))).await;
    assert_eq!(status, 201);

    let ((refused, _), _) = create_project(&server, &mut link, json!({"forge_id": forge_id, "repository": "sam/nix", "path": "/elsewhere"}), Ok(json!({"path": "/elsewhere", "repository": true, "remotes": ["ssh://git@unrelated.test/sam/nix.git"]}))).await;
    assert_eq!(refused, 400);
}

#[tokio::test]
async fn an_existing_folder_that_holds_another_repository_or_no_checkout_is_refused_and_nothing_is_registered() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut link = server.online_node("laptop").await;
    let body = json!({"forge_id": forge_id, "repository": "acme-org/backups", "path": "/x"});

    let ((status, error), _) = create_project(&server, &mut link, body.clone(), Ok(json!({"path": "/x", "repository": true, "remotes": [format!("{github}/other/repo.git")]}))).await;
    assert_eq!((status, error["message"].clone()), (400, json!("The folder at /x is a checkout of other/repo, not acme-org/backups. Choose the folder that holds acme-org/backups.")));

    let ((status, error), _) = create_project(&server, &mut link, body.clone(), Ok(json!({"path": "/x", "repository": true, "remotes": ["https://elsewhere.example/acme-org/backups.git"]}))).await;
    assert_eq!((status, error["error"].clone()), (400, json!("folder_other_repository")));

    let ((status, error), _) = create_project(&server, &mut link, body, Ok(json!({"path": "/x", "repository": false}))).await;
    assert_eq!((status, error["message"].clone()), (400, json!("The folder at /x is not a git checkout. Choose the folder that holds acme-org/backups.")));

    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn a_new_project_adopts_the_unassigned_conversations_of_its_node_inside_its_folder_only() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut other = server.online_node("server").await;
    let inside = conversation_in(&server, link.id, "/home/sam/backups/src").await;
    let exact = conversation_in(&server, link.id, "/home/sam/backups").await;
    let sibling = conversation_in(&server, link.id, "/home/sam/backups-old").await;
    let elsewhere = conversation_in(&server, other.id, "/home/sam/backups").await;
    let taken = conversation_in(&server, link.id, "/home/sam/backups/docs").await;

    let ((_, parent), _) = create_project(&server, &mut link, json!({"path": "/home/sam/backups/docs"}), Ok(json!({"path": "/home/sam/backups/docs", "repository": false}))).await;
    assert_eq!(project_of(&server, taken).await, parent["id"]);

    let mut stream = server.user.stream().await;
    let ((_, project), _) = create_project(&server, &mut link, json!({"path": "/home/sam/backups"}), Ok(json!({"path": "/home/sam/backups", "repository": false}))).await;
    let _ = &mut other;

    assert_eq!(project_of(&server, inside).await, project["id"]);
    assert_eq!(project_of(&server, exact).await, project["id"]);
    assert_eq!(project_of(&server, sibling).await, Value::Null);
    assert_eq!(project_of(&server, elsewhere).await, Value::Null);
    assert_eq!(project_of(&server, taken).await, parent["id"]);

    let streamed: mycelium_proto::conversation::Conversation = stream.until(|message| match message {
        mycelium_proto::stream::StreamMessage::Conversation(conversation) if conversation.id == inside => Some(conversation),
        _ => None,
    }).await;
    assert_eq!(streamed.project_id.map(|id| json!(id)), Some(project["id"].clone()));
}

#[tokio::test]
async fn a_conversation_in_no_project_can_be_put_in_a_project_on_its_node() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    let (_, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/elsewhere"}))).await;
    assert_eq!(conversation["project_id"], Value::Null);

    let (status, patched) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{}", conversation["id"].as_str().unwrap())).json(&json!({"project_id": listed[0]["id"]}))).await;

    assert_eq!(status, 200, "{patched}");
    assert_eq!((patched["project_id"].clone(), patched["cwd"].clone()), (listed[0]["id"].clone(), json!("/elsewhere")));
}

#[tokio::test]
async fn a_conversation_cannot_be_put_in_a_project_on_another_node() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let elsewhere = server.online_node("server").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;
    let (_, conversation) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": elsewhere.id, "agent": "claude", "cwd": "/elsewhere"}))).await;

    let (status, refused) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{}", conversation["id"].as_str().unwrap())).json(&json!({"project_id": listed[0]["id"]}))).await;

    assert_eq!((status, refused["error"].clone()), (400, json!("project_not_on_node")));
}

#[tokio::test]
async fn taking_a_conversation_out_of_its_project_leaves_it_in_no_project() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    create_project(&server, &mut link, json!({"path": "work/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, inside) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app"}))).await;
    assert_ne!(inside["project_id"], Value::Null);

    let (status, patched) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{}", inside["id"].as_str().unwrap())).json(&json!({"clear_project": true}))).await;

    assert_eq!((status, patched["project_id"].clone()), (200, Value::Null));
}

#[tokio::test]
async fn setting_up_a_conversations_folder_as_a_project_binds_every_conversation_in_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, first) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app"}))).await;
    let (_, second) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/w/app/crates"}))).await;

    create_project(&server, &mut link, json!({"path": "/w/app"}), Ok(json!({"path": "/w/app"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/projects")).await;

    for conversation in [first, second] {
        let (_, after) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}", conversation["id"].as_str().unwrap()))).await;

        assert_eq!(after["project_id"], listed[0]["id"]);
    }
}

#[tokio::test]
async fn a_project_is_cloned_onto_a_node_that_lacks_it_and_reused_once_it_is_there() {
    let server = Server::start().await;
    let github = fake_forge("github").await;
    let forge_id = connect_forge(&server, "github", &github, GITHUB_TOKEN).await;
    let mut atlas = server.online_node("atlas").await;
    let mut orion = server.online_node("orion").await;
    let ((_, project), _) = create_project(&server, &mut atlas, json!({"forge_id": forge_id, "repository": "acme-org/acme-backups"}), Ok(json!({"path": "/home/sam/projects/acme-org/acme-backups"}))).await;
    let user = server.user.clone();
    let url = format!("/api/v1/projects/{}/checkout", project["id"].as_str().unwrap());
    let orion_id = orion.id;
    let asking = tokio::spawn(async move { user.json(user.post(&url).json(&json!({"node_id": orion_id}))).await });

    let (cwd, command) = answer_next(&mut orion, Ok(json!({"path": "/srv/projects/acme-org/acme-backups"}))).await;
    let (status, checkout) = asking.await.unwrap();

    assert_eq!((status, cwd.as_str()), (201, "projects/acme-org/acme-backups"));
    assert!(matches!(command, GitCommand::Clone { .. }));
    assert_eq!((checkout["node_id"].clone(), checkout["path"].clone()), (json!(orion.id), json!("/srv/projects/acme-org/acme-backups")));

    let (again_status, again) = server.user.json(server.user.post(&format!("/api/v1/projects/{}/checkout", project["id"].as_str().unwrap())).json(&json!({"node_id": orion.id}))).await;
    assert_eq!((again_status, again["id"].clone()), (200, checkout["id"].clone()));
}

#[tokio::test]
async fn a_project_without_a_forge_repository_cannot_be_cloned_onto_another_node() {
    let server = Server::start().await;
    let mut atlas = server.online_node("atlas").await;
    let orion = server.online_node("orion").await;
    let project = project_on(&server, &mut atlas).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/projects/{}/checkout", project["id"].as_str().unwrap())).json(&json!({"node_id": orion.id}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("project_not_cloneable")));
}
