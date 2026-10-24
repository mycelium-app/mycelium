use crate::config::write_private_file;
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::NodeToControl;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Config, Handle, Handler, Msg, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet, Pty};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use uuid::Uuid;

const RELAY_BUFFER_BYTES: usize = 64 * 1024;
const READ_BYTES: usize = 32 * 1024;
const DRAIN_GRACE: Duration = Duration::from_millis(250);
const PIPE_GRACE: Duration = Duration::from_secs(1);
const KILLED_CODE: u32 = 137;
const COMMAND_NOT_STARTED_CODE: u32 = 127;

struct SshSession {
    input: mpsc::UnboundedSender<Vec<u8>>,
    ended: watch::Sender<bool>,
    tasks: Vec<AbortHandle>,
}

impl SshSession {
    fn end(&self) {
        let _ = self.ended.send(true);

        for task in &self.tasks {
            task.abort();
        }
    }
}

pub struct SshServer {
    out: mpsc::UnboundedSender<NodeToControl>,
    host_key_path: PathBuf,
    home: PathBuf,
    shell: String,
    host_key: Mutex<Option<PrivateKey>>,
    sessions: Arc<Mutex<HashMap<Uuid, SshSession>>>,
}

impl SshServer {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, host_key_path: PathBuf, home: PathBuf, shell: String) -> SshServer {
        SshServer { out, host_key_path, home, shell, host_key: Mutex::new(None), sessions: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn open(&self, session_id: Uuid) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };

        if sessions.contains_key(&session_id) {
            let _ = self.out.send(NodeToControl::SshClosed { session_id });

            return;
        }

        let key = match self.host_key() {
            Ok(key) => key,
            Err(error) => {
                tracing::warn!("could not load the SSH host key: {error:#}");
                let _ = self.out.send(NodeToControl::SshClosed { session_id });

                return;
            }
        };

        let (server_end, bridge_end) = tokio::io::duplex(RELAY_BUFFER_BYTES);
        let (mut reader, mut writer) = tokio::io::split(bridge_end);
        let (input, mut pending_input) = mpsc::unbounded_channel::<Vec<u8>>();
        let (ended, ended_signal) = watch::channel(false);
        let config = Arc::new(Config {
            methods: MethodSet::from(&[MethodKind::None][..]),
            keys: vec![key],
            inactivity_timeout: None,
            keepalive_interval: None,
            ..Config::default()
        });
        let handler = SshHandler { home: self.home.clone(), shell: self.shell.clone(), ended: ended_signal, channels: HashMap::new() };

        let server_task = tokio::spawn(async move {
            if let Ok(running) = russh::server::run_stream(config, server_end, handler).await {
                let _ = running.await;
            }
        });

        let writer_task = tokio::spawn(async move {
            while let Some(bytes) = pending_input.recv().await {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });

        let out = self.out.clone();
        let table = self.sessions.clone();
        let reader_task = tokio::spawn(async move {
            let mut buffer = vec![0u8; READ_BYTES];

            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let _ = out.send(NodeToControl::SshData { session_id, data: STANDARD.encode(&buffer[..count]) });
                    }
                }
            }

            if let Ok(mut sessions) = table.lock() {
                if let Some(session) = sessions.remove(&session_id) {
                    session.end();
                }
            }

