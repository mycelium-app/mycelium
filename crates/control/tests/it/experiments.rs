use crate::common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

fn flags(voice: bool, kubernetes_nodes: bool) -> Value {
    json!({"tasks": true, "workflows": true, "instructions": true, "details": true, "subagents": true, "walkthrough": true, "move_chats": true, "groups": true, "continue_elsewhere": true, "assistants": true, "memory_banks": true, "chat_across_groups": true, "voice": voice, "kubernetes_nodes": kubernetes_nodes})
}

async fn mcp(server: &Server, token: &str, method: &str, params: Value) -> (u16, Value) {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))).await
}

#[tokio::test]
async fn voice_is_off_by_default_is_isolated_per_person_and_survives_a_restart() {
    let server = Server::start().await;
    let (status, defaults) = server.user.json(server.user.get("/api/v1/experiments")).await;
    assert_eq!((status, defaults), (200, flags(false, false)));
    let other = server.second_user("other@example.com").await;
    let (status, saved) = server.user.json(server.user.put("/api/v1/experiments").json(&json!({"voice": true}))).await;
    let (_, theirs) = other.json(other.get("/api/v1/experiments")).await;
    assert_eq!((status, saved), (200, flags(true, false)));
    assert_eq!(theirs, flags(false, false));
    let restarted = Server::start_on(server.database_url.clone(), |_| {}).await;
    let (_, persisted) = restarted.user.json(restarted.user.get("/api/v1/experiments")).await;
    assert_eq!(persisted, flags(true, false));
}

#[tokio::test]
async fn graduated_features_work_for_everyone_whatever_an_older_client_saves() {
    let server = Server::start().await;
    let id = Uuid::new_v4();
    server.user.json(server.user.put("/api/v1/experiments").json(&json!({"tasks": false, "workflows": false, "instructions": false, "groups": false}))).await;

    for path in ["/api/v1/tasks".to_string(), "/api/v1/workflows".into(), "/api/v1/instructions".into(), "/api/v1/instruction-proposals".into(), "/api/v1/assistants".into(), "/api/v1/memory-banks".into(), "/api/v1/group-connections".into(), format!("/api/v1/groups/{id}/instructions")] {
        let (status, body) = server.user.json(server.user.get(&path)).await;
        assert_ne!(body["error"], "experiment_disabled", "{path}: {body}");
        assert_ne!(status, 403, "{path}: {body}");
    }
}

#[tokio::test]
async fn graduated_tools_are_listed_and_callable_for_everyone() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (_, listed) = mcp(&server, &token, "tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect();

    for tool in ["propose_task", "ask_question", "start_workflow", "get_instructions", "propose_instructions_edit", "walkthrough", "start_conversation", "read_earlier_turns", "start_subagent", "get_workflow"] {
        assert!(names.contains(&tool), "{tool}");
    }

    let (_, instructions) = mcp(&server, &token, "tools/call", json!({"name": "get_instructions", "arguments": {}})).await;
    assert_ne!(instructions["result"]["isError"], true);
}

#[tokio::test]
async fn multi_step_workflows_can_be_read_without_any_experiment() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let (status, standalone) = mcp(&server, &token, "tools/call", json!({"name": "start_subagent", "arguments": {"step": {"title": "Pause here", "gate": true}}})).await;
    assert_eq!((status, standalone["result"]["isError"].clone()), (200, json!(true)), "{standalone}");
    let owner = sqlx::query_scalar::<_, Uuid>("select created_by from nodes limit 1").fetch_one(&server.pool).await.unwrap();
    let multi = Uuid::new_v4();
    sqlx::query("insert into workflows (id, owner, title, status, plan, steps) values ($1, $2, 'Existing workflow', 'paused', $3, '[]')").bind(multi).bind(owner).bind(json!({"title": "Existing workflow", "steps": [{"key": "one", "title": "One", "gate": true}, {"key": "two", "title": "Two", "gate": true}]})).execute(&server.pool).await.unwrap();

    let (status, read) = mcp(&server, &token, "tools/call", json!({"name": "get_workflow", "arguments": {"id": multi}})).await;

    assert_eq!((status, read["result"]["isError"].clone()), (200, json!(false)), "{read}");
}

#[tokio::test]
async fn kubernetes_nodes_is_off_by_default_and_saved_beside_voice() {
    let server = Server::start().await;
    let (_, defaults) = server.user.json(server.user.get("/api/v1/experiments")).await;
    assert_eq!(defaults, flags(false, false));

    let (status, saved) = server.user.json(server.user.put("/api/v1/experiments").json(&json!({"kubernetes_nodes": true}))).await;
    let (_, persisted) = server.user.json(server.user.get("/api/v1/experiments")).await;

    assert_eq!((status, saved), (200, flags(false, true)));
    assert_eq!(persisted, flags(false, true));
}
