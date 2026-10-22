use crate::acp::Command;
use crate::agents::{detect_agents, login_command, login_state};
use crate::config::{NodeConfig, agent_command, agent_env, config_path};
use crate::join::hostname;
use crate::link::{Backoff, Liveness, run_link};
use crate::login::LoginManager;
use crate::terminal::{TerminalManager, find_on_path, user_shell};
use crate::session::{Launch, SessionHandle, SessionSpec, next_failure_seq, spawn_session};
use crate::transcripts::TranscriptFollower;
use anyhow::{Context, Result};
use mycelium_proto::conversation::{AgentId, AgentInfo, LoginInfo, SessionPosition};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const VERSION: &str = match option_env!("MYCELIUM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

type CommandSource = Arc<dyn Fn(AgentId) -> Vec<String> + Send + Sync>;
type LoginCommandSource = Arc<dyn Fn(AgentId, &str) -> Option<Vec<String>> + Send + Sync>;

#[derive(Clone)]
pub struct DaemonOptions {
    pub keeper_binary: PathBuf,
    pub agent_commands: CommandSource,
    pub login_commands: LoginCommandSource,
    pub backoff: Backoff,
    pub liveness: Liveness,
    pub home: PathBuf,
    pub transcript_roots: Vec<PathBuf>,
    pub transcript_poll: Duration,
}

impl DaemonOptions {
    pub fn from_environment() -> Result<DaemonOptions> {
        let home = dirs::home_dir().context("This machine has no home directory.")?;

        Ok(DaemonOptions {
            keeper_binary: std::env::current_exe().context("find the mycelium binary")?,
            agent_commands: Arc::new(agent_command),
            login_commands: Arc::new(login_command),
            backoff: Backoff::default(),
            liveness: Liveness::default(),
            transcript_roots: vec![home.join(".claude").join("projects"), home.join(".codex").join("sessions")],
            transcript_poll: Duration::from_secs(2),
            home,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusFile {
    pub pid: u32,
    pub connected: bool,
}

pub fn status_file_path(state_dir: &Path) -> PathBuf {
    state_dir.join("status.json")
}

pub struct Daemon {
    config: NodeConfig,
    config_file: PathBuf,
    options: DaemonOptions,
    out: mpsc::UnboundedSender<NodeToControl>,
    sessions: Mutex<HashMap<Uuid, SessionHandle>>,
    logins: LoginManager,
    terminals: TerminalManager,
    transcripts: Mutex<TranscriptFollower>,
    connected: AtomicBool,
}

pub async fn run_daemon(config: NodeConfig, config_file: PathBuf, options: DaemonOptions) -> Result<()> {
    std::fs::create_dir_all(config.sessions_dir())?;

    let (out, out_rx) = mpsc::unbounded_channel();
    let agents = detect_agents(|agent| (options.agent_commands)(agent)).await;
    let logins_home = options.home.clone();
    let login_report = Arc::new(move || NodeToControl::Logins { logins: login_state(&logins_home, &agents) });
    let transcripts = TranscriptFollower::new(options.home.clone(), options.transcript_roots.clone(), config.state_dir.join("transcripts.json"), out.clone());

    let daemon = Arc::new(Daemon {
        logins: LoginManager::new(out.clone(), login_report),
        terminals: TerminalManager::new(out.clone(), find_on_path("tmux"), user_shell()),
        transcripts: Mutex::new(transcripts),
        sessions: Mutex::new(HashMap::new()),
        connected: AtomicBool::new(false),
        config,
        config_file,
        options,
        out,
    });

    daemon.reattach_sessions();
    daemon.write_status();
    tokio::spawn(follow_transcripts(daemon.clone()));

    run_link(daemon, out_rx).await
}

async fn follow_transcripts(daemon: Arc<Daemon>) {
    let mut ticker = tokio::time::interval(daemon.options.transcript_poll);

    loop {
        ticker.tick().await;

        let connected = daemon.connected.load(Ordering::SeqCst);
        let Ok(mut transcripts) = daemon.transcripts.lock() else {
            return;
        };

        transcripts.poll(connected, |acp_session_id| daemon.conversation_for_session(acp_session_id));
    }
}

impl Daemon {
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    pub fn backoff(&self) -> &Backoff {
        &self.options.backoff
    }

    pub fn liveness(&self) -> &Liveness {
        &self.options.liveness
    }

    pub fn out(&self) -> &mpsc::UnboundedSender<NodeToControl> {
        &self.out
    }

    pub async fn hello(&self) -> NodeToControl {
        let agents = self.agents().await;
        let sessions = self.session_handles().into_iter().map(|(conversation_id, handle)| SessionPosition { conversation_id, last_seq: handle.acked(), live: !handle.ended() }).collect();

        NodeToControl::Hello { version: VERSION.to_string(), hostname: hostname(), os: std::env::consts::OS.to_string(), arch: std::env::consts::ARCH.to_string(), agents, sessions }
    }

    pub async fn logins_report(&self) -> NodeToControl {
        let agents = self.agents().await;

        NodeToControl::Logins { logins: self.logins_for(&agents) }
    }

    pub fn on_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
        self.write_status();

        if let Ok(mut transcripts) = self.transcripts.lock() {
            transcripts.on_connect();
        }

        for (_, handle) in self.session_handles() {
            handle.replay();
        }
    }

    pub fn on_disconnected(&self) {
        self.connected.store(false, Ordering::SeqCst);
        self.write_status();
    }

    pub fn forget_credential(&self) {
        if let Err(error) = std::fs::remove_file(&self.config_file) {
            tracing::warn!("could not delete the node credential: {error}");
        }
    }

    pub fn handle(&self, message: ControlToNode) {
        match message {
            ControlToNode::Ack { conversation_id, seq } => self.with_session(conversation_id, |session| session.ack(seq)),
            ControlToNode::StartSession { conversation_id, agent, cwd, resume } => self.start_session(conversation_id, agent, &cwd, resume),
            ControlToNode::Prompt { conversation_id, prompt_id, text } => self.prompt(conversation_id, prompt_id, text),
            ControlToNode::Cancel { conversation_id } => self.with_session(conversation_id, |session| session.command(Command::Cancel)),
            ControlToNode::PermissionResponse { conversation_id, request_id, outcome } => self.with_session(conversation_id, |session| session.command(Command::PermissionResponse { request_id, outcome })),
            ControlToNode::SetConfig { conversation_id, mode, model, config } => self.with_session(conversation_id, |session| session.command(Command::SetConfig { mode, model, config })),
            ControlToNode::CloseSession { conversation_id } => self.with_session(conversation_id, |session| session.command(Command::Close)),
            ControlToNode::StartLogin { login_id, agent, method } => match (self.options.login_commands)(agent, &method) {
                Some(command) => self.logins.start(login_id, command),
                None => self.logins.refuse(login_id, format!("{} cannot sign in with {method} from here. Use the method this node lists for it, or sign in on the node itself.", agent.as_str())),
            },
            ControlToNode::LoginInput { login_id, text } => self.logins.input(login_id, &text),
            ControlToNode::StopLogin { login_id } => self.logins.stop(login_id),
            ControlToNode::StartTerminal { terminal_id, conversation_id, cwd, cols, rows } => self.terminals.start(terminal_id, conversation_id, &cwd, cols, rows),
            ControlToNode::TerminalInput { terminal_id, data } => self.terminals.input(terminal_id, &data),
            ControlToNode::ResizeTerminal { terminal_id, cols, rows } => self.terminals.resize(terminal_id, cols, rows),
            ControlToNode::StopTerminal { terminal_id } => self.terminals.stop(terminal_id),
            ControlToNode::TranscriptAck { path, bytes } => {
                if let Ok(mut transcripts) = self.transcripts.lock() {
                    transcripts.ack(&path, bytes);
                }
            }
            ControlToNode::ListFiles { request_id, cwd, query } => {
                let out = self.out.clone();

                tokio::task::spawn_blocking(move || {
                    let paths = crate::files::matching_files(std::path::Path::new(&cwd), &query);
                    let _ = out.send(NodeToControl::Files { request_id, paths });
                });
            }
            ControlToNode::Git { request_id, cwd, command } => {
                let out = self.out.clone();
                let root = self.config.projects_root.clone();

                tokio::task::spawn_blocking(move || {
                    let message = match crate::git::run_in(&root, &cwd, command) {
                        Ok(output) => NodeToControl::GitResult { request_id, ok: true, output, error: None },
                        Err(error) => NodeToControl::GitResult { request_id, ok: false, output: serde_json::Value::Null, error: Some(error) },
                    };
                    let _ = out.send(message);
                });
            }
            ControlToNode::Ping => {
                let _ = self.out.send(NodeToControl::Pong);
            }
            ControlToNode::Revoked => {}
        }
    }

    async fn agents(&self) -> Vec<AgentInfo> {
        detect_agents(|agent| (self.options.agent_commands)(agent)).await
    }

    fn logins_for(&self, agents: &[AgentInfo]) -> Vec<LoginInfo> {
        login_state(&self.options.home, agents)
    }

    fn session_handles(&self) -> Vec<(Uuid, SessionHandle)> {
        let Ok(sessions) = self.sessions.lock() else {
            return Vec::new();
        };

        sessions.iter().map(|(id, handle)| (*id, handle.clone())).collect()
    }

    fn with_session(&self, conversation_id: Uuid, action: impl FnOnce(&SessionHandle)) {
        let handle = self.sessions.lock().ok().and_then(|sessions| sessions.get(&conversation_id).cloned());

        match handle {
            Some(handle) => action(&handle),
            None => tracing::warn!(conversation = %conversation_id, "the control plane addressed a conversation this node does not run"),
        }
    }

    fn prompt(&self, conversation_id: Uuid, prompt_id: Uuid, text: String) {
        let handle = self.sessions.lock().ok().and_then(|sessions| sessions.get(&conversation_id).cloned());

        match handle {
            Some(handle) if !handle.ended() => handle.command(Command::Prompt { prompt_id, text }),
            _ => self.refuse(conversation_id, "The agent for this conversation is not running on this node.".to_string()),
        }
    }

    fn conversation_for_session(&self, acp_session_id: &str) -> Option<Uuid> {
        self.session_handles().into_iter().find(|(_, handle)| handle.acp_session_id().as_deref() == Some(acp_session_id)).map(|(id, _)| id)
    }

    fn session_dir(&self, conversation_id: Uuid) -> PathBuf {
        self.config.sessions_dir().join(conversation_id.to_string())
    }

    fn reattach_sessions(&self) {
        let Ok(entries) = std::fs::read_dir(self.config.sessions_dir()) else {
            return;
        };

        for entry in entries.flatten() {
            let Some(conversation_id) = entry.file_name().to_str().and_then(|name| Uuid::parse_str(name).ok()) else {
                continue;
            };

            if !entry.path().join("journal.jsonl").exists() {
                continue;
            }

            let spec = SessionSpec { conversation_id, dir: entry.path(), launch: None };
            let handle = spawn_session(spec, self.out.clone());
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.insert(conversation_id, handle);
            }
        }
    }

    fn start_session(&self, conversation_id: Uuid, agent: AgentId, cwd: &str, resume: Option<String>) {
        let existing = self.session_handles();
        if existing.iter().any(|(id, handle)| *id == conversation_id && !handle.ended()) {
            return;
        }

        let running = existing.iter().filter(|(_, handle)| !handle.ended()).count();
        if running >= self.config.max_sessions {
            self.refuse(conversation_id, format!("This node already runs {running} conversations, its limit. Stop one or raise max_sessions."));

            return;
        }

        let agent_command = (self.options.agent_commands)(agent);
        let available = agent_command.first().and_then(|program| crate::agents::resolve_program(program)).is_some();
        if !available {
            self.refuse(conversation_id, format!("{} is not installed on this node.", agent.as_str()));

            return;
        }

        let requested = self.config.projects_root.join(cwd);
        let (Ok(folder), Ok(root)) = (requested.canonicalize(), self.config.projects_root.canonicalize()) else {
            self.refuse(conversation_id, format!("The folder {} does not exist on this node.", requested.display()));

            return;
        };

        if !folder.is_dir() {
            self.refuse(conversation_id, format!("The folder {} does not exist on this node.", requested.display()));

            return;
        }

        if !folder.starts_with(&root) {
            self.refuse(conversation_id, format!("The folder {} is outside {}. Choose a folder inside it.", requested.display(), self.config.projects_root.display()));

            return;
        }

        let launch = Launch {
            keeper_binary: self.options.keeper_binary.clone(),
            agent_command,
            agent_env: agent_env(agent),
            mcp_server: self.config.mcp_server(conversation_id),
            cwd: folder,
            resume,
        };
        let spec = SessionSpec { conversation_id, dir: self.session_dir(conversation_id), launch: Some(launch) };
        let handle = spawn_session(spec, self.out.clone());
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(conversation_id, handle);
        }
    }

    fn refuse(&self, conversation_id: Uuid, message: String) {
        let seq = next_failure_seq(&self.session_dir(conversation_id));
        let _ = self.out.send(NodeToControl::SessionFailed { conversation_id, seq, message });
    }

    fn write_status(&self) {
        let status = StatusFile { pid: std::process::id(), connected: self.connected.load(Ordering::SeqCst) };
        let Ok(text) = serde_json::to_string(&status) else {
            return;
        };

        if let Err(error) = std::fs::create_dir_all(&self.config.state_dir).and_then(|_| std::fs::write(status_file_path(&self.config.state_dir), text)) {
            tracing::warn!("could not write the status file: {error}");
        }
    }
}

pub async fn run() -> Result<()> {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))).try_init();

    let path = config_path();
    let config = NodeConfig::load(&path)?;
    let options = DaemonOptions::from_environment()?;

    run_daemon(config, path, options).await
}
