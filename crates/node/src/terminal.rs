use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::{NodeTerminal, NodeToControl};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

const SCROLLBACK_BYTES: usize = 256 * 1024;
const MAX_TERMINALS: usize = 32;

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
    running: Arc<Mutex<HashMap<Uuid, TerminalProcess>>>,
}

pub fn find_in_path(program: &str, path: &std::ffi::OsStr) -> Option<String> {
    std::env::split_paths(path).map(|directory| directory.join(program)).find(|candidate| candidate.is_file()).map(|candidate| candidate.to_string_lossy().into_owned())
}

pub fn user_shell() -> String {
    std::env::var("SHELL").ok().filter(|shell| !shell.is_empty()).unwrap_or_else(|| "/bin/sh".to_string())
}

impl TerminalManager {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, shell: String) -> TerminalManager {
        TerminalManager { out, shell, running: Arc::new(Mutex::new(HashMap::new())) }
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

        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).context("Could not open a terminal on this node.")?;
        let mut builder = CommandBuilder::new(&self.shell);
        builder.cwd(cwd);
        builder.env("TERM", "xterm-256color");

        let mut child = pair.slave.spawn_command(builder).context("Could not start a terminal on this node.")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let killer = child.clone_killer();
        self.running.lock().map_err(|_| anyhow::anyhow!("terminal table poisoned"))?.insert(terminal_id, TerminalProcess { conversation_id, cols, rows, scrollback: Scrollback::default(), writer, master: pair.master, killer });

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

        (TerminalManager::new(out, "/bin/sh".into()), messages)
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
