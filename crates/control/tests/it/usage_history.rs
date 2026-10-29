use crate::common;
use crate::common::Server;
use serde_json::{Value, json};

async fn history(server: &Server, query: &str) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/usage/history{query}"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

#[tokio::test]
async fn stored_readings_and_conversation_turns_backfill_windows_and_tokens_per_point() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Old work")).await;
    sqlx::query("update conversations set account = 'me' where id = $1").bind(conversation.id).execute(&server.pool).await.unwrap();
    for (at, five) in [(1_700_000_000, 10.0), (1_700_004_000, 50.0)] {
        sqlx::query("insert into login_usage_history (node_id, agent, account, read_at, windows) values ($1, 'claude', 'me', to_timestamp($2), $3)")
            .bind(link.id)
            .bind(at as f64)
            .bind(json!([
                {"name": "five_hour", "used_percent": five, "resets_at": "2023-11-15T00:00:00Z"},
                {"name": "seven_day", "used_percent": five / 4.0, "resets_at": "2023-11-20T00:00:00Z"}
            ]))
            .execute(&server.pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "insert into usage_turns (conversation_id, owner, node_id, agent, ended_at, day, input_tokens, output_tokens)
         select id, owner, node_id, 'claude', to_timestamp(1700002000), date '2023-11-14', 3000, 1000 from conversations where id = $1",
    )
    .bind(conversation.id)
    .execute(&server.pool)
    .await
    .unwrap();

    let account = common::eventually("the stored readings to appear as a window", || async {
        let body = history(&server, "?range=all&account=claude:me").await;
        let account = body["accounts"][0].clone();

        account["windows"].as_array().is_some_and(|windows| !windows.is_empty()).then_some(account)
    })
    .await;

    assert_eq!(account["windows"][0]["peak_percent"], 50.0);
    assert_eq!(account["windows"][0]["tokens"], 4000);
    assert_eq!(account["tokens_per_point"]["five_hour"], 100.0);
}

#[tokio::test]
async fn an_unknown_range_is_refused() {
    let server = Server::start().await;
    let (status, _) = server.user.json(server.user.get("/api/v1/usage/history?range=forever")).await;

    assert_eq!(status, 400);
}
