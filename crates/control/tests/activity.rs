mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::NodeToControl;
use serde_json::{Value, json};
use uuid::Uuid;

async fn activity(server: &Server, query: &str) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/activity{query}"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn conversation_on(server: &Server, link: &mut NodeLink, cwd: &str) -> Uuid {
    let (status, body) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": cwd, "prompt": "go"}))).await;
    assert_eq!(status, 201, "{body}");
    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();

    link.expect_start_session().await;
    link.start_session(id, "s1").await;
    link.expect_prompt().await;

    id
}

async fn finish_turn(link: &mut NodeLink, conversation_id: Uuid, seq: i64, usage: Option<Value>, cost: Option<f64>, model: Option<&str>) {
    if let Some(cost) = cost {
        let update = json!({"update": {"sessionUpdate": "usage_update", "used": 10, "size": 200, "cost": {"amount": cost, "currency": "USD"}, "_meta": {"_claude/model": model}}});
        link.send(NodeToControl::AcpUpdate { conversation_id, seq, update }).await;
    }

    link.send(NodeToControl::TurnEnd { conversation_id, seq: seq + 1, stop_reason: "end_turn".into(), usage }).await;
    link.barrier(conversation_id, seq + 2).await;
}

fn usage(input: i64, output: i64, read: i64, write: i64) -> Value {
    json!({"inputTokens": input, "outputTokens": output, "cachedReadTokens": read, "cachedWriteTokens": write, "totalTokens": input + output + read + write})
}

#[tokio::test]
async fn a_turns_tokens_cost_and_time_are_totalled_per_conversation_and_per_day() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    sqlx::query("update events set created_at = now() - interval '10 seconds' where kind = 'user_message'").execute(&server.pool).await.unwrap();

    finish_turn(&mut link, conversation, 1, Some(usage(100, 50, 1000, 200)), Some(0.10), Some("claude-opus-5-5")).await;

    let body = activity(&server, "?range=today").await;
    assert_eq!(body["totals"]["turns"], 1);
    assert_eq!(body["totals"]["reported_turns"], 1);
    assert_eq!(body["totals"]["input_tokens"], 100);
    assert_eq!(body["totals"]["output_tokens"], 50);
    assert_eq!(body["totals"]["cache_read_tokens"], 1000);
    assert_eq!(body["totals"]["cache_write_tokens"], 200);
    assert_eq!(body["totals"]["cost_usd"], 0.10);
    assert!(body["totals"]["working_ms"].as_i64().unwrap() >= 10_000);
    assert_eq!(body["conversations"][0]["key"], json!(conversation.to_string()));
    assert_eq!(body["days"].as_array().unwrap().len(), 1);
    assert_eq!(body["days"][0]["turns"], 1);
}

#[tokio::test]
async fn a_later_turns_time_is_measured_from_its_own_user_message() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    sqlx::query("update events set created_at = now() - interval '1 hour' where kind = 'user_message'").execute(&server.pool).await.unwrap();
    finish_turn(&mut link, conversation, 1, None, None, None).await;
    sqlx::query("insert into events (conversation_id, seq, kind, payload, created_at) values ($1, 100, 'user_message', '{\"text\": \"again\"}', now() - interval '5 seconds')").bind(conversation).execute(&server.pool).await.unwrap();

    finish_turn(&mut link, conversation, 101, None, None, None).await;

    let measured = sqlx::query_scalar::<_, Option<i64>>("select working_ms from usage_turns where conversation_id = $1 order by id desc limit 1").bind(conversation).fetch_one(&server.pool).await.unwrap().unwrap();
    assert!((5_000..60_000).contains(&measured), "{measured}");
}

#[tokio::test]
async fn the_agents_running_cost_is_turned_into_a_cost_per_turn() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;

    finish_turn(&mut link, conversation, 1, Some(usage(1, 1, 0, 0)), Some(0.10), None).await;
    finish_turn(&mut link, conversation, 4, Some(usage(1, 1, 0, 0)), Some(0.25), None).await;
    finish_turn(&mut link, conversation, 7, Some(usage(1, 1, 0, 0)), None, None).await;

    let costs = sqlx::query_scalar::<_, Option<f64>>("select cost_usd from usage_turns order by id").fetch_all(&server.pool).await.unwrap();

    assert_eq!(costs.len(), 3);
    assert!((costs[0].unwrap() - 0.10).abs() < 1e-9);
    assert!((costs[1].unwrap() - 0.15).abs() < 1e-9);
    assert!(costs[2].unwrap().abs() < 1e-9);
}

