#![cfg(windows)]

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use mycelium_node::config::NodeConfig;
use mycelium_proto::conversation::AgentId;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;
use uuid::Uuid;

const TOKEN: &str = "fixture-node-token";
const TIMEOUT: Duration = Duration::from_secs(30);

async fn receive(socket: &mut WebSocket, mut wanted: impl FnMut(&NodeToControl) -> bool) -> NodeToControl {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            if let Message::Text(text) = socket.recv().await.expect("node disconnected").unwrap() {
                let message = serde_json::from_str(&text).unwrap();
                if wanted(&message) {
                    return message;
                }

                if let NodeToControl::SessionFailed { message, .. } = message {
                    panic!("agent launch failed: {message}");
                }
            }
        }
    })
    .await
    .expect("no matching message from the node")
}

async fn send(socket: &mut WebSocket, message: ControlToNode) {
    let text = serde_json::to_string(&message).unwrap();
    socket.send(Message::Text(text.into())).await.unwrap();
}

#[tokio::test]
async fn a_windows_node_joins_signs_in_and_runs_an_acp_agent_through_cmd_shims_on_its_path() {
    let (connection, mut connections) = mpsc::unbounded_channel();
    let app = Router::new()
        .route("/api/v1/nodes/join", post(|Json(request): Json<Value>| async move {
            assert_eq!(request["token"], "fixture-join-token");
            assert_eq!(request["os"], "windows");
            assert_eq!(request["arch"], std::env::consts::ARCH);

            Json(json!({"kind": "node", "id": Uuid::from_u128(42), "token": TOKEN}))
        }))
        .route("/api/v1/node/connect", get(move |headers: HeaderMap, upgrade: WebSocketUpgrade| async move {
            assert_eq!(headers["authorization"].to_str().unwrap(), format!("Bearer myc_node_{TOKEN}"));
            upgrade.on_upgrade(move |socket| async move {
                connection.send(socket).unwrap();
            })
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let root = tempfile::tempdir().unwrap();
    let config_path = root.path().join("node.toml");
    let test_exe = std::env::current_exe().unwrap();
    let test_dir = test_exe.parent().unwrap();
    let beside_test = test_dir.join("mycelium.exe");
    let node_exe = if beside_test.is_file() { beside_test } else { test_dir.parent().unwrap().join("mycelium.exe") };
    let mycelium = |path: &Path| {
        let mut command = Command::new(&node_exe);
        command.env("MYCELIUM_NODE_CONFIG", path).kill_on_drop(true);

        command
    };
    let join = mycelium(&config_path).args(["node", "join", &url, "--token", "fixture-join-token"]).output().await.unwrap();
    assert!(join.status.success(), "{}", String::from_utf8_lossy(&join.stderr));
    let mut config = NodeConfig::load(&config_path).unwrap();
    config.state_dir = root.path().join("state with spaces");
    config.projects_root = root.path().join("projects");
    std::fs::create_dir_all(config.projects_root.join("demo workspace")).unwrap();
    config.save(&config_path).unwrap();

    let tools = root.path().join("agent shims");
    std::fs::create_dir(&tools).unwrap();
    let shim = tools.join("example-acp.cmd");
    std::fs::write(&shim, "@echo off\r\nif \"%1\"==\"--version\" (echo example-acp 1.0.0 & exit /b 0)\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0agent.ps1\" %*\r\n").unwrap();
    std::fs::write(tools.join("agent.ps1"), r#"
param([string]$Expected)
if ($Expected -ne 'fixture argument') { exit 2 }
while ($null -ne ($line = [Console]::ReadLine())) {
    $request = ConvertFrom-Json $line
    $result = $null
    switch ($request.method) {
        'initialize' { $result = @{ protocolVersion = 1; agentCapabilities = @{}; authMethods = @() } }
        'session/new' {
            if ((Get-Location).Path -notlike '*demo workspace') { exit 3 }
            $helper = $request.params._meta.claudeCode.options.mcpServers.mycelium.headersHelper
            $headers = cmd.exe /D /C $helper | ConvertFrom-Json
            if ($headers.Authorization -ne 'Bearer fixture-session-token') { exit 4 }
            $result = @{ sessionId = 'fixture-session' }
        }
        'session/prompt' { $result = @{ stopReason = 'end_turn' } }
        default { continue }
    }

    if ($null -ne $result) {
        [Console]::WriteLine((ConvertTo-Json -Compress @{ jsonrpc = '2.0'; id = $request.id; result = $result }))
    }
}
"#).unwrap();
    std::fs::write(tools.join("claude.cmd"), "@echo off\r\necho fixture sign-in %*\r\n").unwrap();
    let path = std::env::join_paths(std::iter::once(tools.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let mut daemon = mycelium(&config_path).args(["node", "run"]).env("PATH", path).env("MYCELIUM_CLAUDE_ACP", "example-acp \"fixture argument\"")
        .env("MYCELIUM_CODEX_ACP", "fixture-missing-codex").env("MYCELIUM_VIBE_ACP", "fixture-missing-vibe")
        .spawn().unwrap();

    let mut socket = tokio::time::timeout(TIMEOUT, connections.recv()).await.unwrap().unwrap();
    let NodeToControl::Hello { agents, .. } = receive(&mut socket, |message| matches!(message, NodeToControl::Hello { .. })).await else { unreachable!() };
    assert!(agents.iter().any(|agent| agent.agent == AgentId::Claude && agent.available && agent.version.as_deref() == Some("example-acp 1.0.0")));
    let conversation_id = Uuid::from_u128(43);
    send(&mut socket, ControlToNode::StartSession { conversation_id, agent: AgentId::Claude, cwd: "demo workspace".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: "fixture-session-token".into() }).await;
    let started = receive(&mut socket, |message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    assert!(matches!(started, NodeToControl::SessionStarted { conversation_id: id, acp_session_id, .. } if id == conversation_id && acp_session_id == "fixture-session"));

    send(&mut socket, ControlToNode::CloseSession { conversation_id }).await;
    receive(&mut socket, |message| matches!(message, NodeToControl::SessionClosed { conversation_id: id } if *id == conversation_id)).await;

    let login_id = Uuid::from_u128(44);
    send(&mut socket, ControlToNode::StartLogin { login_id, agent: AgentId::Claude, method: "oauth_token".into(), account: None }).await;
    let mut printed = String::new();
    let finished = receive(&mut socket, |message| match message {
        NodeToControl::LoginOutput { login_id: id, data } if *id == login_id => {
            printed.push_str(data);
            false
        }
        NodeToControl::LoginFinished { login_id: id, .. } => *id == login_id,
        _ => false,
    })
    .await;
    assert!(matches!(finished, NodeToControl::LoginFinished { ok: true, .. }), "{printed}");
    assert!(printed.contains("fixture sign-in auth login"), "{printed}");
    let exit = std::fs::read_to_string(config.sessions_dir().join(conversation_id.to_string()).join("exit.json")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&exit).unwrap()["exit"], 1);
    daemon.kill().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn an_update_replaces_the_executable_while_it_is_running() {
    const ASSET: &str = "mycelium-windows-arm64.exe";
    let checksum: String = Sha256::digest(b"new build").iter().map(|byte| format!("{byte:02x}")).collect();
    let path = format!("/v9.9.9/{ASSET}");
    let app = Router::new()
        .route(&path, get(|| async { &b"new build"[..] }))
        .route(&format!("{path}.sha256"), get(move || async move { format!("{checksum}  {ASSET}\n") }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("mycelium.exe");
    let system = std::env::var("SystemRoot").unwrap();
    std::fs::copy(Path::new(&system).join("System32").join("PING.EXE"), &target).unwrap();
    let mut running = Command::new(&target).args(["-n", "60", "127.0.0.1"]).kill_on_drop(true).spawn().unwrap();

    let installed = mycelium_node::update::install(&reqwest::Client::new(), &url, "v9.9.9", ASSET, &target).await;

    assert!(installed.is_ok(), "{installed:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
    assert!(running.try_wait().unwrap().is_none());
    running.kill().await.unwrap();
    server.abort();
}
