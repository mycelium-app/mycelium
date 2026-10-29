use crate::common;
use futures_util::{SinkExt, StreamExt};
use mycelium_node::agents::login_command;
use mycelium_node::config::NodeConfig;
use mycelium_node::daemon::{DaemonOptions, run_daemon};
use mycelium_node::link::{Backoff, Liveness};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(15);
const QUIET: Duration = Duration::from_millis(400);

enum Order {
    Send(ControlToNode),
    Drop,
}

struct Connection {
    authorization: String,
    incoming: mpsc::UnboundedReceiver<NodeToControl>,
    orders: mpsc::UnboundedSender<Order>,
}

impl Connection {
    async fn next(&mut self) -> NodeToControl {
        tokio::time::timeout(WAIT, self.incoming.recv()).await.expect("the node went quiet").expect("the node disconnected")
    }

    async fn next_matching(&mut self, wanted: impl Fn(&NodeToControl) -> bool) -> NodeToControl {
        loop {
            let message = self.next().await;
            if wanted(&message) {
                return message;
            }
        }
    }

    async fn nothing_more(&mut self) -> Option<NodeToControl> {
        tokio::time::timeout(QUIET, self.incoming.recv()).await.ok().flatten()
    }

    fn send(&self, message: ControlToNode) {
        let _ = self.orders.send(Order::Send(message));
    }

    fn drop_connection(&self) {
        let _ = self.orders.send(Order::Drop);
    }
}

struct ControlServer {
    url: String,
    connections: mpsc::UnboundedReceiver<Connection>,
}

impl ControlServer {
    async fn start() -> ControlServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (connections_tx, connections) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let connections_tx = connections_tx.clone();
                tokio::spawn(async move {
                    let mut authorization = String::new();
                    let callback = |request: &Request, response: Response| {
                        authorization = request.headers().get("authorization").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();

                        Ok(response)
                    };
                    let Ok(socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
                        return;
                    };

                    let (mut sink, mut source) = socket.split();
                    let (incoming_tx, incoming) = mpsc::unbounded_channel();
                    let (orders, mut orders_rx) = mpsc::unbounded_channel();
                    let _ = connections_tx.send(Connection { authorization, incoming, orders });

                    loop {
                        tokio::select! {
                            frame = source.next() => {
                                let Some(Ok(frame)) = frame else {
                                    return;
                                };

                                if let Message::Text(text) = frame {
                                    let message: NodeToControl = serde_json::from_str(&text).expect("a message the protocol defines");
                                    let _ = incoming_tx.send(message);
                                }
                            }
                            order = orders_rx.recv() => {
                                match order {
                                    Some(Order::Send(message)) => {
                                        sink.send(Message::text(serde_json::to_string(&message).unwrap())).await.unwrap();
                                    }
                                    Some(Order::Drop) | None => return,
                                }
                            }
                        }
                    }
                });
            }
        });

        ControlServer { url, connections }
    }

    async fn accept(&mut self) -> Connection {
        tokio::time::timeout(WAIT, self.connections.recv()).await.expect("the node never connected").unwrap()
    }
}

struct Fixture {
    root: tempfile::TempDir,
    config: NodeConfig,
    options: DaemonOptions,
}

fn fake_acp() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../node/tests/fake-acp").canonicalize().unwrap().display().to_string()
}

impl Fixture {
    fn new(control_plane: &str) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("projects").join("demo")).unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", root.path().join("home"));
            std::env::set_var("XDG_DATA_DIRS", root.path().join("home"));
        }

        let config = NodeConfig {
            control_plane: control_plane.to_string(),
            node_id: Uuid::from_u128(42),
            token: "secret".into(),
            state_dir: root.path().join("state"),
            projects_root: root.path().join("projects"),
            max_sessions: 4,
            kubernetes: Default::default(),
        };
        let agent = fake_acp();
        let options = DaemonOptions {
            keeper_binary: PathBuf::from(env!("CARGO_BIN_EXE_mycelium")),
            agent_commands: Arc::new(move |agent_id| if agent_id == AgentId::Claude { vec![agent.clone()] } else { vec!["/nonexistent/codex-acp".to_string()] }),
            login_commands: Arc::new(|agent, method| login_command(agent, method).map(|_| vec!["sh".into(), "-c".into(), "echo sign-in-link".into()])),
            backoff: Backoff { min: Duration::from_millis(20), max: Duration::from_millis(100) },
            liveness: Liveness::default(),
            home: root.path().join("home"),
            transcript_roots: vec![root.path().join("home").join("projects")],
            transcript_poll: Duration::from_millis(50),
            usage_endpoint: "http://127.0.0.1:9/api/oauth/usage".to_string(),
            codex_usage_endpoint: "http://127.0.0.1:9/backend-api/wham/usage".to_string(),
            codex_command: "codex-not-installed".to_string(),
        };

        Fixture { root, config, options }
    }

    fn config_file(&self) -> PathBuf {
        self.root.path().join("node.toml")
    }

    fn session_dir(&self, conversation: Uuid) -> PathBuf {
        self.config.sessions_dir().join(conversation.to_string())
    }

    fn start_daemon(&self) -> RunningDaemon {
        self.config.save(&self.config_file()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (finished_tx, finished) = std::sync::mpsc::channel();

        let daemon = run_daemon(self.config.clone(), self.config_file(), self.options.clone());
        runtime.spawn(async move {
            let result = daemon.await;
            let _ = finished_tx.send(result.map_err(|error| format!("{error:#}")));
        });

        RunningDaemon { runtime: Some(runtime), finished }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        common::kill_keepers(&self.config.sessions_dir());
    }
}