#[tokio::test]
async fn usage_is_broken_down_by_node_agent_project_and_model() {
    let server = Server::start().await;
    let mut laptop = server.online_node("laptop").await;
    let mut desk = server.online_node("desk").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into projects (owner, node_id, name, path) values ($1, $2, 'backups', '/work/backups')").bind(owner).bind(laptop.id).execute(&server.pool).await.unwrap();
    let in_project = conversation_on(&server, &mut laptop, "/work/backups-worktrees/fix").await;
    let elsewhere = conversation_on(&server, &mut desk, "/work/other").await;

    finish_turn(&mut laptop, in_project, 1, Some(usage(100, 0, 0, 0)), Some(1.0), Some("claude-opus-5-5")).await;
    finish_turn(&mut desk, elsewhere, 1, Some(usage(10, 0, 0, 0)), Some(0.5), Some("claude-sonnet-5-5")).await;

    let body = activity(&server, "?range=7d").await;
    let names = |group: &str| body[group].as_array().unwrap().iter().map(|row| row["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();

    assert_eq!(names("nodes"), ["laptop", "desk"]);
    assert_eq!(names("agents"), ["claude"]);
    assert_eq!(names("projects"), ["backups", "No project"]);
    assert_eq!(names("models"), ["claude-opus-5-5", "claude-sonnet-5-5"]);
    assert_eq!(body["nodes"][0]["input_tokens"], 100);
}

#[tokio::test]
async fn an_agent_that_reports_no_usage_still_counts_turns_and_is_named_as_unreported() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;

    finish_turn(&mut link, conversation, 1, None, None, None).await;

    let body = activity(&server, "").await;
    assert_eq!(body["totals"]["turns"], 1);
    assert_eq!(body["totals"]["reported_turns"], 0);
    assert_eq!(body["totals"]["cost_usd"], Value::Null);
    assert_eq!(body["unreported_agents"], json!(["claude"]));
}

#[tokio::test]
async fn the_range_and_the_node_narrow_what_is_counted() {
    let server = Server::start().await;
    let mut laptop = server.online_node("laptop").await;
    let mut desk = server.online_node("desk").await;
    let first = conversation_on(&server, &mut laptop, "/work/app").await;
    let second = conversation_on(&server, &mut desk, "/work/app").await;
    finish_turn(&mut laptop, first, 1, Some(usage(5, 0, 0, 0)), None, None).await;
    finish_turn(&mut desk, second, 1, Some(usage(7, 0, 0, 0)), None, None).await;
    sqlx::query("update usage_turns set day = day - 10 where node_id = $1").bind(desk.id).execute(&server.pool).await.unwrap();

    assert_eq!(activity(&server, "?range=7d").await["totals"]["input_tokens"], 5);
    assert_eq!(activity(&server, "?range=30d").await["totals"]["input_tokens"], 12);
    assert_eq!(activity(&server, &format!("?range=30d&node_id={}", desk.id)).await["totals"]["input_tokens"], 7);
    assert_eq!(activity(&server, "?range=30d").await["days"].as_array().unwrap().len(), 30);
}

#[tokio::test]
async fn all_time_runs_from_the_first_recorded_day_and_groups_by_day() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, conversation, 1, Some(usage(5, 0, 0, 0)), None, None).await;
    finish_turn(&mut link, conversation, 3, Some(usage(7, 0, 0, 0)), None, None).await;
    sqlx::query("update usage_turns set day = day - 99 where id = (select min(id) from usage_turns)").execute(&server.pool).await.unwrap();

    let body = activity(&server, "?range=all").await;

    assert_eq!(body["range"], "all");
    assert_eq!(body["totals"]["input_tokens"], 12);
    assert_eq!(body["days"].as_array().unwrap().len(), 100);
    assert_eq!(body["hours"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn someone_elses_usage_is_not_visible() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, conversation, 1, Some(usage(5, 0, 0, 0)), None, None).await;
    let other = server.second_user("other@example.com").await;

    let (status, body) = other.json(other.get("/api/v1/activity")).await;

    assert_eq!(status, 200);
    assert_eq!(body["totals"]["turns"], 0);
}

#[tokio::test]
async fn an_unknown_range_is_refused_with_the_choices() {
    let server = Server::start().await;

    let (status, body) = server.user.json(server.user.get("/api/v1/activity?range=year")).await;

    assert_eq!(status, 400);
    assert_eq!(body["message"], "Choose today, 7d, 30d or all as the range.");
}

