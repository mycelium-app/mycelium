mod common;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use common::{NodeLink, Server, serve};
use mycelium_proto::link::{CommitOptions, ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn conversation_on(server: &Server) -> (NodeLink, Uuid) {
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    (link, created.id)
}

async fn answer_next(link: &mut NodeLink, ok: bool, output: Value, error: Option<&str>) -> (String, GitCommand) {
    let (request_id, cwd, command) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };

    link.send(NodeToControl::GitResult { request_id, ok, output, error: error.map(String::from) }).await;

    (cwd, command)
}

#[tokio::test]
async fn git_status_is_read_by_the_node_in_the_conversations_folder() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.get(&format!("/api/v1/conversations/{id}/git/status"))).await });

    let status = json!({"branch": "main", "upstream": null, "ahead": 0, "behind": 0, "files": []});
    let (cwd, command) = answer_next(&mut link, true, status.clone(), None).await;

    assert_eq!((cwd.as_str(), command), ("/work/app", GitCommand::Status));
    assert_eq!(asking.await.unwrap(), (200, status));
}

#[tokio::test]
async fn git_graph_show_branches_and_switch_carry_their_arguments_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let graph = user.json(user.get(&format!("/api/v1/conversations/{id}/git/graph?skip=200"))).await;
        let show = user.json(user.get(&format!("/api/v1/conversations/{id}/git/show?hash=abc1234"))).await;
        let branches = user.json(user.get(&format!("/api/v1/conversations/{id}/git/branches"))).await;
        let switch = user.json(user.post(&format!("/api/v1/conversations/{id}/git/switch")).json(&json!({"branch": " topic ", "create": true, "track": "origin/topic"}))).await;
        let blank = user.json(user.post(&format!("/api/v1/conversations/{id}/git/switch")).json(&json!({"branch": " "}))).await;

        [graph, show, branches, switch, blank].map(|(status, _)| status)
    });

    let mut commands = Vec::new();

    for _ in 0..4 {
        commands.push(answer_next(&mut link, true, json!({}), None).await.1);
    }

    assert_eq!(
        commands,
        vec![
            GitCommand::Graph { limit: 200, skip: 200 },
            GitCommand::Show { hash: "abc1234".into() },
            GitCommand::Branches,
            GitCommand::Switch { branch: "topic".into(), create: true, track: Some("origin/topic".into()) },
        ]
    );
    assert_eq!(asking.await.unwrap(), [200, 200, 200, 200, 400]);
}

#[tokio::test]
async fn git_log_diff_and_stage_carry_their_arguments_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let log = user.json(user.get(&format!("/api/v1/conversations/{id}/git/log?limit=5"))).await;
        let capped = user.json(user.get(&format!("/api/v1/conversations/{id}/git/log?limit=9999"))).await;
        let diff = user.json(user.get(&format!("/api/v1/conversations/{id}/git/diff?path=src/a.rs&staged=true"))).await;
        let stage = user.json(user.post(&format!("/api/v1/conversations/{id}/git/stage")).json(&json!({"paths": ["a", "b"]}))).await;
        let unstage = user.json(user.post(&format!("/api/v1/conversations/{id}/git/unstage")).json(&json!({"paths": ["a"]}))).await;
        let all = user.json(user.post(&format!("/api/v1/conversations/{id}/git/stage-all"))).await;
        let undo = user.json(user.post(&format!("/api/v1/conversations/{id}/git/undo"))).await;

        [log, capped, diff, stage, unstage, all, undo].map(|(status, _)| status)
    });

    let mut commands = Vec::new();

    for _ in 0..7 {
        commands.push(answer_next(&mut link, true, json!({}), None).await.1);
    }

    assert_eq!(
        commands,
        vec![
            GitCommand::Log { limit: 5 },
            GitCommand::Log { limit: 200 },
            GitCommand::Diff { path: "src/a.rs".into(), staged: true },
            GitCommand::Stage { paths: vec!["a".into(), "b".into()] },
            GitCommand::Unstage { paths: vec!["a".into()] },
            GitCommand::StageAll,
            GitCommand::UndoLastCommit,
        ]
    );
    assert_eq!(asking.await.unwrap(), [200; 7]);
}