struct RunningDaemon {
    runtime: Option<tokio::runtime::Runtime>,
    finished: std::sync::mpsc::Receiver<Result<(), String>>,
}

impl RunningDaemon {
    fn crash(mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }

    fn exit_result(&self) -> Result<(), String> {
        self.finished.recv_timeout(WAIT).expect("the daemon did not exit")
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn is_hello(message: &NodeToControl) -> bool {
    matches!(message, NodeToControl::Hello { .. })
}

fn update_text(message: &NodeToControl) -> Option<String> {
    let NodeToControl::AcpUpdate { update, .. } = message else {
        return None;
    };

    update["update"]["content"]["text"].as_str().map(str::to_string)
}

fn seq_of(message: &NodeToControl) -> Option<i64> {
    match message {
        NodeToControl::AcpUpdate { seq, .. }
        | NodeToControl::PermissionRequest { seq, .. }
        | NodeToControl::TurnEnd { seq, .. }
        | NodeToControl::SessionFailed { seq, .. } => Some(*seq),
        _ => None,
    }
}

async fn start_conversation(connection: &mut Connection, conversation: Uuid) {
    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
}

fn prompt(connection: &Connection, conversation: Uuid, text: &str) {
    connection.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id: Uuid::new_v4(), text: text.into(), attachments: vec![] });
}

fn journalled_prompts(fixture: &Fixture, conversation: Uuid) -> usize {
    let journal = std::fs::read_to_string(fixture.session_dir(conversation).join("journal.jsonl")).unwrap_or_default();

    journal.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|entry| entry["frame"]["method"] == "session/prompt").count()
}

async fn wait_for_file(path: &Path) {
    for _ in 0..500 {
        if path.exists() {
            return;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    panic!("{} never appeared", path.display());
}

#[tokio::test]
async fn the_node_authenticates_and_says_hello_with_its_agents() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();

    let mut connection = control.accept().await;
    let hello = connection.next().await;

    assert_eq!(connection.authorization, "Bearer myc_node_secret");
    let NodeToControl::Hello { agents, sessions, os, .. } = hello else {
        panic!("the first message must be hello, got {hello:?}");
    };
    assert_eq!(os, std::env::consts::OS);
    assert!(sessions.is_empty());
    assert!(agents.iter().any(|agent| agent.agent == AgentId::Claude && agent.available));
    assert!(agents.iter().any(|agent| agent.agent == AgentId::Codex && !agent.available));
}

#[tokio::test]
async fn a_prompt_streams_updates_then_turn_end_and_keeps_reading_the_late_update() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(1);

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
    let started = connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    let NodeToControl::SessionStarted { acp_session_id, modes, .. } = started else {
        unreachable!();
    };
    assert_eq!(acp_session_id, "fake-session-1");
    assert!(modes.is_some());

    prompt(&connection, conversation, "hello");

    let mut texts = Vec::new();
    let mut seqs = Vec::new();
    while texts.len() < 3 {
        let message = connection.next_matching(|message| update_text(message).is_some()).await;
        texts.push(update_text(&message).unwrap());
        seqs.push(seq_of(&message).unwrap());
    }
    assert_eq!(texts, ["one", "two", "three"]);

    let turn_end = connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert!(matches!(&turn_end, NodeToControl::TurnEnd { stop_reason, .. } if stop_reason == "end_turn"));
    seqs.push(seq_of(&turn_end).unwrap());

    let late = connection.next_matching(|message| update_text(message).is_some()).await;
    assert_eq!(update_text(&late).unwrap(), "late");
    seqs.push(seq_of(&late).unwrap());

    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "seqs must increase: {seqs:?}");
}

#[tokio::test]
async fn a_permission_request_is_forwarded_and_the_answer_reaches_the_agent() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(2);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "ask permission first");
    let request = connection.next_matching(|message| matches!(message, NodeToControl::PermissionRequest { .. })).await;
    let NodeToControl::PermissionRequest { request_id, request, .. } = request else {
        unreachable!();
    };
    assert_eq!(request["toolCall"]["title"], "Run ls");
    assert_eq!(request["options"][0]["optionId"], "allow");

    connection.send(ControlToNode::PermissionResponse { conversation_id: conversation, request_id, outcome: json!({"outcome": "selected", "optionId": "allow"}) });

    let after = connection.next_matching(|message| update_text(message).is_some_and(|text| text.starts_with("permission "))).await;
    assert_eq!(update_text(&after).unwrap(), "permission allow");
    connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
}