            let _ = out.send(NodeToControl::SshClosed { session_id });
        });

        let tasks = vec![server_task.abort_handle(), writer_task.abort_handle(), reader_task.abort_handle()];
        sessions.insert(session_id, SshSession { input, ended, tasks });
    }

    pub fn data(&self, session_id: Uuid, data: &str) {
        let Ok(bytes) = STANDARD.decode(data) else {
            if self.remove(session_id) {
                let _ = self.out.send(NodeToControl::SshClosed { session_id });
            }

            return;
        };

        let delivered = match self.sessions.lock() {
            Ok(sessions) => sessions.get(&session_id).map(|session| session.input.send(bytes).is_ok()),
            Err(_) => return,
        };

        if delivered != Some(true) {
            self.remove(session_id);
            let _ = self.out.send(NodeToControl::SshClosed { session_id });
        }
    }

    pub fn close(&self, session_id: Uuid) {
        self.remove(session_id);
    }

    pub fn close_all(&self) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };

        for (_, session) in sessions.drain() {
            session.end();
        }
    }

    fn remove(&self, session_id: Uuid) -> bool {
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let Some(session) = sessions.remove(&session_id) else {
            return false;
        };

        session.end();

        true
    }

    fn host_key(&self) -> Result<PrivateKey> {
        let mut cached = self.host_key.lock().map_err(|_| anyhow::anyhow!("The SSH host key cache is poisoned."))?;

        if let Some(key) = cached.as_ref() {
            return Ok(key.clone());
        }

        let key = if self.host_key_path.exists() {
            let text = std::fs::read_to_string(&self.host_key_path).with_context(|| format!("Could not read the SSH host key at {}.", self.host_key_path.display()))?;

            PrivateKey::from_openssh(text).with_context(|| format!("{} is not a valid SSH host key.", self.host_key_path.display()))?
        } else {
            let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).context("Could not generate an SSH host key.")?;
            let text = key.to_openssh(LineEnding::LF).context("Could not encode the SSH host key.")?;
            write_private_file(&self.host_key_path, text.as_bytes()).with_context(|| format!("Could not write the SSH host key at {}.", self.host_key_path.display()))?;

            key
        };

        *cached = Some(key.clone());

        Ok(key)
    }
}

#[derive(Clone)]
struct PtyRequest {
    term: String,
    cols: u16,
    rows: u16,
}

enum ProcessInput {
    Data(Vec<u8>),
    Eof,
    Resize { cols: u16, rows: u16 },
}

#[derive(Default)]
struct ChannelState {
    pty: Option<PtyRequest>,
    env: Vec<(String, String)>,
    input: Option<mpsc::UnboundedSender<ProcessInput>>,
}

struct ProcessSpec {
    shell: String,
    command: Option<String>,
    home: PathBuf,
    env: Vec<(String, String)>,
    pty: Option<PtyRequest>,
}

struct SshHandler {
    home: PathBuf,
    shell: String,
    ended: watch::Receiver<bool>,
    channels: HashMap<ChannelId, ChannelState>,
}

impl SshHandler {
    fn start(&mut self, channel: ChannelId, command: Option<String>, session: &mut Session) -> bool {
        let Some(state) = self.channels.get_mut(&channel) else {
            return false;
        };

        if state.input.is_some() {
            return false;
        }

        let (input, inputs) = mpsc::unbounded_channel();
        let spec = ProcessSpec { shell: self.shell.clone(), command, home: self.home.clone(), env: state.env.clone(), pty: state.pty.clone() };
        state.input = Some(input);
        tokio::spawn(run_process(spec, session.handle(), channel, inputs, self.ended.clone()));

        true
    }

    fn send(&self, channel: ChannelId, input: ProcessInput) {
        let Some(sender) = self.channels.get(&channel).and_then(|state| state.input.as_ref()) else {
            return;
        };

        let _ = sender.send(input);
    }
}

