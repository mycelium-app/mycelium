use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::{NodeTerminal, NodeToControl};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

const SCROLLBACK_BYTES: usize = 256 * 1024;
const MAX_TERMINALS: usize = 32;
const SECRET_NAME_PARTS: [&str; 5] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "API_KEY"];
const NODE_VARIABLE_PREFIX: &str = "MYCELIUM_";
const TMUX_VARIABLES: [&str; 2] = ["TMUX", "TMUX_PANE"];
const TMUX_HISTORY_LINES: usize = 2000;
const TMUX_CONVERSATION_OPTION: &str = "@mycelium_conversation";

#[derive(Default)]
struct Scrollback {
    bytes: Vec<u8>,
    total: u64,
}

impl Scrollback {
    fn push(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
        self.total += data.len() as u64;

        let excess = self.bytes.len().saturating_sub(SCROLLBACK_BYTES);
        self.bytes.drain(..excess);
    }

    fn read_from(&self, from: u64) -> (Vec<u8>, u64) {
        let oldest = self.total - self.bytes.len() as u64;

        if from >= oldest {
            let begin = from.min(self.total);

            return (self.bytes[(begin - oldest) as usize..].to_vec(), begin);
        }

        let cut = safe_start(&self.bytes);

        (self.bytes[cut..].to_vec(), oldest + cut as u64)
    }
}

fn safe_start(bytes: &[u8]) -> usize {
    match bytes.iter().position(|byte| *byte == b'\n' || *byte == 0x1b) {
        Some(position) if bytes[position] == b'\n' => position + 1,
        Some(position) => position,
        None => 0,
    }
}

#[derive(Clone)]
struct Tmux {
    program: String,
    socket: PathBuf,
}

impl Tmux {
    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.arg("-S").arg(&self.socket).arg("-f").arg("/dev/null").args(args);

        for (name, _) in std::env::vars() {
            if is_scrubbed_variable(&name) {
                command.env_remove(&name);
            }
        }

        command
    }

    fn pty_command(&self, args: &[&str]) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        builder.arg("-S");
        builder.arg(&self.socket);
        builder.args(args);
        builder.env("TERM", "xterm-256color");
        scrub(&mut builder);

        builder
    }

    fn output(&self, args: &[&str]) -> Option<String> {
        let output = self.command(args).output().ok()?;

        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn kill_session(&self, terminal_id: Uuid) {
        let target = session_target(terminal_id);
        let _ = self.command(&["kill-session", "-t", &target]).output();
    }

    fn sessions(&self) -> Vec<TmuxSession> {
        let format = format!("#{{session_name}} #{{{TMUX_CONVERSATION_OPTION}}} #{{window_width}} #{{window_height}}");
        let Some(listing) = self.output(&["list-sessions", "-F", &format]) else {
            return Vec::new();
        };

        listing.lines().filter_map(TmuxSession::parse).collect()
    }

    fn history(&self, terminal_id: Uuid) -> Vec<u8> {
        let target = format!("{}:", session_target(terminal_id));
        let start = format!("-{TMUX_HISTORY_LINES}");
        let captured = self.output(&["capture-pane", "-p", "-e", "-S", &start, "-E", "-1", "-t", &target]).unwrap_or_default();

        captured.replace('\n', "\r\n").into_bytes()
    }
}

struct TmuxSession {
    terminal_id: Uuid,
    conversation_id: Uuid,
    cols: u16,
    rows: u16,
}

impl TmuxSession {
    fn parse(line: &str) -> Option<TmuxSession> {
        let mut fields = line.split(' ');
        let terminal_id = fields.next()?.parse().ok()?;
        let conversation_id = fields.next()?.parse().ok()?;
        let cols = fields.next()?.parse().ok()?;
        let rows = fields.next()?.parse().ok()?;

        Some(TmuxSession { terminal_id, conversation_id, cols, rows })
    }
}

fn session_target(terminal_id: Uuid) -> String {
    format!("={terminal_id}")
}

fn is_scrubbed_variable(name: &str) -> bool {
    let upper = name.to_uppercase();

    upper.starts_with(NODE_VARIABLE_PREFIX) || TMUX_VARIABLES.contains(&upper.as_str()) || SECRET_NAME_PARTS.iter().any(|part| upper.contains(part))
}

