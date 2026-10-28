use axum::Json;
use axum::Router;
use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::collections::HashMap;

const GITHUB_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const NARROW_TOKEN: &str = "narrow-scope-token";
const GITLAB_TOKEN: &str = "glpat-0123456789abcdefghij";

fn authorised(headers: &HeaderMap, scheme: &str, token: &str) -> bool {
    match scheme {
        "bearer" => headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {token}")),
        _ => headers.get("private-token").and_then(|value| value.to_str().ok()) == Some(token),
    }
}

fn fake_github() -> Router {
    async fn user(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        if authorised(&headers, "bearer", NARROW_TOKEN) {
            return Err(StatusCode::FORBIDDEN);
        }

        authorised(&headers, "bearer", GITHUB_TOKEN).then(|| Json(json!({"login": "samexample"}))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn orgs(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        authorised(&headers, "bearer", GITHUB_TOKEN).then(|| Json(json!([{"login": "acme-org"}, {"login": "immich-app"}]))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn repositories(headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Result<Json<Value>, StatusCode> {
        if !authorised(&headers, "bearer", GITHUB_TOKEN) {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let page: usize = query.get("page").and_then(|page| page.parse().ok()).unwrap_or(1);
        let mut all = vec![
            json!({"full_name": "acme-org/acme-backups", "description": "Backups", "private": true, "default_branch": "main"}),
            json!({"full_name": "immich-app/immich", "description": null, "private": false, "default_branch": "main"}),
        ];
        all.extend((0..120).map(|number| json!({"full_name": format!("samexample/filler-{number}"), "private": false})));

        Ok(Json(json!(all.into_iter().skip((page - 1) * 100).take(100).collect::<Vec<_>>())))
    }

    Router::new().route("/api/v3/user", get(user)).route("/api/v3/user/orgs", get(orgs)).route("/api/v3/user/repos", get(repositories))
}

fn fake_gitlab() -> Router {
    async fn user(headers: HeaderMap) -> Result<Json<Value>, Response> {
        if authorised(&headers, "private", NARROW_TOKEN) {
            return Err((StatusCode::FORBIDDEN, Json(json!({"error": "insufficient_scope", "error_description": "Request was rejected"}))).into_response());
        }

        if authorised(&headers, "private", "blocked-token") {
            return Err((StatusCode::FORBIDDEN, Json(json!({"error": "access_denied"}))).into_response());
        }

        authorised(&headers, "private", GITLAB_TOKEN).then(|| Json(json!({"username": "sam"}))).ok_or(StatusCode::UNAUTHORIZED.into_response())
    }

    async fn groups(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        authorised(&headers, "private", GITLAB_TOKEN).then(|| Json(json!([{"full_path": "slop"}, {"full_path": "homelab"}]))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn projects(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        authorised(&headers, "private", GITLAB_TOKEN).then(|| Json(json!([
            {"path_with_namespace": "slop/mycelium", "description": "Agents", "visibility": "internal", "default_branch": "main"},
            {"path_with_namespace": "slop/tools/backups", "visibility": "public"},
            {"path_with_namespace": "homelab/homelab", "visibility": "private"},
        ]))).ok_or(StatusCode::UNAUTHORIZED)
    }

    Router::new().route("/api/v4/user", get(user)).route("/api/v4/groups", get(groups)).route("/api/v4/projects", get(projects))
}

fn fake_gitlab_mcp(allowed: bool) -> Router {
    async fn accepted(headers: HeaderMap, Json(body): Json<Value>) -> Response {
        if !authorised(&headers, "bearer", GITLAB_TOKEN) {
            return StatusCode::FORBIDDEN.into_response();
        }

        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "gitlab", "version": "1"}}),
            _ => json!({"tools": [{"name": "get_issue", "description": "Get an issue", "inputSchema": {"type": "object"}}]}),
        };

        Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
    }

    async fn refused() -> StatusCode {
        StatusCode::FORBIDDEN
    }

    match allowed {
        true => fake_gitlab().route("/api/v4/mcp", post(accepted)),
        false => fake_gitlab().route("/api/v4/mcp", post(refused)),
    }
}

async fn mcp_servers(server: &Server) -> Vec<Value> {
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    listed.as_array().unwrap().clone()
}

async fn connect_without_mcp(server: &Server, base_url: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": "gitlab", "base_url": base_url, "token": GITLAB_TOKEN, "add_mcp": false}))).await
}

#[tokio::test]
async fn adding_gitlab_adds_its_mcp_server() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;

    let (status, forge) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let servers = mcp_servers(&server).await;

    assert_eq!(status, 201, "{forge}");
    assert_eq!(servers.len(), 1);
    assert_eq!((servers[0]["name"].clone(), servers[0]["url"].clone(), servers[0]["forge_id"].clone()), (json!("GitLab"), json!(format!("{gitlab}/api/v4/mcp")), forge["id"].clone()));
    assert_eq!(forge["mcp_server_id"], servers[0]["id"]);
    assert_eq!(servers[0]["tools"][0]["name"], "get_issue");
    assert!(!servers.iter().any(|listed| listed.to_string().contains(GITLAB_TOKEN)));
}

#[tokio::test]
async fn add_mcp_false_adds_none() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;

    let (status, forge) = connect_without_mcp(&server, &gitlab).await;

    assert_eq!(status, 201, "{forge}");
    assert_eq!(mcp_servers(&server).await.len(), 0);
    assert_eq!(forge.get("mcp_server_id"), None);
}

#[tokio::test]
async fn a_refused_token_still_connects_the_forge_with_an_mcp_error() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(false)).await;

    let (status, forge) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    assert_eq!(status, 201, "{forge}");
    assert_eq!(forge["mcp_error"], "GitLab's MCP server refused this token. It needs a fine-grained token with the MCP permission, or sign in instead.");
    assert_eq!(mcp_servers(&server).await.len(), 0);
    let (_, listed) = server.user.json(server.user.get("/api/v1/forges")).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn reconnecting_updates_the_linked_server_instead_of_adding_another() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;
    connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    let (_, again) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let servers = mcp_servers(&server).await;

    assert_eq!(servers.len(), 1);
    assert_eq!(again["mcp_server_id"], servers[0]["id"]);
}

