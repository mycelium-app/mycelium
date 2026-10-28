use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "1|coolify-token-secret";
const SECRET_VALUE: &str = "hunter2-do-not-leak";

#[derive(Default)]
struct FakeCoolify {
    created: Mutex<Vec<Value>>,
    envs: Mutex<Vec<Value>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

async fn version(headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"message": "Unauthenticated."}))).into_response();
    }

    Json(json!("4.0.0")).into_response()
}

async fn projects() -> Json<Value> {
    Json(json!([{"uuid": "p1", "name": "Agents"}]))
}

async fn servers() -> Json<Value> {
    Json(json!([{"uuid": "s1", "name": "Main"}]))
}

async fn one_server() -> Json<Value> {
    Json(json!({"uuid": "s1", "settings": {"wildcard_domain": "https://apps.example.com"}}))
}

async fn environments() -> Json<Value> {
    Json(json!([{"id": 7, "name": "production"}]))
}

async fn applications() -> Json<Value> {
    Json(json!([
        {"uuid": "a1", "name": "notes", "status": "running:healthy", "fqdn": "https://notes.apps.example.com", "environment_id": 7, "git_repository": "slop/notes", "git_branch": "main"},
        {"uuid": "other", "name": "elsewhere", "status": "running:healthy", "environment_id": 99}
    ]))
}

async fn one_application() -> Json<Value> {
    Json(json!({"uuid": "a1", "name": "notes", "status": "running:healthy", "fqdn": "https://notes.apps.example.com", "environment_id": 7, "git_repository": "slop/notes", "git_branch": "main", "build_pack": "nixpacks"}))
}

async fn create_public(State(fake): State<Arc<FakeCoolify>>, Json(body): Json<Value>) -> Response {
    fake.created.lock().unwrap().push(body);

    (StatusCode::CREATED, Json(json!({"uuid": "a1"}))).into_response()
}

async fn deploy() -> Json<Value> {
    Json(json!({"deployments": [{"message": "queued", "resource_uuid": "a1", "deployment_uuid": "d1"}]}))
}

async fn deployments() -> Json<Value> {
    Json(json!({"count": 1, "deployments": [{"deployment_uuid": "d1", "status": "finished", "commit": "abcdef1234", "commit_message": "Add notes", "created_at": "2026-10-28T10:00:00Z", "updated_at": "2026-10-28T10:01:30Z", "is_webhook": false}]}))
}

async fn deployment() -> Json<Value> {
    let logs = json!([
        {"output": "Importing slop/notes:main", "timestamp": "2026-10-28T10:00:05Z", "hidden": false},
        {"output": "Building docker image", "timestamp": "2026-10-28T10:00:20Z", "hidden": false},
        {"output": "Starting new application", "timestamp": "2026-10-28T10:01:00Z", "hidden": false}
    ]);

    Json(json!({"deployment_uuid": "d1", "application_id": "a1", "application_name": "notes", "status": "finished", "commit": "abcdef1234", "created_at": "2026-10-28T10:00:00Z", "updated_at": "2026-10-28T10:01:30Z", "logs": logs.to_string()}))
}

async fn runtime_logs() -> Response {
    (StatusCode::FORBIDDEN, Json(json!({"message": "Missing required permissions: read:sensitive"}))).into_response()
}

async fn list_envs(State(fake): State<Arc<FakeCoolify>>) -> Json<Value> {
    Json(Value::Array(fake.envs.lock().unwrap().clone()))
}

async fn save_env(State(fake): State<Arc<FakeCoolify>>, Json(body): Json<Value>) -> Response {
    let mut envs = fake.envs.lock().unwrap();
    let secret = body["is_shown_once"] == json!(true);

    envs.retain(|env| env["key"] != body["key"]);
    envs.push(json!({"uuid": format!("env-{}", body["key"].as_str().unwrap()), "key": body["key"], "value": if secret { Value::Null } else { body["value"].clone() }, "is_shown_once": secret, "is_buildtime": false, "is_preview": false, "updated_at": "2026-10-28T10:05:00Z"}));

    (StatusCode::CREATED, Json(json!({"uuid": "env"}))).into_response()
}

async fn remove_env(State(fake): State<Arc<FakeCoolify>>, Path((_, env)): Path<(String, String)>) -> Json<Value> {
    fake.envs.lock().unwrap().retain(|candidate| candidate["uuid"] != env.as_str());

    Json(json!({"message": "deleted"}))
}