#[tokio::test]
async fn cancel_stops_a_running_turn() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(3);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "do something slow");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    connection.send(ControlToNode::Cancel { conversation_id: conversation });

    let turn_end = connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert!(matches!(&turn_end, NodeToControl::TurnEnd { stop_reason, .. } if stop_reason == "cancelled"));
}

fn steer(connection: &Connection, conversation: Uuid, request_id: Uuid, text: &str) {
    connection.send(ControlToNode::Steer { conversation_id: conversation, request_id, text: text.into(), attachments: vec![] });
}

#[tokio::test]
async fn steering_injects_into_a_running_turn_without_cancelling_it() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(3);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "do something slow");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    let request_id = Uuid::from_u128(77);
    steer(&connection, conversation, request_id, "change course");

    connection.next_matching(|message| update_text(message).as_deref() == Some("steered: change course")).await;
    let answer = connection.next_matching(|message| matches!(message, NodeToControl::Steered { .. })).await;
    assert!(matches!(&answer, NodeToControl::Steered { request_id: answered, outcome, error: None } if *answered == request_id && outcome == "injected"));
    assert!(!matches!(connection.nothing_more().await, Some(NodeToControl::TurnEnd { .. })));
}

#[tokio::test]
async fn steering_an_idle_session_asks_the_host_to_send_a_normal_prompt() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(3);
    start_conversation(&mut connection, conversation).await;

    steer(&connection, conversation, Uuid::from_u128(78), "hello");

    let answer = connection.next_matching(|message| matches!(message, NodeToControl::Steered { .. })).await;
    assert!(matches!(&answer, NodeToControl::Steered { outcome, .. } if outcome == "prompt_required"));
}

#[tokio::test]
async fn steering_an_agent_without_the_extension_is_unsupported_and_leaves_the_turn_running() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    let agent = fake_acp();
    fixture.options.agent_commands = Arc::new(move |_| vec![agent.clone(), "--no-steering".to_string()]);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(3);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "do something slow");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    steer(&connection, conversation, Uuid::from_u128(79), "change course");

    let answer = connection.next_matching(|message| matches!(message, NodeToControl::Steered { .. })).await;
    assert!(matches!(&answer, NodeToControl::Steered { outcome, .. } if outcome == "unsupported"));
    assert!(!matches!(connection.nothing_more().await, Some(NodeToControl::TurnEnd { .. })));
}

#[tokio::test]
async fn steering_a_conversation_the_node_does_not_run_is_unsupported() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    steer(&connection, Uuid::from_u128(404), Uuid::from_u128(80), "hello");

    let answer = connection.next_matching(|message| matches!(message, NodeToControl::Steered { .. })).await;
    assert!(matches!(&answer, NodeToControl::Steered { outcome, .. } if outcome == "unsupported"));
}

#[tokio::test]
async fn stop_ends_a_turn_the_agent_is_not_running_so_the_conversation_stops_working() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(30);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "stuck");
    connection.next_matching(|message| update_text(message).as_deref() == Some("stuck")).await;
    connection.send(ControlToNode::Cancel { conversation_id: conversation });

    let turn_end = connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert!(matches!(&turn_end, NodeToControl::TurnEnd { stop_reason, .. } if stop_reason == "cancelled"));
}

#[tokio::test]
async fn stop_ends_a_turn_whose_agent_ignores_it_and_keeps_streaming() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(31);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "chatty");
    connection.next_matching(|message| update_text(message).as_deref() == Some("chatty")).await;
    connection.send(ControlToNode::Cancel { conversation_id: conversation });

    let turn_end = connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert!(matches!(&turn_end, NodeToControl::TurnEnd { stop_reason, .. } if stop_reason == "cancelled"));
}

#[tokio::test]
async fn a_prompt_sent_right_after_a_stopped_turn_is_not_ended_by_that_stops_timer() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(32);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "slow first");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    connection.send(ControlToNode::Cancel { conversation_id: conversation });
    connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;

    prompt(&connection, conversation, "slow second");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    let mut ended = false;

    while let Some(message) = connection.nothing_more().await {
        ended |= matches!(message, NodeToControl::TurnEnd { .. });
    }

    assert!(!ended, "the second turn was ended by the first turn's stop timer");
}

#[tokio::test]
async fn agent_requests_the_client_does_not_support_get_method_not_found() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(4);
    start_conversation(&mut connection, conversation).await;

    prompt(&connection, conversation, "readfile please");

    let reply = connection.next_matching(|message| update_text(message).is_some_and(|text| text.starts_with("fs "))).await;
    assert_eq!(update_text(&reply).unwrap(), "fs refused");
}

#[tokio::test]
async fn set_config_reaches_the_agent_as_session_set_mode() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(5);
    start_conversation(&mut connection, conversation).await;

    connection.send(ControlToNode::SetConfig { conversation_id: conversation, mode: Some("plan".into()), model: Some("ignored".into()), config: None });

    let journal_path = fixture.session_dir(conversation).join("journal.jsonl");
    let mut frames = Vec::new();
    for _ in 0..250 {
        let journal = std::fs::read_to_string(&journal_path).unwrap_or_default();
        frames = journal.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|entry| entry["frame"]["method"] == "session/set_mode").collect();
        if !frames.is_empty() {
            break;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["frame"]["params"], json!({"sessionId": "fake-session-1", "modeId": "plan"}));
    let journal = std::fs::read_to_string(&journal_path).unwrap();
    assert!(!journal.contains("session/set_model"), "the fake agent advertises no models");
}

