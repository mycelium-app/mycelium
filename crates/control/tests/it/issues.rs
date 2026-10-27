use crate::common;
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use crate::common::{NodeLink, Server, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type Seen = Arc<Mutex<Vec<String>>>;

fn issue_json(number: i64, title: &str, body: Option<&str>) -> Value {
    json!({"number": number, "title": title, "html_url": format!("https://forge.example/acme-org/acme-backups/issues/{number}"), "body": body})
}

async fn fake_github(seen: Seen) -> String {
    async fn search(State(seen): State<Seen>, Query(query): Query<HashMap<String, String>>) -> Json<Value> {
        seen.lock().unwrap().push(query.get("q").cloned().unwrap_or_default());

        Json(json!({"items": [issue_json(412, "Restore fails silently after a 503", Some("Steps:\n1. Restore")), issue_json(377, "Android progress jumps", None)]}))
    }

    async fn one(Path((_, _, number)): Path<(String, String, i64)>) -> Result<Json<Value>, StatusCode> {
        match number {
            412 => Ok(Json(issue_json(412, "Restore fails silently after a 503", Some("Steps:\n1. Restore")))),
            377 => Ok(Json(issue_json(377, "Android progress jumps", None))),
            _ => Err(StatusCode::NOT_FOUND),
        }
    }

    async fn timeline(Path((_, _, number)): Path<(String, String, i64)>) -> Result<Json<Value>, StatusCode> {
        match number {
            412 => Ok(Json(json!([
                {"event": "labeled"},
                {"event": "cross-referenced", "source": {"issue": {"number": 80, "html_url": "https://forge.example/acme-org/acme-backups/issues/80", "state": "open"}}},
                {"event": "cross-referenced", "source": {"issue": {"number": 77, "html_url": "https://forge.example/acme-org/acme-backups/pull/77", "state": "open", "pull_request": {"merged_at": null}}}},
                {"event": "cross-referenced", "source": {"issue": {"number": 70, "html_url": "https://forge.example/acme-org/acme-backups/pull/70", "state": "closed", "pull_request": {"merged_at": "2026-10-01T10:00:00Z"}}}},
                {"event": "cross-referenced", "source": {"issue": {"number": 77, "html_url": "https://forge.example/acme-org/acme-backups/pull/77", "state": "open", "pull_request": {"merged_at": null}}}},
            ]))),
            _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    let router = Router::new()
        .route("/api/v3/repos/{owner}/{name}/issues/{number}/timeline", get(timeline))
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/search/issues", get(search))
        .route("/api/v3/repos/{owner}/{name}/issues/{number}", get(one))
        .with_state(seen);

    serve(router).await
}

async fn fake_gitlab(seen: Seen) -> String {
    async fn issues(State(seen): State<Seen>, headers: HeaderMap, Path(project): Path<String>, Query(query): Query<HashMap<String, String>>) -> Result<Json<Value>, StatusCode> {
        if headers.get("private-token").and_then(|value| value.to_str().ok()) != Some("glpat-0123456789abcdefghij") {
            return Err(StatusCode::UNAUTHORIZED);
        }

        seen.lock().unwrap().push(format!("{project}|{}|{}", query.get("state").cloned().unwrap_or_default(), query.get("search").cloned().unwrap_or_default()));

        Ok(Json(json!([{"iid": 5, "title": "Mirror fails", "web_url": "https://git.example/slop/mycelium/-/issues/5", "description": "It does"}])))
    }

    async fn related(Path((_, iid)): Path<(String, i64)>) -> Json<Value> {
        assert_eq!(iid, 5);

        Json(json!([
            {"iid": 77, "state": "opened", "web_url": "https://git.example/slop/mycelium/-/merge_requests/77"},
            {"iid": 61, "state": "closed", "web_url": "https://git.example/slop/mycelium/-/merge_requests/61"},
        ]))
    }

    let router = Router::new()
        .route("/api/v4/projects/{project}/issues/{iid}/related_merge_requests", get(related))
        .route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) }))
        .route("/api/v4/groups", get(|| async { Json(json!([])) }))
        .route("/api/v4/projects/{project}/issues", get(issues))
        .with_state(seen);

    serve(router).await
}

async fn connect_forge(server: &Server, kind: &str, base: &str) -> Uuid {
    let token = if kind == "github" { "ghp_0123456789abcdefghijklmnopqrstuvwxyz" } else { "glpat-0123456789abcdefghij" };
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base, "token": token}))).await;
    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
}

async fn answer_next(link: &mut NodeLink, output: Value) -> (String, GitCommand) {
    let (request_id, cwd, command) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };

    link.send(NodeToControl::GitResult { request_id, ok: true, output, error: None }).await;

    (cwd, command)
}