#[tokio::test]
async fn pull_and_push_carry_rebase_and_force_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let pull = user.json(user.post(&format!("/api/v1/conversations/{id}/git/pull")).json(&json!({}))).await;
        let rebase = user.json(user.post(&format!("/api/v1/conversations/{id}/git/pull")).json(&json!({"rebase": true}))).await;
        let push = user.json(user.post(&format!("/api/v1/conversations/{id}/git/push")).json(&json!({}))).await;
        let force = user.json(user.post(&format!("/api/v1/conversations/{id}/git/push")).json(&json!({"force": true}))).await;

        [pull, rebase, push, force].map(|(status, _)| status)
    });

    let mut commands = Vec::new();

    for _ in 0..4 {
        commands.push(answer_next(&mut link, true, json!({}), None).await.1);
    }

    assert_eq!(commands, vec![GitCommand::Pull { rebase: false }, GitCommand::Pull { rebase: true }, GitCommand::Push { force: false }, GitCommand::Push { force: true }]);
    assert_eq!(asking.await.unwrap(), [200; 4]);
}

#[tokio::test]
async fn a_commit_is_sent_to_the_node_with_its_message_trimmed() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "  Fix the thing\n"}))).await });

    let (_, command) = answer_next(&mut link, true, json!({}), None).await;

    assert_eq!(command, GitCommand::Commit { message: "Fix the thing".into(), options: CommitOptions::default() });
    assert_eq!(asking.await.unwrap().0, 200);
}

#[tokio::test]
async fn an_amend_may_keep_the_last_message_and_carries_its_options_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "", "amend": true, "signoff": true, "no_verify": true}))).await });

    let (_, command) = answer_next(&mut link, true, json!({}), None).await;

    assert_eq!(command, GitCommand::Commit { message: String::new(), options: CommitOptions { amend: true, signoff: true, no_verify: true } });
    assert_eq!(asking.await.unwrap().0, 200);
}

#[tokio::test]
async fn a_commit_without_a_message_is_refused_before_it_reaches_the_node() {
    let server = Server::start().await;
    let (_link, id) = conversation_on(&server).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "  "}))).await;

    assert_eq!(status, 400);
    assert_eq!(body["message"], "Enter a commit message.");
}

#[tokio::test]
async fn gits_own_error_comes_back_as_a_conflict() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/fetch"))).await });

    let (_, command) = answer_next(&mut link, false, Value::Null, Some("fatal: no remote repository specified")).await;

    assert_eq!(command, GitCommand::Fetch);
    let (status, body) = asking.await.unwrap();
    assert_eq!(status, 409);
    assert_eq!(body["error"], "git_failed");
    assert_eq!(body["message"], "fatal: no remote repository specified");
}

#[tokio::test]
async fn git_on_an_offline_node_says_so() {
    let server = Server::start().await;
    let (link, id) = conversation_on(&server).await;
    drop(link);

    let path = format!("/api/v1/conversations/{id}/git/status");
    let body = common::eventually("the node to go offline", || async {
        let (status, body) = server.user.json(server.user.get(&path)).await;

        (status == 409).then_some(body)
    })
    .await;

    assert_eq!(body["error"], "node_offline");
}

#[tokio::test]
async fn another_user_cannot_run_git_in_a_conversation() {
    let server = Server::start().await;
    let (_link, id) = conversation_on(&server).await;
    let other = server.second_user("eve@example.com").await;

    assert_eq!(other.json(other.get(&format!("/api/v1/conversations/{id}/git/status"))).await.0, 404);
    assert_eq!(other.json(other.post(&format!("/api/v1/conversations/{id}/git/stage-all"))).await.0, 404);
}

fn stack_answer() -> Value {
    json!({"current": "one", "main": "main", "inferred": false, "git_town_available": true, "branches": [
        {"name": "main", "parent": null, "depth": 0, "current": false, "main": true, "ahead": 0, "behind": 0, "has_upstream": true, "pushed": true, "needs_sync": false, "subject": "Base"},
        {"name": "one", "parent": "main", "depth": 1, "current": true, "main": false, "ahead": 2, "behind": 0, "has_upstream": true, "pushed": true, "needs_sync": false, "subject": "One"},
        {"name": "two", "parent": "one", "depth": 2, "current": false, "main": false, "ahead": 1, "behind": 0, "has_upstream": false, "pushed": false, "needs_sync": false, "subject": "Two"},
        {"name": "three", "parent": "two", "depth": 3, "current": false, "main": false, "ahead": 1, "behind": 0, "has_upstream": false, "pushed": false, "needs_sync": false, "subject": "Three"},
    ]})
}

fn pull(number: i64, title: &str, branch: &str, state: &str, draft: bool, merged: bool, repository: &str) -> Value {
    json!({"number": number, "title": title, "state": state, "draft": draft, "merged_at": if merged { json!("2026-10-01T10:00:00Z") } else { Value::Null }, "html_url": format!("https://forge.example/acme-org/acme-backups/pull/{number}"), "head": {"ref": branch, "repo": {"full_name": repository}}})
}

async fn fake_github(pulls: Value) -> String {
    let router = Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/repos/{owner}/{name}/pulls", get(move || { let pulls = pulls.clone(); async move { Json(pulls) } }));

    serve(router).await
}