#[tokio::test]
async fn at_the_session_limit_the_least_recently_used_idle_session_is_closed_to_make_room() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    fixture.config.max_sessions = 2;
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let (first, second, third) = (Uuid::from_u128(30), Uuid::from_u128(31), Uuid::from_u128(32));
    start_conversation(&mut connection, first).await;
    start_conversation(&mut connection, second).await;
    prompt(&connection, first, "hello");
    connection.next_matching(|message| update_text(message).as_deref() == Some("late")).await;

    connection.send(ControlToNode::StartSession { conversation_id: third, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });

    let closed = connection.next_matching(|message| matches!(message, NodeToControl::SessionClosed { .. })).await;
    assert!(matches!(closed, NodeToControl::SessionClosed { conversation_id } if conversation_id == second));
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { conversation_id, .. } if *conversation_id == third)).await;
    wait_for_file(&fixture.session_dir(second).join("exit.json")).await;
    assert!(!fixture.session_dir(first).join("exit.json").exists());
}

#[tokio::test]
async fn at_the_session_limit_with_every_session_busy_the_new_one_is_refused_with_the_setting_to_raise() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    fixture.config.max_sessions = 1;
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let (busy, refused) = (Uuid::from_u128(33), Uuid::from_u128(34));
    start_conversation(&mut connection, busy).await;
    prompt(&connection, busy, "do something slow");
    connection.next_matching(|message| update_text(message).as_deref() == Some("working")).await;

    connection.send(ControlToNode::StartSession { conversation_id: refused, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });

    let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    let NodeToControl::SessionFailed { message, .. } = failed else {
        unreachable!();
    };
    assert!(message.contains("every one of them is busy") && message.contains("raise max_sessions in") && message.contains("node.toml"), "{message}");
}

#[tokio::test]
async fn a_missing_folder_fails_the_session_with_a_reason() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(6);

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "nowhere".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });

    let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    let NodeToControl::SessionFailed { message, .. } = failed else {
        unreachable!();
    };
    assert!(message.contains("does not exist on this node"));
}

#[tokio::test]
async fn an_account_the_node_does_not_hold_fails_the_session_with_a_reason() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(8);

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: Some("nobody@example.com".into()), instructions: None, reload: false, env: vec![], mcp_token: String::new() });

    let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    let NodeToControl::SessionFailed { message, .. } = failed else {
        unreachable!();
    };
    assert!(message.contains("nobody@example.com is not signed in on this node"));
}

#[tokio::test]
async fn a_folder_outside_the_projects_root_is_refused() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    std::fs::create_dir_all(fixture.root.path().join("secrets")).unwrap();
    let outside_by_symlink = fixture.root.path().join("projects").join("link");
    std::os::unix::fs::symlink(fixture.root.path().join("secrets"), &outside_by_symlink).unwrap();
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let escapes = [fixture.root.path().join("secrets").display().to_string(), "../secrets".to_string(), "link".to_string()];

    for (index, cwd) in escapes.into_iter().enumerate() {
        let conversation = Uuid::from_u128(100 + index as u128);
        connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: cwd.clone(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });

        let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { conversation_id, .. } if *conversation_id == conversation)).await;
        let NodeToControl::SessionFailed { message, .. } = failed else {
            unreachable!();
        };
        assert!(message.contains("is outside"), "{cwd} was not refused: {message}");
    }
}

#[tokio::test]
async fn an_agent_that_is_not_installed_fails_the_session() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    connection.send(ControlToNode::StartSession { conversation_id: Uuid::from_u128(7), agent: AgentId::Codex, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });

    let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    let NodeToControl::SessionFailed { message, .. } = failed else {
        unreachable!();
    };
    assert_eq!(message, "codex is not installed on this node.");
}

#[tokio::test]
async fn closing_a_session_ends_the_agent_without_reporting_a_failure() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(8);
    start_conversation(&mut connection, conversation).await;

    connection.send(ControlToNode::CloseSession { conversation_id: conversation });

    wait_for_file(&fixture.session_dir(conversation).join("exit.json")).await;
    assert_eq!(connection.nothing_more().await, None);
}

#[tokio::test]
async fn a_session_started_while_the_previous_one_is_closing_replaces_it_and_takes_the_prompt() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(18);
    start_conversation(&mut connection, conversation).await;

    connection.send(ControlToNode::CloseSession { conversation_id: conversation });
    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    prompt(&connection, conversation, "hello");

    let reply = connection.next_matching(|message| update_text(message).is_some()).await;
    assert_eq!(update_text(&reply).unwrap(), "one");
}