fn scrub(builder: &mut CommandBuilder) {
    for (name, _) in std::env::vars() {
        if is_scrubbed_variable(&name) {
            builder.env_remove(&name);
        }
    }
}

struct TerminalProcess {
    conversation_id: Uuid,
    cols: u16,
    rows: u16,
    scrollback: Scrollback,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Clone)]
pub struct TerminalManager {
    out: mpsc::UnboundedSender<NodeToControl>,
    shell: String,
    tmux: Option<Tmux>,
    running: Arc<Mutex<HashMap<Uuid, TerminalProcess>>>,
}

pub fn find_in_path(program: &str, path: &std::ffi::OsStr) -> Option<String> {
    std::env::split_paths(path).map(|directory| directory.join(program)).find(|candidate| candidate.is_file()).map(|candidate| candidate.to_string_lossy().into_owned())
}

pub fn user_shell() -> String {
    std::env::var("SHELL").ok().filter(|shell| !shell.is_empty()).unwrap_or_else(|| "/bin/sh".to_string())
}

impl TerminalManager {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, shell: String, tmux_dir: Option<&Path>) -> TerminalManager {
        let tmux = tmux_dir.and_then(Self::find_tmux);
        let manager = TerminalManager { out, shell, tmux, running: Arc::new(Mutex::new(HashMap::new())) };
        manager.adopt_sessions();

