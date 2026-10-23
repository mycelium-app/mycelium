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

async fn propose(server: &Server, link: &common::NodeLink, conversation: Uuid, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "propose_schedule", "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn working_conversation(server: &Server, link: &mut common::NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    conversation.id
}

async fn proposed(server: &Server) -> Vec<Value> {
    let (_, schedules) = server.user.json(server.user.get("/api/v1/schedules")).await;

    schedules.as_array().unwrap().iter().filter(|schedule| schedule["proposed"] == true).cloned().collect()
}

#[tokio::test]
async fn an_agent_proposes_a_schedule_in_the_conversation_and_it_waits_unconfirmed_without_ever_running() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    server.user.json(server.user.put("/api/v1/me/time-zone").json(&json!({"time_zone": "Europe/London"}))).await;

    let result = propose(&server, &link, conversation, json!({"prompt": "Run ./deploy.sh", "preset": "weekdays at 20:00"})).await;
    assert_eq!(result["isError"], false, "{result}");

    let waiting = proposed(&server).await;
    assert_eq!(waiting.len(), 1);
    let id = waiting[0]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        (waiting[0]["cron"].clone(), waiting[0]["time_zone"].clone(), waiting[0]["each_run"].clone(), waiting[0]["cwd"].clone(), waiting[0]["conversation_id"].clone(), waiting[0]["last_conversation_id"].clone()),
        (json!("0 20 * * 1-5"), json!("Europe/London"), json!("same"), json!("/work/app"), json!(conversation), json!(conversation)),
    );

    make_due(&server, id).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(runs(&server).await.len(), 0);

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/run"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("schedule_proposed")));
    let (status, body) = server.user.json(server.user.patch(&format!("/api/v1/schedules/{id}")).json(&json!({"paused": true}))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("schedule_proposed")));
}

#[tokio::test]
async fn a_proposal_with_a_cron_expression_a_zone_and_a_new_conversation_each_run_keeps_what_was_asked() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    propose(&server, &link, conversation, json!({"prompt": "Check the backups", "cron": "30 7 * * 1", "each_run": "new", "time_zone": "America/New_York"})).await;

    let waiting = proposed(&server).await;
    assert_eq!((waiting[0]["cron"].clone(), waiting[0]["time_zone"].clone(), waiting[0]["each_run"].clone(), waiting[0]["last_conversation_id"].clone()), (json!("30 7 * * 1"), json!("America/New_York"), json!("new"), Value::Null));
}

#[tokio::test]
async fn a_proposal_that_cannot_work_is_refused_with_how_to_fix_it_and_stores_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    for (arguments, fix) in [
        (json!({"prompt": "Go", "preset": "whenever"}), "Use a preset"),
        (json!({"prompt": "Go"}), "Give a preset or a cron expression"),
        (json!({"prompt": "Go", "preset": "daily at 09:00", "time_zone": "Mars/Olympus"}), "is not a time zone"),
        (json!({"prompt": "Go", "cron": "0 6 31 2 *"}), "never runs"),
        (json!({"prompt": " ", "preset": "daily at 09:00"}), "Give the prompt"),
        (json!({"prompt": "Go", "preset": "daily at 09:00", "each_run": "sometimes"}), "each_run is same or new"),
    ] {
        let result = propose(&server, &link, conversation, arguments.clone()).await;
        assert_eq!(result["isError"], true, "{arguments}");
        assert!(result["content"][0]["text"].as_str().unwrap().contains(fix), "{arguments}: {result}");
    }

    let outside = server.anonymous();
    let request = outside.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "propose_schedule", "arguments": {"prompt": "Go", "preset": "daily at 09:00"}}}));
    let (_, body) = outside.json(request).await;
    assert_eq!(body["result"]["isError"], true);
    assert_eq!(proposed(&server).await.len(), 0);
}

#[tokio::test]
async fn confirming_a_proposal_applies_the_edits_makes_it_run_and_reuses_the_conversation() {
    let server = ticking_server().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    propose(&server, &link, conversation, json!({"prompt": "Run ./deploy.sh", "preset": "weekdays at 20:00", "time_zone": "Europe/London"})).await;
    let id: Uuid = proposed(&server).await[0]["id"].as_str().unwrap().parse().unwrap();

    let (status, confirmed) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/confirm")).json(&json!({"cron": "0 21 * * 1-5", "time_zone": "Europe/Paris"}))).await;
    assert_eq!(status, 200, "{confirmed}");
    assert_eq!((confirmed["proposed"].clone(), confirmed["cron"].clone(), confirmed["time_zone"].clone(), confirmed["each_run"].clone()), (json!(false), json!("0 21 * * 1-5"), json!("Europe/Paris"), json!("same")));

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/confirm"))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("schedule_not_proposed")));

    make_due(&server, id).await;
    let run = first_run(&server).await;
    assert_eq!((run["outcome"].clone(), run["conversation_id"].clone()), (json!("started"), json!(conversation)));
}

