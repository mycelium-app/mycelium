mod common;

use common::Server;
use mycelium_proto::link::NodeToControl;
use serde_json::json;

#[tokio::test]
async fn a_node_can_be_renamed_and_the_new_name_is_listed() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "willow-3").await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": "  willow  "}))).await;
    assert_eq!(status, 204);

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["name"], "willow");
}

#[tokio::test]
async fn renaming_to_a_live_nodes_name_or_to_nothing_is_refused_with_a_reason() {
    let server = Server::start().await;
    server.join("node", "laptop").await;
    let (id, _) = server.join("node", "desk").await;

    let (taken, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": "laptop"}))).await;
    assert_eq!(taken, 409);
    assert_eq!(body["message"], "Another node is already called laptop. Choose a different name.");

    let (empty, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": " "}))).await;
    assert_eq!(empty, 400);
    assert_eq!(body["message"], "Enter a name for this node.");
}

#[tokio::test]
async fn a_machine_joining_again_after_a_revoke_gets_its_old_name_back() {
    let server = Server::start().await;
    let (old, _) = server.join("node", "willow").await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/nodes/{old}"))).await;
    assert_eq!(status, 204);

    server.join("node", "willow").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes.as_array().unwrap().len(), 1);
    assert_eq!(nodes[0]["name"], "willow");
}

#[tokio::test]
async fn the_modes_models_and_settings_a_session_offers_are_kept_on_the_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    let conversation_id = link.expect_start_session().await;
    assert_eq!(conversation_id, created.id);

    let models = json!({"currentModelId": "opus", "availableModels": [{"modelId": "opus", "name": "Opus 5.5"}, {"modelId": "sonnet", "name": "Sonnet 5.5"}]});
    let settings = json!([{"id": "effort", "name": "Effort", "currentValue": "medium", "options": [{"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}]}]);
    link.send(NodeToControl::SessionStarted { conversation_id, acp_session_id: "s1".into(), modes: None, models: Some(models.clone()), config_options: Some(settings.clone()) }).await;

    let user = server.user.clone();
    let conversation = common::eventually("the session options", || async {
        let (_, body) = user.json(user.get(&format!("/api/v1/conversations/{conversation_id}"))).await;

        (!body["options"].is_null()).then_some(body)
    })
    .await;

    assert_eq!(conversation["model"], "opus");
    assert_eq!(conversation["options"]["models"], models);
    assert_eq!(conversation["options"]["config_options"], settings);
}

#[tokio::test]
async fn a_node_icon_can_be_chosen_from_the_set_and_anything_else_is_refused() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "pi").await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"icon": "chip"}))).await;
    assert_eq!(status, 204);

    let (refused, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"icon": "rocket"}))).await;
    assert_eq!(refused, 400);
    assert_eq!(body["message"], "rocket is not a node icon. Choose one of server, laptop, desktop, cloud, terminal, chip.");

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["icon"], "chip");
    assert_eq!(nodes[0]["name"], "pi");
}