impl Handler for SshHandler {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: ChannelOpenHandle, _session: &mut Session) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), ChannelState::default());
        reply.accept().await;

        Ok(())
    }

    async fn channel_close(&mut self, channel: ChannelId, _session: &mut Session) -> Result<(), Self::Error> {
        self.channels.remove(&channel);

        Ok(())
    }

    async fn channel_eof(&mut self, channel: ChannelId, _session: &mut Session) -> Result<(), Self::Error> {
        self.send(channel, ProcessInput::Eof);

        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], _session: &mut Session) -> Result<(), Self::Error> {
        self.send(channel, ProcessInput::Data(data.to_vec()));

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(&mut self, channel: ChannelId, term: &str, col_width: u32, row_height: u32, _pix_width: u32, _pix_height: u32, _modes: &[(Pty, u32)], session: &mut Session) -> Result<(), Self::Error> {
        let Some(state) = self.channels.get_mut(&channel) else {
            return session.channel_failure(channel);
        };

        state.pty = Some(PtyRequest { term: term.to_string(), cols: clamp_dimension(col_width), rows: clamp_dimension(row_height) });

        session.channel_success(channel)
    }

    async fn window_change_request(&mut self, channel: ChannelId, col_width: u32, row_height: u32, _pix_width: u32, _pix_height: u32, _session: &mut Session) -> Result<(), Self::Error> {
        let cols = clamp_dimension(col_width);
        let rows = clamp_dimension(row_height);

        if let Some(pty) = self.channels.get_mut(&channel).and_then(|state| state.pty.as_mut()) {
            pty.cols = cols;
            pty.rows = rows;
        }

        self.send(channel, ProcessInput::Resize { cols, rows });

        Ok(())
    }

    async fn env_request(&mut self, channel: ChannelId, variable_name: &str, variable_value: &str, session: &mut Session) -> Result<(), Self::Error> {
        let Some(state) = self.channels.get_mut(&channel) else {
            return session.channel_failure(channel);
        };

        state.env.push((variable_name.to_string(), variable_value.to_string()));

        session.channel_success(channel)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        if self.start(channel, None, session) {
            return session.channel_success(channel);
        }

        session.channel_failure(channel)
    }

    async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).into_owned();

        if self.start(channel, Some(command), session) {
            return session.channel_success(channel);
        }

        session.channel_failure(channel)
    }

    async fn subsystem_request(&mut self, channel: ChannelId, _name: &str, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }

    async fn x11_request(&mut self, channel: ChannelId, _single_connection: bool, _x11_auth_protocol: &str, _x11_auth_cookie: &str, _x11_screen_number: u32, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }
}

fn clamp_dimension(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX).max(1)
}

fn kill_group(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };

    let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL);
}

async fn run_process(spec: ProcessSpec, handle: Handle, channel: ChannelId, inputs: mpsc::UnboundedReceiver<ProcessInput>, ended: watch::Receiver<bool>) {
    let finished = match &spec.pty {
        Some(pty) => run_with_pty(&spec, pty, &handle, channel, inputs, ended).await,
        None => run_with_pipes(&spec, &handle, channel, inputs, ended).await,
    };
    let code = match finished {
        Ok(code) => code,
        Err(error) => {
            let _ = handle.extended_data(channel, 1, format!("{error:#}\r\n").into_bytes()).await;

            COMMAND_NOT_STARTED_CODE
        }
    };

    let _ = handle.exit_status_request(channel, code).await;
    let _ = handle.eof(channel).await;
    let _ = handle.close(channel).await;
}

async fn run_with_pipes(spec: &ProcessSpec, handle: &Handle, channel: ChannelId, mut inputs: mpsc::UnboundedReceiver<ProcessInput>, mut ended: watch::Receiver<bool>) -> Result<u32> {
    let mut command = tokio::process::Command::new(&spec.shell);

    if let Some(text) = &spec.command {
        command.arg("-c").arg(text);
    }

    command.current_dir(&spec.home).envs(spec.env.iter().cloned());
    command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    command.kill_on_drop(true).process_group(0);

    let mut child = command.spawn().context("Could not start the command on this node.")?;
    let pid = child.id();
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().context("Could not read the command's output.")?;
    let stderr = child.stderr.take().context("Could not read the command's errors.")?;
    let stdout_task = tokio::spawn(forward(stdout, handle.clone(), channel, None));
    let stderr_task = tokio::spawn(forward(stderr, handle.clone(), channel, Some(1)));
    let status = loop {
        tokio::select! {
            status = child.wait() => break Some(status),
            input = inputs.recv() => {
                match input {
                    Some(ProcessInput::Data(bytes)) => {
                        if let Some(pipe) = stdin.as_mut() {
                            let _ = pipe.write_all(&bytes).await;
                        }
                    }
                    Some(ProcessInput::Eof) => stdin = None,
                    Some(ProcessInput::Resize { .. }) => {}
                    None => {
                        kill_group(pid);
                        break None;
                    }
                }
            }
            _ = ended.changed() => {
                kill_group(pid);
                break None;
            }
        }
    };

    let status = match status {
        Some(status) => status?,
        None => child.wait().await?,
    };
    let _ = tokio::time::timeout(PIPE_GRACE, async {
        let _ = stdout_task.await;
        let _ = stderr_task.await;
    })
    .await;

    Ok(status.code().map(|code| code as u32).or_else(|| status.signal().map(|signal| 128 + signal as u32)).unwrap_or(1))
}

