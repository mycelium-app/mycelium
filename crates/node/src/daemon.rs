use crate::acp::Command;
use crate::agents::{ALL_AGENTS, account_env, account_homes, accounts_dir, detect_agents, find_account, login_command, login_state, new_account, resolve_program};
use crate::usage::{CODEX_USAGE_ENDPOINT, USAGE_ENDPOINT, USAGE_MIN_AGE, UsageCache};
use crate::instructions::deliver;
use crate::config::{NodeConfig, agent_command, codex_command, agent_env, config_path, with_developer_instructions};
use crate::join::hostname;
use crate::link::{Backoff, Liveness, run_link};
use crate::login::LoginManager;
use crate::resources::Watcher;
use crate::ssh::SshServer;
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

const UPDATE_EXIT_DELAY: Duration = Duration::from_millis(500);
const RESOURCE_INTERVAL: Duration = Duration::from_secs(3);
const USAGE_INTERVAL: Duration = Duration::from_secs(310);

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
    pub usage_endpoint: String,
    pub codex_usage_endpoint: String,
    pub codex_command: String,
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
            usage_endpoint: USAGE_ENDPOINT.to_string(),
            codex_usage_endpoint: CODEX_USAGE_ENDPOINT.to_string(),
            codex_command: codex_command(),
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
    usage: Arc<UsageCache>,
    terminals: TerminalManager,
    ssh: SshServer,
    watcher: Watcher,
    transcripts: Mutex<TranscriptFollower>,
    connected: AtomicBool,
}

