mod common;

use common::{Server, eventually};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const EVERY_MONDAY: &str = "0 6 * * 1";

async fn ticking_server() -> Server {
    Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await
}

async fn add_schedule(server: &Server, node_id: Uuid, body: Value) -> (u16, Value) {
    let mut body = body;
    body["node_id"] = json!(node_id);
    body["agent"] = json!("claude");

    server.user.json(server.user.post("/api/v1/schedules").json(&body)).await
}

async fn weekly(server: &Server, node_id: Uuid, each_run: &str) -> Uuid {
    let (status, schedule) = add_schedule(server, node_id, json!({"prompt": "Update and test dependencies", "cwd": "/work/app", "cron": EVERY_MONDAY, "each_run": each_run})).await;
    assert_eq!(status, 201, "{schedule}");

    schedule["id"].as_str().unwrap().parse().unwrap()
}

async fn make_due(server: &Server, id: Uuid) {
    sqlx::query("update schedules set next_run_at = now() - interval '1 minute' where id = $1").bind(id).execute(&server.pool).await.unwrap();
}

async fn schedule(server: &Server, id: Uuid) -> Value {
    let (_, schedules) = server.user.json(server.user.get("/api/v1/schedules")).await;

    schedules.as_array().unwrap().iter().find(|schedule| schedule["id"] == json!(id)).unwrap().clone()
}

async fn runs(server: &Server) -> Vec<Value> {
    let (_, runs) = server.user.json(server.user.get("/api/v1/schedule-runs")).await;

    runs.as_array().unwrap().clone()
}

async fn first_run(server: &Server) -> Value {
    eventually("a recorded run", || async { runs(server).await.first().cloned() }).await
}

#[tokio::test]
async fn a_schedule_is_listed_with_its_next_run_in_the_future() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let id = weekly(&server, link.id, "new").await;
    let listed = schedule(&server, id).await;
    let next: chrono::DateTime<chrono::Utc> = listed["next_run_at"].as_str().unwrap().parse().unwrap();

    assert!(next > chrono::Utc::now());
    assert_eq!((listed["cron"].clone(), listed["paused"].clone(), listed["last_run_at"].clone()), (json!(EVERY_MONDAY), json!(false), Value::Null));
}

#[tokio::test]
async fn a_schedule_needs_a_prompt_a_valid_cron_expression_a_folder_or_project_and_a_project_on_its_node() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let other = server.online_node("desktop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let (status, body) = add_schedule(&server, link.id, json!({"prompt": " ", "cwd": "/work/app", "cron": EVERY_MONDAY, "each_run": "new"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("prompt_required")));

    for cron in ["every monday", "0 6 * *", "0 0 6 * * 1", "0 6 31 2 *"] {
        let (status, body) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": cron, "each_run": "new"})).await;
        assert_eq!(status, 400, "{cron}: {body}");
        assert!(body["error"] == json!("cron_invalid") || body["error"] == json!("cron_never_runs"), "{cron}: {body}");
    }

    let (status, body) = add_schedule(&server, link.id, json!({"prompt": "Go", "cron": EVERY_MONDAY, "each_run": "new"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("cwd_required")));

    let (status, body) = add_schedule(&server, other.id, json!({"prompt": "Go", "project_id": project, "cron": EVERY_MONDAY, "each_run": "new"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("project_not_on_node")));

    let (status, created) = add_schedule(&server, link.id, json!({"prompt": "Go", "project_id": project, "cron": "*/15 * * * *", "each_run": "same"})).await;
    assert_eq!((status, created["project_id"].clone(), created["each_run"].clone()), (201, json!(project), json!("same")));
}

#[tokio::test]
async fn a_due_schedule_starts_a_conversation_and_moves_on_to_its_next_run() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;

    make_due(&server, id).await;

    match link.next().await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, "/work/app"),
        other => panic!("expected start_session, got {other:?}"),
    }

    let run = first_run(&server).await;
    let listed = schedule(&server, id).await;
    let conversation = server.conversation(run["conversation_id"].as_str().unwrap().parse().unwrap()).await;
    let next: chrono::DateTime<chrono::Utc> = listed["next_run_at"].as_str().unwrap().parse().unwrap();

    assert_eq!((run["outcome"].clone(), run["reason"].clone()), (json!("started"), Value::Null));
    assert_eq!((conversation["status"].clone(), conversation["title"].clone()), (json!("working"), json!("Update and test dependencies")));
    assert_eq!(listed["last_conversation_id"], run["conversation_id"]);
    assert!(listed["last_run_at"].is_string() && next > chrono::Utc::now());
}

