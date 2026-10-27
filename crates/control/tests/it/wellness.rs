use crate::common::{Server, eventually};
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

fn mycelium_closed(body: &mut Value) {
    body["hours"]["always_open"] = json!(false);

    for day in body["hours"]["days"].as_array_mut().unwrap() {
        day["closed"] = json!(true);
    }
}

#[tokio::test]
async fn settings_round_trip_and_broadcast() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;

    let defaults = settings(&server).await;
    assert_eq!((defaults["break_after_minutes"].clone(), defaults["hours"]["always_open"].clone()), (json!(50), json!(true)));
    assert_eq!(defaults["hours"]["days"][0]["opens"], "09:00");
    assert_eq!(defaults["hours"]["days"][6]["closed"], json!(true));

    let (status, saved) = save(&server, |body| {
        body["break_after_minutes"] = json!(45);
        body["break_minutes"] = json!(12);
        body["time_zone"] = json!("Europe/London");
        body["hours"]["days"][2]["opens"] = json!("08:30");
    })
    .await;

    assert_eq!(status, 200, "{saved}");
    stream.until(|message| matches!(message, StreamMessage::WellnessChanged).then_some(())).await;

    let reloaded = settings(&server).await;
    assert_eq!((reloaded["break_after_minutes"].clone(), reloaded["break_minutes"].clone(), reloaded["time_zone"].clone()), (json!(45), json!(12), json!("Europe/London")));
    assert_eq!(reloaded["hours"]["days"][2]["opens"], "08:30");
}

#[tokio::test]
async fn settings_out_of_range_are_refused_with_a_reason() {
    let server = Server::start().await;

    let (status, body) = save(&server, |body| body["break_after_minutes"] = json!(1)).await;
    assert_eq!((status, body["error"].clone()), (400, json!("break_after_invalid")));

    let (status, body) = save(&server, |body| body["hours"]["days"][0]["opens"] = json!("late")).await;
    assert_eq!((status, body["error"].clone(), body["message"].clone()), (400, json!("hours_time_invalid"), json!("Enter a time like 09:00.")));

    let (status, body) = save(&server, |body| body["hours"]["days"][0]["weekday"] = json!(7)).await;
    assert_eq!((status, body["error"].clone()), (400, json!("weekday_invalid")));

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

async fn puzzles(server: &Server) -> Value {
    server.user.json(server.user.get("/api/v1/wellness/puzzles?days=30")).await.1
}

fn puzzle<'a>(all: &'a Value, name: &str) -> &'a Value {
    all.as_array().unwrap().iter().find(|puzzle| puzzle["puzzle"] == name).unwrap()
}

#[tokio::test]
async fn puzzle_played_today_toggles() {
    let server = Server::start().await;

    let (status, _) = server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(status, 204);
    server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(puzzle(&puzzles(&server).await, "wordle")["stats"]["played"], json!(1));
    assert_eq!(puzzle(&puzzles(&server).await, "wordle")["today"]["source"], json!("manual"));

    let (status, _) = server.user.json(server.user.delete("/api/v1/wellness/puzzles/wordle/today")).await;
    assert_eq!(status, 204);
    assert_eq!(puzzle(&puzzles(&server).await, "wordle")["today"], Value::Null);

    let (status, body) = server.user.json(server.user.put("/api/v1/wellness/puzzles/sudoku/today")).await;
    assert_eq!((status, body["error"].clone()), (404, json!("puzzle_not_found")));
}

#[tokio::test]
async fn a_result_read_from_the_game_is_recorded_and_summarised() {
    let server = Server::start().await;
    let reading = json!({"solved": true, "score": 4, "source": "read"});

    let (status, _) = server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today").json(&reading)).await;
    assert_eq!(status, 204);

    let wordle = puzzles(&server).await;
    let wordle = puzzle(&wordle, "wordle");

    assert_eq!((wordle["today"]["solved"].clone(), wordle["today"]["score"].clone(), wordle["today"]["source"].clone()), (json!(true), json!(4), json!("read")));
    assert_eq!((wordle["stats"]["played"].clone(), wordle["stats"]["solved"].clone(), wordle["stats"]["average"].clone()), (json!(1), json!(1), json!(4.0)));
    assert_eq!(wordle["stats"]["distribution"], json!([{"value": 4, "count": 1}]));

    let (status, body) = server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today").json(&json!({"source": "guess"}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("puzzle_source_invalid")));
}

#[tokio::test]
async fn a_failed_wordle_is_played_but_stays_out_of_the_average() {
    let server = Server::start().await;

    server.user.json(server.user.put("/api/v1/wellness/puzzles/wordle/today").json(&json!({"solved": false, "source": "read"}))).await;

    let all = puzzles(&server).await;
    let wordle = puzzle(&all, "wordle");

    assert_eq!((wordle["stats"]["played"].clone(), wordle["stats"]["average"].clone(), wordle["stats"]["distribution"].clone()), (json!(1), Value::Null, json!([])));
    assert_eq!(wordle["day"].as_str().map(str::len), Some(10));
}

#[tokio::test]
async fn an_assistant_ping_while_mycelium_is_closed_is_held_and_released_when_it_opens() {
    let server = Server::start().await;
    let ping = json!({"kind": "ping", "subject": "conversation-1"});

    save(&server, |body| mycelium_closed(body)).await;
    server.user.json(server.user.post("/api/v1/wellness/holds").json(&ping)).await;
    assert_eq!(overview(&server).await["held_week"]["pings"], json!(1));

    save(&server, |body| body["hours"]["always_open"] = json!(true)).await;
    server.user.json(server.user.post("/api/v1/wellness/holds").json(&ping)).await;
    overview(&server).await;

    let open: i64 = sqlx::query_scalar("select count(*) from quiet_holds where released_at is null").fetch_one(&server.pool).await.unwrap();
    assert_eq!(open, 0);
    assert_eq!(overview(&server).await["held_week"]["pings"], json!(1));
}

#[tokio::test]
async fn pings_are_not_held_when_the_setting_is_off() {
    let server = Server::start().await;

    save(&server, |body| {
        mycelium_closed(body);
        body["quiet_hold_pings"] = json!(false);
    })
    .await;
    server.user.json(server.user.post("/api/v1/wellness/holds").json(&json!({"kind": "ping", "subject": "conversation-1"}))).await;

    assert_eq!(overview(&server).await["held_week"]["pings"], json!(0));
}

#[tokio::test]
async fn schedule_due_while_mycelium_is_closed_is_held_until_it_opens() {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let (status, schedule) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"node_id": link.id, "agent": "claude", "prompt": "Tidy", "cwd": "/work/app", "cron": "0 6 * * 1", "each_run": "new"}))).await;
    assert_eq!(status, 201, "{schedule}");
    let id: Uuid = schedule["id"].as_str().unwrap().parse().unwrap();

    save(&server, mycelium_closed).await;
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

    save(&server, |body| body["hours"]["always_open"] = json!(true)).await;

    eventually("the run once Mycelium opens", || async {
        let (_, runs) = server.user.json(server.user.get("/api/v1/schedule-runs")).await;

        runs.as_array().unwrap().iter().find(|run| run["outcome"] != "held").cloned()
    })
    .await;
}
