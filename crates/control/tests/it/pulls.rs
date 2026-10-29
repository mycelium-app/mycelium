use crate::common::{NodeLink, Server, serve};
use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::http::HeaderMap;
use axum::routing::get;
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

const FORGEJO_TOKEN: &str = "fj0123456789abcdefghijklmnop";
const GITLAB_TOKEN: &str = "glpat-0123456789abcdefghij";

fn forgejo_pull(number: i64, title: &str, updated: &str, state: &str) -> Value {
    json!({
        "number": number,
        "title": title,
        "body": "Rewrites the restore path",
        "state": state,
        "merged": false,
        "draft": false,
        "html_url": format!("https://forge.example/acme-org/acme-backups/pulls/{number}"),
        "user": {"login": "samexample", "avatar_url": "https://forge.example/avatars/sam"},
        "assignees": [{"login": "riverexample", "avatar_url": "https://forge.example/avatars/river"}],
        "head": {"ref": "topic", "repo": {"full_name": "acme-org/acme-backups"}},
        "base": {"ref": "main"},
        "updated_at": updated,
    })
}

async fn fake_forgejo() -> String {
    async fn pulls(headers: HeaderMap) -> Json<Value> {
        assert_eq!(headers.get("authorization").and_then(|value| value.to_str().ok()), Some(format!("token {FORGEJO_TOKEN}").as_str()));

        Json(json!([forgejo_pull(8, "Older change", "2026-09-01T10:00:00Z", "open"), forgejo_pull(9, "Newer change", "2026-10-02T10:00:00Z", "open")]))
    }

    async fn one(Path((_, _, number)): Path<(String, String, i64)>) -> Json<Value> {
        Json(forgejo_pull(number, "Newer change", "2026-10-02T10:00:00Z", "open"))
    }

    async fn comments() -> Json<Value> {
        Json(json!([{"body": "Looks right", "created_at": "2026-10-02T11:00:00Z", "user": {"login": "riverexample"}}]))
    }

    async fn review_comments() -> Json<Value> {
        Json(json!([
            {"body": "Rename this", "created_at": "2026-10-02T12:00:00Z", "path": "src/restore.rs", "user": {"login": "riverexample"}, "resolver": null},
            {"body": "Done", "created_at": "2026-10-02T10:30:00Z", "path": "src/lib.rs", "user": {"login": "samexample"}, "resolver": {"login": "samexample"}},
        ]))
    }

    async fn diff() -> &'static str {
        "diff --git a/src/restore.rs b/src/restore.rs\n--- a/src/restore.rs\n+++ b/src/restore.rs\n@@ -1 +1 @@\n-old\n+new\n"
    }

    let router = Router::new()
        .route("/api/v1/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v1/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v1/repos/{owner}/{name}/pulls", get(pulls))
        .route("/api/v1/repos/{owner}/{name}/pulls/{number}", get(one))
        .route("/api/v1/repos/{owner}/{name}/pulls/{number}/comments", get(review_comments))
        .route("/api/v1/repos/{owner}/{name}/pulls/9.diff", get(diff))
        .route("/api/v1/repos/{owner}/{name}/issues/{number}/comments", get(comments));

    serve(router).await
}

