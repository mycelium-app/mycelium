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

#[tokio::test]
async fn an_ended_break_awaits_a_note_until_one_is_given() {
    let server = Server::start().await;

    let (_, started) = server.user.json(server.user.post("/api/v1/wellness/breaks").json(&json!({"minutes": 5}))).await;
    assert_eq!(overview(&server).await["awaiting_note"], Value::Null);

    server.user.json(server.user.delete("/api/v1/wellness/breaks/current")).await;

    let waiting = overview(&server).await;
    assert_eq!((waiting["awaiting_note"]["id"].clone(), waiting["awaiting_note"]["ended_early"].clone(), waiting["awaiting_note"]["planned_minutes"].clone()), (started["id"].clone(), json!(true), json!(5)));
    assert!(waiting["awaiting_note"]["ended_at"].is_string());

    let path = format!("/api/v1/wellness/breaks/{}/note", started["id"].as_str().unwrap());
    let (status, refused) = server.user.json(server.user.put(&path).json(&json!({"note": "  \n"}))).await;
    assert_eq!((status, refused["error"].clone(), refused["message"].clone()), (400, json!("break_note_missing"), json!("Enter what you did in your break")));
    assert_ne!(overview(&server).await["awaiting_note"], Value::Null);

    let (status, saved) = server.user.json(server.user.put(&path).json(&json!({"note": "  Made a tea  "}))).await;
    assert_eq!((status, saved["note"].clone()), (200, json!("Made a tea")));

    let after = overview(&server).await;
    assert_eq!(after["awaiting_note"], Value::Null);
    assert_eq!(after["recent_breaks"][0]["note"], "Made a tea");
}

