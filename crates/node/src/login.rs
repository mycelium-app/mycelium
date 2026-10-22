use anyhow::{Context, Result};
use mycelium_proto::link::NodeToControl;
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

type StateReport = Arc<dyn Fn() -> NodeToControl + Send + Sync>;

struct LoginProcess {
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Clone)]
pub struct LoginManager {
    out: mpsc::UnboundedSender<NodeToControl>,
    report_state: StateReport,
    running: Arc<Mutex<HashMap<Uuid, LoginProcess>>>,
}

impl LoginManager {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, report_state: StateReport) -> LoginManager {
        LoginManager { out, report_state, running: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn start(&self, login_id: Uuid, command: Vec<String>) {
        if let Err(error) = self.try_start(login_id, &command) {
            let _ = self.out.send(NodeToControl::LoginOutput { login_id, data: format!("{error:#}\r\n") });
            let _ = self.out.send(NodeToControl::LoginFinished { login_id, ok: false });
        }
    }

    pub fn input(&self, login_id: Uuid, text: &str) {
        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&login_id) {
            let _ = process.writer.write_all(text.as_bytes());
            let _ = process.writer.flush();
        }
    }

    pub fn stop(&self, login_id: Uuid) {
        let Ok(mut running) = self.running.lock() else {
            return;
        };

        if let Some(process) = running.get_mut(&login_id) {
            let _ = process.killer.kill();
        }
    }

    fn try_start(&self, login_id: Uuid, command: &[String]) -> Result<()> {
        let Some((program, args)) = command.split_first() else {
            anyhow::bail!("This agent has no sign-in command.");
        };

        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 }).context("open a terminal for the sign-in")?;
        let mut builder = CommandBuilder::new(program);
        builder.args(args);
        let mut child = pair.slave.spawn_command(builder).with_context(|| format!("Could not start {program}. Check that it is installed on this node."))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let killer = child.clone_killer();
        self.running.lock().map_err(|_| anyhow::anyhow!("login table poisoned"))?.insert(login_id, LoginProcess { writer, killer });

        let out = self.out.clone();
        let running = self.running.clone();
        let report_state = self.report_state.clone();
        std::thread::spawn(move || {
            let _master = pair.master;
            let mut buffer = [0u8; 4096];
            let mut carry: Vec<u8> = Vec::new();

            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }

                carry.extend_from_slice(&buffer[..read]);
                let data = take_valid_utf8(&mut carry);
                if !data.is_empty() {
                    let _ = out.send(NodeToControl::LoginOutput { login_id, data });
                }
            }

            let ok = child.wait().map(|status| status.success()).unwrap_or(false);
            if let Ok(mut running) = running.lock() {
                running.remove(&login_id);
            }

            let _ = out.send(NodeToControl::LoginFinished { login_id, ok });
            let _ = out.send(report_state());
        });

        Ok(())
    }
}

fn take_valid_utf8(carry: &mut Vec<u8>) -> String {
    match std::str::from_utf8(carry) {
        Ok(text) => {
            let text = text.to_string();
            carry.clear();

            text
        }
        Err(error) if error.error_len().is_none() => {
            let valid = error.valid_up_to();
            let text = String::from_utf8_lossy(&carry[..valid]).into_owned();
            carry.drain(..valid);

            text
        }
        Err(_) => {
            let text = String::from_utf8_lossy(carry).into_owned();
            carry.clear();

            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_incomplete_character_waits_for_its_remaining_bytes() {
        let mut carry = vec![b'o', b'k', 0xE2, 0x82];

        assert_eq!(take_valid_utf8(&mut carry), "ok");
        carry.push(0xAC);

        assert_eq!(take_valid_utf8(&mut carry), "\u{20AC}");
        assert!(carry.is_empty());
    }

    async fn next_message(messages: &mut mpsc::UnboundedReceiver<NodeToControl>) -> NodeToControl {
        tokio::time::timeout(Duration::from_secs(10), messages.recv()).await.expect("login went quiet").expect("channel closed")
    }

    #[tokio::test]
    async fn output_streams_input_is_written_and_the_finish_is_reported() {
        let (out, mut messages) = mpsc::unbounded_channel();
        let manager = LoginManager::new(out, Arc::new(|| NodeToControl::Pong));
        let login_id = Uuid::from_u128(1);

        manager.start(login_id, vec!["sh".into(), "-c".into(), "echo visit-the-link; read answer; echo got-$answer".into()]);

        let mut seen = String::new();
        while !seen.contains("visit-the-link") {
            if let NodeToControl::LoginOutput { data, .. } = next_message(&mut messages).await {
                seen.push_str(&data);
            }
        }

        manager.input(login_id, "1234\n");

        while !seen.contains("got-1234") {
            if let NodeToControl::LoginOutput { data, .. } = next_message(&mut messages).await {
                seen.push_str(&data);
            }
        }

        let mut next = next_message(&mut messages).await;
        while matches!(next, NodeToControl::LoginOutput { .. }) {
            next = next_message(&mut messages).await;
        }

        assert_eq!(next, NodeToControl::LoginFinished { login_id, ok: true });
        assert_eq!(next_message(&mut messages).await, NodeToControl::Pong);
    }

    #[tokio::test]
    async fn stopping_a_login_finishes_it_unsuccessfully() {
        let (out, mut messages) = mpsc::unbounded_channel();
        let manager = LoginManager::new(out, Arc::new(|| NodeToControl::Pong));
        let login_id = Uuid::from_u128(2);

        manager.start(login_id, vec!["sh".into(), "-c".into(), "echo ready; sleep 30".into()]);
        while !matches!(next_message(&mut messages).await, NodeToControl::LoginOutput { .. }) {}

        manager.stop(login_id);

        let mut next = next_message(&mut messages).await;
        while matches!(next, NodeToControl::LoginOutput { .. }) {
            next = next_message(&mut messages).await;
        }

        assert_eq!(next, NodeToControl::LoginFinished { login_id, ok: false });
    }

    #[tokio::test]
    async fn a_missing_command_is_reported_as_a_failed_login() {
        let (out, mut messages) = mpsc::unbounded_channel();
        let manager = LoginManager::new(out, Arc::new(|| NodeToControl::Pong));
        let login_id = Uuid::from_u128(3);

        manager.start(login_id, vec!["/nonexistent/agent".into(), "login".into()]);

        let NodeToControl::LoginOutput { data, .. } = next_message(&mut messages).await else {
            panic!("expected an explanation first");
        };
        assert!(data.contains("Could not start"));
        assert_eq!(next_message(&mut messages).await, NodeToControl::LoginFinished { login_id, ok: false });
    }
}
