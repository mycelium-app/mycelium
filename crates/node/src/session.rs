use crate::acp::{Action, Command, Direction, SessionSnapshot, SessionState, StartPlan};
use anyhow::{Context, Result, bail};
use mycelium_keeper::socket_path;
use mycelium_proto::link::NodeToControl;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::mpsc;
use uuid::Uuid;

const KEEPER_SOCKET_ATTEMPTS: usize = 250;
const KEEPER_SOCKET_RETRY: Duration = Duration::from_millis(20);

const LONGEST_SOCKET_PATH: usize = if cfg!(target_os = "linux") { 107 } else { 103 };

#[derive(Debug, Clone)]
pub struct Launch {
    pub keeper_binary: PathBuf,
    pub agent_command: Vec<String>,
    pub agent_env: Vec<(String, String)>,
    pub mcp_server: Value,
    pub cwd: PathBuf,
    pub resume: Option<String>,
}

pub struct SessionSpec {
    pub conversation_id: Uuid,
    pub dir: PathBuf,
    pub launch: Option<Launch>,
}

enum Message {
    Command(Command),
    Ack(i64),
    Replay,
}

#[derive(Clone)]
pub struct SessionHandle {
    messages: mpsc::UnboundedSender<Message>,
    acked: Arc<AtomicI64>,
    ended: Arc<AtomicBool>,
    acp_session_id: Arc<Mutex<Option<String>>>,
}

impl SessionHandle {
    pub fn acked(&self) -> i64 {
        self.acked.load(Ordering::SeqCst)
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    pub fn acp_session_id(&self) -> Option<String> {
        self.acp_session_id.lock().ok().and_then(|id| id.clone())
    }

    pub fn command(&self, command: Command) {
        let _ = self.messages.send(Message::Command(command));
    }

    pub fn ack(&self, seq: i64) {
        let _ = self.messages.send(Message::Ack(seq));
    }

    pub fn replay(&self) {
        let _ = self.messages.send(Message::Replay);
    }
}

fn acked_path(dir: &Path) -> PathBuf {
    dir.join("acked")
}

fn read_acked(dir: &Path) -> i64 {
    std::fs::read_to_string(acked_path(dir)).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
struct SavedState {
    state: SessionSnapshot,
    started: Option<NodeToControl>,
}

fn snapshot_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

fn load_snapshot(dir: &Path, base: i64) -> Option<SavedState> {
    let saved = std::fs::read_to_string(snapshot_path(dir)).ok().and_then(|text| serde_json::from_str::<SavedState>(&text).ok())?;

    (saved.state.position() >= base).then_some(saved)
}

fn written_prompts_path(dir: &Path) -> PathBuf {
    dir.join("prompts_written")
}

fn read_written_prompts(dir: &Path) -> HashSet<Uuid> {
    std::fs::read_to_string(written_prompts_path(dir)).map(|text| text.lines().filter_map(|line| Uuid::parse_str(line.trim()).ok()).collect()).unwrap_or_default()
}

fn closed_path(dir: &Path) -> PathBuf {
    dir.join("closed")
}

fn base_path(dir: &Path) -> PathBuf {
    dir.join("seq_base")
}

fn read_base(dir: &Path) -> i64 {
    std::fs::read_to_string(base_path(dir)).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0)
}

fn failure_seq_path(dir: &Path) -> PathBuf {
    dir.join("failure_seq")
}

fn read_failure_seq(dir: &Path) -> i64 {
    std::fs::read_to_string(failure_seq_path(dir)).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0)
}

pub fn next_failure_seq(dir: &Path) -> i64 {
    let seq = journal_head(dir).max(read_failure_seq(dir)) + 1;

    if let Err(error) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(failure_seq_path(dir), seq.to_string())) {
        tracing::warn!("could not record the failure seq: {error}");
    }

    seq
}

pub fn journal_head(dir: &Path) -> i64 {
    let entries = std::fs::read_to_string(dir.join("journal.jsonl")).map(|journal| journal.lines().count() as i64).unwrap_or(0);

    read_base(dir) + entries
}

