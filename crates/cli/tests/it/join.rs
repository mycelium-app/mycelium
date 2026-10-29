use assert_cmd::Command;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use mycelium_node::config::NodeConfig;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Default)]
struct Script {
    token_answers: Mutex<Vec<(StatusCode, Value)>>,
    device_requests: Mutex<Vec<Value>>,
}

async fn start_control_plane(token_answers: Vec<(StatusCode, Value)>) -> (String, Arc<Script>) {
    let script = Arc::new(Script { token_answers: Mutex::new(token_answers), device_requests: Mutex::new(Vec::new()) });

    async fn device(State(script): State<Arc<Script>>, Json(body): Json<Value>) -> Json<Value> {
        script.device_requests.lock().unwrap().push(body);

        Json(json!({
            "device_code": "device-secret",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://mycelium.example/approve",
            "verification_uri_complete": "https://mycelium.example/approve?code=WDJB-MJHT",
            "interval": 0,
            "expires_in": 600,
        }))
    }

    async fn token(State(script): State<Arc<Script>>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        assert_eq!(body["device_code"], "device-secret");
        let (status, answer) = script.token_answers.lock().unwrap().remove(0);

        (status, Json(answer))
    }

    let app = Router::new().route("/api/v1/device", post(device)).route("/api/v1/device/token", post(token)).with_state(script.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, script)
}

fn pending() -> (StatusCode, Value) {
    (StatusCode::BAD_REQUEST, json!({"error": "authorization_pending", "message": "Waiting for approval."}))
}

fn mycelium(config_path: &Path) -> Command {
    let mut command = Command::cargo_bin("mycelium").unwrap();
    command.env("MYCELIUM_NODE_CONFIG", config_path).env_remove("MYCELIUM_JOIN_TOKEN_FILE");

    command
}

async fn run_join(url: String, config_path: std::path::PathBuf) -> assert_cmd::assert::Assert {
    tokio::task::spawn_blocking(move || mycelium(&config_path).args(["node", "join", &url]).assert()).await.unwrap()
}

#[tokio::test]
async fn join_prints_the_code_waits_for_approval_and_writes_a_private_credential() {
    let granted = (StatusCode::OK, json!({"kind": "node", "id": "7f0c1d52-3c2e-4b43-a0a5-1c8a1e2b9d11", "token": "tok-123"}));
    let (url, script) = start_control_plane(vec![pending(), pending(), granted]).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config").join("node.toml");

    let assert = run_join(url.clone(), config_path.clone()).await;

    assert.success().stdout(predicate::str::contains("WDJB-MJHT").and(predicate::str::contains("https://mycelium.example/approve?code=WDJB-MJHT")));
    let config = NodeConfig::load(&config_path).unwrap();
    assert_eq!(config.control_plane, url);
    assert_eq!(config.node_id.to_string(), "7f0c1d52-3c2e-4b43-a0a5-1c8a1e2b9d11");
    assert_eq!(config.token, "tok-123");
    assert_eq!(config.max_sessions, 8);
    assert_eq!(std::fs::metadata(&config_path).unwrap().permissions().mode() & 0o777, 0o600);

    let requests = script.device_requests.lock().unwrap();
    assert_eq!(requests[0]["kind"], "node");
    assert_eq!(requests[0]["os"], std::env::consts::OS);
    assert_eq!(requests[0]["arch"], std::env::consts::ARCH);
}

#[tokio::test]
async fn join_stops_with_advice_when_the_code_expires() {
    let expired = (StatusCode::BAD_REQUEST, json!({"error": "expired_token", "message": "expired"}));
    let (url, _script) = start_control_plane(vec![pending(), expired]).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("node.toml");

    let assert = run_join(url, config_path.clone()).await;

    assert.failure().stderr(predicate::str::contains("The code expired before it was approved"));
    assert!(!config_path.exists());
}

#[tokio::test]
async fn join_stops_when_the_request_is_denied() {
    let denied = (StatusCode::BAD_REQUEST, json!({"error": "access_denied", "message": "denied"}));
    let (url, _script) = start_control_plane(vec![denied]).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("node.toml");

    let assert = run_join(url, config_path.clone()).await;

    assert.failure().stderr(predicate::str::contains("The join was denied"));
    assert!(!config_path.exists());
}

#[test]
fn join_tokens_report_that_they_arrive_in_a_later_release() {
    let dir = tempfile::tempdir().unwrap();

    mycelium(&dir.path().join("node.toml"))
        .args(["node", "join", "https://mycelium.example", "--join-token-file", "/nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Join tokens arrive in a later release"));
}

#[test]
fn join_reports_an_unreachable_control_plane_in_plain_words() {
    let dir = tempfile::tempdir().unwrap();

    mycelium(&dir.path().join("node.toml"))
        .args(["node", "join", "http://127.0.0.1:1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Could not reach http://127.0.0.1:1"));
}

#[test]
fn status_without_a_credential_says_to_join() {
    let dir = tempfile::tempdir().unwrap();

    mycelium(&dir.path().join("node.toml")).args(["node", "status"]).assert().failure().stderr(predicate::str::contains("mycelium node join"));
}

#[test]
fn status_prints_the_node_the_connection_and_the_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("node.toml");
    let config = NodeConfig {
        control_plane: "https://mycelium.example".into(),
        node_id: uuid::Uuid::from_u128(42),
        token: "tok".into(),
        state_dir: dir.path().join("state"),
        projects_root: dir.path().join("projects"),
        max_sessions: 4,
        kubernetes: Default::default(),
    };
    config.save(&config_path).unwrap();
    let ended = config.sessions_dir().join("00000000-0000-0000-0000-000000000001");
    std::fs::create_dir_all(&ended).unwrap();
    std::fs::write(ended.join("exit.json"), "{\"exit\":3}").unwrap();

    mycelium(&config_path).args(["node", "status"]).assert().success().stdout(
        predicate::str::contains("00000000-0000-0000-0000-00000000002a")
            .and(predicate::str::contains("https://mycelium.example"))
            .and(predicate::str::contains("daemon not running"))
            .and(predicate::str::contains("00000000-0000-0000-0000-000000000001  ended (exit 3)")),
    );
}
