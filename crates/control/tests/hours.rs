mod common;

use common::Server;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn make_group(server: &Server, name: &str) -> Uuid {
    let (status, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": name}))).await;

    assert_eq!(status, 201, "{group}");

    group["id"].as_str().unwrap().parse().unwrap()
}

async fn group(server: &Server, id: Uuid) -> Value {
    let (_, groups) = server.user.json(server.user.get("/api/v1/groups")).await;

    groups.as_array().unwrap().iter().find(|group| group["id"] == json!(id)).cloned().unwrap()
}

async fn close_group_all_week(server: &Server, id: Uuid) -> (u16, Value) {
    let days: Vec<Value> = (0..7).map(|weekday| json!({"weekday": weekday, "closed": true, "opens": "09:00", "closes": "18:00"})).collect();

    server.user.json(server.user.put(&format!("/api/v1/groups/{id}/hours")).json(&json!({"always_open": false, "days": days}))).await
}

async fn overview(server: &Server) -> Value {
    server.user.json(server.user.get("/api/v1/wellness/overview?days=7")).await.1
}

#[tokio::test]
async fn a_new_group_is_always_open() {
    let server = Server::start().await;
    let id = make_group(&server, "personal").await;
    let personal = group(&server, id).await;

    assert_eq!((personal["hours"]["always_open"].clone(), personal["open_now"].clone()), (json!(true), json!(true)));
}

#[tokio::test]
async fn setting_a_group_closed_all_week_closes_it_and_broadcasts() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let id = make_group(&server, "work").await;

    let (status, saved) = close_group_all_week(&server, id).await;

    assert_eq!(status, 200, "{saved}");
    assert_eq!((saved["open_now"].clone(), saved["opens_at"].is_null()), (json!(false), true));
    stream.until(|message| matches!(message, StreamMessage::WellnessChanged).then_some(())).await;
}

#[tokio::test]
async fn group_hours_with_a_bad_time_are_refused_with_how_to_fix_it() {
    let server = Server::start().await;
    let id = make_group(&server, "work").await;
    let days = json!([{"weekday": 0, "closed": false, "opens": "nine", "closes": "18:00"}]);

    let (status, body) = server.user.json(server.user.put(&format!("/api/v1/groups/{id}/hours")).json(&json!({"always_open": false, "days": days}))).await;

    assert_eq!((status, body["error"].clone(), body["message"].clone()), (400, json!("hours_time_invalid"), json!("Enter a time like 09:00.")));
}

#[tokio::test]
async fn opening_a_closed_group_needs_a_reason_and_lasts_fifteen_minutes() {
    let server = Server::start().await;
    let id = make_group(&server, "work").await;

    close_group_all_week(&server, id).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "  "}))).await;
    assert_eq!((status, body["error"].clone(), body["message"].clone()), (400, json!("reason_required"), json!("Enter why you need to open work")));

    let (status, opened) = server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "Deploy is failing"}))).await;
    assert_eq!(status, 201, "{opened}");

    let now = group(&server, id).await;
    assert_eq!(now["open_now"], json!(true));
    assert_eq!(now["opened_until"], opened["opened_until"]);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "Again"}))).await;
    assert_eq!(status, 409);

    let listed = overview(&server).await["opened_out_of_hours"].clone();
    assert_eq!((listed[0]["reason"].clone(), listed[0]["group_name"].clone()), (json!("Deploy is failing"), json!("work")));
}

#[tokio::test]
async fn closing_an_opening_early_closes_the_group_again() {
    let server = Server::start().await;
    let id = make_group(&server, "work").await;

    close_group_all_week(&server, id).await;
    server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "Quick check"}))).await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/groups/{id}/open"))).await;

    assert_eq!(status, 204);
    assert_eq!(group(&server, id).await["open_now"], json!(false));
}

#[tokio::test]
async fn an_open_group_cannot_be_opened_out_of_hours() {
    let server = Server::start().await;
    let id = make_group(&server, "personal").await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "Why not"}))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("group_open")));
}

#[tokio::test]
async fn a_ping_for_a_closed_group_is_held_and_released_when_the_group_is_opened() {
    let server = Server::start().await;
    let id = make_group(&server, "work").await;
    let ping = json!({"kind": "ping", "subject": "conversation-1", "group_id": id});

    close_group_all_week(&server, id).await;
    server.user.json(server.user.post("/api/v1/wellness/holds").json(&ping)).await;
    assert_eq!(overview(&server).await["held_week"]["pings"], json!(1));

    server.user.json(server.user.post(&format!("/api/v1/groups/{id}/open")).json(&json!({"reason": "Need it"}))).await;

    let open: i64 = sqlx::query_scalar("select count(*) from quiet_holds where released_at is null").fetch_one(&server.pool).await.unwrap();
    assert_eq!(open, 0);
}

#[tokio::test]
async fn a_ping_for_an_open_group_is_not_held() {
    let server = Server::start().await;
    let id = make_group(&server, "personal").await;

    server.user.json(server.user.post("/api/v1/wellness/holds").json(&json!({"kind": "ping", "subject": "conversation-1", "group_id": id}))).await;

    assert_eq!(overview(&server).await["held_week"]["pings"], json!(0));
}

#[tokio::test]
async fn closing_mycelium_closes_every_group_even_those_always_open() {
    let server = Server::start().await;
    let id = make_group(&server, "personal").await;
    let (_, mut settings) = server.user.json(server.user.get("/api/v1/wellness/settings")).await;

    settings["hours"]["always_open"] = json!(false);

    for day in settings["hours"]["days"].as_array_mut().unwrap() {
        day["closed"] = json!(true);
    }

    server.user.json(server.user.put("/api/v1/wellness/settings").json(&settings)).await;

    assert_eq!(group(&server, id).await["open_now"], json!(false));
}

#[tokio::test]
async fn a_group_may_close_at_24_00_and_keeps_it_when_read_back() {
    let server = Server::start().await;
    let id = make_group(&server, "work").await;
    let days: Vec<Value> = (0..7).map(|weekday| json!({"weekday": weekday, "closed": false, "opens": "00:00", "closes": "24:00"})).collect();

    let (status, saved) = server.user.json(server.user.put(&format!("/api/v1/groups/{id}/hours")).json(&json!({"always_open": false, "days": days}))).await;

    assert_eq!(status, 200, "{saved}");
    assert_eq!((saved["hours"]["days"][3]["closes"].clone(), saved["open_now"].clone()), (json!("24:00"), json!(true)));
}