#[tokio::test]
async fn secrets_live_in_a_private_file_that_follows_changes_and_a_changed_start_replaces_the_session() {
    use std::os::unix::fs::PermissionsExt;

    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(19);
    let file = fixture.session_dir(conversation).join("secrets.env");
    let start = |token: &str| ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![("TOKEN".into(), token.into())], mcp_token: String::new() };

    connection.send(start("it's one"));
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "TOKEN='it'\\''s one'\n");
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);

    connection.send(ControlToNode::SecretsChanged { conversation_id: conversation, env: vec![("TOKEN".into(), "two".into())] });
    prompt(&connection, conversation, "hello");
    connection.next_matching(|message| update_text(message).is_some()).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "TOKEN='two'\n");

    connection.send(start("two"));
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "TOKEN='two'\n");
}

#[tokio::test]
async fn a_reload_start_closes_the_idle_session_and_resumes_it_with_the_same_session_id() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    let agent = fake_acp();
    fixture.options.agent_commands = Arc::new(move |_| vec![agent.clone()]);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(19);
    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: Some("old".into()), reload: false, env: vec![], mcp_token: String::new() });
    let started = connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    let NodeToControl::SessionStarted { acp_session_id, .. } = started else { panic!("expected a started session") };

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "demo".into(), resume: Some(acp_session_id.clone()), fork: false, account: None, instructions: Some("new".into()), reload: true, env: vec![], mcp_token: String::new() });
    let resumed = connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    let NodeToControl::SessionStarted { acp_session_id: resumed_id, .. } = resumed else { panic!("expected a resumed session") };
    prompt(&connection, conversation, "hello");
    let reply = connection.next_matching(|message| update_text(message).is_some()).await;

    assert_eq!(resumed_id, acp_session_id);
    assert_eq!(update_text(&reply).unwrap(), "one");
}

#[tokio::test]
async fn a_session_started_for_another_agent_replaces_the_running_one() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    let agent = fake_acp();
    fixture.options.agent_commands = Arc::new(move |_| vec![agent.clone()]);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(19);
    start_conversation(&mut connection, conversation).await;

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Codex, cwd: "demo".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
    connection.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    prompt(&connection, conversation, "hello");

    let reply = connection.next_matching(|message| update_text(message).is_some() || matches!(message, NodeToControl::SessionClosed { .. } | NodeToControl::SessionFailed { .. })).await;
    assert_eq!(update_text(&reply).unwrap(), "one");
}

#[tokio::test]
async fn after_a_reconnect_only_unacknowledged_messages_are_replayed_once() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(9);
    start_conversation(&mut first, conversation).await;
    prompt(&first, conversation, "hello");

    let mut seqs = Vec::new();
    while seqs.len() < 5 {
        let message = first.next_matching(|message| seq_of(message).is_some()).await;
        seqs.push(seq_of(&message).unwrap());
    }

    let acked = seqs[1];
    first.send(ControlToNode::Ack { conversation_id: conversation, seq: acked });
    first.nothing_more().await;
    first.drop_connection();

    let mut second = control.accept().await;
    let hello = second.next().await;
    let NodeToControl::Hello { sessions, .. } = hello else {
        panic!("the reconnect must open with hello, got {hello:?}");
    };
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].last_seq, acked);

    let mut replayed = Vec::new();
    while replayed.len() < seqs.len() - 2 {
        let message = second.next_matching(|message| seq_of(message).is_some()).await;
        replayed.push(seq_of(&message).unwrap());
    }

    assert_eq!(replayed, seqs[2..]);
    assert!(second.nothing_more().await.is_none_or(|message| seq_of(&message).is_none()), "replay must not repeat messages");
}

#[tokio::test]
async fn a_restarted_daemon_reattaches_to_the_live_keeper_and_can_cancel_the_turn() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(10);
    start_conversation(&mut first, conversation).await;
    prompt(&first, conversation, "do something slow");
    let working = first.next_matching(|message| update_text(message).as_deref() == Some("working")).await;
    let working_seq = seq_of(&working).unwrap();
    first.send(ControlToNode::Ack { conversation_id: conversation, seq: working_seq });
    let acked_file = fixture.config.sessions_dir().join(conversation.to_string()).join("acked");
    let deadline = tokio::time::Instant::now() + WAIT;
    while std::fs::read_to_string(&acked_file).ok().and_then(|text| text.trim().parse::<i64>().ok()) != Some(working_seq) {
        assert!(tokio::time::Instant::now() < deadline, "the node never recorded the acknowledgement");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    daemon.crash();

    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;
    let hello = second.next_matching(is_hello).await;
    let NodeToControl::Hello { sessions, .. } = hello else {
        unreachable!();
    };
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].conversation_id, conversation);
    assert_eq!(sessions[0].last_seq, working_seq);

    second.send(ControlToNode::Cancel { conversation_id: conversation });

    let turn_end = second.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert!(matches!(&turn_end, NodeToControl::TurnEnd { stop_reason, seq, .. } if stop_reason == "cancelled" && *seq > working_seq));
}

