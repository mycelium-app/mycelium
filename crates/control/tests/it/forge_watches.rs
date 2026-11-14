use crate::common::{NodeLink, Server, serve};
use crate::watches::{begun, connect, end_turn, prompt_for, quiet_for, text, tool};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const FORGE_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const GITLAB_TOKEN: &str = "glpat-0123456789abcdefghij";
const FAST: Duration = Duration::from_millis(50);

#[derive(Default)]
struct FakeForge {
    hooks_refused: bool,
    runs_refused: AtomicBool,
    runs_authorised: AtomicBool,
    hook_secret: Mutex<Option<String>>,
    runs: Mutex<Vec<Value>>,
    release: Mutex<Option<Value>>,
    release_lookups: AtomicUsize,
}

type Fake = Arc<FakeForge>;

fn github_api(fake: &Fake) -> Router {
    async fn register_hook(State(fake): State<Fake>, Path(_): Path<(String, String)>, Json(body): Json<Value>) -> StatusCode {
        if fake.hooks_refused {
            return StatusCode::FORBIDDEN;
        }

        *fake.hook_secret.lock().unwrap() = body["config"]["secret"].as_str().map(String::from);

        StatusCode::CREATED
    }

    async fn runs(State(fake): State<Fake>, Path(_): Path<(String, String)>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        if headers.contains_key("authorization") {
            fake.runs_authorised.store(true, Ordering::SeqCst);
        }

        if fake.runs_refused.load(Ordering::SeqCst) {
            return Err(StatusCode::FORBIDDEN);
        }

        Ok(Json(json!({"workflow_runs": fake.runs.lock().unwrap().clone()})))
    }

    async fn release(State(fake): State<Fake>, Path(_): Path<(String, String, String)>) -> Result<Json<Value>, StatusCode> {
        fake.release_lookups.fetch_add(1, Ordering::SeqCst);
        fake.release.lock().unwrap().clone().map(Json).ok_or(StatusCode::NOT_FOUND)
    }

    Router::new()
        .route("/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/user/orgs", get(|| async { Json(json!([])) }))
        .route("/repos/{owner}/{name}/hooks", post(register_hook))
        .route("/repos/{owner}/{name}/actions/runs", get(runs))
        .route("/repos/{owner}/{name}/releases/tags/{tag}", get(release))
        .with_state(fake.clone())
}

async fn serve_connected_github(fake: &Fake) -> String {
    serve(Router::new().nest("/api/v3", github_api(fake))).await
}

async fn serve_gitlab(fake: &Fake) -> String {
    async fn register_hook(State(fake): State<Fake>, Path(_): Path<String>, Json(body): Json<Value>) -> StatusCode {
        *fake.hook_secret.lock().unwrap() = body["token"].as_str().map(String::from);

        StatusCode::CREATED
    }

    async fn pipelines(State(fake): State<Fake>, Path(_): Path<String>) -> Json<Value> {
        Json(Value::Array(fake.runs.lock().unwrap().clone()))
    }

    let api = Router::new()
        .route("/user", get(|| async { Json(json!({"username": "sam"})) }))
        .route("/groups", get(|| async { Json(json!([])) }))
        .route("/projects/{id}/pipelines", get(pipelines))
        .route("/projects/{id}/hooks", post(register_hook))
        .with_state(fake.clone());

    serve(Router::new().nest("/api/v4", api)).await
}

fn run(id: i64, branch: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({"id": id, "name": "CI", "head_branch": branch, "head_sha": "abc123def", "status": status, "conclusion": conclusion, "html_url": format!("https://github.example.com/acme/app/actions/runs/{id}")})
}

fn sign(secret: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();

    mac.update(body.as_bytes());

    let digest: String = mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect();

    format!("sha256={digest}")
}

async fn connect_forge(server: &Server, kind: &str, base_url: &str, token: &str) -> Uuid {
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": kind, "base_url": base_url, "token": token, "add_mcp": false}))).await;

    assert_eq!(status, 201, "{forge}");

    forge["id"].as_str().unwrap().parse().unwrap()
}

async fn github_webhook(server: &Server, forge: Uuid, event: &str, payload: &Value, signature: Option<String>) -> u16 {
    let body = payload.to_string();
    let secret = secret_of(server, forge).await;
    let signature = signature.unwrap_or_else(|| sign(&secret, &body));
    let sent = server.anonymous().post(&format!("/api/v1/forges/{forge}/webhook")).header("x-github-event", event).header("x-hub-signature-256", signature).body(body).send().await.unwrap();

    sent.status().as_u16()
}