pub fn spawn_session(spec: SessionSpec, out: mpsc::UnboundedSender<NodeToControl>) -> SessionHandle {
    let (messages, inbox) = mpsc::unbounded_channel();
    let acked = Arc::new(AtomicI64::new(read_acked(&spec.dir)));
    let already_ended = spec.launch.is_none() && (spec.dir.join("exit.json").exists() || !socket_path(&spec.dir).exists());
    let handle = SessionHandle { messages, acked: acked.clone(), ended: Arc::new(AtomicBool::new(already_ended)), acp_session_id: Arc::new(Mutex::new(None)) };

    let base = if spec.launch.is_some() { start_new_journal_epoch(&spec.dir) } else { read_base(&spec.dir) };
    let saved = if spec.launch.is_none() { load_snapshot(&spec.dir, base) } else { None };
    let attach_after = saved.as_ref().map_or(0, |saved| (saved.state.position() - base) as usize);
    let started = saved.as_ref().and_then(|saved| saved.started.clone());
    if let Some(NodeToControl::SessionStarted { acp_session_id, .. }) = &started
        && let Ok(mut slot) = handle.acp_session_id.lock()
    {
        *slot = Some(acp_session_id.clone());
    }

    let acked_now = acked.load(Ordering::SeqCst);
    let mut state = match saved {
        Some(saved) => SessionState::restore(spec.conversation_id, saved.state, acked_now),
        None => {
            let plan = spec.launch.as_ref().map(|launch| StartPlan { cwd: launch.cwd.display().to_string(), resume: launch.resume.clone(), mcp_server: launch.mcp_server.clone() });

            SessionState::new(spec.conversation_id, plan, acked_now)
        }
    };
    if spec.launch.is_none() && closed_path(&spec.dir).exists() {
        state.mark_closing();
    }

    let actor = Actor {
        conversation_id: spec.conversation_id,
        dir: spec.dir.clone(),
        base,
        attach_after,
        out,
        handle: handle.clone(),
        outbox: VecDeque::new(),
        started,
        received_prompts: HashSet::new(),
        written_prompts: read_written_prompts(&spec.dir),
    };
    tokio::spawn(actor.run(spec, state, inbox));

    handle
}

fn start_new_journal_epoch(dir: &Path) -> i64 {
    let base = journal_head(dir).max(read_failure_seq(dir));

    if let Err(error) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(base_path(dir), base.to_string())) {
        tracing::warn!("could not record the journal base: {error}");
    }

    base
}

struct Connection {
    entries: mpsc::UnboundedReceiver<Value>,
    writer: Option<OwnedWriteHalf>,
}

struct Actor {
    conversation_id: Uuid,
    dir: PathBuf,
    base: i64,
    attach_after: usize,
    out: mpsc::UnboundedSender<NodeToControl>,
    handle: SessionHandle,
    outbox: VecDeque<(i64, NodeToControl)>,
    started: Option<NodeToControl>,
    received_prompts: HashSet<Uuid>,
    written_prompts: HashSet<Uuid>,
}

impl Actor {
    async fn run(mut self, spec: SessionSpec, mut state: SessionState, mut inbox: mpsc::UnboundedReceiver<Message>) {
        let connection = connect(&spec, self.attach_after).await;
        let mut connection = match connection {
            Ok(connection) => connection,
            Err(error) => {
                tracing::error!(conversation = %self.conversation_id, "keeper unavailable: {error:#}");
                self.handle.ended.store(true, Ordering::SeqCst);
                let seq = next_failure_seq(&self.dir);
                let message = NodeToControl::SessionFailed { conversation_id: self.conversation_id, seq, message: format!("{error:#}") };
                self.emit(seq, message);
                self.serve_commands_only(inbox).await;

                return;
            }
        };

        let mut stream_open = true;
        loop {
            tokio::select! {
                entry = connection.entries.recv(), if stream_open => {
                    let Some(entry) = entry else {
                        stream_open = false;
                        continue;
                    };

                    let actions = self.interpret(&mut state, entry);
                    self.perform(actions, &mut connection).await;
                }
                message = inbox.recv() => {
                    let Some(message) = message else {
                        return;
                    };

                    match message {
                        Message::Command(command) => {
                            if matches!(command, Command::Close) {
                                self.record_close_request();
                            }

                            if let Command::Prompt { prompt_id, .. } = &command
                                && self.is_duplicate_prompt(*prompt_id)
                            {
                                continue;
                            }

                            let actions = state.on_command(command);
                            self.perform(actions, &mut connection).await;
                        }
                        Message::Ack(seq) => self.acknowledge(seq, &mut state),
                        Message::Replay => self.replay(),
                    }
                }
            }
        }
    }

