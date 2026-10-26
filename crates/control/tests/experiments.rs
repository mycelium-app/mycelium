mod common;

use common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

fn flags(enabled: bool) -> Value {
    json!({"tasks": enabled, "workflows": enabled, "instructions": enabled, "details": enabled, "subagents": enabled, "walkthrough": enabled, "move_chats": enabled, "groups": enabled, "continue_elsewhere": enabled, "assistants": enabled, "memory_banks": enabled, "chat_across_groups": enabled, "voice": enabled})
}

async fn fresh() -> Server {
    let server = Server::start().await;
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();

    server
}

async fn mcp(server: &Server, token: &str, method: &str, params: Value) -> (u16, Value) {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))).await
}

#[tokio::test]
async fn experiments_default_off_are_isolated_per_person_and_survive_a_restart() {
    let server = fresh().await;
    let (status, defaults) = server.user.json(server.user.get("/api/v1/experiments")).await;
    assert_eq!((status, defaults), (200, flags(false)));
    let other = server.second_user("other@example.com").await;
    sqlx::query("delete from user_settings where owner = (select id from users where email = 'other@example.com')").execute(&server.pool).await.unwrap();
    let mut chosen = flags(false);
    chosen["tasks"] = json!(true);
    chosen["details"] = json!(true);
    let (status, saved) = server.user.json(server.user.put("/api/v1/experiments").json(&chosen)).await;
    let (_, theirs) = other.json(other.get("/api/v1/experiments")).await;
    assert_eq!((status, saved), (200, chosen.clone()));
    assert_eq!(theirs, flags(false));
    let restarted = Server::start_on(server.database_url.clone(), |_| {}).await;
    let (_, persisted) = restarted.user.json(restarted.user.get("/api/v1/experiments")).await;
    assert_eq!(persisted, chosen);
}

#[tokio::test]
async fn disabled_features_refuse_direct_requests_but_schedules_remain_available() {
    let server = fresh().await;
    let id = Uuid::new_v4();

    for path in ["/api/v1/tasks".to_string(), "/api/v1/workflows".into(), "/api/v1/instructions".into(), "/api/v1/instruction-proposals".into(), format!("/api/v1/conversations/{id}/walkthrough"), format!("/api/v1/conversations/{id}/move-plan"), format!("/api/v1/groups/{id}/instructions"), "/api/v1/group-connections".into()] {
        let (status, refused) = server.user.json(server.user.get(&path)).await;
        assert_eq!(status, 403, "{path}: {refused}");
        assert_eq!(refused["error"], "experiment_disabled");
        assert!(refused["message"].as_str().unwrap().contains("Settings"));
    }

    let (status, _) = server.user.json(server.user.get("/api/v1/schedules")).await;
    assert_eq!(status, 200);
    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/move")).json(&json!({}))).await;
    assert_eq!((status, refused["error"].clone()), (403, json!("experiment_disabled")));
}

#[tokio::test]
async fn disabled_tools_are_hidden_and_cached_tool_calls_are_refused_until_enabled() {
    let server = fresh().await;
    let (_, token) = server.join("node", "laptop").await;
    let (_, listed) = mcp(&server, &token, "tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect();

    for tool in ["propose_task", "ask_question", "start_workflow", "get_instructions", "propose_instructions_edit", "walkthrough"] {
        assert!(!names.contains(&tool), "{tool}");
        let (status, refused) = mcp(&server, &token, "tools/call", json!({"name": tool, "arguments": {}})).await;
        assert_eq!(status, 200);
        assert_eq!(refused["result"]["isError"], true);
        assert!(refused["result"]["content"][0]["text"].as_str().unwrap().contains("Settings"));
    }

    assert!(names.contains(&"propose_schedule"));
    assert!(names.contains(&"start_subagent"));
    assert!(names.contains(&"get_workflow"));
    server.user.json(server.user.put("/api/v1/experiments").json(&flags(true))).await;
    let (_, enabled) = mcp(&server, &token, "tools/list", json!({})).await;
    assert!(enabled["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "propose_task"));
    let (_, instructions) = mcp(&server, &token, "tools/call", json!({"name": "get_instructions", "arguments": {}})).await;
    assert_ne!(instructions["result"]["isError"], true);
}

#[tokio::test]
async fn disabling_and_enabling_tasks_keeps_existing_tasks() {
    let server = fresh().await;
    let (node, _) = server.join("node", "laptop").await;
    server.user.json(server.user.put("/api/v1/experiments").json(&flags(true))).await;
    let (created, task) = server.user.json(server.user.post("/api/v1/tasks").json(&json!({"title": "Keep me", "node_id": node, "agent": "claude", "cwd": "/work/app"}))).await;
    assert_eq!(created, 201, "{task}");
    server.user.json(server.user.put("/api/v1/experiments").json(&flags(false))).await;
    let (blocked, _) = server.user.json(server.user.get("/api/v1/tasks")).await;
    assert_eq!(blocked, 403);
    server.user.json(server.user.put("/api/v1/experiments").json(&flags(true))).await;
    let (_, restored) = server.user.json(server.user.get("/api/v1/tasks")).await;
    assert!(restored.as_array().unwrap().iter().any(|old| old["id"] == task["id"]));
}

#[tokio::test]
async fn standalone_subagents_can_start_and_be_read_without_enabling_workflows() {
    let server = fresh().await;
    let (_, token) = server.join("node", "laptop").await;
    let (status, standalone) = mcp(&server, &token, "tools/call", json!({"name": "start_subagent", "arguments": {"step": {"title": "Pause here", "gate": true}}})).await;
    assert_eq!(status, 200, "{standalone}");
    assert_eq!(standalone["result"]["isError"], false, "{standalone}");
    let id = sqlx::query_scalar::<_, Uuid>("select id from workflows order by created_at desc limit 1").fetch_one(&server.pool).await.unwrap();
    let (status, read) = mcp(&server, &token, "tools/call", json!({"name": "get_workflow", "arguments": {"id": id}})).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["result"]["isError"], false);
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from workflows where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    let multi = Uuid::new_v4();
    sqlx::query("insert into workflows (id, owner, title, status, plan, steps) values ($1, $2, 'Existing workflow', 'paused', $3, '[]')").bind(multi).bind(owner).bind(json!({"title": "Existing workflow", "steps": [{"key": "one", "title": "One", "gate": true}, {"key": "two", "title": "Two", "gate": true}]})).execute(&server.pool).await.unwrap();
    let (status, refused) = mcp(&server, &token, "tools/call", json!({"name": "get_workflow", "arguments": {"id": multi}})).await;
    assert!(status == 403 || refused["result"]["isError"] == true, "{refused}");
    assert!(refused.to_string().contains("Enable the Workflows experiment"));
    let (status, disabled) = server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title": "Disabled", "steps": [{"key": "gate", "title": "Gate", "gate": true}]}))).await;
    assert_eq!((status, disabled["error"].clone()), (403, json!("experiment_disabled")));
}

#[tokio::test]
async fn experiments_left_out_of_a_save_are_turned_off() {
    let server = fresh().await;

    let (status, saved) = server.user.json(server.user.put("/api/v1/experiments").json(&json!({"workflows": true}))).await;

    let mut expected = flags(false);
    expected["workflows"] = json!(true);
    assert_eq!((status, saved), (200, expected));
}
