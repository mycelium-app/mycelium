use crate::common::Server;
use crate::watches::{begun, end_turn, text, tool};
use serde_json::{Value, json};

fn report(events: Value) -> Value {
    json!({"session_id": "tab-1", "client": "web", "version": "0.34.40", "events": events})
}

fn an_error(message: &str) -> Value {
    json!({"at": "2026-11-15T09:00:00Z", "kind": "error", "message": message, "detail": {"stack": "at App (mycelium-web.js:1:2)"}})
}

#[tokio::test]
async fn an_error_a_client_reports_is_stored_and_read_back_with_where_it_came_from() {
    let server = Server::start().await;
    let user = &server.user;

    let (status, _) = user.json(user.post("/api/v1/client-events").header("user-agent", "Mozilla/5.0 (iPad; CPU OS 27_2 like Mac OS X) AppleWebKit/605.1.15").json(&report(json!([an_error("Size(1011 x 2147483647) is out of range")])))).await;
    let (_, read) = user.json(user.get("/api/v1/client-events")).await;
    let event = &read["events"][0];

    assert_eq!(status, 204);
    assert_eq!(event["kind"], "error");
    assert_eq!(event["message"], "Size(1011 x 2147483647) is out of range");
    assert_eq!(event["detail"]["stack"], "at App (mycelium-web.js:1:2)");
    assert_eq!((event["client"].as_str(), event["version"].as_str(), event["session_id"].as_str()), (Some("web"), Some("0.34.40"), Some("tab-1")));
    assert!(event["user_agent"].as_str().unwrap().contains("iPad"), "{event}");
}

#[tokio::test]
async fn tokens_and_secrets_in_a_report_are_never_stored() {
    let server = Server::start().await;
    let user = &server.user;
    let leaky = json!({"at": "2026-11-15T09:00:00Z", "kind": "connection", "message": "closed wss://mycelium.example.com/api/v1/stream?token=abc123secret with Bearer myc_cli_0123456789abcdef after password=hunter2secret", "detail": {"url": "https://mycelium.example.com/auth/callback?code=the-code&state=s1", "authorization": "Bearer ghp_0123456789abcdefghijklmnopqrstuvwxyz"}});

    user.json(user.post("/api/v1/client-events").json(&report(json!([leaky])))).await;
    let (_, read) = user.json(user.get("/api/v1/client-events")).await;
    let stored = read["events"][0].to_string();

    for secret in ["abc123secret", "myc_cli_0123456789abcdef", "the-code", "ghp_0123456789abcdefghijklmnopqrstuvwxyz", "hunter2secret"] {
        assert!(!stored.contains(secret), "{secret} was stored: {stored}");
    }

    assert!(stored.contains("closed wss://mycelium.example.com/api/v1/stream"), "{stored}");
}

#[tokio::test]
async fn a_report_with_an_unknown_kind_or_too_many_events_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;
    let user = &server.user;
    let unknown = json!([{"at": "2026-11-15T09:00:00Z", "kind": "keystroke", "message": "x"}]);
    let too_many: Vec<Value> = (0..51).map(|index| an_error(&format!("error {index}"))).collect();

    let (unknown_status, unknown_body) = user.json(user.post("/api/v1/client-events").json(&report(unknown))).await;
    let (many_status, many_body) = user.json(user.post("/api/v1/client-events").json(&report(Value::Array(too_many)))).await;

    assert_eq!((unknown_status, unknown_body["error"].as_str()), (400, Some("client_event_kind")));
    assert!(unknown_body["message"].as_str().unwrap().contains("connection"), "{unknown_body}");
    assert_eq!((many_status, many_body["error"].as_str()), (400, Some("client_events_too_many")));
}

#[tokio::test]
async fn a_client_that_reports_too_often_is_told_to_wait() {
    let server = Server::start().await;
    let user = &server.user;
    let mut statuses = Vec::new();

    for index in 0..40 {
        let (status, _) = user.json(user.post("/api/v1/client-events").json(&report(json!([an_error(&format!("error {index}"))])))).await;
        statuses.push(status);
    }

    assert!(statuses.contains(&204), "{statuses:?}");
    assert!(statuses.contains(&429), "{statuses:?}");
}