async fn stop() -> Json<Value> {
    Json(json!({"message": "Application stopping request queued."}))
}

async fn fake_coolify() -> (String, Arc<FakeCoolify>) {
    let fake = Arc::new(FakeCoolify::default());
    let router = Router::new()
        .route("/api/v1/version", get(version))
        .route("/api/v1/projects", get(projects))
        .route("/api/v1/projects/p1/environments", get(environments))
        .route("/api/v1/servers", get(servers))
        .route("/api/v1/servers/s1", get(one_server))
        .route("/api/v1/applications", get(applications))
        .route("/api/v1/applications/public", post(create_public))
        .route("/api/v1/applications/{uuid}", get(one_application))
        .route("/api/v1/applications/{uuid}/logs", get(runtime_logs))
        .route("/api/v1/applications/{uuid}/envs", get(list_envs).post(save_env).patch(save_env))
        .route("/api/v1/applications/{uuid}/envs/{env}", delete(remove_env))
        .route("/api/v1/applications/{uuid}/stop", post(stop))
        .route("/api/v1/deploy", post(deploy))
        .route("/api/v1/deployments/applications/{uuid}", get(deployments))
        .route("/api/v1/deployments/{uuid}", get(deployment))
        .with_state(fake.clone());

    (serve(router).await, fake)
}

async fn add_coolify(server: &Server, base: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Coolify", "kind": "coolify", "url": base, "bearer_token": TOKEN, "settings": {"project_uuid": "p1", "server_uuid": "s1"}}))).await
}

fn mcp(tool: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("coolify__{tool}"), "arguments": arguments}})
}

fn text(result: &Value) -> String {
    result["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn the_setup_test_lists_the_projects_and_servers_with_the_wildcard_domain() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;

    let (status, found) = server.user.json(server.user.post("/api/v1/chat-tool-servers/coolify-test").json(&json!({"url": base, "token": TOKEN}))).await;

    assert_eq!(status, 200, "{found}");
    assert_eq!(found["projects"][0]["name"], "Agents");
    assert_eq!(found["servers"][0]["wildcard_domain"], "https://apps.example.com");
}

#[tokio::test]
async fn the_setup_test_says_how_to_fix_a_refused_token_and_an_unreachable_address() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;
    let test = |url: &str, token: &str| server.user.json(server.user.post("/api/v1/chat-tool-servers/coolify-test").json(&json!({"url": url, "token": token})));

    let (refused, token) = test(&base, "wrong").await;
    let (unreachable, gone) = test("http://127.0.0.1:9", TOKEN).await;

    assert_eq!((refused, token["error"].clone()), (400, json!("coolify_token")));
    assert!(token["message"].as_str().unwrap().ends_with("did not accept the API token. Check it has the read, write and deploy permissions, or create a new one."));
    assert_eq!((unreachable, gone["error"].clone()), (400, json!("coolify_address")));
    assert!(gone["message"].as_str().unwrap().starts_with("Could not reach 127.0.0.1. Check the address and that Coolify is running."));
}

#[tokio::test]
async fn the_account_keeps_where_apps_go_and_never_returns_the_token() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;

    let (status, added) = add_coolify(&server, &base).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 201, "{added}");
    assert_eq!((added["settings"]["project_name"].clone(), added["settings"]["server_name"].clone()), (json!("Agents"), json!("Main")));
    assert!(!stored.contains(TOKEN));
}

#[tokio::test]
async fn there_are_no_apps_without_a_coolify_account_and_only_the_projects_apps_with_one() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;

    let (_, none) = server.user.json(server.user.get("/api/v1/apps")).await;
    add_coolify(&server, &base).await;
    let (_, some) = server.user.json(server.user.get("/api/v1/apps")).await;

    assert_eq!(none["apps"], json!([]));
    assert_eq!(some["apps"].as_array().unwrap().len(), 1);
    assert_eq!((some["apps"][0]["name"].clone(), some["apps"][0]["status"].clone()), (json!("notes"), json!("running")));
}

#[tokio::test]
async fn creating_an_app_asks_first_and_records_the_conversation_that_built_it() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_coolify().await;
    add_coolify(&server, &base).await;
    let conversation = server.node_conversation(&node.token).await;

    let body = mcp("create_app", json!({"name": "Notes", "git_repository": "https://git.example.com/slop/notes"}));
    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "approve").await;
    });
    let (_, apps) = server.user.json(server.user.get("/api/v1/apps")).await;
    let created = fake.created.lock().unwrap().clone();

    assert!(text(&reply).contains("\"deployment_uuid\":\"d1\""), "{reply}");
    assert_eq!(created[0]["domains"], "https://notes.apps.example.com");
    assert_eq!(apps["apps"][0]["conversation_id"], json!(conversation.to_string()));
}

