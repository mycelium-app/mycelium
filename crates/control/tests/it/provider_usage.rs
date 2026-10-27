use crate::common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn openrouter_spend_survives_restart_and_failed_refresh_without_leaking_other_providers() {
    let server = Server::start().await;
    let owner: Uuid = sqlx::query_scalar("select id from users limit 1")
        .fetch_one(&server.pool)
        .await
        .unwrap();
    let provider: Uuid = sqlx::query_scalar("insert into model_providers (owner, kind, name, base_url, models) values ($1, 'openai_compatible', 'OpenRouter', 'https://openrouter.ai/api/v1', '{}') returning id").bind(owner).fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into model_providers (owner, kind, name, base_url, models) values ($1, 'openai_compatible', 'Local', 'http://localhost:11434/v1', '{}')").bind(owner).execute(&server.pool).await.unwrap();
    let other = server.second_user("other@example.test").await;
    let (_, hidden) = other.json(other.get("/api/v1/model-provider-usage")).await;
    assert_eq!(hidden, json!([]));
    let (status, empty) = server
        .user
        .json(server.user.get("/api/v1/model-provider-usage"))
        .await;
    assert_eq!(status, 200);
    assert_eq!(empty.as_array().unwrap().len(), 1);
    assert_eq!(empty[0]["spend"], Value::Null);
    let spend = json!({"total":12.3,"daily":0.4,"weekly":2.0,"monthly":8.0,"limit_remaining":null});
    sqlx::query("insert into model_provider_usage (provider_id, read_at, spend) values ($1, now() - interval '1 hour', $2)").bind(provider).bind(&spend).execute(&server.pool).await.unwrap();
    let at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("select read_at from model_provider_usage where provider_id = $1")
            .bind(provider)
            .fetch_one(&server.pool)
            .await
            .unwrap();
    let restarted = Server::start_on(server.database_url.clone(), |_| {}).await;
    let (status, cached) = restarted
        .user
        .json(restarted.user.get("/api/v1/model-provider-usage"))
        .await;
    assert_eq!(status, 200);
    assert_eq!(cached[0]["spend"], spend);
    assert!(cached[0]["read_at"].is_string());
    assert_eq!(cached[0]["read_at"], json!(at));
    assert_eq!(cached[0]["provider_id"], provider.to_string());
    assert!(!cached.to_string().contains("api_key"));
}