async fn secret_of(server: &Server, forge: Uuid) -> String {
    let stored = sqlx::query_scalar::<_, Option<String>>("select webhook_secret from forges where id = $1").bind(forge).fetch_one(&server.pool).await.unwrap();

    mycelium_control::secrets::installed_ring().unwrap().open(&stored.unwrap_or_default()).unwrap_or_default()
}

async fn quick_server(github_api_url: &str) -> Server {
    let github_api_url = github_api_url.to_string();

    Server::start_with(move |config| {
        config.scheduler_tick = FAST;
        config.forge_watch_interval = FAST;
        config.github_api_url = github_api_url;
    })
    .await
}

async fn idle_watcher(server: &Server) -> (NodeLink, Uuid) {
    let mut link = server.online_node("atlas").await;
    let watcher = begun(server, &mut link, "Ship the release").await;

    end_turn(&mut link, watcher, 1).await;

    (link, watcher)
}

#[tokio::test]
async fn a_signed_workflow_run_tells_the_watcher_once() {
    let fake = Fake::default();
    let github = serve_connected_github(&fake).await;
    let server = quick_server("http://127.0.0.1:1").await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let forge = connect_forge(&server, "github", &github, FORGE_TOKEN).await;

    let watched = tool(&server, &link, watcher, "watch_pipeline", json!({"forge": forge.to_string(), "repository": "acme/app", "ref": "main"})).await;
    let finished = json!({"action": "completed", "repository": {"full_name": "acme/app"}, "workflow_run": run(42, "main", "completed", Some("success"))});
    let first = github_webhook(&server, forge, "workflow_run", &finished, None).await;
    let told = prompt_for(&mut link, watcher).await;
    let second = github_webhook(&server, forge, "workflow_run", &finished, None).await;

    assert_eq!(watched["isError"], false, "{watched}");
    assert!(fake.hook_secret.lock().unwrap().is_some(), "the hook was not registered");
    assert_eq!((first, second), (204, 204));
    assert!(told.contains("From acme/app main:") && told.contains("succeeded") && told.contains("actions/runs/42"), "{told}");
    assert!(quiet_for(&mut link, watcher).await, "a finished watch must not notify again");
}

#[tokio::test]
async fn a_webhook_with_a_bad_signature_is_refused_and_delivers_nothing() {
    let fake = Fake::default();
    let github = serve_connected_github(&fake).await;
    let server = quick_server("http://127.0.0.1:1").await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let forge = connect_forge(&server, "github", &github, FORGE_TOKEN).await;

    tool(&server, &link, watcher, "watch_pipeline", json!({"forge": forge.to_string(), "repository": "acme/app", "ref": "main"})).await;
    let finished = json!({"action": "completed", "repository": {"full_name": "acme/app"}, "workflow_run": run(42, "main", "completed", Some("success"))});
    let forged = github_webhook(&server, forge, "workflow_run", &finished, Some(sign("not-the-secret", &finished.to_string()))).await;
    let unsigned = server.anonymous().post(&format!("/api/v1/forges/{forge}/webhook")).header("x-github-event", "workflow_run").body(finished.to_string()).send().await.unwrap().status().as_u16();
    let listed = tool(&server, &link, watcher, "list_watches", json!({})).await;

    assert_eq!((forged, unsigned), (401, 401));
    assert!(quiet_for(&mut link, watcher).await);
    assert!(text(&listed).contains("pipeline"), "{listed}");
}

#[tokio::test]
async fn a_gitlab_pipeline_hook_with_the_right_token_tells_the_watcher() {
    let fake = Fake::default();
    let gitlab = serve_gitlab(&fake).await;
    let server = quick_server("http://127.0.0.1:1").await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let forge = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    sqlx::query("insert into projects (owner, node_id, name, path, forge_id, repository) select owner, node_id, 'app', '/work/app', $2, 'slop/app' from conversations where id = $1").bind(watcher).bind(forge).execute(&server.pool).await.unwrap();
    let hook = |token: String| {
        let body = json!({"object_kind": "pipeline", "object_attributes": {"id": 77, "ref": "main", "sha": "abc123def", "status": "failed", "url": "https://gitlab.example.com/slop/app/-/pipelines/77"}, "project": {"path_with_namespace": "slop/app"}});

        server.anonymous().post(&format!("/api/v1/forges/{forge}/webhook")).header("x-gitlab-token", token).json(&body).send()
    };

    tool(&server, &link, watcher, "watch_pipeline", json!({"project": "app", "ref": "main"})).await;
    let refused = hook("wrong-token".to_string()).await.unwrap().status().as_u16();
    let accepted = hook(secret_of(&server, forge).await).await.unwrap().status().as_u16();
    let told = prompt_for(&mut link, watcher).await;

    assert_eq!((refused, accepted), (401, 204));
    assert!(told.contains("From slop/app main:") && told.contains("pipeline 77") && told.contains("failed"), "{told}");
}