async fn fake_gitlab() -> String {
    async fn merge_requests() -> Json<Value> {
        Json(json!([{
            "iid": 5,
            "title": "Mirror fix",
            "description": "",
            "state": "merged",
            "web_url": "https://git.example/slop/mycelium/-/merge_requests/5",
            "author": {"username": "sam", "avatar_url": "https://git.example/avatars/sam"},
            "assignees": [],
            "source_branch": "mirror",
            "target_branch": "main",
            "updated_at": "2026-10-03T10:00:00Z",
        }]))
    }

    async fn one() -> Json<Value> {
        Json(merge_requests().await.0[0].clone())
    }

    async fn discussions() -> Json<Value> {
        Json(json!([
            {"notes": [{"body": "merged", "system": true, "created_at": "2026-10-03T09:00:00Z", "author": {"username": "sam"}}]},
            {"notes": [{"body": "Check this", "system": false, "resolvable": true, "resolved": false, "created_at": "2026-10-03T08:00:00Z", "author": {"username": "river"}, "position": {"new_path": "a.rs"}}]},
            {"notes": [{"body": "Thanks", "system": false, "resolvable": false, "created_at": "2026-10-03T07:00:00Z", "author": {"username": "sam"}}]},
        ]))
    }

    async fn changes() -> Json<Value> {
        Json(json!({"changes": [{"old_path": "a.rs", "new_path": "a.rs", "diff": "@@ -1 +1 @@\n-x\n+y\n"}]}))
    }

    let router = Router::new()
        .route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) }))
        .route("/api/v4/groups", get(|| async { Json(json!([])) }))
        .route("/api/v4/projects/{project}/merge_requests", get(merge_requests))
        .route("/api/v4/projects/{project}/merge_requests/5", get(one))
        .route("/api/v4/projects/{project}/merge_requests/5/discussions", get(discussions))
        .route("/api/v4/projects/{project}/merge_requests/5/changes", get(changes));

    serve(router).await
}

async fn connect_forge(server: &Server, kind: &str, base: &str, token: &str) -> Uuid {
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base, "token": token}))).await;
    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
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

async fn answer_next(link: &mut NodeLink, output: Value) -> (String, GitCommand) {
    let (request_id, cwd, command) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };

    link.send(NodeToControl::GitResult { request_id, ok: true, output, error: None }).await;

    (cwd, command)
}

#[tokio::test]
async fn pull_requests_of_every_forge_are_listed_newest_first_with_their_people() {
    let server = Server::start().await;
    let forgejo = connect_forge(&server, "forgejo", &fake_forgejo().await, FORGEJO_TOKEN).await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab().await, GITLAB_TOKEN).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, forgejo, "acme-org/acme-backups", "/p/acme-backups").await;
    clone_project(&server, &mut link, gitlab, "slop/mycelium", "/p/mycelium").await;

    let (status, listed) = server.user.json(server.user.get("/api/v1/pulls")).await;

    assert_eq!(status, 200, "{listed}");
    let numbers: Vec<(String, i64)> = listed["pull_requests"].as_array().unwrap().iter().map(|item| (item["repository"].as_str().unwrap().to_string(), item["number"].as_i64().unwrap())).collect();
    assert_eq!(numbers, vec![("slop/mycelium".to_string(), 5), ("acme-org/acme-backups".to_string(), 9), ("acme-org/acme-backups".to_string(), 8)]);
    let merged = &listed["pull_requests"][0];
    assert_eq!((merged["state"].clone(), merged["label"].clone(), merged["source_branch"].clone()), (json!("merged"), json!("MR !5"), json!("mirror")));
    let open = &listed["pull_requests"][1];
    assert_eq!((open["state"].clone(), open["author"].clone(), open["assignees"][0]["name"].clone()), (json!("open"), json!({"name": "samexample", "avatar_url": "https://forge.example/avatars/sam"}), json!("riverexample")));
}

#[tokio::test]
async fn the_pull_request_list_can_be_narrowed_by_title_or_number() {
    let server = Server::start().await;
    let forgejo = connect_forge(&server, "forgejo", &fake_forgejo().await, FORGEJO_TOKEN).await;
    let mut link = server.online_node("laptop").await;
    clone_project(&server, &mut link, forgejo, "acme-org/acme-backups", "/p/acme-backups").await;

    let (_, by_title) = server.user.json(server.user.get("/api/v1/pulls?q=older")).await;
    let (_, by_number) = server.user.json(server.user.get("/api/v1/pulls?q=%239")).await;

    assert_eq!(by_title["pull_requests"].as_array().unwrap().len(), 1);
    assert_eq!(by_title["pull_requests"][0]["number"], 8);
    assert_eq!(by_number["pull_requests"][0]["number"], 9);
}

