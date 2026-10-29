use crate::common::Server;
use mycelium_proto::link::{NodeToControl, ResourceSample};
use serde_json::json;
use uuid::Uuid;

fn sample(cpu_percent: f32, memory_available_bytes: u64) -> ResourceSample {
    ResourceSample {
        at_ms: chrono::Utc::now().timestamp_millis(),
        available: true,
        cpu_percent,
        cores: 4,
        load: [0.0; 3],
        memory_total_bytes: 100,
        memory_available_bytes,
        swap_total_bytes: 0,
        swap_free_bytes: 0,
        disks: vec![],
        processes: vec![],
        ports: vec![],
        docker: None,
    }
}

async fn save_preferences(server: &Server, preferences: serde_json::Value) {
    let (status, body) = server.user.json(server.user.put("/api/v1/agent-settings").json(&json!({"node_preferences": preferences}))).await;
    assert_eq!(status, 200, "{body}");
}

async fn start_on_any_node(server: &Server) -> (u16, serde_json::Value) {
    server.user.json(server.user.post("/api/v1/conversations").json(&json!({"node_id": Uuid::nil(), "agent": "claude", "cwd": "/work/app"}))).await
}

#[tokio::test]
async fn any_node_starts_on_the_node_with_the_most_free_cpu_and_memory_and_says_so_in_the_thread() {
    let server = Server::start().await;
    let mut atlas = server.online_node("atlas").await;
    let mut orion = server.online_node("orion").await;
    atlas.send(NodeToControl::Resources { sample: sample(90.0, 20) }).await;
    orion.send(NodeToControl::Resources { sample: sample(10.0, 80) }).await;

    let (status, conversation) = start_on_any_node(&server).await;

    assert_eq!(status, 201, "{conversation}");
    assert_eq!(conversation["node_id"], json!(orion.id));
    orion.next().await;
    orion.expect_start_session().await;
    let (_, events) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/events", conversation["id"].as_str().unwrap()))).await;
    assert!(events.as_array().unwrap().iter().any(|event| event["payload"]["kind"] == "any_node" && event["payload"]["node_name"] == "orion"));
}

#[tokio::test]
async fn any_node_follows_the_node_preferences_and_never_picks_a_node_set_to_never() {
    let server = Server::start().await;
    let mut atlas = server.online_node("atlas").await;
    let mut orion = server.online_node("orion").await;
    atlas.send(NodeToControl::Resources { sample: sample(10.0, 80) }).await;
    orion.send(NodeToControl::Resources { sample: sample(10.0, 80) }).await;

    save_preferences(&server, json!({orion.id.to_string(): "preferred"})).await;
    let (_, preferred) = start_on_any_node(&server).await;
    assert_eq!(preferred["node_id"], json!(orion.id));

    save_preferences(&server, json!({orion.id.to_string(): "never"})).await;
    let (_, avoided) = start_on_any_node(&server).await;
    assert_eq!(avoided["node_id"], json!(atlas.id));
}

#[tokio::test]
async fn any_node_says_what_to_do_when_every_node_is_set_to_never() {
    let server = Server::start().await;
    let mut willow = server.online_node("willow").await;
    willow.send(NodeToControl::Resources { sample: sample(10.0, 80) }).await;
    save_preferences(&server, json!({willow.id.to_string(): "never"})).await;

    let (status, body) = start_on_any_node(&server).await;

    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], json!("no_node_available"));
}

#[tokio::test]
async fn the_any_node_preview_names_the_node_it_would_pick_now() {
    let server = Server::start().await;
    let mut atlas = server.online_node("atlas").await;
    atlas.send(NodeToControl::Resources { sample: sample(10.0, 80) }).await;

    let (status, pick) = server.user.json(server.user.get("/api/v1/nodes/any")).await;

    assert_eq!(status, 200, "{pick}");
    assert_eq!(pick, json!({"node_id": atlas.id, "name": "atlas"}));
}

#[tokio::test]
async fn saving_agent_settings_without_node_preferences_leaves_the_stored_ones_alone() {
    let server = Server::start().await;
    let willow = server.online_node("willow").await;
    save_preferences(&server, json!({willow.id.to_string(): "avoid"})).await;

    let (status, settings) = server.user.json(server.user.put("/api/v1/agent-settings").json(&json!({"any_model": []}))).await;

    assert_eq!(status, 200, "{settings}");
    assert_eq!(settings["node_preferences"], json!({willow.id.to_string(): "avoid"}));
}