#[tokio::test]
async fn a_release_watch_waits_for_every_run_of_the_tag_to_finish() {
    let fake = Fake::default();
    *fake.runs.lock().unwrap() = vec![run(7, "v1.0.0", "in_progress", None)];
    let public = serve(github_api(&fake)).await;
    let server = quick_server(&public).await;
    let (mut link, watcher) = idle_watcher(&server).await;

    let watched = tool(&server, &link, watcher, "watch_release", json!({"forge": "github.com", "repository": "acme/app", "tag": "v1.0.0"})).await;
    let early = quiet_for(&mut link, watcher).await;
    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/conversations/{watcher}/watches"))).await;
    assert_eq!((listed[0]["repository"].clone(), listed[0]["reference"].clone()), (json!("acme/app"), json!("v1.0.0")), "the person sees the tag, not its encoding");

    *fake.runs.lock().unwrap() = vec![run(7, "v1.0.0", "completed", Some("success"))];
    *fake.release.lock().unwrap() = Some(json!({"html_url": "https://github.example.com/acme/app/releases/tag/v1.0.0"}));
    let told = prompt_for(&mut link, watcher).await;

    assert_eq!(watched["isError"], false, "{watched}");
    assert!(early, "the run was still going");
    assert!(told.contains("From acme/app v1.0.0:") && told.contains("succeeded") && told.contains("releases/tag/v1.0.0"), "{told}");
    assert!(quiet_for(&mut link, watcher).await);
}

#[tokio::test]
async fn a_forge_that_refuses_the_webhook_is_checked_every_minute_instead() {
    let fake = Arc::new(FakeForge { hooks_refused: true, ..FakeForge::default() });
    *fake.runs.lock().unwrap() = vec![run(9, "main", "completed", Some("failure"))];
    let github = serve_connected_github(&fake).await;
    let server = quick_server("http://127.0.0.1:1").await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let forge = connect_forge(&server, "github", &github, FORGE_TOKEN).await;

    let watched = tool(&server, &link, watcher, "watch_pipeline", json!({"forge": forge.to_string(), "repository": "acme/app", "ref": "main"})).await;
    let told = prompt_for(&mut link, watcher).await;

    assert!(text(&watched).contains("checks every minute"), "{watched}");
    assert!(told.contains("From acme/app main:") && told.contains("failed"), "{told}");
}