#[tokio::test]
async fn disconnecting_removes_the_linked_server() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;
    let (_, forge) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    let status = server.user.delete(&format!("/api/v1/forges/{}", forge["id"].as_str().unwrap())).send().await.unwrap().status().as_u16();

    assert_eq!(status, 204);
    assert_eq!(mcp_servers(&server).await.len(), 0);
}

#[tokio::test]
async fn github_enterprise_is_not_offered_an_mcp_server() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;

    let (status, forge) = connect(&server, "github", &github, GITHUB_TOKEN).await;

    assert_eq!(status, 201, "{forge}");
    assert_eq!(mcp_servers(&server).await.len(), 0);
    assert_eq!(forge.get("mcp_error"), None);
}

#[tokio::test]
async fn a_github_forge_never_starts_the_mcp_oauth_flow_that_github_cannot_register_for() {
    let registrations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = registrations.clone();
    let rejecting = Router::new().route("/register", post(move || async move {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        StatusCode::BAD_REQUEST
    }));
    let _mcp = serve(rejecting).await;
    let server = Server::start().await;
    let github = serve(fake_github()).await;
    let (_, forge) = connect(&server, "github", &github, GITHUB_TOKEN).await;

    let (status, refused) = server.user.json(server.user.post("/api/v1/chat-tool-servers/oauth").json(&json!({"name": "GitHub", "url": "https://api.githubcopilot.com/mcp/", "forge_id": forge["id"]}))).await;

    assert_eq!((status, refused["error"].as_str()), (400, Some("oauth_unsupported")));
    assert!(refused["message"].as_str().unwrap().contains("fine-grained token"), "{refused}");
    assert_eq!(registrations.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_forge_connected_without_its_mcp_server_adds_it_later_with_its_token() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;
    let (_, forge) = connect_without_mcp(&server, &gitlab).await;
    assert!(mcp_servers(&server).await.is_empty());

    let (status, linked) = server.user.json(server.user.post(&format!("/api/v1/forges/{}/mcp", forge["id"].as_str().unwrap()))).await;

    assert_eq!(status, 200, "{linked}");
    assert!(linked["mcp_error"].is_null(), "{linked}");
    assert_eq!(mcp_servers(&server).await[0]["tools"][0]["name"], "get_issue");
}

#[tokio::test]
async fn a_forge_linked_server_with_no_token_of_its_own_uses_the_forge_token() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab_mcp(true)).await;
    connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    let linked = mcp_servers(&server).await[0]["id"].as_str().unwrap().to_string();
    sqlx::query("update chat_tool_servers set bearer_token = null where id = $1::uuid").bind(&linked).execute(&server.pool).await.unwrap();

    let (status, refreshed) = server.user.json(server.user.post(&format!("/api/v1/chat-tool-servers/{linked}/refresh"))).await;

    assert_eq!(status, 200, "{refreshed}");
    assert_eq!(refreshed["tools"][0]["name"], "get_issue");
}

async fn connect(server: &Server, kind: &str, base_url: &str, token: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base_url, "token": token}))).await
}