#[tokio::test]
async fn the_hourly_series_runs_from_the_start_of_the_range_to_the_current_hour_and_holds_the_turns_of_each_hour() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, conversation, 1, Some(usage(10, 5, 0, 0)), None, None).await;
    finish_turn(&mut link, conversation, 3, Some(usage(20, 5, 0, 0)), None, None).await;

    let today = activity(&server, "?range=today").await;
    let week = activity(&server, "?range=7d").await;
    let hours = today["hours"].as_array().unwrap();
    let week_hours = week["hours"].as_array().unwrap();

    assert!(!hours.is_empty() && hours.len() <= 24);
    assert!(hours.iter().all(|hour| hour["key"].as_str().unwrap().len() == 13));
    assert_eq!(hours.iter().map(|hour| hour["input_tokens"].as_i64().unwrap()).sum::<i64>(), 30);
    assert_eq!(hours.last().unwrap()["turns"].as_i64().unwrap() + hours.iter().rev().skip(1).map(|hour| hour["turns"].as_i64().unwrap()).sum::<i64>(), 2);
    assert!(week_hours.len() > 6 * 24 && week_hours.len() <= 7 * 24);
    assert_eq!(week_hours.last().unwrap()["key"], hours.last().unwrap()["key"]);
}

#[tokio::test]
async fn per_conversation_usage_lists_every_conversation_with_turns_and_all_time_totals() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let busy = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, busy, 1, Some(usage(100, 50, 0, 0)), Some(0.10), Some("claude-opus-5-5")).await;
    let silent = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, silent, 1, None, None, None).await;

    let (status, rows) = server.user.json(server.user.get("/api/v1/activity/conversations")).await;

    assert_eq!(status, 200, "{rows}");
    let row = |id: Uuid| rows.as_array().unwrap().iter().find(|row| row["key"] == json!(id.to_string())).unwrap().clone();
    assert_eq!((row(busy)["turns"].clone(), row(busy)["input_tokens"].clone(), row(busy)["cost_usd"].clone()), (json!(1), json!(100), json!(0.10)));
    assert_eq!((row(silent)["turns"].clone(), row(silent)["reported_turns"].clone()), (json!(1), json!(0)));
}

#[tokio::test]
async fn per_conversation_usage_does_not_show_another_persons_conversations() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mine = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, mine, 1, Some(usage(1, 1, 0, 0)), None, None).await;
    let other = server.second_user("other@example.com").await;

    let (_, rows) = other.json(other.get("/api/v1/activity/conversations")).await;

    assert_eq!(rows, json!([]));
}

#[tokio::test]
async fn a_conversations_context_and_spend_are_reported_separately() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    let update = json!({"update": {"sessionUpdate": "usage_update", "used": 52000, "size": 200000, "cost": {"amount": 0.10, "currency": "USD"}}});
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation, seq: 1, update }).await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 2, stop_reason: "end_turn".into(), usage: Some(usage(100, 50, 1000, 200)) }).await;
    link.barrier(conversation, 3).await;
    finish_turn(&mut link, conversation, 4, Some(usage(10, 5, 0, 0)), Some(0.25), None).await;

    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/usage"))).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(body["context_used"], 10);
    assert_eq!(body["context_size"], 200);
    assert_eq!(body["input_tokens"], 110);
    assert_eq!(body["output_tokens"], 55);
    assert_eq!(body["cache_read_tokens"], 1000);
    assert!((body["cost_usd"].as_f64().unwrap() - 0.25).abs() < 1e-9);
}

#[tokio::test]
async fn chats_without_request_usage_do_not_treat_billed_tokens_as_context() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, conversation, 1, Some(usage(100, 50, 1000, 200)), None, None).await;
    sqlx::query("update conversations set agent = 'chat' where id = $1").bind(conversation).execute(&server.pool).await.unwrap();

    let (_, body) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/usage"))).await;

    assert_eq!(body["context_used"], Value::Null);
    assert_eq!(body["context_size"], Value::Null);
    assert_eq!(body["input_tokens"], 100);
    assert_eq!(body["cache_read_tokens"], 1000);
}

#[tokio::test]
async fn context_without_a_reported_limit_falls_back_to_the_last_prompt_size() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = conversation_on(&server, &mut link, "/work/app").await;
    finish_turn(&mut link, conversation, 1, Some(usage(100, 50, 1000, 200)), None, None).await;

    let (_, body) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/usage"))).await;

    assert_eq!(body["context_used"], 1300);
    assert_eq!(body["context_size"], Value::Null);
    assert_eq!(body["cost_usd"], Value::Null);
}
