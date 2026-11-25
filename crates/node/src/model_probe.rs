use crate::config::agent_env;
use anyhow::{Result, anyhow, bail};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::link::NodeToControl;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn offered(agent: AgentId, command: &[String], folder: &Path) -> Result<NodeToControl> {
    tokio::time::timeout(PROBE_TIMEOUT, ask(agent, command, folder)).await.map_err(|_| anyhow!("{} did not say which models it offers within a minute.", agent.as_str()))?
}

async fn ask(agent: AgentId, command: &[String], folder: &Path) -> Result<NodeToControl> {
    let (program, arguments) = command.split_first().ok_or_else(|| anyhow!("{} has no ACP command on this node.", agent.as_str()))?;
    std::fs::create_dir_all(folder)?;

    let mut child = Command::new(program)
        .args(arguments)
        .envs(agent_env(agent))
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("{} has no input.", agent.as_str()))?;
    let mut lines = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("{} has no output.", agent.as_str()))?).lines();
    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}, "clientInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": folder, "mcpServers": []}}),
    ];
    let mut session = Value::Null;

    for request in requests {
        stdin.write_all(format!("{request}\n").as_bytes()).await?;

        let id = request["id"].clone();

        session = loop {
            let Some(line) = lines.next_line().await? else {
                bail!("{} stopped before saying which models it offers.", agent.as_str());
            };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            if message["id"] != id || message.get("method").is_some() {
                continue;
            }

            if let Some(error) = message["error"]["message"].as_str() {
                bail!("{} refused {}: {error}", agent.as_str(), request["method"]);
            }

            break message["result"].clone();
        };
    }

    let field = |name: &str| session.get(name).filter(|value| !value.is_null()).cloned();

    Ok(NodeToControl::HarnessOffered { agent, modes: field("modes"), models: field("models"), config_options: field("configOptions") })
}
