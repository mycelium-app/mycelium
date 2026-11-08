use crate::common::{Server, TEST_SECRET_KEY};
use mycelium_control::secrets::{KeyRing, generate_key, installed_ring, reseal};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const FIXTURES: [(&str, &str, &str); 9] = [
    ("forges", "token", "glpat-fixture-forge-0001"),
    ("forges", "webhook_secret", "fixture-webhook-0010"),
    ("model_providers", "api_key", "sk-fixture-provider-0002"),
    ("decision_providers", "api_key", "sk-fixture-decision-0003"),
    ("chat_tool_servers", "bearer_token", "fixture-bearer-0004"),
    ("chat_tool_servers", "oauth_refresh_token", "fixture-refresh-0005"),
    ("chat_tool_servers", "session_key", "fixture-session-0006"),
    ("chat_tool_servers", "settings->>'encryption_password'", "fixture-budget-password-0007"),
    ("assistant_secrets", "value", "fixture-assistant-secret-0008"),
];

async fn insert_plaintext_rows(pool: &PgPool) {
    let owner: Uuid = sqlx::query_scalar("select id from users limit 1").fetch_one(pool).await.unwrap();
    let assistant: Uuid = sqlx::query_scalar("insert into assistants (owner, name) values ($1, 'Fixture') returning id").bind(owner).fetch_one(pool).await.unwrap();

    sqlx::query("insert into forges (owner, kind, base_url, account, token, webhook_secret) values ($1, 'gitlab', 'https://gitlab.example.com', 'sam', $2, $3)").bind(owner).bind(FIXTURES[0].2).bind(FIXTURES[1].2).execute(pool).await.unwrap();
    sqlx::query("insert into model_providers (owner, kind, name, base_url, api_key) values ($1, 'openai_compatible', 'Provider', 'https://llm.example.com/v1', $2)").bind(owner).bind(FIXTURES[2].2).execute(pool).await.unwrap();
    sqlx::query("insert into decision_providers (owner, name, base_url, api_key, model) values ($1, 'Decisions', 'https://llm.example.com/v1', $2, 'small')").bind(owner).bind(FIXTURES[3].2).execute(pool).await.unwrap();
    sqlx::query("insert into chat_tool_servers (owner, name, kind, url, bearer_token, oauth_refresh_token, session_key, settings) values ($1, 'Budget', 'actual', 'https://budget.example.com', $2, $3, $4, $5)")
        .bind(owner)
        .bind(FIXTURES[4].2)
        .bind(FIXTURES[5].2)
        .bind(FIXTURES[6].2)
        .bind(sqlx::types::Json(json!({"budget_sync_id": "sync-1", "encryption_password": FIXTURES[7].2})))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("insert into assistant_secrets (assistant_id, name, value) values ($1, 'GITHUB_TOKEN', $2)").bind(assistant).bind(FIXTURES[8].2).execute(pool).await.unwrap();
}

async fn stored(pool: &PgPool, table: &str, column: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select {column} from {table} limit 1"))).fetch_one(pool).await.unwrap()
}

async fn restarted_with_plaintext_rows() -> Server {
    let first = Server::start().await;

    insert_plaintext_rows(&first.pool).await;

    Server::start_on(first.database_url.clone(), |_| {}).await
}

#[tokio::test]
async fn plaintext_secrets_are_encrypted_in_place_when_the_control_plane_boots() {
    let server = restarted_with_plaintext_rows().await;

    for (table, column, plaintext) in FIXTURES {
        let value = stored(&server.pool, table, column).await;

        assert!(value.starts_with("enc:v1:"), "{table}.{column} was left as {value}");
        assert!(!value.contains(plaintext), "{table}.{column} still holds its plaintext");
    }
}

#[tokio::test]
async fn encrypted_secrets_still_decrypt_for_use() {
    let server = restarted_with_plaintext_rows().await;
    let ring = installed_ring().unwrap();

    for (table, column, plaintext) in FIXTURES {
        let value = stored(&server.pool, table, column).await;

        assert_eq!(ring.open(&value).unwrap(), plaintext);
    }
}

