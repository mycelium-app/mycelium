#![cfg(windows)]

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use mycelium_node::agents::{Tools, install_command, resolve_program, update_command};
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
async fn a_windows_node_joins_signs_in_and_runs_agents_found_only_on_its_own_path() {
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
    std::fs::write(&shim, "@echo off\r\nif \"%~1\"==\"--version\" (echo example-acp 1.0.0 & exit /b 0)\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0agent.ps1\" %*\r\n").unwrap();
    std::fs::write(tools.join("agent.ps1"), r#"
param([string]$Expected)
if ($Expected -ne 'fixture argument') { exit 2 }
while ($null -ne ($line = [Console]::ReadLine())) {
    $request = ConvertFrom-Json $line
    $result = $null
    switch ($request.method) {
        'initialize' { $result = @{ protocolVersion = 1; agentCapabilities = @{ mcpCapabilities = @{ http = $true } }; authMethods = @() } }
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
        [Console]::WriteLine((ConvertTo-Json -Compress -Depth 10 @{ jsonrpc = '2.0'; id = $request.id; result = $result }))
    }
}
"#).unwrap();
    std::fs::write(tools.join("claude.cmd"), "@echo off\r\necho fixture sign-in %*\r\necho Open https://claude.example.com/oauth?code=fixture to sign in\r\nset /p code=Paste code here: \r\necho got %code%\r\n").unwrap();
    let native = root.path().join("native tools");
    std::fs::create_dir(&native).unwrap();
    std::fs::copy(&node_exe, native.join("codex.exe")).unwrap();
    let path = std::env::join_paths([tools.clone(), native].into_iter().chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let mut daemon = mycelium(&config_path).args(["node", "run"]).env("Path", path).env("MYCELIUM_CLAUDE_ACP", "example-acp \"fixture argument\"")
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
    receive(&mut socket, |message| match message {
        NodeToControl::LoginOutput { login_id: id, data } if *id == login_id => {
            printed.push_str(data);
            printed.contains("Paste code here")
        }
        _ => false,
    })
    .await;
    assert!(printed.contains("fixture sign-in auth login"), "{printed}");
    assert!(printed.contains("https://claude.example.com/oauth?code=fixture"), "{printed}");
    send(&mut socket, ControlToNode::LoginInput { login_id, text: "1234\r\n".into() }).await;
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
    assert!(printed.contains("got 1234"), "{printed}");

    let login_id = Uuid::from_u128(45);
    send(&mut socket, ControlToNode::StartLogin { login_id, agent: AgentId::Codex, method: "device_auth".into(), account: None }).await;
    let mut printed = String::new();
    receive(&mut socket, |message| match message {
        NodeToControl::LoginOutput { login_id: id, data } if *id == login_id => {
            printed.push_str(data);
            false
        }
        NodeToControl::LoginFinished { login_id: id, .. } => *id == login_id,
        _ => false,
    })
    .await;
    assert!(!printed.contains("Could not start") && printed.contains("unrecognized subcommand"), "an .exe found only on the node's Path did not start: {printed}");
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

struct SshClient;

impl russh::client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn ssh_to(server: std::sync::Arc<mycelium_node::ssh::SshServer>, mut messages: mpsc::UnboundedReceiver<NodeToControl>) -> russh::client::Handle<SshClient> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let session_id = Uuid::from_u128(46);
    let (client_end, bridge_end) = tokio::io::duplex(64 * 1024);
    let (mut bridge_reader, mut bridge_writer) = tokio::io::split(bridge_end);
    tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            if let NodeToControl::SshData { data, .. } = message {
                if bridge_writer.write_all(&STANDARD.decode(data).unwrap()).await.is_err() {
                    break;
                }
            }
        }
    });
    let relay = server.clone();
    tokio::spawn(async move {
        let mut buffer = vec![0u8; 32 * 1024];
        while let Ok(count) = bridge_reader.read(&mut buffer).await {
            if count == 0 {
                break;
            }

            relay.data(session_id, &STANDARD.encode(&buffer[..count]));
        }
    });
    server.open(session_id);

    let mut handle = russh::client::connect_stream(std::sync::Arc::new(russh::client::Config::default()), client_end, SshClient).await.unwrap();
    assert!(handle.authenticate_none("anyone").await.unwrap().success());

    handle
}

async fn ssh_output(channel: &mut russh::Channel<russh::client::Msg>, answer_cursor_queries: bool) -> (String, Option<u32>) {
    let mut output = Vec::new();
    let mut status = None;

    loop {
        let message = tokio::time::timeout(TIMEOUT, channel.wait()).await.expect("the SSH command went quiet");
        match message {
            Some(russh::ChannelMsg::Data { data }) => {
                if answer_cursor_queries && data.windows(4).any(|window| window == b"\x1b[6n") {
                    channel.data(&b"\x1b[1;1R"[..]).await.unwrap();
                }

                output.extend_from_slice(&data);
            }
            Some(russh::ChannelMsg::ExtendedData { data, .. }) => output.extend_from_slice(&data),
            Some(russh::ChannelMsg::ExitStatus { exit_status }) => status = Some(exit_status),
            Some(_) => {}
            None => break,
        }
    }

    (String::from_utf8_lossy(&output).into_owned(), status)
}

