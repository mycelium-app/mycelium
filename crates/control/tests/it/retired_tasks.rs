use crate::common::Server;
use serde_json::{Value, json};
use uuid::Uuid;

const RETIRED_TOOLS: [&str; 6] = ["propose_task", "ask_question", "list_tasks", "approve_task", "decline_task", "close_tasks"];

async fn mcp(server: &Server, token: &str, method: &str, params: Value) -> (u16, Value) {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))).await
}

#[tokio::test]
async fn the_task_tools_are_not_listed() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let (_, listed) = mcp(&server, &token, "tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect();

    for tool in RETIRED_TOOLS {
        assert!(!names.contains(&tool), "{tool}");
    }

    assert!(names.contains(&"propose_schedule"));
}

#[tokio::test]
async fn calling_a_task_tool_is_an_unknown_tool_error() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    for tool in RETIRED_TOOLS {
        let (_, reply) = mcp(&server, &token, "tools/call", json!({"name": tool, "arguments": {}})).await;

        assert!(reply["error"]["message"].as_str().unwrap().starts_with("Unknown tool"), "{tool}: {reply}");
    }
}

#[tokio::test]
async fn the_task_routes_are_gone() {
    let server = Server::start().await;
    let id = Uuid::new_v4();

    for path in ["/api/v1/tasks".to_string(), format!("/api/v1/tasks/{id}/start")] {
        let (get, _) = server.user.json(server.user.get(&path)).await;
        let (post, _) = server.user.json(server.user.post(&path).json(&json!({}))).await;

        assert_eq!((get, post), (404, 404), "{path}");
    }
}

#[tokio::test]
async fn dropping_the_task_tables_works_on_a_populated_database_and_again_after_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();

    for shipped in [include_str!("../../migrations/0013_tasks.sql"), include_str!("../../migrations/0026_task_board.sql"), include_str!("../../migrations/0027_task_dependencies.sql"), include_str!("../../migrations/0157_task_closed.sql")] {
        sqlx::raw_sql(shipped).execute(&server.pool).await.unwrap();
    }

    let first = sqlx::query_scalar::<_, Uuid>("insert into tasks (owner, node_id, agent, title, status) values ($1, $2, 'claude', 'First', 'open') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into tasks (owner, node_id, agent, title, status, depends_on) values ($1, $2, 'claude', 'Second', 'proposed', $3)").bind(owner).bind(link.id).bind(vec![first]).execute(&server.pool).await.unwrap();
    sqlx::query("insert into tasks (owner, node_id, agent, title, status, options) values ($1, $2, 'claude', 'Which?', 'question', '{a,b}')").bind(owner).bind(link.id).execute(&server.pool).await.unwrap();

    let drop_tasks = include_str!("../../migrations/0166_drop_tasks.sql");
    sqlx::raw_sql(drop_tasks).execute(&server.pool).await.unwrap();
    sqlx::raw_sql(drop_tasks).execute(&server.pool).await.unwrap();

    let remaining = sqlx::query_scalar::<_, Option<String>>("select to_regclass('tasks')::text").fetch_one(&server.pool).await.unwrap();
    assert_eq!(remaining, None);
}
