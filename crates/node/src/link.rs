use crate::daemon::Daemon;
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Backoff {
    pub min: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff { min: Duration::from_secs(1), max: Duration::from_secs(60) }
    }
}

impl Backoff {
    pub fn ceiling_for(&self, attempt: u32) -> Duration {
        let factor = 2u32.checked_pow(attempt).unwrap_or(u32::MAX);

        self.min.saturating_mul(factor).min(self.max)
    }

    pub fn delay_for(&self, attempt: u32, jitter: f64) -> Duration {
        let ceiling = self.ceiling_for(attempt);
        let floor = ceiling / 2;

        floor + (ceiling - floor).mul_f64(jitter.clamp(0.0, 1.0))
    }
}

enum Ended {
    Revoked,
    Dropped,
}

pub async fn run_link(daemon: Arc<Daemon>, mut outgoing: mpsc::UnboundedReceiver<NodeToControl>) -> Result<()> {
    let mut attempt = 0u32;

    loop {
        let mut established = false;
        let result = connect_once(&daemon, &mut outgoing, &mut established).await;
        daemon.on_disconnected();

        match result {
            Ok(Ended::Revoked) => {
                daemon.forget_credential();

                return Ok(());
            }
            Ok(Ended::Dropped) => tracing::warn!("the control plane closed the connection"),
            Err(error) => tracing::warn!("cannot reach the control plane: {error:#}"),
        }

        if established {
            attempt = 0;
        }

        let delay = daemon.backoff().delay_for(attempt, rand::random::<f64>());
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(delay).await;
    }
}

async fn connect_once(daemon: &Daemon, outgoing: &mut mpsc::UnboundedReceiver<NodeToControl>, established: &mut bool) -> Result<Ended> {
    let url = daemon.config().connect_url()?;
    let mut request = url.as_str().into_client_request()?;
    request.headers_mut().insert("Authorization", format!("Bearer {}", daemon.config().bearer_token()).parse()?);

    let connecting = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request)).await.context("The control plane did not answer in time.")?;
    let (mut socket, _) = connecting.with_context(|| format!("connect to {url}"))?;

    while outgoing.try_recv().is_ok() {}

    send(&mut socket, &daemon.hello().await).await?;
    send(&mut socket, &daemon.logins_report().await).await?;
    daemon.on_connected();
    *established = true;

    loop {
        tokio::select! {
            frame = socket.next() => {
                let Some(frame) = frame else {
                    return Ok(Ended::Dropped);
                };

                match frame? {
                    Message::Text(text) => {
                        let Ok(message) = serde_json::from_str::<ControlToNode>(&text) else {
                            tracing::warn!("ignoring a message from the control plane that this node does not understand");
                            continue;
                        };

                        if matches!(message, ControlToNode::Revoked) {
                            return Ok(Ended::Revoked);
                        }

                        daemon.handle(message);
                    }
                    Message::Close(_) => return Ok(Ended::Dropped),
                    _ => {}
                }
            }
            message = outgoing.recv() => {
                let Some(message) = message else {
                    return Ok(Ended::Dropped);
                };

                send(&mut socket, &message).await?;
            }
        }
    }
}

async fn send<S>(socket: &mut S, message: &NodeToControl) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(message)?;
    socket.send(Message::text(text)).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ceiling_doubles_from_one_second_up_to_sixty() {
        let backoff = Backoff::default();

        let ceilings: Vec<u64> = (0..8).map(|attempt| backoff.ceiling_for(attempt).as_secs()).collect();

        assert_eq!(ceilings, [1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[test]
    fn a_huge_attempt_count_does_not_overflow() {
        assert_eq!(Backoff::default().ceiling_for(500), Duration::from_secs(60));
    }

    #[test]
    fn jitter_spreads_the_delay_between_half_and_all_of_the_ceiling() {
        let backoff = Backoff::default();

        assert_eq!(backoff.delay_for(3, 0.0), Duration::from_secs(4));
        assert_eq!(backoff.delay_for(3, 1.0), Duration::from_secs(8));
        assert_eq!(backoff.delay_for(3, 0.5), Duration::from_secs(6));
    }
}