#[tokio::test]
async fn secrets_saved_through_the_api_are_stored_as_ciphertext() {
    let server = Server::start().await;
    let created = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Vault", "instructions": "Keep it safe."}))).await.1;
    let id = created["id"].as_str().unwrap();

    server.user.put(&format!("/api/v1/assistants/{id}/secrets/API_TOKEN")).json(&json!({"value": "fixture-api-secret-0009"})).send().await.unwrap();

    let value = stored(&server.pool, "assistant_secrets", "value").await;

    assert!(value.starts_with("enc:v1:") && !value.contains("fixture-api-secret-0009"), "{value}");
}

#[tokio::test]
async fn rotating_to_a_new_key_re_encrypts_every_secret_and_the_old_key_is_no_longer_needed() {
    let server = restarted_with_plaintext_rows().await;
    let new_key = generate_key();
    let rotated = KeyRing::parse(&new_key, &[TEST_SECRET_KEY.to_string()]).unwrap();
    let only_new = KeyRing::parse(&new_key, &[]).unwrap();

    let changed = reseal(&server.pool, &rotated).await.unwrap();
    let changed_again = reseal(&server.pool, &rotated).await.unwrap();

    assert_eq!((changed, changed_again), (FIXTURES.len() as u64, 0));

    for (table, column, plaintext) in FIXTURES {
        let value = stored(&server.pool, table, column).await;

        assert_eq!(only_new.open(&value).unwrap(), plaintext);
    }
}

#[tokio::test]
async fn a_stored_secret_cannot_be_opened_once_its_key_is_gone() {
    let server = restarted_with_plaintext_rows().await;
    let value = stored(&server.pool, "forges", "token").await;
    let stranger = KeyRing::parse(&generate_key(), &[]).unwrap();

    assert!(stranger.open(&value).is_err());
}

#[tokio::test]
async fn no_list_route_returns_a_stored_secret() {
    let server = restarted_with_plaintext_rows().await;
    let source = include_str!("../../src/lib.rs");
    let routes: Vec<&str> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(".route(\"")?.split_once("\", get(").map(|(path, _)| path))
        .filter(|path| path.starts_with("/api/v1/") && !path.contains('{') && !["/api/v1/stream", "/api/v1/link", "/api/v1/mcp"].iter().any(|skipped| path.starts_with(skipped)))
        .collect();

    assert!(routes.contains(&"/api/v1/forges") && routes.contains(&"/api/v1/chat-tool-servers") && routes.contains(&"/api/v1/model-providers"));

    let assistants = server.user.json(server.user.get("/api/v1/assistants")).await.1;
    let assistant = assistants.as_array().and_then(|list| list.first()).map(|assistant| assistant["id"].as_str().unwrap().to_string());
    let mut paths: Vec<String> = routes.iter().map(|path| path.to_string()).collect();

    if let Some(id) = assistant {
        paths.push(format!("/api/v1/assistants/{id}"));
        paths.push(format!("/api/v1/assistants/{id}/secrets"));
    }

    for path in paths {
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), server.user.get(&path).send()).await;
        let Ok(Ok(response)) = response else {
            continue;
        };
        let body = response.text().await.unwrap_or_default();

        for (_, _, plaintext) in FIXTURES {
            assert!(!body.contains(plaintext), "GET {path} returned a stored secret");
        }
    }
}

#[tokio::test]
async fn the_listed_integration_shows_that_a_password_is_set_without_its_value() {
    let server = restarted_with_plaintext_rows().await;

    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let budget: &Value = listed.as_array().unwrap().iter().find(|entry| entry["name"] == "Budget").unwrap();

    assert_eq!(budget["settings"]["encryption_password_set"], true);
    assert!(budget["settings"].get("encryption_password").is_none());
}
