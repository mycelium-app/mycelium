use axum::Router;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use mycelium_node::config::NodeConfig;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(15);

type Seen = Arc<Mutex<Vec<(String, String)>>>;

async fn echoing(State(seen): State<Seen>, Path(node): Path<String>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    let authorization = headers.get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    seen.lock().unwrap().push((node, authorization));

    upgrade.on_upgrade(|mut socket| async move {
        while let Some(Ok(message)) = socket.recv().await {
            match message {
                Message::Binary(data) => {
                    let _ = socket.send(Message::Binary(data.to_ascii_uppercase().into())).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    })
}

async fn refusing() -> StatusCode {
    StatusCode::UNAUTHORIZED
}

async fn serve(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    url
}

struct Fixture {
    _dir: tempfile::TempDir,
    config_file: PathBuf,
    token_file: PathBuf,
}

impl Fixture {
    fn new(control_plane: &str, token: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("node.toml");
        let token_file = dir.path().join("ssh-token");
        let config = NodeConfig {
            control_plane: control_plane.to_string(),
            node_id: Uuid::from_u128(1),
            token: "secret".into(),
            state_dir: dir.path().join("state"),
            projects_root: dir.path().join("projects"),
            max_sessions: 4,
        };
        config.save(&config_file).unwrap();
        std::fs::write(&token_file, format!("{token}\n")).unwrap();

        Fixture { _dir: dir, config_file, token_file }
    }

    fn proxy(&self, node: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mycelium"));
        command.arg("ssh-proxy").arg(node).arg("--token-file").arg(&self.token_file);
        command.env("MYCELIUM_NODE_CONFIG", &self.config_file);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);

        command
    }
}

#[tokio::test]
async fn bytes_written_to_stdin_travel_to_the_node_route_and_the_answer_reaches_stdout() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(Router::new().route("/api/v1/nodes/{id}/ssh", get(echoing)).with_state(seen.clone())).await;
    let fixture = Fixture::new(&url, "myc_ssh_example");
    let mut child = fixture.proxy("node-one").spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    stdin.write_all(b"hello\n").await.unwrap();
    let mut answer = [0u8; 6];
    tokio::time::timeout(WAIT, stdout.read_exact(&mut answer)).await.expect("the proxy went quiet").unwrap();

    assert_eq!(&answer, b"HELLO\n");
    assert_eq!(seen.lock().unwrap().clone(), vec![("node-one".to_string(), "Bearer myc_ssh_example".to_string())]);

    drop(stdin);
    let status = tokio::time::timeout(WAIT, child.wait()).await.expect("the proxy did not stop").unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn a_refused_token_stops_the_proxy_and_says_the_access_has_ended() {
    let url = serve(Router::new().route("/api/v1/nodes/{id}/ssh", get(refusing))).await;
    let fixture = Fixture::new(&url, "myc_ssh_example");

    let output = tokio::time::timeout(WAIT, fixture.proxy("node-one").output()).await.expect("the proxy did not stop").unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("This SSH access has ended or was revoked."));
    assert!(output.stdout.is_empty());
}

#[tokio::test]
async fn a_token_file_without_the_ssh_prefix_is_refused_before_connecting() {
    let fixture = Fixture::new("http://127.0.0.1:9", "myc_client_example");

    let output = tokio::time::timeout(WAIT, fixture.proxy("node-one").output()).await.expect("the proxy did not stop").unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not hold an SSH access token."));
}

#[tokio::test]
async fn a_missing_token_file_says_to_ask_for_access() {
    let fixture = Fixture::new("http://127.0.0.1:9", "myc_ssh_example");
    std::fs::remove_file(&fixture.token_file).unwrap();

    let output = tokio::time::timeout(WAIT, fixture.proxy("node-one").output()).await.expect("the proxy did not stop").unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ask for SSH access first."));
}