async fn forward(mut source: impl AsyncRead + Unpin, handle: Handle, channel: ChannelId, extended: Option<u32>) {
    let mut buffer = vec![0u8; READ_BYTES];

    loop {
        let Ok(count) = source.read(&mut buffer).await else {
            return;
        };

        if count == 0 {
            return;
        }

        let bytes = buffer[..count].to_vec();
        let sent = match extended {
            Some(code) => handle.extended_data(channel, code, bytes).await,
            None => handle.data(channel, bytes).await,
        };

        if sent.is_err() {
            return;
        }
    }
}

async fn run_with_pty(spec: &ProcessSpec, pty: &PtyRequest, handle: &Handle, channel: ChannelId, mut inputs: mpsc::UnboundedReceiver<ProcessInput>, mut ended: watch::Receiver<bool>) -> Result<u32> {
    let pair = native_pty_system().openpty(PtySize { rows: pty.rows, cols: pty.cols, pixel_width: 0, pixel_height: 0 }).context("Could not open a terminal on this node.")?;
    let mut builder = CommandBuilder::new(&spec.shell);

    if let Some(text) = &spec.command {
        builder.args(["-c", text]);
    }

    builder.cwd(&spec.home);

    for (name, value) in &spec.env {
        builder.env(name, value);
    }

    builder.env("TERM", &pty.term);

    let mut child = pair.slave.spawn_command(builder).context("Could not start the command on this node.")?;
    drop(pair.slave);

    let pid = child.process_id();
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let master = pair.master;
    let (chunks, mut output) = mpsc::unbounded_channel::<Vec<u8>>();
    let (exit_sender, mut exit_receiver) = oneshot::channel();

    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];

        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || chunks.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        let _ = exit_sender.send(child.wait());
    });

    let mut output_open = true;
    let mut inputs_open = true;

    let code = loop {
        tokio::select! {
            chunk = output.recv(), if output_open => {
                match chunk {
                    Some(bytes) => {
                        if handle.data(channel, bytes).await.is_err() {
                            kill_group(pid);

                            return Ok(KILLED_CODE);
                        }
                    }
                    None => output_open = false,
                }
            }
            status = &mut exit_receiver => {
                break match status {
                    Ok(Ok(status)) => status.exit_code(),
                    _ => 1,
                };
            }
            input = inputs.recv(), if inputs_open => {
                match input {
                    Some(ProcessInput::Data(bytes)) => {
                        let _ = writer.write_all(&bytes);
                        let _ = writer.flush();
                    }
                    Some(ProcessInput::Eof) => {}
                    Some(ProcessInput::Resize { cols, rows }) => {
                        let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                    }
                    None => inputs_open = false,
                }
            }
            _ = ended.changed() => {
                kill_group(pid);

                return Ok(KILLED_CODE);
            }
        }
    };

    while let Ok(Some(bytes)) = tokio::time::timeout(DRAIN_GRACE, output.recv()).await {
        if handle.data(channel, bytes).await.is_err() {
            break;
        }
    }

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::{ChannelMsg, client};
    use russh::keys::{HashAlg, PublicKeyOrCertificate};
    use std::os::unix::fs::PermissionsExt;

    const WAIT: Duration = Duration::from_secs(15);

    type Routes = Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>;

    struct Client {
        fingerprint: Arc<Mutex<Option<String>>>,
    }

    impl client::Handler for Client {
        type Error = russh::Error;

        async fn check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
            if let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key {
                *self.fingerprint.lock().unwrap() = Some(key.fingerprint(HashAlg::Sha256).to_string());
            }

            Ok(true)
        }
    }

    struct Connected {
        handle: client::Handle<Client>,
        session_id: Uuid,
        fingerprint: Arc<Mutex<Option<String>>>,
    }

    struct Output {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        status: Option<u32>,
    }

    fn start_server(key_path: PathBuf) -> (Arc<SshServer>, Routes) {
        let (out, mut messages) = mpsc::unbounded_channel();
        let server = Arc::new(SshServer::new(out, key_path, std::env::temp_dir(), "/bin/sh".to_string()));
        let routes: Routes = Arc::new(Mutex::new(HashMap::new()));
        let table = routes.clone();

        tokio::spawn(async move {
            while let Some(message) = messages.recv().await {
                if let NodeToControl::SshData { session_id, data } = message {
                    let bytes = STANDARD.decode(data).unwrap();

                    if let Some(route) = table.lock().unwrap().get(&session_id) {
                        let _ = route.send(bytes);
                    }
                }
            }
        });

        (server, routes)
    }

    async fn connect(server: &Arc<SshServer>, routes: &Routes) -> Connected {
        let session_id = Uuid::new_v4();
        let (client_end, bridge_end) = tokio::io::duplex(RELAY_BUFFER_BYTES);
        let (mut bridge_reader, mut bridge_writer) = tokio::io::split(bridge_end);
        let (to_client, mut for_client) = mpsc::unbounded_channel::<Vec<u8>>();
        routes.lock().unwrap().insert(session_id, to_client);

        tokio::spawn(async move {
            while let Some(bytes) = for_client.recv().await {
                if bridge_writer.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });

        let relay = server.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; READ_BYTES];

            while let Ok(count) = bridge_reader.read(&mut buffer).await {
                if count == 0 {
                    break;
                }

                relay.data(session_id, &STANDARD.encode(&buffer[..count]));
            }
        });

        server.open(session_id);

        let fingerprint = Arc::new(Mutex::new(None));
        let handle = client::connect_stream(Arc::new(client::Config::default()), client_end, Client { fingerprint: fingerprint.clone() }).await.unwrap();

        Connected { handle, session_id, fingerprint }
    }

    async fn signed_in(server: &Arc<SshServer>, routes: &Routes) -> Connected {
        let mut connected = connect(server, routes).await;
        let result = connected.handle.authenticate_none("anyone").await.unwrap();
        assert!(result.success());

        connected
    }

    async fn collect(channel: &mut Channel<client::Msg>) -> Output {
        let mut output = Output { stdout: Vec::new(), stderr: Vec::new(), status: None };

        loop {
            let message = tokio::time::timeout(WAIT, channel.wait()).await.expect("the command went quiet");

            match message {
                Some(ChannelMsg::Data { data }) => output.stdout.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, .. }) => output.stderr.extend_from_slice(&data),
                Some(ChannelMsg::ExitStatus { exit_status }) => output.status = Some(exit_status),
                Some(ChannelMsg::Close) | None => return output,
                Some(_) => {}
            }
        }
    }

    async fn pid_of_running_sleep(connected: &Connected) -> i32 {
        let mut channel = connected.handle.channel_open_session().await.unwrap();
        channel.exec(true, "echo $$; exec sleep 30").await.unwrap();

        let mut seen = Vec::new();

        while !seen.contains(&b'\n') {
            let message = tokio::time::timeout(WAIT, channel.wait()).await.expect("the command went quiet");

            if let Some(ChannelMsg::Data { data }) = message {
                seen.extend_from_slice(&data);
            }
        }

        String::from_utf8(seen).unwrap().trim().parse().unwrap()
    }

    async fn gone(pid: i32) -> bool {
        let deadline = tokio::time::Instant::now() + WAIT;

        while tokio::time::Instant::now() < deadline {
            if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err() {
                return true;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        false
    }

    async fn shell_output(cols: u32, rows: u32, typed: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let connected = signed_in(&server, &routes).await;
        let mut channel = connected.handle.channel_open_session().await.unwrap();

        channel.request_pty(true, "xterm", cols, rows, 0, 0, &[]).await.unwrap();
        channel.request_shell(true).await.unwrap();
        channel.data(typed.as_bytes()).await.unwrap();

        String::from_utf8_lossy(&collect(&mut channel).await.stdout).into_owned()
    }

    #[tokio::test]
    async fn a_command_run_over_ssh_returns_its_output_and_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let connected = signed_in(&server, &routes).await;
        let mut channel = connected.handle.channel_open_session().await.unwrap();

        channel.exec(true, "echo hello").await.unwrap();
        let output = collect(&mut channel).await;

        assert_eq!(output.stdout, b"hello\n");
        assert_eq!(output.status, Some(0));
    }

    #[tokio::test]
    async fn a_failing_command_reports_its_exit_status_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let connected = signed_in(&server, &routes).await;
        let mut channel = connected.handle.channel_open_session().await.unwrap();

        channel.exec(true, "echo oops >&2; exit 3").await.unwrap();
        let output = collect(&mut channel).await;

        assert_eq!(output.stderr, b"oops\n");
        assert_eq!(output.status, Some(3));
    }

    #[tokio::test]
    async fn a_shell_with_a_pty_echoes_what_is_typed() {
        let output = shell_output(80, 24, "echo typed-words\nexit\n").await;

        assert!(output.contains("typed-words"));
    }

    #[tokio::test]
    async fn a_shell_with_a_pty_sees_the_requested_size() {
        let output = shell_output(100, 30, "stty size\nexit\n").await;

        assert!(output.contains("30 100"));
    }

    #[tokio::test]
    async fn any_user_is_accepted_without_a_password_or_key() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));

        for user in ["mycelium", "root", "somebody-else"] {
            let mut connected = connect(&server, &routes).await;
            let result = connected.handle.authenticate_none(user).await.unwrap();

            assert!(result.success());
        }
    }

    #[tokio::test]
    async fn the_host_key_is_generated_once_kept_private_and_reused_by_a_later_server() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("state").join("host_key");
        let (first, first_routes) = start_server(key_path.clone());
        let (second, second_routes) = start_server(key_path.clone());

        let seen_first = connect(&first, &first_routes).await.fingerprint.lock().unwrap().clone();
        let seen_second = connect(&second, &second_routes).await.fingerprint.lock().unwrap().clone();

        assert!(seen_first.is_some());
        assert_eq!(seen_first, seen_second);
        assert_eq!(std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn data_for_an_unknown_session_is_answered_with_ssh_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (out, mut messages) = mpsc::unbounded_channel();
        let server = SshServer::new(out, dir.path().join("host_key"), dir.path().to_path_buf(), "/bin/sh".to_string());
        let unknown = Uuid::from_u128(5);

        server.data(unknown, &STANDARD.encode(b"SSH-2.0-test\r\n"));

        let answer = tokio::time::timeout(WAIT, messages.recv()).await.unwrap().unwrap();
        assert_eq!(answer, NodeToControl::SshClosed { session_id: unknown });
    }

    #[tokio::test]
    async fn an_unreadable_host_key_answers_the_open_with_ssh_closed() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("host_key");
        std::fs::write(&key_path, "not a key").unwrap();
        let (out, mut messages) = mpsc::unbounded_channel();
        let server = SshServer::new(out, key_path, dir.path().to_path_buf(), "/bin/sh".to_string());
        let session_id = Uuid::from_u128(6);

        server.open(session_id);

        let answer = tokio::time::timeout(WAIT, messages.recv()).await.unwrap().unwrap();
        assert_eq!(answer, NodeToControl::SshClosed { session_id });
    }

    #[tokio::test]
    async fn closing_a_session_ends_its_running_command() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let connected = signed_in(&server, &routes).await;
        let pid = pid_of_running_sleep(&connected).await;

        server.close(connected.session_id);

        assert!(gone(pid).await);
    }

    #[tokio::test]
    async fn closing_every_session_ends_every_running_command() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let first = signed_in(&server, &routes).await;
        let second = signed_in(&server, &routes).await;
        let first_pid = pid_of_running_sleep(&first).await;
        let second_pid = pid_of_running_sleep(&second).await;

        server.close_all();

        assert!(gone(first_pid).await);
        assert!(gone(second_pid).await);
    }

    #[tokio::test]
    async fn sftp_and_port_forwarding_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (server, routes) = start_server(dir.path().join("host_key"));
        let connected = signed_in(&server, &routes).await;
        let mut channel = connected.handle.channel_open_session().await.unwrap();

        channel.request_subsystem(true, "sftp").await.unwrap();
        let reply = tokio::time::timeout(WAIT, channel.wait()).await.unwrap();

        assert!(matches!(reply, Some(ChannelMsg::Failure)));
        assert!(connected.handle.tcpip_forward("127.0.0.1", 0).await.is_err());
        assert!(connected.handle.channel_open_direct_tcpip("127.0.0.1", 22, "127.0.0.1", 1).await.is_err());
    }
}