#[tokio::test]
async fn a_break_that_ran_over_lasts_until_the_person_came_back() {
    let server = Server::start().await;
    let (_, started) = server.user.json(server.user.post("/api/v1/wellness/breaks").json(&json!({"minutes": 5}))).await;
    let id: uuid::Uuid = started["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("update breaks set started_at = now() - interval '40 minutes', ends_at = now() - interval '35 minutes' where id = $1").bind(id).execute(&server.pool).await.unwrap();

    let (_, saved) = server.user.json(server.user.put(&format!("/api/v1/wellness/breaks/{id}/note")).json(&json!({"note": "Sat in a meeting"}))).await;
    let started_at = chrono::DateTime::parse_from_rfc3339(saved["started_at"].as_str().unwrap()).unwrap();
    let ended_at = chrono::DateTime::parse_from_rfc3339(saved["ended_at"].as_str().unwrap()).unwrap();

    assert!((ended_at - started_at).num_minutes() >= 39, "{saved}");
}

#[tokio::test]
async fn a_break_still_running_cannot_be_noted() {
    let server = Server::start().await;

    let (_, started) = server.user.json(server.user.post("/api/v1/wellness/breaks").json(&json!({"minutes": 5}))).await;
    let (status, _) = server.user.json(server.user.put(&format!("/api/v1/wellness/breaks/{}/note", started["id"].as_str().unwrap())).json(&json!({"note": "Too soon"}))).await;

    assert_eq!(status, 404);
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

async fn beat(server: &Server, at: chrono::DateTime<chrono::Utc>, extra: Value) -> u16 {
    let mut body = json!({"seconds": 60, "at": at.to_rfc3339()});

    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }

    server.user.json(server.user.post("/api/v1/wellness/heartbeat").json(&body)).await.0
}

fn earlier_this_hour() -> chrono::DateTime<chrono::Utc> {
    use chrono::{DurationRound, Duration};

    (chrono::Utc::now() - Duration::hours(3)).duration_trunc(Duration::hours(1)).unwrap()
}

async fn activity_day(server: &Server, date: &str) -> Value {
    server.user.json(server.user.get(&format!("/api/v1/wellness/activity/day?date={date}"))).await.1
}

#[tokio::test]
async fn heartbeats_on_the_same_target_extend_one_span_and_a_change_or_a_gap_opens_another() {
    use chrono::Duration;

    let server = Server::start().await;
    let base = earlier_this_hour();
    let date = base.date_naive().to_string();

    for (offset, route) in [(60, "history"), (120, "history"), (180, "wellness"), (440, "wellness")] {
        assert_eq!(beat(&server, base + Duration::seconds(offset), json!({"kind": "elsewhere", "route": route, "title": route})).await, 204);
    }

    let day = activity_day(&server, &date).await;
    let spans: Vec<(String, i64)> = day["spans"].as_array().unwrap().iter().map(|span| (span["title"].as_str().unwrap().to_string(), (span["end"].as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap() - span["start"].as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap()).num_seconds())).collect();

    assert_eq!(spans, vec![("history".to_string(), 120), ("wellness".to_string(), 60), ("wellness".to_string(), 60)]);
    assert_eq!(day["on_screen_minutes"], json!(4));
    assert_eq!(day["recorded_since"], json!(date));
}

#[tokio::test]
async fn the_old_heartbeat_body_counts_viewing_time_and_records_no_span() {
    let server = Server::start().await;

    let (status, _) = server.user.json(server.user.post("/api/v1/wellness/heartbeat").json(&json!({"seconds": 60}))).await;
    assert_eq!(status, 204);

    let today = chrono::Utc::now().date_naive().to_string();
    let day = activity_day(&server, &today).await;

    assert_eq!((day["spans"].clone(), day["recorded_since"].clone()), (json!([]), Value::Null));
    assert_eq!(overview(&server).await["days"][6]["viewing_minutes"], json!(1));
}

#[tokio::test]
async fn time_elsewhere_in_the_app_is_not_counted_as_looking_at_sessions() {
    let server = Server::start().await;

    assert_eq!(beat(&server, chrono::Utc::now(), json!({"kind": "elsewhere", "route": "history"})).await, 204);
    assert_eq!(overview(&server).await["days"][6]["viewing_minutes"], json!(0));
}

#[tokio::test]
async fn an_unknown_kind_is_refused_with_a_reason() {
    let server = Server::start().await;

    let (status, body) = server.user.json(server.user.post("/api/v1/wellness/heartbeat").json(&json!({"seconds": 60, "kind": "dreaming"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("activity_kind_invalid")));
}

#[tokio::test]
async fn a_span_across_local_midnight_is_split_between_the_two_days() {
    use chrono::{Duration, TimeZone};

    let server = Server::start().await;
    let zone: chrono_tz::Tz = "Pacific/Kiritimati".parse().unwrap();

    save(&server, |body| body["time_zone"] = json!("Pacific/Kiritimati")).await;

    let yesterday = chrono::Utc::now().with_timezone(&zone).date_naive() - Duration::days(1);
    let midnight = zone.from_local_datetime(&yesterday.and_hms_opt(0, 0, 0).unwrap()).unwrap().with_timezone(&chrono::Utc);

    assert_eq!(beat(&server, midnight + Duration::seconds(30), json!({"kind": "elsewhere", "route": "history"})).await, 204);

    let before = activity_day(&server, &(yesterday - Duration::days(1)).to_string()).await;
    let after = activity_day(&server, &yesterday.to_string()).await;

    assert_eq!(before["spans"][0]["end"].as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap(), midnight);
    assert_eq!(after["spans"][0]["start"].as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap(), midnight);
    assert_eq!(after["recorded_since"], json!((yesterday - Duration::days(1)).to_string()));
}

#[tokio::test]
async fn the_day_names_the_conversation_and_lists_breaks() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Tidy the changelog")).await;

    assert_eq!(beat(&server, chrono::Utc::now(), json!({"kind": "conversation", "route": "conversation", "conversation_id": conversation.id})).await, 204);
    server.user.json(server.user.post("/api/v1/wellness/breaks").json(&json!({"minutes": 5}))).await;

    let day = activity_day(&server, &chrono::Utc::now().date_naive().to_string()).await;

    assert_eq!((day["spans"][0]["kind"].clone(), day["spans"][0]["conversation_id"].clone()), (json!("conversation"), json!(conversation.id)));
    assert_eq!(day["spans"][0]["title"], json!(conversation.title));
    assert_eq!(day["breaks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_date_that_is_not_a_date_is_refused_with_a_reason() {
    let server = Server::start().await;

    let (status, body) = server.user.json(server.user.get("/api/v1/wellness/activity/day?date=yesterday")).await;

    assert_eq!((status, body["error"].clone()), (400, json!("date_invalid")));
}

#[tokio::test]
async fn the_hours_add_up_minutes_per_local_hour_and_name_the_main_target() {
    use chrono::{Duration, Timelike};

    let server = Server::start().await;
    let base = earlier_this_hour();

    for offset in [10, 11, 12] {
        beat(&server, base + Duration::minutes(offset), json!({"kind": "elsewhere", "route": "history", "title": "History"})).await;
    }

    beat(&server, base + Duration::seconds(59 * 60 + 30), json!({"kind": "elsewhere", "route": "wellness", "title": "Wellness"})).await;

    let (status, hours) = server.user.json(server.user.get("/api/v1/wellness/activity/hours?days=7")).await;
    let days = hours["days"].as_array().unwrap();
    let day = days.iter().find(|day| day["date"] == json!(base.date_naive().to_string())).unwrap();
    let hour = base.hour() as usize;

    assert_eq!((status, days.len()), (200, 7));
    assert_eq!((day["minutes"][hour].clone(), day["top"][hour].clone()), (json!(4), json!("History")));
    assert_eq!((day["minutes"][(hour + 1) % 24].clone(), day["top"][(hour + 1) % 24].clone()), (json!(0), Value::Null));
}
