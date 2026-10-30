use crate::common::Server;
use crate::continue_elsewhere::{next_command, node_with_accounts, with_usage};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn save(server: &Server, body: Value) -> (u16, Value) {
    server.user.json(server.user.put("/api/v1/agent-settings").json(&body)).await
}

async fn start_with_modes(server: &Server, link: &mut crate::common::NodeLink, account: &str) -> Uuid {
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": account}))).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    next_command(link).await;
    let modes = json!({"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}, {"id": "bypassPermissions", "name": "Bypass"}]});
    link.send(NodeToControl::SessionStarted { conversation_id: id, acp_session_id: "acp-1".into(), modes: Some(modes), models: None, config_options: None }).await;
    crate::common::eventually("the session to start", || async { Some(server.conversation(id).await).filter(|conversation| conversation["acp_session_id"].is_string()) }).await;

    id
}

async fn mode_set_on_start(link: &mut crate::common::NodeLink) -> Option<String> {
    match next_command(link).await {
        ControlToNode::SetConfig { mode, .. } => mode,
        other => panic!("expected set_config, got {other:?}"),
    }
}

#[tokio::test]
async fn settings_start_empty_and_keep_what_was_saved() {
    let server = Server::start().await;

    let (_, empty) = server.user.json(server.user.get("/api/v1/agent-settings")).await;
    assert_eq!(empty["any_model"], json!([]));
    assert!(empty["default_modes"]["claude"].is_null());

    let (status, saved) = save(&server, json!({"default_modes": {"claude": "plan", "codex": "auto"}, "any_model": [{"agent": "claude"}, {"agent": "codex", "model": "gpt-5.5"}]})).await;

    assert_eq!(status, 200, "{saved}");
    let (_, again) = server.user.json(server.user.get("/api/v1/agent-settings")).await;
    assert_eq!((again["default_modes"]["claude"].clone(), again["default_modes"]["codex"].clone()), (json!("plan"), json!("auto")));
    assert_eq!(again["any_model"][1]["model"], "gpt-5.5");
}

#[tokio::test]
async fn a_mode_the_agent_does_not_have_is_refused_in_plain_words() {
    let server = Server::start().await;

    let (status, body) = save(&server, json!({"default_modes": {"codex": "plan"}, "any_model": []})).await;

    assert_eq!((status, body["error"].as_str()), (400, Some("mode_invalid")));
    assert_eq!(body["message"], "Choose Default, Auto or Bypass as the default mode for Codex.");
}

#[tokio::test]
async fn the_any_model_list_never_holds_the_same_entry_twice() {
    let server = Server::start().await;
    let (status, saved) = save(&server, json!({"default_modes": {}, "any_model": [{"agent": "claude"}, {"agent": "claude", "account": "sam@example.com"}, {"agent": "codex"}, {"agent": "claude"}]})).await;

    assert_eq!(status, 200);
    assert_eq!(saved["any_model"], json!([{"agent": "claude"}, {"agent": "codex"}]));

    sqlx::query("update user_settings set any_model = '[{\"agent\": \"claude\"}, {\"agent\": \"claude\"}]'::jsonb").execute(&server.pool).await.unwrap();

    let (_, stored) = server.user.json(server.user.get("/api/v1/agent-settings")).await;

    assert_eq!(stored["any_model"], json!([{"agent": "claude"}]));
}

#[tokio::test]
async fn an_any_model_entry_must_be_one_kind_of_choice() {
    let server = Server::start().await;

    let (status, body) = save(&server, json!({"default_modes": {}, "any_model": [{"model": "gpt-5.5"}]})).await;

    assert_eq!((status, body["error"].as_str()), (400, Some("any_model_entry_invalid")));
}

#[tokio::test]
async fn a_new_conversation_starts_in_the_default_mode_for_its_agent() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    save(&server, json!({"default_modes": {"claude": "plan"}, "any_model": []})).await;

    start_with_modes(&server, &mut link, "sam@example.com").await;

    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));
}

#[tokio::test]
async fn an_account_override_wins_over_the_default_and_clearing_it_goes_back() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90), with_usage("work@example.org", 90)]).await;
    save(&server, json!({"default_modes": {"claude": "plan"}, "any_model": []})).await;
    let (status, _) = server.user.json(server.user.put("/api/v1/agent-settings/account-mode").json(&json!({"agent": "claude", "account": "work@example.org", "mode": "bypassPermissions"}))).await;
    assert_eq!(status, 204);
    let (_, settings) = server.user.json(server.user.get("/api/v1/agent-settings")).await;
    assert_eq!(settings["account_modes"], json!([{"agent": "claude", "account": "work@example.org", "mode": "bypassPermissions"}]));

    start_with_modes(&server, &mut link, "work@example.org").await;
    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("bypassPermissions"));

    server.user.json(server.user.put("/api/v1/agent-settings/account-mode").json(&json!({"agent": "claude", "account": "work@example.org"}))).await;
    start_with_modes(&server, &mut link, "work@example.org").await;
    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));
}