#[tokio::test]
async fn confirming_without_edits_keeps_the_proposal_as_it_was() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    propose(&server, &link, conversation, json!({"prompt": "Go", "preset": "daily at 09:00", "each_run": "new", "time_zone": "Asia/Tokyo"})).await;
    let id: Uuid = proposed(&server).await[0]["id"].as_str().unwrap().parse().unwrap();

    let (status, confirmed) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/confirm"))).await;

    assert_eq!(status, 200, "{confirmed}");
    assert_eq!((confirmed["cron"].clone(), confirmed["time_zone"].clone(), confirmed["each_run"].clone()), (json!("0 9 * * *"), json!("Asia/Tokyo"), json!("new")));
}

#[tokio::test]
async fn declining_a_proposal_removes_it_and_tells_the_agent_not_to_set_it_up_another_way() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    propose(&server, &link, conversation, json!({"prompt": "Run ./deploy.sh", "preset": "daily at 20:00"})).await;
    let id: Uuid = proposed(&server).await[0]["id"].as_str().unwrap().parse().unwrap();

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/decline"))).await;
    assert_eq!(status, 204);

    assert_eq!(server.user.json(server.user.get("/api/v1/schedules")).await.1, json!([]));
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;
    let text = queue[0]["text"].as_str().unwrap();
    assert!(text.contains("declined") && text.contains("Run ./deploy.sh") && text.contains("another way"), "{text}");

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/decline"))).await;
    assert_eq!((status, body["error"].clone()), (404, json!("not_found")));
}

#[tokio::test]
async fn a_confirmed_schedule_cannot_be_declined_and_only_its_owner_can_confirm_or_decline_a_proposal() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    propose(&server, &link, conversation, json!({"prompt": "Go", "preset": "daily at 09:00"})).await;
    let id: Uuid = proposed(&server).await[0]["id"].as_str().unwrap().parse().unwrap();
    let other = server.second_user("other@example.com").await;

    for action in ["confirm", "decline"] {
        assert_eq!(other.json(other.post(&format!("/api/v1/schedules/{id}/{action}"))).await.0, 404);
    }

    server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/confirm"))).await;
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/schedules/{id}/decline"))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("schedule_not_proposed")));
}

#[tokio::test]
async fn a_schedule_runs_in_the_time_zone_it_was_created_with_and_the_first_zone_becomes_the_users() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, created) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": "0 20 * * *", "each_run": "new", "time_zone": "Asia/Tokyo"})).await;
    assert_eq!(status, 201, "{created}");
    let next: chrono::DateTime<chrono::Utc> = created["next_run_at"].as_str().unwrap().parse().unwrap();
    assert_eq!(chrono::Timelike::hour(&next), 11);

    let (_, without_zone) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": "0 20 * * *", "each_run": "new"})).await;
    assert_eq!(without_zone["time_zone"], json!("Asia/Tokyo"));

    let (status, body) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": "0 20 * * *", "each_run": "new", "time_zone": "Mars/Olympus"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("time_zone_invalid")));
}

#[tokio::test]
async fn setting_the_time_zone_needs_a_real_one_and_defaults_to_utc_before_it_is_set() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (_, utc) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": EVERY_MONDAY, "each_run": "new"})).await;
    assert_eq!(utc["time_zone"], json!("UTC"));

    let (status, body) = server.user.json(server.user.put("/api/v1/me/time-zone").json(&json!({"time_zone": "Nowhere"}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("time_zone_invalid")));
    assert_eq!(server.user.json(server.user.put("/api/v1/me/time-zone").json(&json!({"time_zone": "Europe/London"}))).await.0, 204);

    let (_, london) = add_schedule(&server, link.id, json!({"prompt": "Go", "cwd": "/work/app", "cron": EVERY_MONDAY, "each_run": "new"})).await;
    assert_eq!(london["time_zone"], json!("Europe/London"));
}