async fn clone_project(server: &Server, link: &mut NodeLink, forge_id: Uuid, repository: &str, path: &str) -> Value {
    let user = server.user.clone();
    let body = json!({"node_id": link.id, "forge_id": forge_id, "repository": repository});
    let asking = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&body)).await });

    answer_next(link, json!({"path": path})).await;
    let (status, project) = asking.await.unwrap();
    assert_eq!(status, 201, "{project}");

    project
}

#[tokio::test]
async fn open_issues_are_listed_for_the_forge_repositories_set_up_as_projects_with_the_query_passed_on() {
    let server = Server::start().await;
    let github_seen = Seen::default();
    let gitlab_seen = Seen::default();
    let github = connect_forge(&server, "github", &fake_github(github_seen.clone()).await).await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab(gitlab_seen.clone()).await).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/acme-backups").await;
    clone_project(&server, &mut link, gitlab, "slop/mycelium", "/p/mycelium").await;
    let mut second = server.online_node("server").await;
    clone_project(&server, &mut second, github, "acme-org/acme-backups", "/q/acme-backups").await;

    let (status, listed) = server.user.json(server.user.get("/api/v1/issues?q=label%3Abug+assignee%3A%40me")).await;

    assert_eq!(status, 200, "{listed}");
    let mut issues = listed["issues"].as_array().unwrap().clone();
    issues.sort_by_key(|issue| (issue["repository"].as_str().unwrap().to_string(), std::cmp::Reverse(issue["number"].as_i64().unwrap())));
    assert_eq!(issues.len(), 3, "a repository set up on two nodes is fetched once");
    assert_eq!((issues[0]["repository"].clone(), issues[0]["number"].clone(), issues[0]["forge_id"].clone(), issues[0]["body"].clone()), (json!("acme-org/acme-backups"), json!(412), json!(github), json!("Steps:\n1. Restore")));
    assert_eq!(issues[1]["body"], "");
    assert_eq!((issues[2]["title"].clone(), issues[2]["url"].clone()), (json!("Mirror fails"), json!("https://git.example/slop/mycelium/-/issues/5")));
    assert_eq!(*github_seen.lock().unwrap(), vec!["repo:acme-org/acme-backups is:issue is:open label:bug assignee:@me".to_string()]);
    assert_eq!(*gitlab_seen.lock().unwrap(), vec!["slop/mycelium|opened|label:bug assignee:@me".to_string()]);
}

#[tokio::test]
async fn a_forge_that_fails_is_reported_beside_the_issues_of_the_others() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/a").await;
    clone_project(&server, &mut link, gitlab, "slop/mycelium", "/p/b").await;
    sqlx::query("update forges set token = 'expired' where kind = 'gitlab'").execute(&server.pool).await.unwrap();

    let (_, listed) = server.user.json(server.user.get("/api/v1/issues")).await;

    assert_eq!(listed["issues"].as_array().unwrap().len(), 2);
    assert_eq!(listed["failures"][0]["forge_id"], json!(gitlab));
}

#[tokio::test]
async fn issues_are_only_listed_for_your_own_projects() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/a").await;
    let other = server.second_user("other@example.com").await;

    let (_, theirs) = other.json(other.get("/api/v1/issues")).await;

    assert_eq!(theirs, json!({"issues": [], "failures": []}));
}

#[tokio::test]
async fn starting_a_workspace_makes_a_worktree_on_an_issue_branch_and_prompts_with_the_issue() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    let project = clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/acme-backups").await;
    let user = server.user.clone();
    let mut stream = server.user.stream().await;
    let path = format!("/api/v1/projects/{}/issues/412/workspace", project["id"].as_str().unwrap());
    let starting = tokio::spawn(async move { user.json(user.post(&path).json(&json!({"agent": "claude"}))).await });

    let (cwd, command) = answer_next(&mut link, json!({"path": "/p/acme-backups-worktrees/issue-412-restore-fails-silently-after-a-503"})).await;
    let (status, conversation) = starting.await.unwrap();

    assert_eq!((cwd.as_str(), command), ("/p/acme-backups", GitCommand::AddWorktree { branch: "issue/412-restore-fails-silently-after-a-503".into() }));
    assert_eq!(status, 201, "{conversation}");
    assert_eq!((conversation["cwd"].clone(), conversation["title"].clone()), (json!("/p/acme-backups-worktrees/issue-412-restore-fails-silently-after-a-503"), json!("#412 Restore fails silently after a 503")));

    let streamed = stream.until(|message| match message {
        mycelium_proto::stream::StreamMessage::Conversation(streamed) => Some(streamed),
        _ => None,
    })
    .await;
    assert_eq!(streamed.title, "#412 Restore fails silently after a 503");

    let conversation_id = link.expect_start_session().await;
    link.start_session(conversation_id, "acp-1").await;
    let (_, prompt) = link.expect_prompt().await;
    assert_eq!(prompt, "Work on issue #412: Restore fails silently after a 503\nhttps://forge.example/acme-org/acme-backups/issues/412\n\nSteps:\n1. Restore");

    let (_, worktrees) = server.user.json(server.user.get("/api/v1/worktrees")).await;
    assert_eq!((worktrees[0]["issue_number"].clone(), worktrees[0]["branch"].clone()), (json!(412), json!("issue/412-restore-fails-silently-after-a-503")));
}

