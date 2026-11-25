use crate::config::{NodeConfig, config_path};
use crate::daemon::{StatusFile, status_file_path};
use anyhow::Result;
#[cfg(unix)]
use nix::sys::signal::kill;
#[cfg(unix)]
use nix::unistd::Pid;
use std::fmt::Write;
use std::path::Path;

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return false;
    }

    let status = unsafe { WaitForSingleObject(handle, 0) };
    unsafe { CloseHandle(handle) };

    status == windows_sys::Win32::Foundation::WAIT_TIMEOUT
}

fn connection_label(state_dir: &Path) -> &'static str {
    let status = std::fs::read_to_string(status_file_path(state_dir)).ok().and_then(|text| serde_json::from_str::<StatusFile>(&text).ok());

    match status {
        Some(status) if process_alive(status.pid) && status.connected => "connected",
        Some(status) if process_alive(status.pid) => "reconnecting",
        _ => "daemon not running",
    }
}

fn session_labels(config: &NodeConfig) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(config.sessions_dir()) else {
        return Vec::new();
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let exit = std::fs::read_to_string(entry.path().join("exit.json")).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let label = match exit {
            Some(exit) => format!("ended (exit {})", exit["exit"]),
            None if mycelium_keeper::socket_path(&entry.path()).exists() => "running".to_string(),
            None => "not started".to_string(),
        };

        sessions.push((name, label));
    }

    sessions.sort();

    sessions
}

pub fn render_status(config: &NodeConfig) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Node:          {}", config.node_id);
    let _ = writeln!(text, "Control plane: {}", config.control_plane);
    let _ = writeln!(text, "Connection:    {}", connection_label(&config.state_dir));

    let sessions = session_labels(config);
    if sessions.is_empty() {
        let _ = writeln!(text, "Sessions:      none");
    } else {
        let _ = writeln!(text, "Sessions:");
        for (conversation, label) in sessions {
            let _ = writeln!(text, "  {conversation}  {label}");
        }
    }

    text
}

pub async fn status() -> Result<()> {
    let config = NodeConfig::load(&config_path())?;
    print!("{}", render_status(&config));

    Ok(())
}
