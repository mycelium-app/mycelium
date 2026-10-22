use crate::acp::{Action, Command, Direction, SessionState, StartPlan};
use anyhow::{Context, Result, bail};
use mycelium_keeper::socket_path;
use mycelium_proto::link::NodeToControl;
use serde_json::{Value, json};
use std::collections::VecDeque;
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

#[derive(Debug, Clone)]
pub struct Launch {
    pub keeper_binary: PathBuf,
    pub agent_command: Vec<String>,
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

pub fn spawn_session(spec: SessionSpec, out: mpsc::UnboundedSender<NodeToControl>) -> SessionHandle {
    let (messages, inbox) = mpsc::unbounded_channel();
    let acked = Arc::new(AtomicI64::new(read_acked(&spec.dir)));
    let handle = SessionHandle { messages, acked: acked.clone(), ended: Arc::new(AtomicBool::new(false)), acp_session_id: Arc::new(Mutex::new(None)) };

    let actor = Actor { conversation_id: spec.conversation_id, dir: spec.dir.clone(), out, handle: handle.clone(), outbox: VecDeque::new(), started: None };
    let plan = spec.launch.as_ref().map(|launch| StartPlan { cwd: launch.cwd.display().to_string(), resume: launch.resume.clone() });
    let state = SessionState::new(spec.conversation_id, plan, acked.load(Ordering::SeqCst));
    tokio::spawn(actor.run(spec, state, inbox));

    handle
}

struct Connection {
    entries: mpsc::UnboundedReceiver<Value>,
    writer: Option<OwnedWriteHalf>,
}

struct Actor {
    conversation_id: Uuid,
    dir: PathBuf,
    out: mpsc::UnboundedSender<NodeToControl>,
    handle: SessionHandle,
    outbox: VecDeque<(i64, NodeToControl)>,
    started: Option<NodeToControl>,
}

impl Actor {
    async fn run(mut self, spec: SessionSpec, mut state: SessionState, mut inbox: mpsc::UnboundedReceiver<Message>) {
        let connection = connect(&spec).await;
        let mut connection = match connection {
            Ok(connection) => connection,
            Err(error) => {
                tracing::error!(conversation = %self.conversation_id, "keeper unavailable: {error:#}");
                self.handle.ended.store(true, Ordering::SeqCst);
                let message = NodeToControl::SessionFailed { conversation_id: self.conversation_id, seq: 1, message: format!("{error:#}") };
                self.emit(1, message);
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

        state.on_entry(seq, direction, entry.get("frame").cloned().unwrap_or(Value::Null))
    }

    async fn perform(&mut self, actions: Vec<Action>, connection: &mut Connection) {
        for action in actions {
            match action {
                Action::Emit(seq, message) => self.emit(seq, message),
                Action::Started(message) => {
                    self.started = Some(message.clone());
                    let _ = self.out.send(message);
                }
                Action::Send(frame) => write_to_keeper(connection, json!({"send": frame})).await,
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
    }

    fn replay(&self) {
        if let Some(started) = &self.started {
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

async fn connect(spec: &SessionSpec) -> Result<Connection> {
    if let Some(launch) = &spec.launch {
        launch_keeper(&spec.dir, launch).await?;
        let stream = connect_socket(&spec.dir).await?;

        return attach(stream).await;
    }

    match UnixStream::connect(socket_path(&spec.dir)).await {
        Ok(stream) => attach(stream).await,
        Err(_) => replay_ended_journal(&spec.dir),
    }
}

async fn attach(stream: UnixStream) -> Result<Connection> {
    let (read_half, mut write_half) = stream.into_split();
    let (entries_tx, entries) = mpsc::unbounded_channel();

    let attach = format!("{}\n", json!({"attach": {"after": 0}}));
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

fn replay_ended_journal(dir: &Path) -> Result<Connection> {
    let journal = std::fs::read_to_string(dir.join("journal.jsonl")).with_context(|| format!("The agent session at {} is gone.", dir.display()))?;
    let (entries_tx, entries) = mpsc::unbounded_channel();

    for line in journal.lines() {
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            let _ = entries_tx.send(entry);
        }
    }

    let _ = entries_tx.send(json!({"live": true}));

    let exit_code = std::fs::read_to_string(dir.join("exit.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|exit| exit["exit"].as_i64()).unwrap_or(-1);
    let _ = entries_tx.send(json!({"exit": exit_code}));

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
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(acked_path(dir));
    let _ = std::fs::remove_file(socket_path(dir));
    let _ = std::fs::remove_file(dir.join("exit.json"));

    let mut command = tokio::process::Command::new(&launch.keeper_binary);
    command
        .arg("keeper")
        .arg("--dir")
        .arg(dir)
        .arg("--")
        .args(&launch.agent_command)
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