pub async fn run_daemon(config: NodeConfig, config_file: PathBuf, options: DaemonOptions) -> Result<()> {
    std::fs::create_dir_all(config.sessions_dir())?;

    let (out, out_rx) = mpsc::unbounded_channel();
    let agents = detect_agents(|agent| (options.agent_commands)(agent)).await;
    let logins_home = options.home.clone();
    let logins_accounts = accounts_dir(&config.state_dir);
    let usage = Arc::new(UsageCache::new(&options.usage_endpoint, &options.codex_usage_endpoint, &options.codex_command, USAGE_MIN_AGE));
    let logins_usage = usage.clone();
    let login_report = Arc::new(move || NodeToControl::Logins { logins: login_state(&logins_home, &logins_accounts, &agents, &logins_usage) });
    let transcripts = TranscriptFollower::new(options.home.clone(), options.transcript_roots.clone(), config.state_dir.join("transcripts.json"), out.clone());

    let daemon = Arc::new(Daemon {
        logins: LoginManager::new(out.clone(), login_report),
        usage,
        terminals: TerminalManager::new(out.clone(), find_on_path("tmux"), user_shell()),
        ssh: SshServer::new(out.clone(), config.ssh_host_key_path(), options.home.clone(), user_shell()),
        watcher: Watcher::new(out.clone(), PathBuf::from("/proc"), watched_disks(&config.projects_root), RESOURCE_INTERVAL),
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
    tokio::spawn(follow_usage(daemon.clone()));

    run_link(daemon, out_rx).await
}

fn watched_disks(projects_root: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let root = PathBuf::from("/");
    let on_root = |path: &Path| std::fs::metadata(path).ok().zip(std::fs::metadata("/").ok()).is_some_and(|(left, right)| left.dev() == right.dev());

    if projects_root.exists() && !on_root(projects_root) {
        return vec![root, projects_root.to_path_buf()];
    }

    vec![root]
}

async fn follow_transcripts(daemon: Arc<Daemon>) {
    let mut ticker = tokio::time::interval(daemon.options.transcript_poll);

    loop {
        ticker.tick().await;

        let connected = daemon.connected.load(Ordering::SeqCst);
        let Ok(mut transcripts) = daemon.transcripts.lock() else {
            return;
        };

        transcripts.roots = crate::agents::transcript_roots(&daemon.options.home, &accounts_dir(&daemon.config.state_dir), &daemon.options.transcript_roots);
        transcripts.poll(connected, |acp_session_id| daemon.conversation_for_session(acp_session_id));
    }
}

async fn follow_usage(daemon: Arc<Daemon>) {
    let mut ticker = tokio::time::interval(USAGE_INTERVAL);

    loop {
        ticker.tick().await;

        let accounts = accounts_dir(&daemon.config.state_dir);
        let folders: Vec<PathBuf> = [AgentId::Claude, AgentId::Codex].into_iter().flat_map(|agent| account_homes(&daemon.options.home, &accounts, agent)).map(|account| account.dir).collect();

        daemon.usage.refresh(&folders).await;

        if daemon.connected.load(Ordering::SeqCst) {
            let report = daemon.logins_report().await;

            let _ = daemon.out.send(report);
        }
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

    fn use_reset(&self, request_id: Uuid, agent: AgentId, account: Option<String>, idempotency_key: String) {
        let accounts = accounts_dir(&self.config.state_dir);
        let found = match &account {
            Some(label) => find_account(&self.options.home, &accounts, agent, label),
            None => account_homes(&self.options.home, &accounts, agent).into_iter().find(|home| home.profile.is_none()),
        };
        let out = self.out.clone();
        let usage = self.usage.clone();
        let report = self.logins.report_state();
        let codex = self.options.codex_command.clone();

        tokio::spawn(async move {
            let (Some(found), AgentId::Codex) = (found, agent) else {
                let _ = out.send(NodeToControl::ResetUsed { request_id, outcome: None, error: Some("That account is not signed in to Codex on this node.".to_string()) });

                return;
            };

            let used = crate::codex_app_server::use_reset(&codex, &found.dir, &idempotency_key).await;
            usage.invalidate(&found.dir);
            usage.refresh(std::slice::from_ref(&found.dir)).await;
            let _ = out.send(report());
            let (outcome, error) = match used {
                Ok(outcome) => (Some(outcome), None),
                Err(error) => (None, Some(error)),
            };
            let _ = out.send(NodeToControl::ResetUsed { request_id, outcome, error });
        });
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
        self.ssh.close_all();
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
            ControlToNode::StartSession { conversation_id, agent, cwd, resume, fork, account, instructions, env } => self.start_session(conversation_id, agent, &cwd, resume, fork, account, instructions, env),
            ControlToNode::Instructions { revision, global, take_over } => self.hold_instructions(revision, global, take_over),
            ControlToNode::Prompt { conversation_id, prompt_id, text, attachments } => self.prompt(conversation_id, prompt_id, text, attachments),
            ControlToNode::Steer { conversation_id, request_id, text, attachments } => self.steer(conversation_id, request_id, text, attachments),
            ControlToNode::Cancel { conversation_id } => self.with_session(conversation_id, |session| session.command(Command::Cancel)),
            ControlToNode::PermissionResponse { conversation_id, request_id, outcome } => self.with_session(conversation_id, |session| session.command(Command::PermissionResponse { request_id, outcome })),
            ControlToNode::SetConfig { conversation_id, mode, model, config } => self.with_session(conversation_id, |session| session.command(Command::SetConfig { mode, model, config })),
            ControlToNode::CloseSession { conversation_id } => self.with_session(conversation_id, |session| session.close()),
            ControlToNode::StartLogin { login_id, agent, method, account } => match (self.options.login_commands)(agent, &method) {
                Some(command) => self.start_login(login_id, agent, command, account),
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
            ControlToNode::RestoreTranscript { path, offset, data } => {
                let roots = crate::agents::transcript_roots(&self.options.home, &accounts_dir(&self.config.state_dir), &self.options.transcript_roots);

                if let Err(error) = crate::transcripts::restore(&self.options.home, &roots, &path, offset, &data) {
                    tracing::warn!("could not restore a transcript: {error}");
                }
            }
            ControlToNode::ContinueSequence { conversation_id, after } => crate::session::continue_sequence(&self.session_dir(conversation_id), after),
            ControlToNode::ListFiles { request_id, cwd, query } => {
                let out = self.out.clone();

                tokio::task::spawn_blocking(move || {
                    let paths = crate::files::matching_files(std::path::Path::new(&cwd), &query);
                    let _ = out.send(NodeToControl::Files { request_id, paths });
                });
            }
            ControlToNode::WriteFolder { root, files, replace } => {
                if let Err(error) = crate::assistant_folder::write(&self.config.projects_root, &root, &files, replace) {
                    tracing::warn!("could not write the assistant folder {root}: {error}");
                }
            }
            ControlToNode::ReadFolder { request_id, root } => {
                let out = self.out.clone();
                let projects_root = self.config.projects_root.clone();

                tokio::task::spawn_blocking(move || {
                    let message = match crate::assistant_folder::read(&projects_root, &root) {
                        Ok(read) => NodeToControl::FolderFiles { request_id, files: read.files, truncated: read.truncated, error: None },
                        Err(error) => NodeToControl::FolderFiles { request_id, files: Vec::new(), truncated: false, error: Some(error) },
                    };
                    let _ = out.send(message);
                });
            }
            ControlToNode::Git { request_id, cwd, command } => {
                let out = self.out.clone();
                let root = self.config.projects_root.clone();
                let home = self.options.home.clone();
                let transcript_roots = crate::agents::transcript_roots(&self.options.home, &accounts_dir(&self.config.state_dir), &self.options.transcript_roots);

                tokio::task::spawn_blocking(move || {
                    let session_id = match &command {
                        mycelium_proto::link::GitCommand::Snapshot { session_id } => session_id.clone(),
                        _ => None,
                    };
                    let message = match crate::git::run_in(&root, &cwd, command) {
                        Ok(mut output) => {
                            if let Some((path, bytes)) = session_id.and_then(|session_id| crate::transcripts::locate(&home, &transcript_roots, &session_id)) {
                                output["transcript"] = serde_json::json!({"path": path, "bytes": bytes});
                            }

                            NodeToControl::GitResult { request_id, ok: true, output, error: None }
                        }
                        Err(error) => NodeToControl::GitResult { request_id, ok: false, output: serde_json::Value::Null, error: Some(error) },
                    };
                    let _ = out.send(message);
                });
            }
            ControlToNode::Watch { seconds } => {
                self.watcher.extend(Duration::from_secs(seconds.into()));
            }
            ControlToNode::StopProcess { request_id, pid, name } => {
                let out = self.out.clone();
                let proc_root = self.watcher.proc_root().to_path_buf();

                tokio::task::spawn_blocking(move || {
                    let stopped = crate::resources::Sampler::new(proc_root, Vec::new()).stop(pid, &name);
                    let _ = out.send(NodeToControl::ProcessStopped { request_id, ok: stopped.is_ok(), error: stopped.err() });
                });
            }
            ControlToNode::UseReset { request_id, agent, account, idempotency_key } => self.use_reset(request_id, agent, account, idempotency_key),
            ControlToNode::ApiToken { conversation_id, token } => {
                let saved = self.config.save_api_token(conversation_id, &token);

                if let Err(error) = &saved {
                    tracing::warn!("could not save an API token for a conversation: {error}");
                }

                let path = saved.ok().map(|path| path.display().to_string());
                let _ = self.out.send(NodeToControl::ApiTokenSaved { conversation_id, path });
            }
            ControlToNode::SshToken { request_id, conversation_id, token } => {
                let saved = self.config.save_ssh_token(request_id, conversation_id, &token);

                if let Err(error) = &saved {
                    tracing::warn!("could not save an SSH token for a conversation: {error}");
                }

                let path = saved.ok().map(|path| path.display().to_string());
                let _ = self.out.send(NodeToControl::SshTokenSaved { request_id, path });
            }
            ControlToNode::SshOpen { session_id } => self.ssh.open(session_id),
            ControlToNode::SshData { session_id, data } => self.ssh.data(session_id, &data),
            ControlToNode::SshClose { session_id } => self.ssh.close(session_id),
            ControlToNode::Ping => {
                let _ = self.out.send(NodeToControl::Pong);
            }
            ControlToNode::Update { releases_url, version } => self.update(releases_url, version),
            ControlToNode::Revoked => {}
        }
    }

    fn update(&self, releases_url: String, version: String) {
        if crate::update::is_current(VERSION, &version) {
            return;
        }

        let out = self.out.clone();

        tokio::spawn(async move {
            let asset = crate::update::asset_name(std::env::consts::OS, std::env::consts::ARCH);
            let installed = match (asset, crate::update::running_binary()) {
                (Ok(asset), Ok(target)) => crate::update::install(&reqwest::Client::new(), &releases_url, &version, &asset, &target).await,
                (Err(message), _) | (_, Err(message)) => Err(message),
            };

            let ok = installed.is_ok();
            let message = installed.err().unwrap_or_else(|| format!("Updated to {version}"));
            let _ = out.send(NodeToControl::UpdateResult { ok, message });

            if ok {
                tokio::time::sleep(UPDATE_EXIT_DELAY).await;
                std::process::exit(0);
            }
        });
    }

    async fn agents(&self) -> Vec<AgentInfo> {
        detect_agents(|agent| (self.options.agent_commands)(agent)).await
    }

    fn logins_for(&self, agents: &[AgentInfo]) -> Vec<LoginInfo> {
        login_state(&self.options.home, &accounts_dir(&self.config.state_dir), agents, &self.usage)
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

    fn prompt(&self, conversation_id: Uuid, prompt_id: Uuid, text: String, attachments: Vec<mycelium_proto::link::PromptAttachment>) {
        let handle = self.sessions.lock().ok().and_then(|sessions| sessions.get(&conversation_id).cloned());

        match handle {
            Some(handle) if !handle.ended() => handle.command(Command::Prompt { prompt_id, text, attachments }),
            _ => self.refuse(conversation_id, "The agent for this conversation is not running on this node.".to_string()),
        }
    }

    fn steer(&self, conversation_id: Uuid, request_id: Uuid, text: String, attachments: Vec<mycelium_proto::link::PromptAttachment>) {
        let handle = self.sessions.lock().ok().and_then(|sessions| sessions.get(&conversation_id).cloned());

        match handle {
            Some(handle) if !handle.ended() => handle.command(Command::Steer { request_id, text, attachments }),
            _ => {
                let _ = self.out.send(NodeToControl::Steered { request_id, outcome: "unsupported".into(), error: None });
            }
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

    fn start_login(&self, login_id: Uuid, agent: AgentId, command: Vec<String>, account: Option<String>) {
        let Some(label) = account else {
            self.logins.start(login_id, command, Vec::new(), None);

            return;
        };

        let accounts = accounts_dir(&self.config.state_dir);
        let found = find_account(&self.options.home, &accounts, agent, &label);

        let created = found.is_none();
        match found.map(Ok).unwrap_or_else(|| new_account(&accounts, agent, &label)) {
            Ok(account) => {
                let discard_on_failure = created.then(|| account.dir.clone());
                self.logins.start(login_id, command, account_env(agent, &account), discard_on_failure);
            }
            Err(error) => self.logins.refuse(login_id, format!("Could not create a place for {label} on this node: {error}.")),
        }
    }

    fn start_session(&self, conversation_id: Uuid, agent: AgentId, cwd: &str, resume: Option<String>, fork: bool, account: Option<String>, instructions: Option<String>, secrets: Vec<(String, String)>) {
        let existing = self.session_handles();
        let running_here = existing.iter().find(|(id, handle)| *id == conversation_id && handle.running());
        if let Some((_, handle)) = running_here {
            if handle.launched_as(agent, &account) {
                return;
            }

            handle.close();
        }

        let running = existing.iter().filter(|(_, handle)| handle.running()).count();
        if running >= self.config.max_sessions {
            let least_recently_used = existing.iter().filter(|(_, handle)| handle.idle()).min_by_key(|(_, handle)| handle.last_active());

            let Some((idle_id, idle)) = least_recently_used else {
                self.refuse(conversation_id, format!("This node is already running {running} conversations, its limit, and every one of them is busy. Wait for one to finish or stop one, or raise max_sessions in {} on this node and restart it.", self.config_file.display()));

                return;
            };

            idle.close();
            let _ = self.out.send(NodeToControl::SessionClosed { conversation_id: *idle_id });
        }

        let agent_command = (self.options.agent_commands)(agent);
        let available = agent_command.first().and_then(|program| crate::agents::resolve_program(program)).is_some();
        if !available {
            self.refuse(conversation_id, format!("{} is not installed on this node.", agent.as_str()));

            return;
        }

        let mut env = agent_env(agent);
        env.extend(secrets);

        if let (AgentId::Codex, Some(text)) = (agent, &instructions) {
            env = with_developer_instructions(env, text);
        }

        let requested_account = account.clone();
        let accounts = accounts_dir(&self.config.state_dir);
        let mut chosen = None;

        if let Some(label) = account {
            let Some(found) = find_account(&self.options.home, &accounts, agent, &label) else {
                self.refuse(conversation_id, format!("{label} is not signed in on this node. Sign in to it there, or start the conversation with another account."));

                return;
            };

            env.extend(account_env(agent, &found));
            chosen = Some((label, found));
        }

        if let Some(session_id) = &resume {
            let (label, home) = match chosen {
                Some((label, found)) => (label, found),
                None => ("the default account".to_string(), crate::agents::account_homes(&self.options.home, &accounts, agent).remove(0)),
            };
            let roots = crate::agents::transcript_roots(&self.options.home, &accounts, &self.options.transcript_roots);
            let target = crate::agents::transcript_root(agent, &home);

            if let Err(error) = crate::transcripts::adopt(&self.options.home, &roots, &target, session_id) {
                self.refuse(conversation_id, format!("The conversation's transcript could not be copied to {label}: {error}"));

                return;
            }
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
            agent,
            account: requested_account,
            keeper_binary: self.options.keeper_binary.clone(),
            agent_command,
            agent_env: env,
            mcp_server: self.config.mcp_server(conversation_id),
            instructions: instructions.filter(|_| agent == AgentId::Claude),
            cwd: folder,
            resume,
            fork,
        };
        let spec = SessionSpec { conversation_id, dir: self.session_dir(conversation_id), launch: Some(launch) };
        let handle = spawn_session(spec, self.out.clone());
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(conversation_id, handle);
        }
    }

    fn hold_instructions(&self, revision: String, global: Option<String>, take_over: bool) {
        let installed: Vec<AgentId> = ALL_AGENTS.into_iter().filter(|agent| (self.options.agent_commands)(*agent).first().and_then(|program| resolve_program(program)).is_some()).collect();
        let unmanaged = match global {
            Some(global) => deliver(&self.options.home, &accounts_dir(&self.config.state_dir), &installed, &global, take_over),
            None => Vec::new(),
        };

        let _ = self.out.send(NodeToControl::InstructionsHeld { revision, unmanaged });
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