#[tokio::test]
async fn connecting_github_returns_the_account_and_a_hint_but_never_the_token() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;

    let (status, forge) = connect(&server, "github", &github, GITHUB_TOKEN).await;

    assert_eq!(status, 201, "{forge}");
    assert_eq!((forge["kind"].clone(), forge["account"].clone(), forge["base_url"].clone()), (json!("github"), json!("samexample"), json!(github)));
    assert_eq!(forge["organisations"], json!(["samexample", "acme-org", "immich-app"]));
    assert_eq!(forge["token_hint"], "••••wxyz");

    let (_, listed) = server.user.json(server.user.get("/api/v1/forges")).await;
    let response = server.user.get("/api/v1/repositories").send().await.unwrap().text().await.unwrap();

    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(!forge.to_string().contains(GITHUB_TOKEN) && !listed.to_string().contains(GITHUB_TOKEN) && !response.contains(GITHUB_TOKEN));
}

#[tokio::test]
async fn a_token_the_forge_refuses_is_rejected_and_nothing_is_stored() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;

    let (status, error) = connect(&server, "github", &github, "ghp_wrong_wrong_wrong").await;

    assert_eq!((status, error["error"].clone()), (400, json!("forge_token_rejected")));
    assert!(error["message"].as_str().unwrap().starts_with("GitHub did not accept this token"));
    let (_, listed) = server.user.json(server.user.get("/api/v1/forges")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn an_expired_token_and_a_token_without_scope_are_told_apart() {
    let server = Server::start().await;
    let gitlab = serve(fake_gitlab()).await;
    let github = serve(fake_github()).await;

    let (_, expired) = connect(&server, "gitlab", &gitlab, "glpat-expired").await;
    let (status, gitlab_scope) = connect(&server, "gitlab", &gitlab, NARROW_TOKEN).await;
    let (_, github_scope) = connect(&server, "github", &github, NARROW_TOKEN).await;
    let (_, other) = connect(&server, "gitlab", &gitlab, "blocked-token").await;

    assert_eq!(expired["error"], "forge_token_rejected");
    assert!(expired["message"].as_str().unwrap().contains("has not expired"));
    assert_eq!((status, gitlab_scope["error"].clone()), (400, json!("forge_token_scope")));
    assert!(gitlab_scope["message"].as_str().unwrap().starts_with("This token cannot read your account. Create a token with the api scope (read_api is enough to browse;"));
    assert_eq!(github_scope["error"], "forge_token_scope");
    assert!(github_scope["message"].as_str().unwrap().contains("classic token with the repo scope"));
    assert_eq!(other["error"], "forge_failed");
}

#[tokio::test]
async fn an_unreachable_forge_and_a_malformed_address_are_refused_with_what_to_fix() {
    let server = Server::start().await;

    let (status, unreachable) = connect(&server, "gitlab", "http://127.0.0.1:9", GITLAB_TOKEN).await;
    let (_, malformed) = connect(&server, "gitlab", "git.example.com", GITLAB_TOKEN).await;
    let (_, empty) = connect(&server, "gitlab", "http://127.0.0.1:9", "  ").await;

    assert_eq!((status, unreachable["error"].clone()), (400, json!("forge_unreachable")));
    assert_eq!(malformed["error"], "base_url_invalid");
    assert_eq!(empty["error"], "token_required");
}

#[tokio::test]
async fn repositories_from_every_forge_are_listed_with_their_organisations_and_can_be_filtered() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;
    let gitlab = serve(fake_gitlab()).await;
    let (_, github_forge) = connect(&server, "github", &github, GITHUB_TOKEN).await;
    let (_, gitlab_forge) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    let (_, all) = server.user.json(server.user.get("/api/v1/repositories")).await;
    let names: Vec<&str> = all["repositories"].as_array().unwrap().iter().map(|repository| repository["full_name"].as_str().unwrap()).collect();

    assert_eq!(names.len(), 122 + 3, "the second page of GitHub repositories is read too");
    assert!(names.contains(&"acme-org/acme-backups") && names.contains(&"slop/mycelium") && names.contains(&"samexample/filler-119"));
    assert_eq!(all["failures"], json!([]));
    let organisations: Vec<(&str, &str)> = all["organisations"].as_array().unwrap().iter().map(|organisation| (organisation["forge_id"].as_str().unwrap(), organisation["name"].as_str().unwrap())).collect();
    assert!(organisations.contains(&(github_forge["id"].as_str().unwrap(), "acme-org")) && organisations.contains(&(gitlab_forge["id"].as_str().unwrap(), "homelab")));

    let backups = all["repositories"].as_array().unwrap().iter().find(|repository| repository["full_name"] == "acme-org/acme-backups").unwrap();
    assert_eq!((backups["owner"].clone(), backups["name"].clone(), backups["private"].clone(), backups["forge_id"].clone()), (json!("acme-org"), json!("acme-backups"), json!(true), github_forge["id"].clone()));
    let tools = all["repositories"].as_array().unwrap().iter().find(|repository| repository["full_name"] == "slop/tools/backups").unwrap();
    assert_eq!((tools["owner"].clone(), tools["private"].clone()), (json!("slop/tools"), json!(false)));

    let (_, slop) = server.user.json(server.user.get("/api/v1/repositories?organisation=slop")).await;
    let slop_names: Vec<&str> = slop["repositories"].as_array().unwrap().iter().map(|repository| repository["full_name"].as_str().unwrap()).collect();
    assert_eq!(slop_names, vec!["slop/mycelium", "slop/tools/backups"]);

    let (_, searched) = server.user.json(server.user.get("/api/v1/repositories?q=BACKUPS")).await;
    assert_eq!(searched["repositories"].as_array().unwrap().len(), 2);

    let one_forge = format!("/api/v1/repositories?forge={}&q=immich", github_forge["id"].as_str().unwrap());
    let (_, only) = server.user.json(server.user.get(&one_forge)).await;
    assert_eq!(only["repositories"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_forge_that_fails_is_reported_beside_the_repositories_of_the_others() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;
    let gitlab = serve(fake_gitlab()).await;
    connect(&server, "github", &github, GITHUB_TOKEN).await;
    let (_, gitlab_forge) = connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    sqlx::query("update forges set token = 'expired' where kind = 'gitlab'").execute(&server.pool).await.unwrap();

    let (status, listed) = server.user.json(server.user.get("/api/v1/repositories")).await;

    assert_eq!(status, 200);
    assert_eq!(listed["repositories"].as_array().unwrap().len(), 122);
    assert_eq!(listed["failures"][0]["forge_id"], gitlab_forge["id"]);
    assert!(listed["failures"][0]["message"].as_str().unwrap().starts_with("GitLab did not accept this token"));
}

#[tokio::test]
async fn connecting_the_same_account_again_replaces_its_token_instead_of_adding_a_row() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;
    connect(&server, "github", &github, GITHUB_TOKEN).await;

    let (status, _) = connect(&server, "github", &github, GITHUB_TOKEN).await;

    assert_eq!(status, 201);
    let (_, listed) = server.user.json(server.user.get("/api/v1/forges")).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn disconnecting_removes_the_forge_and_only_its_owner_can_do_it() {
    let server = Server::start().await;
    let github = serve(fake_github()).await;
    let (_, forge) = connect(&server, "github", &github, GITHUB_TOKEN).await;
    let id = forge["id"].as_str().unwrap();
    let other = server.second_user("other@example.com").await;

    let (_, theirs) = other.json(other.get("/api/v1/forges")).await;
    let (refused, _) = other.json(other.delete(&format!("/api/v1/forges/{id}"))).await;
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/forges/{id}"))).await;
    let (again, _) = server.user.json(server.user.delete(&format!("/api/v1/forges/{id}"))).await;

    assert_eq!((theirs, refused, removed, again), (json!([]), 404, 204, 404));
    let (_, listed) = server.user.json(server.user.get("/api/v1/forges")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn forges_need_a_signed_in_user() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, _) = anonymous.json(anonymous.get("/api/v1/forges")).await;

    assert_eq!(status, 401);
}

#[tokio::test]
async fn repositories_of_every_forge_are_merged_newest_first_and_those_without_a_time_come_last() {
    async fn github(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        authorised(&headers, "bearer", GITHUB_TOKEN).then(|| Json(json!([
            {"full_name": "acme-org/old", "pushed_at": "2026-01-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
            {"full_name": "acme-org/newest", "pushed_at": "2026-10-20T09:00:00Z"},
            {"full_name": "acme-org/untimed"},
        ]))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn gitlab(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        authorised(&headers, "private", GITLAB_TOKEN).then(|| Json(json!([
            {"path_with_namespace": "slop/middle", "last_activity_at": "2026-06-01T12:00:00.000Z"},
            {"path_with_namespace": "slop/recent", "last_activity_at": "2026-10-10T12:00:00.000Z"},
        ]))).ok_or(StatusCode::UNAUTHORIZED)
    }

    let server = Server::start().await;
    let github = serve(Router::new().route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) })).route("/api/v3/user/orgs", get(|| async { Json(json!([])) })).route("/api/v3/user/repos", get(github))).await;
    let gitlab = serve(Router::new().route("/api/v4/user", get(|| async { Json(json!({"username": "sam"})) })).route("/api/v4/groups", get(|| async { Json(json!([])) })).route("/api/v4/projects", get(gitlab))).await;
    connect(&server, "github", &github, GITHUB_TOKEN).await;
    connect(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    let (_, listed) = server.user.json(server.user.get("/api/v1/repositories")).await;
    let names: Vec<&str> = listed["repositories"].as_array().unwrap().iter().map(|repository| repository["full_name"].as_str().unwrap()).collect();

    assert_eq!(names, vec!["acme-org/newest", "slop/recent", "slop/middle", "acme-org/old", "acme-org/untimed"]);
}