#[tokio::test]
async fn a_restarted_daemon_replays_what_it_never_got_acknowledged() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(11);
    start_conversation(&mut first, conversation).await;
    prompt(&first, conversation, "hello");
    first.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;

    daemon.crash();
    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;

    let started = second.next_matching(|message| matches!(message, NodeToControl::SessionStarted { .. })).await;
    assert!(matches!(started, NodeToControl::SessionStarted { acp_session_id, .. } if acp_session_id == "fake-session-1"));
    let texts: Vec<String> = {
        let mut texts = Vec::new();
        while texts.len() < 3 {
            let message = second.next_matching(|message| update_text(message).is_some()).await;
            texts.push(update_text(&message).unwrap());
        }

        texts
    };
    assert_eq!(texts, ["one", "two", "three"]);
}

#[tokio::test]
async fn a_daemon_restarted_while_a_session_is_idle_writes_and_acknowledges_a_new_prompt() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(31);
    start_conversation(&mut first, conversation).await;
    prompt(&first, conversation, "hello");
    first.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;

    daemon.crash();
    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;
    second.next_matching(is_hello).await;
    let prompt_id = Uuid::new_v4();
    second.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id, text: "again".into(), attachments: vec![] });

    let received = second.next_matching(|message| matches!(message, NodeToControl::PromptReceived { .. })).await;
    assert_eq!(received, NodeToControl::PromptReceived { conversation_id: conversation, prompt_id });

    for _ in 0..500 {
        if journalled_prompts(&fixture, conversation) == 2 {
            break;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(journalled_prompts(&fixture, conversation), 2);
}

#[tokio::test]
async fn a_reattached_keeper_that_is_killed_is_reported_as_an_ended_session() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(32);
    start_conversation(&mut first, conversation).await;

    daemon.crash();
    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;
    second.next_matching(is_hello).await;
    common::kill_keepers(&fixture.config.sessions_dir());

    let failed = second.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    assert!(matches!(failed, NodeToControl::SessionFailed { conversation_id, .. } if conversation_id == conversation));
}

#[tokio::test]
async fn a_revoked_node_deletes_its_credential_and_exits_cleanly() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let connection = control.accept().await;
    assert!(fixture.config_file().exists());

    connection.send(ControlToNode::Revoked);

    let result = tokio::task::spawn_blocking(move || daemon.exit_result()).await.unwrap();
    assert_eq!(result, Ok(()));
    assert!(!fixture.config_file().exists());
}

#[tokio::test]
async fn a_node_whose_control_plane_goes_silent_gives_up_on_the_link_and_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    fixture.options.liveness = Liveness { ping_every: Duration::from_millis(50), dead_after: Duration::from_millis(400) };
    let _daemon = fixture.start_daemon();

    let (first, _) = tokio::time::timeout(WAIT, listener.accept()).await.expect("the node never connected").unwrap();
    let _silent = tokio_tungstenite::accept_async(first).await.unwrap();

    let (second, _) = tokio::time::timeout(WAIT, listener.accept()).await.expect("the node never reconnected after the silence").unwrap();
    tokio_tungstenite::accept_async(second).await.unwrap();
}

#[tokio::test]
async fn a_node_pings_the_control_plane_so_a_quiet_but_healthy_link_stays_up() {
    let mut control = ControlServer::start().await;
    let mut fixture = Fixture::new(&control.url);
    fixture.options.liveness = Liveness { ping_every: Duration::from_millis(50), dead_after: Duration::from_millis(400) };
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    connection.next_matching(is_hello).await;

    tokio::time::sleep(Duration::from_millis(1200)).await;

    assert!(tokio::time::timeout(Duration::from_millis(300), control.connections.recv()).await.is_err(), "the node dropped a link that was answering its pings");
}

#[tokio::test]
async fn a_node_the_control_plane_rejects_with_401_deletes_its_credential_and_exits_cleanly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0u8; 4096];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer).await;
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
        }
    });
    let daemon = fixture.start_daemon();
    assert!(fixture.config_file().exists());

    let result = tokio::task::spawn_blocking(move || daemon.exit_result()).await.unwrap();

    assert_eq!(result, Ok(()));
    assert!(!fixture.config_file().exists());
}

#[test]
fn killing_keepers_ends_their_whole_process_group() {
    use std::os::unix::process::CommandExt;

    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let mut keeper = std::process::Command::new("sh").args(["-c", "sleep 300 & wait", "keeper", "--dir"]).arg(sessions.join("a")).process_group(0).spawn().unwrap();
    let started = std::time::Instant::now();
    while common::keeper_pids(&sessions).is_empty() {
        assert!(started.elapsed() < WAIT, "the stand-in keeper never appeared");
        std::thread::sleep(Duration::from_millis(20));
    }

    common::kill_keepers(&sessions);

    assert!(!keeper.wait().unwrap().success());
    assert!(common::keeper_pids(&sessions).is_empty());
}

#[tokio::test]
async fn ping_is_answered_with_pong() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    connection.send(ControlToNode::Ping);

    connection.next_matching(|message| matches!(message, NodeToControl::Pong)).await;
}