#[tokio::test]
async fn a_schedule_for_an_offline_node_skips_the_run_and_records_why() {
    let server = ticking_server().await;
    let link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;
    drop(link);

    eventually("the node going offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;
    make_due(&server, id).await;

    let run = first_run(&server).await;
    let listed = schedule(&server, id).await;
    let next: chrono::DateTime<chrono::Utc> = listed["next_run_at"].as_str().unwrap().parse().unwrap();

    assert_eq!((run["outcome"].clone(), run["conversation_id"].clone()), (json!("skipped"), Value::Null));
    assert_eq!(run["reason"], json!("This node is offline. Start it, then try again."));
    assert!(next > chrono::Utc::now());
    assert_eq!(server.user.json(server.user.get("/api/v1/conversations")).await.1, json!([]));
}

#[tokio::test]
async fn a_paused_schedule_does_not_run_until_it_is_resumed() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;

    let (status, paused) = server.user.json(server.user.patch(&format!("/api/v1/schedules/{id}")).json(&json!({"paused": true}))).await;
    assert_eq!((status, paused["paused"].clone()), (200, json!(true)));
    make_due(&server, id).await;

    let (_, resumed) = server.user.json(server.user.patch(&format!("/api/v1/schedules/{id}")).json(&json!({"paused": false}))).await;
    let next: chrono::DateTime<chrono::Utc> = resumed["next_run_at"].as_str().unwrap().parse().unwrap();
    assert!(next > chrono::Utc::now(), "resuming skips the runs missed while paused");

    make_due(&server, id).await;
    link.expect_start_session().await;
    assert_eq!(first_run(&server).await["outcome"], json!("started"));
}

#[tokio::test]
async fn a_paused_schedule_is_left_alone_by_the_scheduler() {
    let server = ticking_server().await;
    let link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;

    server.user.json(server.user.patch(&format!("/api/v1/schedules/{id}")).json(&json!({"paused": true}))).await;
    make_due(&server, id).await;
    let other = weekly(&server, link.id, "new").await;
    make_due(&server, other).await;

    eventually("the other schedule's run", || async { runs(&server).await.first().cloned() }).await;

    let recorded = runs(&server).await;
    assert!(recorded.iter().all(|run| run["schedule_id"] == json!(other)));
}

#[tokio::test]
async fn running_a_schedule_now_starts_a_conversation_without_moving_its_next_run() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;
    let before = schedule(&server, id).await["next_run_at"].clone();

    let (status, run) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;
    assert_eq!((status, run["outcome"].clone()), (201, json!("started")));
    link.expect_start_session().await;

    assert_eq!(schedule(&server, id).await["next_run_at"], before);
    assert_eq!(runs(&server).await.len(), 1);
}

#[tokio::test]
async fn a_schedule_that_reuses_its_conversation_sends_the_prompt_to_it_again() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "same").await;

    let (_, first) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;
    link.expect_start_session().await;
    let (_, second) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;

    assert_eq!(first["conversation_id"], second["conversation_id"]);
    assert_eq!(server.user.json(server.user.get("/api/v1/conversations")).await.1.as_array().unwrap().len(), 1);

    let conversation_id = first["conversation_id"].as_str().unwrap();
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation_id}/queue"))).await;
    assert_eq!(queue.as_array().unwrap().len(), 2, "both runs queued their prompt on the one conversation: {queue}");
}

#[tokio::test]
async fn a_schedule_that_reuses_its_conversation_starts_a_new_one_when_that_conversation_failed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "same").await;

    let (_, first) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;
    link.expect_start_session().await;
    sqlx::query("update conversations set status = 'failed' where id = $1").bind(first["conversation_id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(&server.pool).await.unwrap();

    let (_, second) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;

    assert_eq!(second["outcome"], json!("started"));
    assert_ne!(first["conversation_id"], second["conversation_id"]);
}

#[tokio::test]
async fn two_control_planes_on_one_database_run_a_due_schedule_once() {
    let first = ticking_server().await;
    let link = first.online_node("laptop").await;
    let second = Server::start_on(first.database_url.clone(), |config| config.scheduler_tick = Duration::from_millis(50)).await;
    let id = weekly(&first, link.id, "new").await;

    make_due(&first, id).await;
    eventually("a recorded run", || async { (runs(&first).await.len() == 1).then_some(()) }).await;

    let (_, from_second) = second.user.json(second.user.get("/api/v1/schedule-runs")).await;
    assert_eq!(from_second.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_schedule_can_be_deleted_and_only_by_its_owner_and_its_runs_go_with_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = weekly(&server, link.id, "new").await;
    server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;
    link.expect_start_session().await;
    let other = server.second_user("other@example.com").await;
    let path = format!("/api/v1/schedules/{id}");

    for response in [other.delete(&path), other.post(&format!("{path}/run")), other.patch(&path).json(&json!({"paused": true}))] {
        assert_eq!(other.json(response).await.0, 404);
    }

    assert_eq!(other.json(other.get("/api/v1/schedules")).await.1, json!([]));
    assert_eq!(other.json(other.get("/api/v1/schedule-runs")).await.1, json!([]));

    let (status, _) = server.user.json(server.user.delete(&path)).await;
    assert_eq!(status, 204);
    assert_eq!((runs(&server).await.len(), server.user.json(server.user.get("/api/v1/schedules")).await.1), (0, json!([])));
}