#[tokio::test]
async fn reports_beyond_a_persons_cap_drop_their_oldest_events() {
    let server = Server::start().await;
    let user = &server.user;
    let owner = sqlx::query_scalar::<_, uuid::Uuid>("select id from users where email = 'dev@example.com'").fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into client_events (owner, at, session_id, client, version, kind, message) select $1, now() - make_interval(secs => n), 'old', 'web', '0.34.39', 'error', 'old ' || n from generate_series(1, 5000) n")
        .bind(owner)
        .execute(&server.pool)
        .await
        .unwrap();

    user.json(user.post("/api/v1/client-events").json(&report(json!([an_error("newest")])))).await;
    let kept = sqlx::query_scalar::<_, i64>("select count(*) from client_events where owner = $1").bind(owner).fetch_one(&server.pool).await.unwrap();
    let oldest_gone = sqlx::query_scalar::<_, bool>("select not exists (select 1 from client_events where owner = $1 and message = 'old 1')").bind(owner).fetch_one(&server.pool).await.unwrap();

    assert_eq!(kept, 5000);
    assert!(oldest_gone);
}

#[tokio::test]
async fn events_older_than_fourteen_days_are_swept() {
    let server = Server::start_with(|config| config.scheduler_tick = std::time::Duration::from_millis(50)).await;
    let owner = sqlx::query_scalar::<_, uuid::Uuid>("select id from users where email = 'dev@example.com'").fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into client_events (owner, at, received_at, session_id, client, version, kind, message) values ($1, now() - interval '15 days', now() - interval '15 days', 's', 'web', '0.34.39', 'error', 'stale'), ($1, now(), now(), 's', 'web', '0.34.40', 'error', 'fresh')")
        .bind(owner)
        .execute(&server.pool)
        .await
        .unwrap();

    let mut left = Vec::new();
    for _ in 0..100 {
        left = sqlx::query_scalar::<_, String>("select message from client_events where owner = $1 order by message").bind(owner).fetch_all(&server.pool).await.unwrap();
        if left.len() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert_eq!(left, ["fresh"]);
}

#[tokio::test]
async fn read_client_events_returns_the_persons_own_events_filtered_by_client_and_kind() {
    let server = Server::start().await;
    let user = &server.user;
    let alex = server.second_user("alex@example.com").await;
    let mut link = server.online_node("atlas").await;
    let conversation = begun(&server, &mut link, "Why does the iPad reconnect?").await;
    end_turn(&mut link, conversation, 1).await;

    user.json(user.post("/api/v1/client-events").json(&report(json!([an_error("on the web"), {"at": "2026-11-15T09:01:00Z", "kind": "connection", "message": "closed 1006", "detail": {"close_code": 1006}}])))).await;
    user.json(user.post("/api/v1/client-events").json(&json!({"session_id": "desk-1", "client": "desktop", "version": "0.34.40", "events": [an_error("on the desktop")]}))).await;
    alex.json(alex.post("/api/v1/client-events").json(&report(json!([an_error("alex's error")])))).await;

    let all = tool(&server, &link, conversation, "read_client_events", json!({})).await;
    let connections = tool(&server, &link, conversation, "read_client_events", json!({"client": "web", "kind": "connection"})).await;
    let read: Value = serde_json::from_str(&text(&all)).unwrap();
    let filtered: Value = serde_json::from_str(&text(&connections)).unwrap();
    let messages: Vec<&str> = read["events"].as_array().unwrap().iter().map(|event| event["message"].as_str().unwrap()).collect();

    assert_eq!(all["isError"], false, "{all}");
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(!messages.contains(&"alex's error"), "{messages:?}");
    assert_eq!(filtered["events"].as_array().unwrap().len(), 1, "{filtered}");
    assert_eq!(filtered["events"][0]["detail"]["close_code"], 1006);
}
