use crate::config::{self, NodeConfig};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::{Error, Message};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const SSH_TOKEN_PREFIX: &str = "myc_ssh_";
const READ_BYTES: usize = 32 * 1024;

pub async fn run(node: String, token_file: PathBuf) -> Result<()> {
    let config = NodeConfig::load(&config::config_path())?;
    let token = read_token(&token_file)?;
    let url = config.ssh_url(&node)?;
    let mut request = url.as_str().into_client_request()?;
    request.headers_mut().insert("Authorization", format!("Bearer {token}").parse()?);

    let connecting = tokio::time::timeout(CONNECT_TIMEOUT, connect_async_with_config(request, None, false)).await.context("The control plane did not answer in time.")?;
    let socket = match connecting {
        Ok((socket, _)) => socket,
        Err(Error::Http(response)) => anyhow::bail!(refusal(response.status())),
        Err(error) => return Err(anyhow::Error::new(error).context(format!("connect to {url}"))),
    };

    let (mut sink, mut stream) = socket.split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut buffer = vec![0u8; READ_BYTES];
    let mut stdin_open = true;

    loop {
        tokio::select! {
            read = stdin.read(&mut buffer), if stdin_open => {
                match read {
                    Ok(0) | Err(_) => {
                        stdin_open = false;
                        let _ = sink.send(Message::Close(None)).await;
                    }
                    Ok(count) => sink.send(Message::Binary(buffer[..count].to_vec().into())).await?,
                }
            }
            frame = stream.next() => {
                match frame {
                    None | Some(Ok(Message::Close(_))) | Some(Err(Error::ConnectionClosed | Error::AlreadyClosed)) => return Ok(()),
                    Some(Ok(Message::Binary(data))) => {
                        stdout.write_all(&data).await?;
                        stdout.flush().await?;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) if !stdin_open => return Ok(()),
                    Some(Err(error)) => return Err(error.into()),
                }
            }
        }
    }
}

fn read_token(token_file: &Path) -> Result<String> {
    let path = token_file.display();
    let text = std::fs::read_to_string(token_file).map_err(|_| anyhow::anyhow!("There is no SSH token at {path}. Ask for SSH access first."))?;
    let token = text.trim().to_string();

    anyhow::ensure!(token.starts_with(SSH_TOKEN_PREFIX), "{path} does not hold an SSH access token.");

    Ok(token)
}

fn refusal(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "This SSH access has ended or was revoked. Ask for access again.".to_string(),
        StatusCode::FORBIDDEN => "This SSH access covers another node.".to_string(),
        StatusCode::CONFLICT => "The node is offline, so there is nothing to connect to.".to_string(),
        other => format!("The control plane refused the connection with status {other}."),
    }
}