#[tokio::test]
async fn a_pipeline_cannot_be_watched_through_a_project_the_person_does_not_have() {
    let server = Server::start().await;
    let (link, watcher) = idle_watcher(&server).await;
    server.second_user("alex@example.com").await;
    sqlx::query("insert into projects (owner, node_id, name, path, repository) select id, $1, 'private', '/work/private', 'acme/private' from users where email = 'alex@example.com'").bind(link.id).execute(&server.pool).await.unwrap();

    let refused = tool(&server, &link, watcher, "watch_pipeline", json!({"project": "private", "ref": "main"})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert!(text(&refused).contains("Nothing is watched. None of the person's projects is private."), "{refused}");
}

#[tokio::test]
async fn list_watches_lists_conversation_release_and_pipeline_watches_together() {
    let fake = Fake::default();
    let public = serve(github_api(&fake)).await;
    let server = quick_server(&public).await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let other = begun(&server, &mut link, "Write the notes").await;
    connect(&server, watcher, other).await;

    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": other.to_string()})).await;
    tool(&server, &link, watcher, "watch_release", json!({"forge": "github.com", "repository": "acme/app", "tag": "v1.0.0"})).await;
    let gitlab = serve_gitlab(&fake).await;
    let forge = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;
    tool(&server, &link, watcher, "watch_pipeline", json!({"forge": forge.to_string(), "repository": "slop/app", "ref": "main"})).await;
    let listed = tool(&server, &link, watcher, "list_watches", json!({})).await;
    let kinds: Vec<String> = serde_json::from_str::<Value>(&text(&listed)).unwrap()["watches"].as_array().unwrap().iter().map(|watch| watch["kind"].as_str().unwrap().to_string()).collect();

    assert_eq!(listed["isError"], false, "{listed}");
    assert_eq!(kinds, ["conversation", "release", "pipeline"]);
}

#[tokio::test]
async fn a_pipeline_watch_is_told_even_when_the_registered_webhook_never_arrives() {
    let fake = Fake::default();
    *fake.runs.lock().unwrap() = vec![json!({"id": 77, "ref": "main", "sha": "abc123def", "status": "running", "web_url": "https://gitlab.example.com/slop/app/-/pipelines/77"})];
    let gitlab = serve_gitlab(&fake).await;
    let server = quick_server("http://127.0.0.1:1").await;
    let (mut link, watcher) = idle_watcher(&server).await;
    let forge = connect_forge(&server, "gitlab", &gitlab, GITLAB_TOKEN).await;

    tool(&server, &link, watcher, "watch_pipeline", json!({"forge": forge.to_string(), "repository": "slop/app", "ref": "main"})).await;
    let early = quiet_for(&mut link, watcher).await;
    *fake.runs.lock().unwrap() = vec![json!({"id": 77, "ref": "main", "sha": "abc123def", "status": "success", "web_url": "https://gitlab.example.com/slop/app/-/pipelines/77"})];
    let told = prompt_for(&mut link, watcher).await;

    assert!(early, "the pipeline was still running");
    assert!(told.contains("From slop/app main:") && told.contains("succeeded"), "{told}");
}

#[tokio::test]
async fn a_release_watch_does_not_look_for_the_release_while_a_run_is_still_going() {
    let fake = Fake::default();
    *fake.runs.lock().unwrap() = vec![run(7, "v1.0.0", "in_progress", None)];
    let public = serve(github_api(&fake)).await;
    let server = quick_server(&public).await;
    let (mut link, watcher) = idle_watcher(&server).await;

    tool(&server, &link, watcher, "watch_release", json!({"forge": "github.com", "repository": "acme/app", "tag": "v1.0.0"})).await;
    let early = quiet_for(&mut link, watcher).await;

    assert!(early);
    assert_eq!(fake.release_lookups.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_release_watch_that_cannot_be_checked_tells_the_watcher_why_once_and_keeps_checking() {
    let fake = Fake::default();
    fake.runs_refused.store(true, Ordering::SeqCst);
    *fake.runs.lock().unwrap() = vec![run(7, "v1.0.0", "completed", Some("success"))];
    let public = serve(github_api(&fake)).await;
    let server = quick_server(&public).await;
    let (mut link, watcher) = idle_watcher(&server).await;

    tool(&server, &link, watcher, "watch_release", json!({"forge": "github.com", "repository": "acme/app", "tag": "v1.0.0"})).await;
    let failed = prompt_for(&mut link, watcher).await;
    let repeated = quiet_for(&mut link, watcher).await;
    end_turn(&mut link, watcher, 3).await;
    fake.runs_refused.store(false, Ordering::SeqCst);
    let told = prompt_for(&mut link, watcher).await;

    assert!(failed.contains("Mycelium could not check acme/app v1.0.0") && failed.contains("This token cannot read your account"), "{failed}");
    assert!(repeated, "the same failure was told again");
    assert!(told.contains("From acme/app v1.0.0:") && told.contains("succeeded"), "{told}");
}

#[tokio::test]
async fn a_release_watch_on_github_com_reads_the_public_api_even_with_a_github_com_forge_connected() {
    let fake = Fake::default();
    *fake.runs.lock().unwrap() = vec![run(7, "v1.0.0", "completed", Some("success"))];
    let public = serve(github_api(&fake)).await;
    let server = quick_server(&public).await;
    let (mut link, watcher) = idle_watcher(&server).await;
    sqlx::query("insert into forges (owner, kind, base_url, account, token) select owner, 'github', 'https://github.com', 'samexample', 'ghp_0123456789abcdefghijklmnopqrstuvwxyz' from conversations where id = $1").bind(watcher).execute(&server.pool).await.unwrap();

    let watched = tool(&server, &link, watcher, "watch_release", json!({"forge": "github.com", "repository": "acme/app", "tag": "v1.0.0"})).await;
    let told = prompt_for(&mut link, watcher).await;

    assert_eq!(watched["isError"], false, "{watched}");
    assert!(told.contains("From acme/app v1.0.0:") && told.contains("succeeded"), "{told}");
    assert!(!fake.runs_authorised.load(Ordering::SeqCst), "the public repository was read with the connected token");
}