async fn conversation_in_forge_project(server: &Server, github: &str) -> (NodeLink, Uuid) {
    let mut link = server.online_node("laptop").await;
    let (_, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": "github", "base_url": github, "token": "ghp_0123456789abcdefghijklmnopqrstuvwxyz"}))).await;
    let body = json!({"node_id": link.id, "forge_id": forge["id"], "repository": "acme-org/acme-backups"});
    let user = server.user.clone();
    let cloning = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });

    answer_next(&mut link, true, json!({"path": "/work/backups"}), None).await;
    let (status, project) = cloning.await.unwrap();
    assert_eq!(status, 201, "{project}");

    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    sqlx::query("update conversations set project_id = $1 where id = $2").bind(project["id"].as_str().unwrap().parse::<Uuid>().unwrap()).bind(created.id).execute(&server.pool).await.unwrap();

    (link, created.id)
}

async fn ask_stack(server: &Server, link: &mut NodeLink, id: Uuid) -> (u16, Value) {
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.get(&format!("/api/v1/conversations/{id}/git/stack"))).await });
    let (_, command) = answer_next(link, true, stack_answer(), None).await;

    assert_eq!(command, GitCommand::Stack);

    asking.await.unwrap()
}

#[tokio::test]
async fn the_stack_of_a_conversation_without_a_forge_project_has_no_pull_request_information() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;

    let (status, stack) = ask_stack(&server, &mut link, id).await;

    assert_eq!(status, 200);
    assert_eq!(stack["pull_requests_available"], false);
    assert_eq!(stack["pull_requests_problem"], Value::Null);
    assert_eq!(stack["branches"][1]["pull_request"], Value::Null);
}

#[tokio::test]
async fn each_branch_of_the_stack_gets_the_pull_request_of_its_name_preferring_an_open_one_and_ignoring_forks() {
    let server = Server::start().await;
    let pulls = json!([
        pull(14, "Rework one again", "one", "closed", false, true, "acme-org/acme-backups"),
        pull(12, "Rework one", "one", "open", false, false, "acme-org/acme-backups"),
        pull(13, "Sketch two", "two", "open", true, false, "acme-org/acme-backups"),
        pull(15, "A fork's three", "three", "open", false, false, "someone/acme-backups"),
        pull(9, "Old base", "main", "closed", false, false, "acme-org/acme-backups"),
    ]);
    let (mut link, id) = conversation_in_forge_project(&server, &fake_github(pulls).await).await;

    let (status, stack) = ask_stack(&server, &mut link, id).await;

    assert_eq!(status, 200, "{stack}");
    assert_eq!(stack["pull_requests_available"], true);
    assert_eq!(stack["branches"][1]["pull_request"], json!({"number": 12, "label": "PR #12", "title": "Rework one", "url": "https://forge.example/acme-org/acme-backups/pull/12", "state": "open"}));
    assert_eq!((stack["branches"][2]["pull_request"]["number"].clone(), stack["branches"][2]["pull_request"]["state"].clone()), (json!(13), json!("draft")));
    assert_eq!(stack["branches"][3]["pull_request"], Value::Null);
    assert_eq!(stack["branches"][0]["pull_request"]["state"], "closed");
}

#[tokio::test]
async fn a_forge_that_cannot_answer_leaves_the_stack_without_pull_requests_and_says_why() {
    let server = Server::start().await;
    let failing = Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/repos/{owner}/{name}/pulls", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }));
    let (mut link, id) = conversation_in_forge_project(&server, &serve(failing).await).await;

    let (status, stack) = ask_stack(&server, &mut link, id).await;

    assert_eq!(status, 200);
    assert_eq!(stack["pull_requests_available"], false);
    assert!(stack["pull_requests_problem"].as_str().is_some_and(|problem| !problem.is_empty()), "{stack}");
    assert_eq!(stack["branches"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn git_town_sync_and_propose_are_run_by_the_node_and_its_output_is_returned() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let sync = user.json(user.post(&format!("/api/v1/conversations/{id}/git/town/sync"))).await;
        let propose = user.json(user.post(&format!("/api/v1/conversations/{id}/git/town/propose"))).await;

        [sync, propose]
    });

    let (_, first) = answer_next(&mut link, true, json!({"output": "synced"}), None).await;
    let (_, second) = answer_next(&mut link, false, Value::Null, Some("conflict in a.txt")).await;

    assert_eq!((first, second), (GitCommand::TownSync, GitCommand::TownPropose));

    let [sync, propose] = asking.await.unwrap();

    assert_eq!(sync, (200, json!({"output": "synced"})));
    assert_eq!((propose.0, propose.1["message"].clone()), (409, json!("conflict in a.txt")));
}
