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
