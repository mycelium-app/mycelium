use crate::config::agent_command;
use anyhow::{Result, anyhow, bail};
use mycelium_proto::conversation::AgentId;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

const DELEGATED: &str = "browser-auth-delegated";
const NO_KEYRING: &str = "keyring.backends.fail.Keyring";

pub async fn run() -> Result<()> {
    let command = agent_command(AgentId::Vibe);
    let (program, arguments) = command.split_first().ok_or_else(|| anyhow!("Mistral Vibe has no ACP command on this node."))?;
    let mut child = Command::new(program)
        .args(arguments)
        .env("PYTHON_KEYRING_BACKEND", NO_KEYRING)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| anyhow!("Mistral Vibe could not start: {error}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("Mistral Vibe has no input."))?;
    let mut lines = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("Mistral Vibe has no output."))?).lines();

    send(&mut stdin, 1, "initialize", json!({"protocolVersion": 1, "clientCapabilities": {"_meta": {DELEGATED: true}}, "clientInfo": {"name": "mycelium", "version": env!("CARGO_PKG_VERSION")}})).await?;
    let initialized = answer(&mut lines, 1).await?;

    if !initialized["authMethods"].as_array().is_some_and(|methods| methods.iter().any(|method| method["id"] == DELEGATED)) {
        bail!("This version of Mistral Vibe cannot sign in from Mycelium. Update it, then try again.");
    }

    send(&mut stdin, 2, "authenticate", json!({"methodId": DELEGATED, "action": "start"})).await?;
    let started = answer(&mut lines, 2).await?;
    let attempt = &started["_meta"][DELEGATED];
    let (Some(url), Some(attempt_id)) = (attempt["signInUrl"].as_str(), attempt["attemptId"].as_str()) else {
        println!("Mistral Vibe is already signed in on this node.");

        return Ok(());
    };

    println!("Open this page to sign in to Mistral Vibe with your Mistral account: {url}");
    send(&mut stdin, 3, "authenticate", json!({"methodId": DELEGATED, "action": "complete", "attemptId": attempt_id})).await?;
    answer(&mut lines, 3).await?;
    println!("Signed in to Mistral Vibe.");

    Ok(())
}

async fn send(stdin: &mut ChildStdin, id: u64, method: &str, params: Value) -> Result<()> {
    let line = format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));

    stdin.write_all(line.as_bytes()).await.map_err(|error| anyhow!("Mistral Vibe closed early: {error}"))
}

async fn answer(lines: &mut tokio::io::Lines<BufReader<ChildStdout>>, id: u64) -> Result<Value> {
    while let Some(line) = lines.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if message["id"].as_u64() != Some(id) {
            continue;
        }

        if let Some(error) = message["error"]["message"].as_str() {
            bail!("Mistral Vibe could not sign in: {error}");
        }

        return Ok(message["result"].clone());
    }

    Err(anyhow!("Mistral Vibe stopped before signing in."))
}
