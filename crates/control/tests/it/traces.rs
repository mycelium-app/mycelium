use crate::common::{Client, Server};
use crate::watches::{begun, end_turn, text, tool};
use serde_json::{Value, json};
use std::time::Duration;

async fn traced() -> Server {
    Server::start_with(|config| config.trace_kept_in_full = Duration::ZERO).await
}

pub async fn traces_of(client: &Client, query: &str, until: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let mut traces = Vec::new();

    for _ in 0..100 {
        let (_, read) = client.json(client.get(&format!("/api/v1/traces{query}"))).await;
        traces = read["traces"].as_array().cloned().unwrap_or_default();

        if until(&traces) {
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    traces
}

#[tokio::test]
async fn a_request_is_traced_under_the_request_id_it_answers_with() {
    let server = Server::start().await;
    let user = &server.user;
    let request_id = "7f3c2a10-1111-4222-8333-944455556666";

    let response = user.get("/api/v1/conversations?archived=false").header("x-request-id", request_id).send().await.unwrap();
    let traces = traces_of(user, "?name=/api/v1/conversations", |traces| traces.iter().any(|trace| trace["trace_id"] == request_id)).await;
    let trace = traces.iter().find(|trace| trace["trace_id"] == request_id).cloned().unwrap_or(Value::Null);

    assert_eq!(response.headers()["x-request-id"], request_id);
    assert_eq!(trace["label"], "GET /api/v1/conversations", "{traces:?}");
    assert_eq!(trace["fields"]["status"], 200);
    assert!(trace["duration_ms"].as_f64().unwrap() > 0.0, "{trace}");
}

#[tokio::test]
async fn a_slow_request_keeps_the_database_queries_inside_it_without_their_values() {
    let server = traced().await;
    let user = &server.user;

    user.get("/api/v1/conversations?archived=false&token=abc123secret").header("x-request-id", "8f3c2a10-1111-4222-8333-944455556666").send().await.unwrap();
    let traces = traces_of(user, "?name=/api/v1/conversations", |traces| traces.iter().any(|trace| trace["children"].as_array().is_some_and(|children| !children.is_empty()))).await;
    let trace = traces.iter().find(|trace| trace["trace_id"] == "8f3c2a10-1111-4222-8333-944455556666").cloned().unwrap_or(Value::Null);
    let queries: Vec<&Value> = trace["children"].as_array().map(|children| children.iter().filter(|child| child["name"] == "db").collect()).unwrap_or_default();

    assert!(!queries.is_empty(), "{trace}");
    assert!(queries.iter().all(|query| query["label"].as_str().is_some_and(|label| !label.is_empty()) && query["duration_ms"].is_number()), "{queries:?}");
    assert!(queries.iter().any(|query| query["fields"]["rows_returned"].is_number()), "{queries:?}");
    assert!(!trace.to_string().contains("abc123secret"), "{trace}");
}

#[tokio::test]
async fn a_quick_request_keeps_only_its_own_span() {
    let server = Server::start_with(|config| config.trace_kept_in_full = Duration::from_secs(60)).await;
    let user = &server.user;

    user.get("/api/v1/conversations?archived=false").header("x-request-id", "9f3c2a10-1111-4222-8333-944455556666").send().await.unwrap();
    let traces = traces_of(user, "?name=/api/v1/conversations", |traces| traces.iter().any(|trace| trace["trace_id"] == "9f3c2a10-1111-4222-8333-944455556666")).await;
    let trace = traces.iter().find(|trace| trace["trace_id"] == "9f3c2a10-1111-4222-8333-944455556666").cloned().unwrap_or(Value::Null);

    assert_eq!(trace["label"], "GET /api/v1/conversations", "{traces:?}");
    assert!(trace.get("children").is_none(), "{trace}");
}

#[tokio::test]
async fn read_traces_returns_the_persons_own_tool_calls_and_requests_only() {
    let server = traced().await;
    let user = &server.user;
    let alex = server.second_user("alex@example.com").await;
    let mut link = server.online_node("atlas").await;
    let conversation = begun(&server, &mut link, "Why is it slow?").await;
    end_turn(&mut link, conversation, 1).await;

    alex.get("/api/v1/conversations?archived=false").header("x-request-id", "af3c2a10-1111-4222-8333-944455556666").send().await.unwrap();
    tool(&server, &link, conversation, "find_conversations", json!({})).await;
    traces_of(user, "?name=find_conversations", |traces| !traces.is_empty()).await;

    let read = tool(&server, &link, conversation, "read_traces", json!({"name": "find_conversations"})).await;
    let found: Value = serde_json::from_str(&text(&read)).unwrap();
    let everything = tool(&server, &link, conversation, "read_traces", json!({"limit": 100})).await;
    let all: Value = serde_json::from_str(&text(&everything)).unwrap();

    assert_eq!(read["isError"], false, "{read}");
    assert!(found["traces"].as_array().unwrap().iter().any(|trace| trace.to_string().contains("\"find_conversations\"")), "{found}");
    assert!(!all.to_string().contains("af3c2a10-1111-4222-8333-944455556666"), "alex's request was visible: {all}");
}

#[tokio::test]
async fn traces_older_than_two_days_are_swept() {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let owner = sqlx::query_scalar::<_, uuid::Uuid>("select id from users where email = 'dev@example.com'").fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into trace_spans (trace_id, owner, position, name, label, started_at, duration_ms) values (gen_random_uuid(), $1, 0, 'http', 'GET /old', now() - interval '49 hours', 5), (gen_random_uuid(), $1, 0, 'http', 'GET /fresh', now(), 5)")
        .bind(owner)
        .execute(&server.pool)
        .await
        .unwrap();

    let mut left = Vec::new();
    for _ in 0..100 {
        left = sqlx::query_scalar::<_, String>("select label from trace_spans where label in ('GET /old', 'GET /fresh') order by label").fetch_all(&server.pool).await.unwrap();
        if left.len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(left, ["GET /fresh"]);
}
