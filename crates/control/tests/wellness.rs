mod common;

use common::{Server, eventually};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

async fn settings(server: &Server) -> Value {
    server.user.json(server.user.get("/api/v1/wellness/settings")).await.1
}

async fn overview(server: &Server) -> Value {
    server.user.json(server.user.get("/api/v1/wellness/overview?days=7")).await.1
}

async fn save(server: &Server, change: impl FnOnce(&mut Value)) -> (u16, Value) {
    let mut body = settings(server).await;

    change(&mut body);

    server.user.json(server.user.put("/api/v1/wellness/settings").json(&body)).await
}

fn always_quiet(body: &mut Value) {
    body["quiet_hours"] = json!(true);

    for day in body["quiet_days"].as_array_mut().unwrap() {
        day["enabled"] = json!(true);
        day["all_day"] = json!(true);
    }
}

#[tokio::test]
async fn settings_round_trip_and_broadcast() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;

    let defaults = settings(&server).await;
    assert_eq!((defaults["break_after_minutes"].clone(), defaults["quiet_hours"].clone(), defaults["pomodoro"].clone()), (json!(50), json!(false), json!(false)));
    assert_eq!(defaults["quiet_days"][0]["starts"], "22:00");
    assert_eq!(defaults["quiet_days"][6]["all_day"], json!(true));

    let (status, saved) = save(&server, |body| {
        body["break_after_minutes"] = json!(45);
        body["pomodoro"] = json!(true);
        body["time_zone"] = json!("Europe/London");
        body["quiet_days"][2]["starts"] = json!("21:30");
    })
    .await;

    assert_eq!(status, 200, "{saved}");
    stream.until(|message| matches!(message, StreamMessage::WellnessChanged).then_some(())).await;

    let reloaded = settings(&server).await;
    assert_eq!((reloaded["break_after_minutes"].clone(), reloaded["pomodoro"].clone(), reloaded["time_zone"].clone()), (json!(45), json!(true), json!("Europe/London")));
    assert_eq!(reloaded["quiet_days"][2]["starts"], "21:30");
}

#[tokio::test]
async fn settings_out_of_range_are_refused_with_a_reason() {
    let server = Server::start().await;

    let (status, body) = save(&server, |body| body["break_after_minutes"] = json!(1)).await;
    assert_eq!((status, body["error"].clone()), (400, json!("break_after_invalid")));

    let (status, body) = save(&server, |body| body["quiet_days"][0]["starts"] = json!("late")).await;
    assert_eq!((status, body["error"].clone()), (400, json!("quiet_time_invalid")));

    let (status, body) = save(&server, |body| body["time_zone"] = json!("Mars/Base")).await;
    assert_eq!((status, body["error"].clone()), (400, json!("time_zone_invalid")));
}

#[tokio::test]
async fn heartbeat_accumulates_per_local_day_and_is_capped() {
    let server = Server::start().await;
    let zone: chrono_tz::Tz = "Pacific/Kiritimati".parse().unwrap();

    save(&server, |body| body["time_zone"] = json!("Pacific/Kiritimati")).await;

    for seconds in [60, 60, 3600] {
        let (status, _) = server.user.json(server.user.post("/api/v1/wellness/heartbeat").json(&json!({"seconds": seconds}))).await;
        assert_eq!(status, 204);
    }

    let today = chrono::Utc::now().with_timezone(&zone).date_naive().to_string();
    let days = overview(&server).await["days"].clone();
    let last = days.as_array().unwrap().last().unwrap().clone();

    assert_eq!((last["day"].clone(), last["viewing_minutes"].clone()), (json!(today), json!(3)));
}

#[tokio::test]
async fn overview_reports_seven_days_and_breaks() {
    let server = Server::start().await;

    let (status, started) = server.user.json(server.user.post("/api/v1/wellness/breaks").json(&json!({"minutes": 5}))).await;
    assert_eq!(status, 201, "{started}");

    let during = overview(&server).await;
    assert_eq!(during["days"].as_array().unwrap().len(), 7);
    assert_eq!(during["current_break"]["id"], started["id"]);
    assert_eq!(during["days"][6]["breaks"], json!(1));

    let (status, _) = server.user.json(server.user.delete("/api/v1/wellness/breaks/current")).await;
    assert_eq!(status, 204);

    let after = overview(&server).await;
    assert_eq!(after["current_break"], Value::Null);
    assert!(after["last_break_at"].is_string());
}

#[tokio::test]
async fn puzzle_played_today_toggles() {
    let server = Server::start().await;

    let (status, _) = server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(status, 204);
    server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(overview(&server).await["days"][6]["puzzles"], json!(["wordle"]));

    let (status, _) = server.user.json(server.user.delete("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(status, 204);
    assert_eq!(overview(&server).await["days"][6]["puzzles"], json!([]));

    let (status, body) = server.user.json(server.user.put("/api/v1/wellness/puzzles/sudoku/today")).await;
    assert_eq!((status, body["error"].clone()), (404, json!("puzzle_not_found")));
}

#[tokio::test]
async fn schedule_due_in_quiet_hours_is_held_until_they_end() {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let (status, schedule) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"node_id": link.id, "agent": "claude", "prompt": "Tidy", "cwd": "/work/app", "cron": "0 6 * * 1", "each_run": "new"}))).await;
    assert_eq!(status, 201, "{schedule}");
    let id: Uuid = schedule["id"].as_str().unwrap().parse().unwrap();

    save(&server, always_quiet).await;
    sqlx::query("update schedules set next_run_at = now() - interval '1 minute' where id = $1").bind(id).execute(&server.pool).await.unwrap();

    let held = eventually("a held run", || async {
        let (_, runs) = server.user.json(server.user.get("/api/v1/schedule-runs")).await;

        runs.as_array().unwrap().first().cloned()
    })
    .await;
    assert_eq!(held["outcome"], "held");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, runs) = server.user.json(server.user.get("/api/v1/schedule-runs")).await;
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(overview(&server).await["held"], json!(1));

    save(&server, |body| body["quiet_hours"] = json!(false)).await;

    eventually("the run once quiet hours end", || async {
        let (_, runs) = server.user.json(server.user.get("/api/v1/schedule-runs")).await;

        runs.as_array().unwrap().iter().find(|run| run["outcome"] != "held").cloned()
    })
    .await;
}