#[tokio::test]
async fn a_pull_request_timeline_joins_conversation_and_review_comments_oldest_first_and_says_which_are_resolved() {
    let server = Server::start().await;
    let forgejo = connect_forge(&server, "forgejo", &fake_forgejo().await, FORGEJO_TOKEN).await;

    let (status, detail) = server.user.json(server.user.get(&format!("/api/v1/forges/{forgejo}/pulls/9?repository=acme-org%2Facme-backups"))).await;

    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["pull_request"]["title"], "Newer change");
    let timeline: Vec<(Value, Value)> = detail["timeline"].as_array().unwrap().iter().map(|comment| (comment["body"].clone(), comment["resolved"].clone())).collect();
    assert_eq!(timeline, vec![(json!("Done"), json!(true)), (json!("Looks right"), Value::Null), (json!("Rename this"), json!(false))]);
}

#[tokio::test]
async fn a_gitlab_timeline_leaves_out_system_notes_and_reports_discussion_resolution() {
    let server = Server::start().await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab().await, GITLAB_TOKEN).await;

    let (_, detail) = server.user.json(server.user.get(&format!("/api/v1/forges/{gitlab}/pulls/5?repository=slop%2Fmycelium"))).await;

    let timeline: Vec<(Value, Value)> = detail["timeline"].as_array().unwrap().iter().map(|comment| (comment["body"].clone(), comment["resolved"].clone())).collect();
    assert_eq!(timeline, vec![(json!("Thanks"), Value::Null), (json!("Check this"), json!(false))]);
}

#[tokio::test]
async fn the_diff_of_a_pull_request_comes_from_the_forge() {
    let server = Server::start().await;
    let forgejo = connect_forge(&server, "forgejo", &fake_forgejo().await, FORGEJO_TOKEN).await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab().await, GITLAB_TOKEN).await;

    let (_, from_forgejo) = server.user.json(server.user.get(&format!("/api/v1/forges/{forgejo}/pulls/9/diff?repository=acme-org%2Facme-backups"))).await;
    let (_, from_gitlab) = server.user.json(server.user.get(&format!("/api/v1/forges/{gitlab}/pulls/5/diff?repository=slop%2Fmycelium"))).await;

    assert!(from_forgejo["diff"].as_str().unwrap().contains("+new"), "{from_forgejo}");
    assert_eq!(from_gitlab["diff"], "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\n");
}

#[tokio::test]
async fn loading_a_pull_request_as_a_workspace_checks_out_its_branch_and_prompts_with_it() {
    let server = Server::start().await;
    let forgejo = connect_forge(&server, "forgejo", &fake_forgejo().await, FORGEJO_TOKEN).await;
    let mut link = server.online_node("laptop").await;
    let project = clone_project(&server, &mut link, forgejo, "acme-org/acme-backups", "/p/acme-backups").await;
    let user = server.user.clone();
    let path = format!("/api/v1/projects/{}/pulls/9/workspace", project["id"].as_str().unwrap());
    let starting = tokio::spawn(async move { user.json(user.post(&path).json(&json!({"agent": "claude"}))).await });

    let (cwd, command) = answer_next(&mut link, json!({"path": "/p/acme-backups-worktrees/topic"})).await;
    let (status, conversation) = starting.await.unwrap();

    assert_eq!((cwd.as_str(), command), ("/p/acme-backups", GitCommand::AddRemoteWorktree { branch: "topic".into() }));
    assert_eq!(status, 201, "{conversation}");
    assert_eq!((conversation["cwd"].clone(), conversation["title"].clone()), (json!("/p/acme-backups-worktrees/topic"), json!("PR #9 Newer change")));

    let conversation_id = link.expect_start_session().await;
    link.start_session(conversation_id, "acp-1").await;
    let (_, prompt) = link.expect_prompt().await;
    assert_eq!(prompt, "Work on PR #9: Newer change\nhttps://forge.example/acme-org/acme-backups/pulls/9\n\nRewrites the restore path");
}

#[tokio::test]
async fn a_pull_request_that_does_not_exist_is_not_found() {
    let server = Server::start().await;
    let gitlab = connect_forge(&server, "gitlab", &fake_gitlab().await, GITLAB_TOKEN).await;

    let (status, _) = server.user.json(server.user.get(&format!("/api/v1/forges/{gitlab}/pulls/6/diff?repository=slop%2Fmycelium"))).await;

    assert_eq!(status, 400, "the forge's own 404 is relayed as a forge failure");
}