    async fn serve_commands_only(&mut self, mut inbox: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = inbox.recv().await {
            match message {
                Message::Ack(seq) => {
                    self.outbox.retain(|(entry_seq, _)| *entry_seq > seq);
                }
                Message::Replay => self.replay(),
                Message::Command(_) => {}
            }
        }
    }

    fn interpret(&mut self, state: &mut SessionState, entry: Value) -> Vec<Action> {
        if entry.get("live").is_some() {
            return state.on_live();
        }

        if let Some(code) = entry.get("exit") {
            self.handle.ended.store(true, Ordering::SeqCst);

            return state.on_exit(code.as_i64().unwrap_or(-1));
        }

        let Some(seq) = entry.get("seq").and_then(Value::as_i64) else {
            return Vec::new();
        };
        let direction = if entry.get("dir").and_then(Value::as_str) == Some("out") { Direction::Out } else { Direction::In };

        state.on_entry(self.base + seq, direction, entry.get("frame").cloned().unwrap_or(Value::Null))
    }

    async fn perform(&mut self, actions: Vec<Action>, connection: &mut Connection) {
        for action in actions {
            match action {
                Action::Emit(seq, message) => self.emit(seq, message),
                Action::Started(message) => {
                    self.started = Some(message.clone());

                    if !self.handle.ended() {
                        let _ = self.out.send(message);
                    }
                }
                Action::Send(frame) => write_to_keeper(connection, json!({"send": frame})).await,
                Action::PromptWritten(prompt_id) => {
                    if connection.writer.is_some() && !self.handle.ended() {
                        self.record_written_prompt(prompt_id);
                    }
                }
                Action::CloseKeeper => write_to_keeper(connection, json!({"close": true})).await,
            }
        }

        if let Ok(mut slot) = self.handle.acp_session_id.lock()
            && slot.is_none()
            && let Some(NodeToControl::SessionStarted { acp_session_id, .. }) = &self.started
        {
            *slot = Some(acp_session_id.clone());
        }
    }

    fn is_duplicate_prompt(&mut self, prompt_id: Uuid) -> bool {
        if self.written_prompts.contains(&prompt_id) {
            let _ = self.out.send(NodeToControl::PromptReceived { conversation_id: self.conversation_id, prompt_id });

            return true;
        }

        !self.received_prompts.insert(prompt_id)
    }

    fn record_written_prompt(&mut self, prompt_id: Uuid) {
        self.written_prompts.insert(prompt_id);

        let appended = std::fs::OpenOptions::new().create(true).append(true).open(written_prompts_path(&self.dir)).and_then(|mut file| writeln!(file, "{prompt_id}"));
        if let Err(error) = appended {
            tracing::warn!(conversation = %self.conversation_id, "could not record the prompt: {error}");
        }

        let _ = self.out.send(NodeToControl::PromptReceived { conversation_id: self.conversation_id, prompt_id });
    }

    fn emit(&mut self, seq: i64, message: NodeToControl) {
        self.outbox.push_back((seq, message.clone()));
        let _ = self.out.send(message);
    }

    fn acknowledge(&mut self, seq: i64, state: &mut SessionState) {
        if seq <= self.handle.acked.load(Ordering::SeqCst) {
            return;
        }

        self.handle.acked.store(seq, Ordering::SeqCst);
        state.set_acked(seq);
        self.outbox.retain(|(entry_seq, _)| *entry_seq > seq);

        if let Err(error) = std::fs::write(acked_path(&self.dir), seq.to_string()) {
            tracing::warn!(conversation = %self.conversation_id, "could not record acked seq: {error}");
        }

        if state.head() == seq {
            self.save_snapshot(state);
        }
    }

    fn save_snapshot(&self, state: &SessionState) {
        let saved = SavedState { state: state.snapshot(), started: self.started.clone() };
        let written = serde_json::to_string(&saved).map_err(std::io::Error::other).and_then(|text| std::fs::write(snapshot_path(&self.dir), text));
        if let Err(error) = written {
            tracing::warn!(conversation = %self.conversation_id, "could not record the session state: {error}");
        }
    }

    fn record_close_request(&self) {
        if let Err(error) = std::fs::write(closed_path(&self.dir), "") {
            tracing::warn!(conversation = %self.conversation_id, "could not record the close request: {error}");
        }
    }

    fn replay(&self) {
        if let Some(started) = &self.started
            && !self.handle.ended()
        {
            let _ = self.out.send(started.clone());
        }

        for (_, message) in &self.outbox {
            let _ = self.out.send(message.clone());
        }
    }
}