#[tokio::test]
async fn a_login_streams_its_output_and_reports_the_finish() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let login_id = Uuid::from_u128(12);

    connection.send(ControlToNode::StartLogin { login_id, agent: AgentId::Claude, method: "oauth_token".into(), account: None });

    let mut output = String::new();
    while !output.contains("sign-in-link") {
        let message = connection.next_matching(|message| matches!(message, NodeToControl::LoginOutput { .. })).await;
        let NodeToControl::LoginOutput { data, .. } = message else {
            unreachable!();
        };
        output.push_str(&data);
    }

    let finished = connection.next_matching(|message| matches!(message, NodeToControl::LoginFinished { .. })).await;
    assert_eq!(finished, NodeToControl::LoginFinished { login_id, ok: true });
    connection.next_matching(|message| matches!(message, NodeToControl::Logins { .. })).await;
}

#[tokio::test]
async fn removing_a_login_deletes_its_account_folder_but_never_the_agents_home_login() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let managed = fixture.config.state_dir.join("accounts").join("claude").join("account-1970");
    let home = fixture.options.home.join(".claude");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(fixture.options.home.join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "sam@example.com"}}"#).unwrap();
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    connection.send(ControlToNode::RemoveLogin { agent: AgentId::Claude, account: "account-1970".into() });
    connection.send(ControlToNode::RemoveLogin { agent: AgentId::Claude, account: "sam@example.com".into() });
    connection.send(ControlToNode::StartLogin { login_id: Uuid::from_u128(14), agent: AgentId::Claude, method: "carrier_pigeon".into(), account: None });
    connection.next_matching(|message| matches!(message, NodeToControl::LoginFinished { .. })).await;

    assert!(!managed.exists());
    assert!(home.exists());
}

#[tokio::test]
async fn a_login_with_an_unknown_method_is_refused_with_an_explanation() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let login_id = Uuid::from_u128(13);

    connection.send(ControlToNode::StartLogin { login_id, agent: AgentId::Claude, method: "carrier_pigeon".into(), account: None });

    let output = connection.next_matching(|message| matches!(message, NodeToControl::LoginOutput { .. })).await;
    let NodeToControl::LoginOutput { data, .. } = output else {
        unreachable!();
    };
    assert!(data.contains("carrier_pigeon"), "the explanation must name the method, got {data:?}");
    let finished = connection.next_matching(|message| matches!(message, NodeToControl::LoginFinished { .. })).await;
    assert_eq!(finished, NodeToControl::LoginFinished { login_id, ok: false });
}

#[tokio::test]
async fn repeated_refusals_for_one_conversation_use_increasing_seqs() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(14);

    let mut seqs = Vec::new();
    for _ in 0..3 {
        connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "nowhere".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
        let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
        seqs.push(seq_of(&failed).unwrap());
    }

    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "refusal seqs must increase, got {seqs:?}");
}

#[tokio::test]
async fn a_session_started_after_a_refusal_continues_above_the_refusal_seq() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(15);

    connection.send(ControlToNode::StartSession { conversation_id: conversation, agent: AgentId::Claude, cwd: "nowhere".into(), resume: None, fork: false, account: None, instructions: None, reload: false, env: vec![], mcp_token: String::new() });
    let failed = connection.next_matching(|message| matches!(message, NodeToControl::SessionFailed { .. })).await;
    let refused_at = seq_of(&failed).unwrap();

    start_conversation(&mut connection, conversation).await;
    prompt(&connection, conversation, "hello");
    let update = connection.next_matching(|message| update_text(message).is_some()).await;

    assert!(seq_of(&update).unwrap() > refused_at);
}

#[tokio::test]
async fn a_prompt_is_acknowledged_once_it_is_written_to_the_agent() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(16);
    let prompt_id = Uuid::new_v4();
    start_conversation(&mut connection, conversation).await;

    connection.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id, text: "hello".into(), attachments: vec![] });

    let received = connection.next_matching(|message| matches!(message, NodeToControl::PromptReceived { .. })).await;
    assert_eq!(received, NodeToControl::PromptReceived { conversation_id: conversation, prompt_id });

    for _ in 0..500 {
        if journalled_prompts(&fixture, conversation) > 0 {
            break;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(journalled_prompts(&fixture, conversation), 1);
}

#[tokio::test]
async fn a_prompt_sent_twice_reaches_the_agent_once_and_is_acknowledged_twice() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(17);
    let prompt_id = Uuid::new_v4();
    start_conversation(&mut connection, conversation).await;

    for _ in 0..2 {
        connection.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id, text: "hello".into(), attachments: vec![] });
        connection.next_matching(|message| matches!(message, NodeToControl::PromptReceived { .. })).await;
    }

    connection.next_matching(|message| matches!(message, NodeToControl::TurnEnd { .. })).await;
    assert_eq!(journalled_prompts(&fixture, conversation), 1);
}

#[tokio::test]
async fn a_prompt_received_before_a_daemon_restart_is_not_written_again_after_it() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(18);
    let prompt_id = Uuid::new_v4();
    start_conversation(&mut first, conversation).await;
    first.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id, text: "hello".into(), attachments: vec![] });
    first.next_matching(|message| matches!(message, NodeToControl::PromptReceived { .. })).await;

    daemon.crash();
    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;
    second.next_matching(is_hello).await;

    second.send(ControlToNode::Prompt { conversation_id: conversation, prompt_id, text: "hello".into(), attachments: vec![] });
    second.next_matching(|message| matches!(message, NodeToControl::PromptReceived { .. })).await;

    assert_eq!(journalled_prompts(&fixture, conversation), 1);
}