#[tokio::test]
async fn with_no_default_the_agents_own_mode_is_left_alone() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;

    let id = start_with_modes(&server, &mut link, "sam@example.com").await;

    let conversation = crate::common::eventually("the session to start", || async { Some(server.conversation(id).await).filter(|conversation| conversation["acp_session_id"].is_string()) }).await;
    assert_eq!(conversation["mode"], "default");
}

async fn restart_reporting_default(link: &mut crate::common::NodeLink, id: Uuid, session: &str) {
    let modes = json!({"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}]});
    link.send(NodeToControl::SessionStarted { conversation_id: id, acp_session_id: session.into(), modes: Some(modes), models: None, config_options: None }).await;
}

#[tokio::test]
async fn a_resumed_session_is_put_back_in_the_conversations_mode() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    let id = start_with_modes(&server, &mut link, "sam@example.com").await;
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "plan"}))).await;
    assert_eq!(status, 204);
    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));

    restart_reporting_default(&mut link, id, "acp-1").await;

    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));
}

#[tokio::test]
async fn a_new_session_for_the_same_conversation_keeps_its_mode() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    let id = start_with_modes(&server, &mut link, "sam@example.com").await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "plan"}))).await;
    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));

    restart_reporting_default(&mut link, id, "acp-2").await;

    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));
}

#[tokio::test]
async fn a_mode_chosen_before_the_session_starts_is_applied_when_it_does() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    let (_, created) = server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "account": "sam@example.com"}))).await;
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    next_command(&mut link).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"mode": "plan"}))).await;
    assert_eq!(status, 204);
    restart_reporting_default(&mut link, id, "acp-1").await;

    assert_eq!(mode_set_on_start(&mut link).await.as_deref(), Some("plan"));
}

#[tokio::test]
async fn the_conversation_shows_the_mode_the_agent_reports() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    let id = start_with_modes(&server, &mut link, "sam@example.com").await;

    link.send(NodeToControl::AcpUpdate { conversation_id: id, seq: 1, update: json!({"update": {"sessionUpdate": "current_mode_update", "currentModeId": "bypassPermissions"}}) }).await;

    crate::common::eventually("the reported mode", || async { (server.conversation(id).await["mode"] == "bypassPermissions").then_some(()) }).await;
}

#[tokio::test]
async fn a_resumed_session_is_put_back_on_the_conversations_model() {
    let server = Server::start().await;
    let mut link = node_with_accounts(&server, "v0.11.0", vec![with_usage("sam@example.com", 90)]).await;
    let id = start_with_modes(&server, &mut link, "sam@example.com").await;
    let models = json!({"currentModelId": "sonnet", "availableModels": [{"modelId": "sonnet", "name": "Sonnet"}, {"modelId": "opus", "name": "Opus"}]});
    link.send(NodeToControl::SessionStarted { conversation_id: id, acp_session_id: "acp-1".into(), modes: None, models: Some(models.clone()), config_options: None }).await;
    crate::common::eventually("the reported model", || async { (server.conversation(id).await["model"] == "sonnet").then_some(()) }).await;
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/config")).json(&json!({"model": "opus"}))).await;
    assert_eq!(status, 204);
    match next_command(&mut link).await {
        ControlToNode::SetConfig { model, .. } => assert_eq!(model.as_deref(), Some("opus")),
        other => panic!("expected set_config, got {other:?}"),
    }

    link.send(NodeToControl::SessionStarted { conversation_id: id, acp_session_id: "acp-1".into(), modes: None, models: Some(models), config_options: None }).await;

    match next_command(&mut link).await {
        ControlToNode::SetConfig { model, .. } => assert_eq!(model.as_deref(), Some("opus")),
        other => panic!("expected set_config, got {other:?}"),
    }
}

#[tokio::test]
async fn provider_favourites_from_before_the_merge_become_any_model_entries_once() {
    let server = Server::start().await;
    let provider_id: Uuid = sqlx::query_scalar(
        "insert into model_providers (owner, kind, name, base_url, models, favourite_models) select id, 'openai_compatible', 'Local', 'http://127.0.0.1:1', array['llama3', 'qwen3'], array['qwen3', 'llama3'] from users order by created_at limit 1 returning id",
    )
    .fetch_one(&server.pool)
    .await
    .unwrap();
    let migration = include_str!("../../migrations/0144_favourites_into_any_model.sql");

    sqlx::raw_sql(migration).execute(&server.pool).await.unwrap();
    sqlx::raw_sql(migration).execute(&server.pool).await.unwrap();

    let (_, settings) = server.user.json(server.user.get("/api/v1/agent-settings")).await;

    assert_eq!(settings["any_model"], json!([{"provider_id": provider_id, "model": "qwen3"}, {"provider_id": provider_id, "model": "llama3"}]));
}

