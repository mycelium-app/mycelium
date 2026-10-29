use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::NodeToControl;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

struct TerminalProcess {
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

    pub fn start(&self, terminal_id: Uuid, cwd: &str, cols: u16, rows: u16) {
        if let Err(error) = self.try_start(terminal_id, cwd, cols, rows) {
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
        let Ok(running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get(&terminal_id) {
            let _ = process.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    pub fn stop(&self, terminal_id: Uuid) {
        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&terminal_id) {
            let _ = process.killer.kill();
        }
    }

    fn try_start(&self, terminal_id: Uuid, cwd: &str, cols: u16, rows: u16) -> Result<()> {
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
        self.running.lock().map_err(|_| anyhow::anyhow!("terminal table poisoned"))?.insert(terminal_id, TerminalProcess { writer, master: pair.master, killer });

        let out = self.out.clone();
        let running = self.running.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];

            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
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

        manager.start(terminal_id, folder.path().to_str().unwrap(), 80, 24);
        manager.input(terminal_id, &STANDARD.encode("pwd; echo sum-$((20+22))\n"));

        let seen = read_until(&mut messages, "sum-42").await;
        assert!(seen.contains(folder.path().file_name().unwrap().to_str().unwrap()));
    }

    #[tokio::test]
    async fn resizing_changes_the_size_the_program_sees() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(2);

        manager.start(terminal_id, "/", 80, 24);
        manager.resize(terminal_id, 100, 30);
        manager.input(terminal_id, &STANDARD.encode("stty size\n"));

        read_until(&mut messages, "30 100").await;
    }

    #[tokio::test]
    async fn a_shell_that_exits_reports_the_terminal_as_exited() {
        let (manager, mut messages) = shell_manager();
        let terminal_id = Uuid::from_u128(3);

        manager.start(terminal_id, "/", 80, 24);
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

        manager.start(terminal_id, "/", 80, 24);
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

        manager.start(terminal_id, "/nonexistent/folder", 80, 24);

        let NodeToControl::TerminalOutput { data, .. } = next_message(&mut messages).await else {
            panic!("expected an explanation first");
        };
        assert!(String::from_utf8(STANDARD.decode(data).unwrap()).unwrap().contains("Could not start a terminal"));
        assert_eq!(next_message(&mut messages).await, NodeToControl::TerminalExited { terminal_id });
    }
}