async fn write_to_keeper(connection: &mut Connection, message: Value) {
    let Some(writer) = connection.writer.as_mut() else {
        return;
    };

    let line = format!("{message}\n");
    if let Err(error) = writer.write_all(line.as_bytes()).await {
        tracing::warn!("keeper write failed: {error}");
    }
}

async fn connect(spec: &SessionSpec, after: usize) -> Result<Connection> {
    if let Some(launch) = &spec.launch {
        launch_keeper(&spec.dir, launch).await?;
        let stream = connect_socket(&spec.dir).await?;

        return attach(stream, 0).await;
    }

    match UnixStream::connect(socket_path(&spec.dir)).await {
        Ok(stream) => attach(stream, after).await,
        Err(_) => replay_ended_journal(&spec.dir, after),
    }
}

async fn attach(stream: UnixStream, after: usize) -> Result<Connection> {
    let (read_half, mut write_half) = stream.into_split();
    let (entries_tx, entries) = mpsc::unbounded_channel();

    let attach = format!("{}\n", json!({"attach": {"after": after}}));
    write_half.write_all(attach.as_bytes()).await?;

    tokio::spawn(async move {
        let mut lines = BufReader::new(read_half).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(entry) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            if entries_tx.send(entry).is_err() {
                break;
            }
        }
    });

    Ok(Connection { entries, writer: Some(write_half) })
}

fn replay_ended_journal(dir: &Path, after: usize) -> Result<Connection> {
    let journal = std::fs::File::open(dir.join("journal.jsonl")).with_context(|| format!("The agent session at {} is gone.", dir.display()))?;
    let (entries_tx, entries) = mpsc::unbounded_channel();
    let exit_code = std::fs::read_to_string(dir.join("exit.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|exit| exit["exit"].as_i64()).unwrap_or(-1);

    std::thread::spawn(move || {
        for line in std::io::BufReader::new(journal).lines().skip(after).map_while(|line| line.ok()) {
            if let Ok(entry) = serde_json::from_str::<Value>(&line) {
                let _ = entries_tx.send(entry);
            }
        }

        let _ = entries_tx.send(json!({"live": true}));
        let _ = entries_tx.send(json!({"exit": exit_code}));
    });

    Ok(Connection { entries, writer: None })
}

async fn connect_socket(dir: &Path) -> Result<UnixStream> {
    for _ in 0..KEEPER_SOCKET_ATTEMPTS {
        if let Ok(stream) = UnixStream::connect(socket_path(dir)).await {
            return Ok(stream);
        }

        tokio::time::sleep(KEEPER_SOCKET_RETRY).await;
    }

    bail!("The keeper for this conversation did not start. Check {}.", dir.join("stderr.log").display());
}

async fn launch_keeper(dir: &Path, launch: &Launch) -> Result<()> {
    let socket = socket_path(dir);

    if socket.as_os_str().len() > LONGEST_SOCKET_PATH {
        bail!(
            "The keeper's socket path {} is {} bytes, and Unix sockets allow {LONGEST_SOCKET_PATH}. Set state_dir in node.toml to a shorter directory, such as ~/.local/state/mycelium.",
            socket.display(),
            socket.as_os_str().len()
        );
    }

    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(acked_path(dir));
    let _ = std::fs::remove_file(snapshot_path(dir));
    let _ = std::fs::remove_file(closed_path(dir));
    let _ = std::fs::remove_file(socket_path(dir));
    let _ = std::fs::remove_file(dir.join("exit.json"));

    let mut command = tokio::process::Command::new(&launch.keeper_binary);
    command
        .arg("keeper")
        .arg("--dir")
        .arg(dir)
        .arg("--")
        .args(&launch.agent_command)
        .envs(launch.agent_env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if launch.cwd.is_dir() {
        command.current_dir(&launch.cwd);
    }

    let mut child = command.spawn().with_context(|| format!("start the keeper {}", launch.keeper_binary.display()))?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_state_directory_too_deep_for_a_unix_socket_is_refused_with_the_fix() {
        let dir = PathBuf::from("/tmp").join("d".repeat(120));
        let launch = Launch { keeper_binary: PathBuf::from("/bin/true"), agent_command: vec![], agent_env: vec![], mcp_server: Value::Null, cwd: PathBuf::from("/"), resume: None };

        let error = launch_keeper(&dir, &launch).await.unwrap_err().to_string();

        assert!(error.contains("Set state_dir in node.toml to a shorter directory"), "{error}");
        assert!(!dir.exists());
    }
}