#[tokio::test]
async fn deployments_say_who_triggered_them_the_agent_or_the_person() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_coolify().await;
    add_coolify(&server, &base).await;

    server.approved_agent_mcp(&node.token, mcp("deploy", json!({"app": "a1"}))).await;
    let (status, _) = server.user.json(server.user.post("/api/v1/apps/a1/redeploy").json(&json!({}))).await;
    let (_, card) = server.user.json(server.user.get("/api/v1/app-deployments/d1")).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/apps/a1/deployments")).await;

    assert_eq!(status, 200);
    assert_eq!(listed[0]["triggered_by"], "agent");
    assert_eq!(listed[0]["duration_seconds"], 90);
    assert_eq!(card["status"], "deployed");
    assert_eq!(card["steps"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn a_secret_variable_is_hidden_in_the_approval_and_never_read_back() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_coolify().await;
    add_coolify(&server, &base).await;
    let conversation = server.node_conversation(&node.token).await;

    let body = mcp("set_env", json!({"app": "a1", "key": "API_KEY", "value": SECRET_VALUE, "secret": true}));
    let (_, shown) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "approve").await;

        request
    });
    let (_, envs) = server.user.json(server.user.get("/api/v1/apps/a1/envs")).await;
    let everything = server.user.get("/api/v1/apps/a1/envs").send().await.unwrap().text().await.unwrap();

    assert_eq!(shown["toolCall"]["rawInput"]["value"], "Secret, hidden");
    assert_eq!((envs[0]["key"].clone(), envs[0]["secret"].clone(), envs[0]["value"].clone(), envs[0]["changed_by"].clone()), (json!("API_KEY"), json!(true), Value::Null, json!("agent")));
    assert!(!everything.contains(SECRET_VALUE));
}

#[tokio::test]
async fn saving_a_variable_without_a_value_says_which_one_needs_it_and_a_changed_one_is_stale_until_redeployed() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;
    add_coolify(&server, &base).await;

    let (empty, refusal) = server.user.json(server.user.put("/api/v1/apps/a1/envs/PORT").json(&json!({"value": "", "secret": false, "build_time": false}))).await;
    server.user.json(server.user.post("/api/v1/apps/a1/redeploy").json(&json!({}))).await;
    sqlx::query("update coolify_deployments set created_at = now() - interval '1 hour'").execute(&server.pool).await.unwrap();
    server.user.json(server.user.put("/api/v1/apps/a1/envs/PORT").json(&json!({"value": "8080", "secret": false, "build_time": false}))).await;
    let (_, envs) = server.user.json(server.user.get("/api/v1/apps/a1/envs")).await;

    assert_eq!((empty, refusal["message"].clone()), (400, json!("Enter a value for PORT")));
    assert_eq!((envs[0]["value"].clone(), envs[0]["stale"].clone(), envs[0]["changed_by"].clone()), (json!("8080"), json!(true), json!("person")));
}

#[tokio::test]
async fn runtime_logs_explain_the_missing_permission_while_build_logs_are_read() {
    let server = Server::start().await;
    let (base, _) = fake_coolify().await;
    add_coolify(&server, &base).await;

    let (_, runtime) = server.user.json(server.user.get("/api/v1/apps/a1/logs?kind=runtime")).await;
    let (_, build) = server.user.json(server.user.get("/api/v1/apps/a1/logs?kind=build&deployment=d1")).await;

    assert_eq!(runtime["available"], false);
    assert!(runtime["message"].as_str().unwrap().contains("read:sensitive"));
    assert_eq!(build["lines"][1], "Building docker image");
}

#[tokio::test]
async fn stopping_an_app_asks_first() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, _) = fake_coolify().await;
    add_coolify(&server, &base).await;
    let conversation = server.node_conversation(&node.token).await;

    let body = mcp("stop", json!({"app": "a1"}));
    let (reply, asked) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "deny").await;

        request["toolCall"]["title"].clone()
    });

    assert_eq!(asked, json!("coolify__stop"));
    assert_eq!(reply["result"]["isError"], true);
}
