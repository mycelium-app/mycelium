mod common;

use common::Server;
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn add_task(server: &Server, node_id: Uuid, body: Value) -> (u16, Value) {
    let mut body = body;
    body["node_id"] = json!(node_id);
    body["agent"] = json!("claude");

    server.user.json(server.user.post("/api/v1/tasks").json(&body)).await
}

async fn open_tasks(server: &Server) -> Vec<Value> {
    let (_, tasks) = server.user.json(server.user.get("/api/v1/tasks")).await;

    tasks.as_array().unwrap().clone()
}

#[tokio::test]
async fn a_task_waits_in_the_backlog_until_it_is_started() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, task) = add_task(&server, link.id, json!({"title": "Rotate the S3 keys", "cwd": "/work/app"})).await;
    assert_eq!(status, 201, "{task}");

    let tasks = open_tasks(&server).await;
    assert_eq!((tasks.len(), tasks[0]["title"].clone(), tasks[0]["cwd"].clone()), (1, json!("Rotate the S3 keys"), json!("/work/app")));
}

#[tokio::test]
async fn starting_a_task_starts_a_conversation_with_its_title_and_prompt_and_leaves_the_backlog() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Rebuild install.md", "cwd": "/work/app", "prompt": "Rebuild install.md for the new flags"})).await;

    let (status, conversation) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!((status, conversation["title"].clone(), conversation["cwd"].clone(), conversation["status"].clone()), (201, json!("Rebuild install.md"), json!("/work/app"), json!("working")));

    match link.next().await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, "/work/app"),
        other => panic!("expected start_session, got {other:?}"),
    }

    assert_eq!(open_tasks(&server).await.len(), 0);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_task_without_a_prompt_starts_with_its_title_as_the_prompt() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Verify v0.1.11 from S3", "cwd": "/work/app"})).await;

    let (_, conversation) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    link.expect_start_session().await;

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation["id"].as_str().unwrap()))).await;
    assert_eq!(queue[0]["text"], json!("Verify v0.1.11 from S3"));
}

#[tokio::test]
async fn a_task_on_an_offline_node_stays_in_the_backlog_with_a_reason() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app"})).await;
    drop(link);

    common::eventually("the node going offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("node_offline")));
    assert_eq!(open_tasks(&server).await.len(), 1);
}

#[tokio::test]
async fn a_task_needs_a_title_a_folder_or_project_and_a_project_on_its_node() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let other = server.online_node("desktop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let (status, body) = add_task(&server, link.id, json!({"title": "  ", "cwd": "/work/app"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("title_required")));

    let (status, body) = add_task(&server, link.id, json!({"title": "Tidy"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("cwd_required")));

    let (status, body) = add_task(&server, other.id, json!({"title": "Tidy", "project_id": project})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("project_not_on_node")));

    let (status, body) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app", "branch": "tidy"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("project_required")));

    let (status, task) = add_task(&server, link.id, json!({"title": "Tidy", "project_id": project, "branch": "tidy"})).await;
    assert_eq!((status, task["project_id"].clone(), task["branch"].clone()), (201, json!(project), json!("tidy")));
}

#[tokio::test]
async fn a_task_can_be_deleted_and_only_by_its_owner() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app"})).await;
    let path = format!("/api/v1/tasks/{}", task["id"].as_str().unwrap());
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.delete(&path)).await;
    assert_eq!(status, 404);
    let (status, _) = other.json(other.post(&format!("{path}/start"))).await;
    assert_eq!(status, 404);
    let (_, theirs) = other.json(other.get("/api/v1/tasks")).await;
    assert_eq!(theirs, json!([]));

    let (status, _) = server.user.json(server.user.delete(&path)).await;
    assert_eq!(status, 204);
    assert_eq!(open_tasks(&server).await.len(), 0);
}