        manager
    }

    pub fn start(&self, terminal_id: Uuid, conversation_id: Uuid, cwd: &str, cols: u16, rows: u16) {
        if let Err(error) = self.try_start(terminal_id, conversation_id, cwd, cols, rows) {
            let explanation = base64_of(format!("{error:#}\r\n").as_bytes());
            let _ = self.out.send(NodeToControl::TerminalOutput { terminal_id, data: explanation });
            let _ = self.out.send(NodeToControl::TerminalExited { terminal_id });
        }
    }

    pub fn input(&self, terminal_id: Uuid, data: &str) {
        let Ok(bytes) = STANDARD.decode(data) else {
            return;
        };
        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&terminal_id) {
            let _ = process.writer.write_all(&bytes);
            let _ = process.writer.flush();
        }
    }

    pub fn resize(&self, terminal_id: Uuid, cols: u16, rows: u16) {
        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&terminal_id) {
            let _ = process.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
            process.cols = cols;
            process.rows = rows;
        }
    }

    pub fn list(&self) -> Vec<NodeTerminal> {
        let Ok(running) = self.running.lock() else {
            return Vec::new();
        };

        running.iter().map(|(terminal_id, process)| NodeTerminal { terminal_id: *terminal_id, conversation_id: process.conversation_id, total: process.scrollback.total, cols: process.cols, rows: process.rows }).collect()
    }

    pub fn read(&self, terminal_id: Uuid, from: u64) -> Option<(Vec<u8>, u64, u64)> {
        let running = self.running.lock().ok()?;
        let process = running.get(&terminal_id)?;
        let (data, offset) = process.scrollback.read_from(from);

        Some((data, offset, process.scrollback.total))
    }

    pub fn stop(&self, terminal_id: Uuid) {
        if let Some(tmux) = &self.tmux {
            tmux.kill_session(terminal_id);
        }

        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&terminal_id) {
            let _ = process.killer.kill();
        }
    }

    fn try_start(&self, terminal_id: Uuid, conversation_id: Uuid, cwd: &str, cols: u16, rows: u16) -> Result<()> {
        let open = self.running.lock().map_err(|_| anyhow::anyhow!("terminal table poisoned"))?.len();
        anyhow::ensure!(open < MAX_TERMINALS, "This node already has {MAX_TERMINALS} terminals open. Close one before opening another.");
        anyhow::ensure!(std::path::Path::new(cwd).is_dir(), "Could not start a terminal in {cwd}. Check that the folder exists on this node.");

        let builder = match &self.tmux {
            Some(tmux) => {
                let created = self.create_session(tmux, terminal_id, conversation_id, cwd, cols, rows)?;
                anyhow::ensure!(created, "Could not start a terminal on this node.");
                tmux.pty_command(&["attach-session", "-t", &session_target(terminal_id)])
            }
            None => {
                let mut builder = CommandBuilder::from_argv(self.shell_in(cwd).into_iter().map(Into::into).collect());
                builder.cwd(cwd);
                builder.env("TERM", "xterm-256color");
                scrub(&mut builder);
                builder
            }
        };

        self.attach(terminal_id, conversation_id, cols, rows, builder, Vec::new())
    }

    fn attach(&self, terminal_id: Uuid, conversation_id: Uuid, cols: u16, rows: u16, builder: CommandBuilder, history: Vec<u8>) -> Result<()> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).context("Could not open a terminal on this node.")?;
        let mut child = pair.slave.spawn_command(builder).context("Could not start a terminal on this node.")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let killer = child.clone_killer();
        let mut scrollback = Scrollback::default();
        scrollback.push(&history);
        self.running.lock().map_err(|_| anyhow::anyhow!("terminal table poisoned"))?.insert(terminal_id, TerminalProcess { conversation_id, cols, rows, scrollback, writer, master: pair.master, killer });

        let out = self.out.clone();
        let running = self.running.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];

            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }

                if let Ok(mut running) = running.lock() {
                    if let Some(process) = running.get_mut(&terminal_id) {
                        process.scrollback.push(&buffer[..read]);
                    }
                }

                let _ = out.send(NodeToControl::TerminalOutput { terminal_id, data: base64_of(&buffer[..read]) });
            }

            let _ = child.wait();
            if let Ok(mut running) = running.lock() {
                running.remove(&terminal_id);
            }

            let _ = out.send(NodeToControl::TerminalExited { terminal_id });
        });

        Ok(())
    }

    fn shell_in(&self, cwd: &str) -> Vec<String> {
        [crate::direnv::exec_prefix(Path::new(cwd)), vec![self.shell.clone()]].concat()
    }

    fn find_tmux(directory: &Path) -> Option<Tmux> {
        let program = find_in_path("tmux", &std::env::var_os("PATH").unwrap_or_default());
        let Some(program) = program else {
            tracing::warn!("tmux is not installed, so terminals end when the node restarts");

            return None;
        };

        if let Err(error) = crate::config::create_private_directory(directory) {
            tracing::warn!("could not create the tmux directory {}, so terminals end when the node restarts: {error}", directory.display());

            return None;
        }

        Some(Tmux { program, socket: directory.join("tmux.sock") })
    }

    fn create_session(&self, tmux: &Tmux, terminal_id: Uuid, conversation_id: Uuid, cwd: &str, cols: u16, rows: u16) -> Result<bool> {
        let name = terminal_id.to_string();
        let target = format!("{}:", session_target(terminal_id));
        let size = (cols.to_string(), rows.to_string());
        let shell = self.shell_in(cwd);
        let history = TMUX_HISTORY_LINES.to_string();
        let conversation = conversation_id.to_string();
        let status = tmux
            .command(&[
                "start-server", ";",
                "set-option", "-g", "history-limit", &history, ";",
                "set-option", "-g", "default-terminal", "xterm-256color", ";",
                "set-option", "-s", "escape-time", "0", ";",
                "set-option", "-s", "-a", "terminal-overrides", ",*:smcup@:rmcup@", ";",
                "new-session", "-d", "-s", &name, "-x", &size.0, "-y", &size.1, "-c", cwd,
            ])
            .args(&shell)
            .args([
                ";",
                "set-option", "-t", &target, "status", "off", ";",
                "set-option", "-t", &target, "prefix", "None", ";",
                "set-option", "-t", &target, TMUX_CONVERSATION_OPTION, &conversation,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Could not start a terminal on this node.")?;

        Ok(status.success())
    }

    fn adopt_sessions(&self) {
        let Some(tmux) = &self.tmux else {
            return;
        };

        for session in tmux.sessions() {
            let open = self.running.lock().map(|running| running.len()).unwrap_or(MAX_TERMINALS);

            if open >= MAX_TERMINALS {
                break;
            }

            let builder = tmux.pty_command(&["attach-session", "-t", &session_target(session.terminal_id)]);
            let attached = self.attach(session.terminal_id, session.conversation_id, session.cols, session.rows, builder, tmux.history(session.terminal_id));

            if let Err(error) = attached {
                tracing::warn!("could not reattach terminal {}: {error:#}", session.terminal_id);
            }
        }
    }
}

fn base64_of(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn next_message(messages: &mut mpsc::UnboundedReceiver<NodeToControl>) -> NodeToControl {
        tokio::time::timeout(Duration::from_secs(10), messages.recv()).await.expect("terminal went quiet").expect("channel closed")
    }

    async fn read_until(messages: &mut mpsc::UnboundedReceiver<NodeToControl>, wanted: &str) -> String {
        let mut seen = String::new();

        while !seen.contains(wanted) {
            if let NodeToControl::TerminalOutput { data, .. } = next_message(messages).await {
                seen.push_str(&String::from_utf8_lossy(&STANDARD.decode(data).unwrap()));
            }
        }

        seen
    }

    fn shell_manager() -> (TerminalManager, mpsc::UnboundedReceiver<NodeToControl>) {
        let (out, messages) = mpsc::unbounded_channel();

        (TerminalManager::new(out, "/bin/sh".into(), None), messages)
    }

    fn tmux_manager(directory: &Path) -> Option<(TerminalManager, mpsc::UnboundedReceiver<NodeToControl>)> {
        if find_in_path("tmux", &std::env::var_os("PATH").unwrap_or_default()).is_none() {
            eprintln!("skipping: tmux is not installed");

            return None;
        }

        let (out, messages) = mpsc::unbounded_channel();

        Some((TerminalManager::new(out, "/bin/sh".into(), Some(directory)), messages))
    }

    async fn simulate_node_restart(manager: TerminalManager) {
        for process in manager.running.lock().unwrap().values_mut() {
            process.killer.kill().unwrap();
        }

        let tmux = manager.tmux.as_ref().unwrap();

        while tmux.output(&["list-clients"]).is_some_and(|clients| !clients.is_empty()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        manager.running.lock().unwrap().clear();
    }

    fn end_tmux(manager: &TerminalManager) {
        manager.tmux.as_ref().unwrap().command(&["kill-server"]).output().unwrap();
    }

    async fn wait_for_history(manager: &TerminalManager, terminal_id: Uuid, wanted: &str) {
        for _ in 0..250 {
            if let Some((data, _, _)) = manager.read(terminal_id, 0) {
                if String::from_utf8_lossy(&data).contains(wanted) {
                    return;
                }
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        panic!("the scrollback never contained {wanted}");
    }

    #[tokio::test]
    async fn a_tmux_terminal_survives_a_node_restart_with_its_conversation_scrollback_and_input() {
        let directory = tempfile::tempdir().unwrap();
        let Some((manager, mut messages)) = tmux_manager(directory.path()) else {
            return;
        };
        let terminal_id = Uuid::from_u128(11);
        let conversation_id = Uuid::from_u128(12);

        manager.start(terminal_id, conversation_id, "/", 90, 30);
        manager.input(terminal_id, &STANDARD.encode("echo before-$((20+22))\n"));
        read_until(&mut messages, "before-42").await;
        simulate_node_restart(manager).await;

        let (restarted, mut restarted_messages) = tmux_manager(directory.path()).unwrap();
        let listed = restarted.list();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].terminal_id, listed[0].conversation_id, listed[0].cols, listed[0].rows), (terminal_id, conversation_id, 90, 30));

        wait_for_history(&restarted, terminal_id, "before-42").await;
        restarted.input(terminal_id, &STANDARD.encode("echo after-$((1+2))\n"));
        read_until(&mut restarted_messages, "after-3").await;

        restarted.stop(terminal_id);
        end_tmux(&restarted);
    }

    #[tokio::test]
    async fn output_scrolled_off_the_screen_is_restored_after_a_node_restart() {
        let directory = tempfile::tempdir().unwrap();
        let Some((manager, mut messages)) = tmux_manager(directory.path()) else {
            return;
        };
        let terminal_id = Uuid::from_u128(13);

        manager.start(terminal_id, Uuid::from_u128(14), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("echo old-$((20+22)); seq 1 60\n"));
        read_until(&mut messages, "old-42").await;
        read_until(&mut messages, "60").await;
        simulate_node_restart(manager).await;

        let (restarted, _messages) = tmux_manager(directory.path()).unwrap();

        wait_for_history(&restarted, terminal_id, "old-42").await;

        restarted.stop(terminal_id);
        end_tmux(&restarted);
    }

    #[tokio::test]
    async fn closing_a_tmux_terminal_ends_its_session() {
        let directory = tempfile::tempdir().unwrap();
        let Some((manager, mut messages)) = tmux_manager(directory.path()) else {
            return;
        };
        let terminal_id = Uuid::from_u128(15);

        manager.start(terminal_id, Uuid::from_u128(16), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("echo ready\n"));
        read_until(&mut messages, "ready").await;
        assert_eq!(manager.tmux.as_ref().unwrap().sessions().len(), 1);

        manager.stop(terminal_id);

        let mut next = next_message(&mut messages).await;
        while matches!(next, NodeToControl::TerminalOutput { .. }) {
            next = next_message(&mut messages).await;
        }

        assert_eq!(next, NodeToControl::TerminalExited { terminal_id });
        assert!(manager.tmux.as_ref().unwrap().sessions().is_empty());
        end_tmux(&manager);
    }

    #[tokio::test]
    async fn the_tmux_socket_directory_is_private_to_the_node_user() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("tmux");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        if tmux_manager(&directory).is_none() {
            return;
        }

        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&directory).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o700);
    }

    #[tokio::test]
    async fn a_tmux_terminal_does_not_inherit_the_nodes_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let Some((manager, mut messages)) = tmux_manager(directory.path()) else {
            return;
        };
        let terminal_id = Uuid::from_u128(17);

        unsafe {
            std::env::set_var("MYCELIUM_TMUX_TEST_NODE", "node-value");
            std::env::set_var("TMUX_TEST_API_TOKEN", "token-value");
            std::env::set_var("TMUX_TEST_HARMLESS", "harmless-value");
        }

        manager.start(terminal_id, Uuid::from_u128(18), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("echo seen-[$MYCELIUM_TMUX_TEST_NODE]-[$TMUX_TEST_API_TOKEN]-[$TMUX_TEST_HARMLESS]\n"));

        let seen = read_until(&mut messages, "-[harmless-value]").await;
        assert!(seen.contains("seen-[]-[]-[harmless-value]"), "{seen}");

        manager.stop(terminal_id);
        end_tmux(&manager);
    }

    #[tokio::test]
    async fn a_shell_starts_in_the_folder_and_echoes_what_is_typed() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(1);
        let folder = tempfile::tempdir().unwrap();

        manager.start(terminal_id, Uuid::from_u128(100), folder.path().to_str().unwrap(), 80, 24);
        manager.input(terminal_id, &STANDARD.encode("pwd; echo sum-$((20+22))\n"));

        let seen = read_until(&mut messages, "sum-42").await;
        assert!(seen.contains(folder.path().file_name().unwrap().to_str().unwrap()));
    }

    #[tokio::test]
    async fn a_shell_does_not_inherit_the_nodes_secrets() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(4);

        unsafe {
            std::env::set_var("MYCELIUM_TERMINAL_TEST_NODE", "node-value");
            std::env::set_var("TERMINAL_TEST_API_TOKEN", "token-value");
            std::env::set_var("TERMINAL_TEST_HARMLESS", "harmless-value");
        }

        manager.start(terminal_id, Uuid::from_u128(100), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("echo seen-[$MYCELIUM_TERMINAL_TEST_NODE]-[$TERMINAL_TEST_API_TOKEN]-[$TERMINAL_TEST_HARMLESS]\n"));

        let seen = read_until(&mut messages, "-[harmless-value]").await;
        assert!(seen.contains("seen-[]-[]-[harmless-value]"), "{seen}");
    }

    #[tokio::test]
    async fn resizing_changes_the_size_the_program_sees() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(2);

        manager.start(terminal_id, Uuid::from_u128(100), "/", 80, 24);
        manager.resize(terminal_id, 100, 30);
        manager.input(terminal_id, &STANDARD.encode("stty size\n"));

        read_until(&mut messages, "30 100").await;
    }

    #[tokio::test]
    async fn a_shell_that_exits_reports_the_terminal_as_exited() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(3);

        manager.start(terminal_id, Uuid::from_u128(100), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("exit\n"));

        let mut next = next_message(&mut messages).await;
        while matches!(next, NodeToControl::TerminalOutput { .. }) {
            next = next_message(&mut messages).await;
        }

        assert_eq!(next, NodeToControl::TerminalExited { terminal_id });
    }

    #[tokio::test]
    async fn stopping_a_terminal_ends_it() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(4);

        manager.start(terminal_id, Uuid::from_u128(100), "/", 80, 24);
        manager.input(terminal_id, &STANDARD.encode("echo ready\n"));
        read_until(&mut messages, "ready").await;

        manager.stop(terminal_id);

        let mut next = next_message(&mut messages).await;
        while matches!(next, NodeToControl::TerminalOutput { .. }) {
            next = next_message(&mut messages).await;
        }

        assert_eq!(next, NodeToControl::TerminalExited { terminal_id });
    }

    #[tokio::test]
    async fn a_missing_folder_is_explained_and_the_terminal_exits() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(5);

        manager.start(terminal_id, Uuid::from_u128(100), "/nonexistent/folder", 80, 24);

        let NodeToControl::TerminalOutput { data, .. } = next_message(&mut messages).await else {
            panic!("expected an explanation first");
        };
        assert!(String::from_utf8(STANDARD.decode(data).unwrap()).unwrap().contains("Could not start a terminal"));
        assert_eq!(next_message(&mut messages).await, NodeToControl::TerminalExited { terminal_id });
    }

    #[test]
    fn the_scrollback_keeps_the_newest_bytes_and_counts_everything_written() {
        let mut scrollback = Scrollback::default();

        scrollback.push(&vec![b'a'; SCROLLBACK_BYTES]);
        scrollback.push(b"line\ntail");

        assert_eq!(scrollback.total, (SCROLLBACK_BYTES + 9) as u64);
        assert_eq!(scrollback.bytes.len(), SCROLLBACK_BYTES);
        assert!(scrollback.bytes.ends_with(b"line\ntail"));
    }

    #[test]
    fn reading_from_an_offset_still_held_returns_exactly_what_follows_it() {
        let mut scrollback = Scrollback::default();
        scrollback.push(b"one\ntwo\n");

        assert_eq!(scrollback.read_from(4), (b"two\n".to_vec(), 4));
        assert_eq!(scrollback.read_from(99), (Vec::new(), 8));
    }

    #[test]
    fn a_replay_that_lost_its_start_resumes_after_the_next_newline() {
        let tail = b"1mred\nnext";
        let scrollback = Scrollback { bytes: tail.to_vec(), total: 3 + tail.len() as u64 };

        let (data, offset) = scrollback.read_from(0);

        assert_eq!(data, b"next");
        assert_eq!(offset, scrollback.total - 4);
    }

    #[test]
    fn a_replay_that_lost_its_start_resumes_at_an_escape_sequence_before_any_newline() {
        let mut scrollback = Scrollback::default();
        scrollback.push(&vec![b'x'; SCROLLBACK_BYTES]);
        scrollback.push(b"\x1b[31mred");

        let (data, _) = scrollback.read_from(0);

        assert_eq!(data, b"\x1b[31mred");
    }

    #[tokio::test]
    async fn a_terminal_keeps_its_output_while_nobody_is_attached() {
        let (manager, messages) = shell_manager();
        let terminal_id = Uuid::from_u128(6);
        let conversation_id = Uuid::from_u128(7);

        manager.start(terminal_id, conversation_id, "/", 80, 24);
        drop(messages);
        manager.input(terminal_id, &STANDARD.encode("echo sum-$((20+22))\n"));

        let mut seen = String::new();

        while !seen.contains("sum-42") {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let (data, _, _) = manager.read(terminal_id, 0).expect("the terminal should still be running");
            seen = String::from_utf8_lossy(&data).into_owned();
        }

        let listed = manager.list();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].terminal_id, listed[0].conversation_id), (terminal_id, conversation_id));
        assert!(listed[0].total >= seen.len() as u64);
    }

    #[tokio::test]
    async fn a_node_that_is_full_refuses_another_terminal_and_says_why() {
        let (manager, mut messages) = shell_manager();

        for index in 0..MAX_TERMINALS {
            manager.start(Uuid::from_u128(1000 + index as u128), Uuid::from_u128(7), "/", 80, 24);
        }

        let refused = Uuid::from_u128(2000);
        manager.start(refused, Uuid::from_u128(7), "/", 80, 24);

        let seen = read_until(&mut messages, "Close one before opening another.").await;
        assert!(seen.contains("32 terminals"));
        assert_eq!(manager.list().len(), MAX_TERMINALS);

        for index in 0..MAX_TERMINALS {
            manager.stop(Uuid::from_u128(1000 + index as u128));
        }
    }
}