#[tokio::test]
async fn a_restarted_daemon_resumes_from_what_it_had_acknowledged_and_keeps_the_session_working() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let daemon = fixture.start_daemon();
    let mut first = control.accept().await;
    let conversation = Uuid::from_u128(19);
    start_conversation(&mut first, conversation).await;
    prompt(&first, conversation, "hello");

    let mut highest = 0;
    for _ in 0..5 {
        let message = first.next_matching(|message| seq_of(message).is_some()).await;
        highest = highest.max(seq_of(&message).unwrap());
    }
    first.send(ControlToNode::Ack { conversation_id: conversation, seq: highest });
    first.nothing_more().await;

    daemon.crash();
    let _restarted = fixture.start_daemon();
    let mut second = control.accept().await;
    let hello = second.next_matching(is_hello).await;
    let NodeToControl::Hello { sessions, .. } = hello else {
        unreachable!();
    };
    assert_eq!(sessions[0].last_seq, highest);

    prompt(&second, conversation, "again");
    let update = second.next_matching(|message| update_text(message).is_some()).await;

    assert!(seq_of(&update).unwrap() > highest);
    assert_eq!(update_text(&update).as_deref(), Some("one"));
}

#[tokio::test]
async fn transcripts_are_sent_from_the_acknowledged_offset() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let transcript = fixture.root.path().join("home").join("projects").join("demo").join("session.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "{\"a\":1}\n").unwrap();
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    let first = connection.next_matching(|message| matches!(message, NodeToControl::Transcript { .. })).await;
    let NodeToControl::Transcript { path, offset, data, size } = first else {
        unreachable!();
    };
    assert_eq!(offset, 0);
    assert_eq!(data, "eyJhIjoxfQo=");
    assert_eq!(size, Some(8));

    connection.send(ControlToNode::TranscriptAck { path: path.clone(), bytes: 8 });
    connection.nothing_more().await;
    std::fs::write(&transcript, "{\"a\":1}\n{\"b\":2}\n").unwrap();

    let next = connection.next_matching(|message| matches!(message, NodeToControl::Transcript { .. })).await;
    assert!(matches!(next, NodeToControl::Transcript { offset: 8, .. }));
}

#[tokio::test]
async fn pushed_instructions_are_written_for_an_installed_agent_and_the_held_version_is_reported() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    connection.send(ControlToNode::Instructions { revision: "afad776".into(), global: Some("# Rules".into()), take_over: false });

    let held = connection.next_matching(|message| matches!(message, NodeToControl::InstructionsHeld { .. })).await;
    assert_eq!(held, NodeToControl::InstructionsHeld { revision: "afad776".into(), unmanaged: vec![] });
    let written = std::fs::read_to_string(fixture.root.path().join("home").join(".claude").join("CLAUDE.md")).unwrap();
    assert!(written.ends_with("# Rules\n"), "{written}");
    assert!(!fixture.root.path().join("home").join(".codex").exists(), "Codex is not installed here");
}

#[tokio::test]
async fn pushed_instructions_never_replace_a_file_the_person_wrote() {
    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let own = fixture.root.path().join("home").join(".claude").join("CLAUDE.md");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "My own rules\n").unwrap();
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;

    connection.send(ControlToNode::Instructions { revision: "afad776".into(), global: Some("# Rules".into()), take_over: false });

    let held = connection.next_matching(|message| matches!(message, NodeToControl::InstructionsHeld { .. })).await;
    assert_eq!(held, NodeToControl::InstructionsHeld { revision: "afad776".into(), unmanaged: vec![format!("Claude Code: {}", own.display())] });
    assert_eq!(std::fs::read_to_string(&own).unwrap(), "My own rules\n");
}

#[tokio::test]
async fn a_pushed_api_token_is_saved_for_the_conversation_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(7);

    connection.send(ControlToNode::ApiToken { conversation_id: conversation, token: "myc_client_example".into() });
    let saved = connection.next_matching(|message| matches!(message, NodeToControl::ApiTokenSaved { .. })).await;

    let path = fixture.session_dir(conversation).join("api-token");
    assert_eq!(saved, NodeToControl::ApiTokenSaved { conversation_id: conversation, path: Some(path.display().to_string()) });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "myc_client_example");
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[tokio::test]
async fn a_pushed_ssh_token_is_saved_for_the_conversation_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let mut control = ControlServer::start().await;
    let fixture = Fixture::new(&control.url);
    let _daemon = fixture.start_daemon();
    let mut connection = control.accept().await;
    let conversation = Uuid::from_u128(7);
    let request = Uuid::from_u128(8);

    connection.send(ControlToNode::SshToken { request_id: request, conversation_id: conversation, token: "myc_ssh_example".into() });
    let saved = connection.next_matching(|message| matches!(message, NodeToControl::SshTokenSaved { .. })).await;

    let path = fixture.session_dir(conversation).join(format!("ssh-token-{request}"));
    assert_eq!(saved, NodeToControl::SshTokenSaved { request_id: request, path: Some(path.display().to_string()) });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "myc_ssh_example");
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}