#[tokio::test]
async fn starting_a_workspace_again_reuses_the_issues_worktree_and_an_issue_without_a_body_prompts_with_its_link_only() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    let project = clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/acme-backups").await;
    let path = format!("/api/v1/projects/{}/issues/377/workspace", project["id"].as_str().unwrap());
    let user = server.user.clone();
    let first_path = path.clone();
    let first = tokio::spawn(async move { user.json(user.post(&first_path).json(&json!({"agent": "claude"}))).await });
    answer_next(&mut link, json!({"path": "/p/wt/issue-377-android-progress-jumps"})).await;
    first.await.unwrap();
    link.expect_start_session().await;

    let (status, again) = server.user.json(server.user.post(&path).json(&json!({"agent": "claude"}))).await;

    assert_eq!((status, again["cwd"].clone()), (201, json!("/p/wt/issue-377-android-progress-jumps")));
    let conversation_id = link.expect_start_session().await;
    link.start_session(conversation_id, "acp-2").await;
    let (_, prompt) = link.expect_prompt().await;
    assert_eq!(prompt, "Work on issue #377: Android progress jumps\nhttps://forge.example/acme-org/acme-backups/issues/377");
    let (_, worktrees) = server.user.json(server.user.get("/api/v1/worktrees")).await;
    assert_eq!(worktrees.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_workspace_needs_an_issue_that_exists_a_forge_project_and_a_project_of_your_own() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    let project = clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/acme-backups").await;
    let user = server.user.clone();
    let registering = tokio::spawn(async move { user.json(user.post("/api/v1/projects").json(&json!({"node_id": link.id, "path": "plain"}))).await });
    let mut link = link;
    answer_next(&mut link, json!({"path": "/p/plain"})).await;
    let (_, folder) = registering.await.unwrap();
    let other = server.second_user("other@example.com").await;
    let start = |client: &common::Client, project: &Value, number: i64| {
        let client = client.clone();
        let path = format!("/api/v1/projects/{}/issues/{number}/workspace", project["id"].as_str().unwrap());

        async move { client.json(client.post(&path).json(&json!({"agent": "claude"}))).await }
    };

    let (missing, _) = start(&server.user, &project, 9999).await;
    let (no_forge, error) = start(&server.user, &folder, 1).await;
    let (not_theirs, _) = start(&other, &project, 412).await;

    assert_eq!((missing, no_forge, error["error"].clone(), not_theirs), (404, 409, json!("project_without_forge"), 404));
}

#[tokio::test]
async fn each_issue_lists_the_pull_requests_linked_to_it_and_one_the_forge_cannot_answer_for_is_unknown_without_hiding_the_list() {
    let server = Server::start().await;
    let github = connect_forge(&server, "github", &fake_github(Seen::default()).await).await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab(Seen::default()).await).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, github, "acme-org/acme-backups", "/p/acme-backups").await;
    clone_project(&server, &mut link, gitlab, "slop/mycelium", "/p/mycelium").await;

    let (status, listed) = server.user.json(server.user.get("/api/v1/issues")).await;

    assert_eq!(status, 200, "{listed}");
    let issue = |number: i64| listed["issues"].as_array().unwrap().iter().find(|issue| issue["number"] == number).unwrap().clone();
    assert_eq!(
        issue(412)["pull_requests"],
        json!([
            {"number": 77, "label": "PR #77", "url": "https://forge.example/acme-org/acme-backups/pull/77", "state": "open"},
            {"number": 70, "label": "PR #70", "url": "https://forge.example/acme-org/acme-backups/pull/70", "state": "merged"},
        ])
    );
    assert_eq!(
        issue(5)["pull_requests"],
        json!([
            {"number": 77, "label": "MR !77", "url": "https://git.example/slop/mycelium/-/merge_requests/77", "state": "open"},
            {"number": 61, "label": "MR !61", "url": "https://git.example/slop/mycelium/-/merge_requests/61", "state": "closed"},
        ])
    );
    assert_eq!(issue(377).get("pull_requests"), None, "an issue whose pull requests could not be read carries no answer");
    assert_eq!(listed["failures"], json!([]));
}