#[tokio::test]
async fn ssh_to_a_windows_node_runs_commands_and_an_interactive_shell() {
    let root = tempfile::tempdir().unwrap();
    let (out, messages) = mpsc::unbounded_channel();
    let server = std::sync::Arc::new(mycelium_node::ssh::SshServer::new(out, root.path().join("ssh_host_key"), root.path().to_path_buf(), mycelium_node::terminal::user_shell()));
    let handle = ssh_to(server, messages).await;

    let mut command = handle.channel_open_session().await.unwrap();
    command.exec(true, "echo fixture over ssh & exit /b 3").await.unwrap();
    let (printed, status) = ssh_output(&mut command, false).await;
    assert!(printed.contains("fixture over ssh"), "{printed}");
    assert_eq!(status, Some(3));

    let mut shell = handle.channel_open_session().await.unwrap();
    shell.request_pty(true, "xterm", 100, 30, 0, 0, &[]).await.unwrap();
    shell.request_shell(true).await.unwrap();
    shell.data(&b"echo typed over ssh\r\nexit 4\r\n"[..]).await.unwrap();
    let (printed, status) = ssh_output(&mut shell, true).await;
    assert!(printed.contains("typed over ssh"), "{printed}");
    assert_eq!(status, Some(4));
}

const RECORDING_NPM: &str = "@echo off\r\necho %*> \"%~dp0npm-arguments.txt\"\r\n";

fn run_install(command: Vec<String>) -> std::process::Output {
    let program = resolve_program(&command[0]).unwrap();

    std::process::Command::new(program).args(&command[1..]).output().unwrap()
}

fn recorded_arguments(folder: &Path) -> String {
    std::fs::read_to_string(folder.join("npm-arguments.txt")).unwrap().trim().to_string()
}

#[test]
fn installing_claude_code_runs_the_nodes_npm_from_powershell() {
    let folder = tempfile::tempdir().unwrap();
    let npm = folder.path().join("npm.cmd");
    std::fs::write(&npm, RECORDING_NPM).unwrap();
    let tools = Tools { npm: Some(&npm), uv: None, node_download: None, node_folder: folder.path() };

    let output = run_install(install_command(AgentId::Claude, "0.13.0", &tools).unwrap());

    assert!(output.status.success(), "{output:?}");
    assert_eq!(recorded_arguments(folder.path()), "install --global @agentclientprotocol/claude-agent-acp@0.13.0 @anthropic-ai/claude-code");
}

#[test]
fn without_npm_codex_unpacks_the_node_js_zip_into_mycelium_and_installs_there() {
    let work = tempfile::tempdir().unwrap();
    let build = work.path().join("node-v24.1.0-win-arm64");
    std::fs::create_dir(&build).unwrap();
    std::fs::write(build.join("npm.cmd"), RECORDING_NPM).unwrap();
    let packed = std::process::Command::new("tar.exe").current_dir(work.path()).args(["-a", "-cf", "node.zip", "node-v24.1.0-win-arm64"]).status().unwrap();
    assert!(packed.success());
    let url = format!("file:///{}", work.path().join("node.zip").display().to_string().replace('\\', "/"));
    let folder = work.path().join("state").join("tools").join("node");
    let tools = Tools { npm: None, uv: None, node_download: Some(&url), node_folder: &folder };

    let output = run_install(install_command(AgentId::Codex, "2.7.0", &tools).unwrap());

    assert!(output.status.success(), "{output:?}");
    assert_eq!(recorded_arguments(&folder), format!("install --global --prefix {} @agentclientprotocol/codex-acp@2.7.0 @openai/codex", folder.display()));
}

#[test]
fn an_agent_installed_with_npm_updates_its_package_into_the_folder_of_its_shim() {
    let prefix = tempfile::tempdir().unwrap();
    let shim = prefix.path().join("claude-agent-acp.cmd");
    std::fs::write(&shim, "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%_prog%\"  \"%dp0%\\node_modules\\@agentclientprotocol\\claude-agent-acp\\dist\\index.js\" %*\r\n").unwrap();

    let command = update_command(AgentId::Claude, "0.108.1", &shim).unwrap();

    assert_eq!(command, ["npm", "install", "--global", "--prefix", &prefix.path().display().to_string(), "@agentclientprotocol/claude-agent-acp@0.108.1"]);
}

#[test]
fn an_agent_installed_as_a_uv_tool_is_upgraded_with_uv_from_its_launcher() {
    let home = tempfile::tempdir().unwrap();
    let launcher = home.path().join("vibe-acp.exe");
    std::fs::write(&launcher, b"MZ\x90\x00launcher\x00C:\\Users\\alex\\AppData\\Roaming\\uv\\data\\tools\\mistral-vibe\\Scripts\\python.exe\x00UVUV").unwrap();

    assert_eq!(update_command(AgentId::Vibe, "2.1.0", &launcher).unwrap(), ["uv", "tool", "upgrade", "mistral-vibe"]);
}
