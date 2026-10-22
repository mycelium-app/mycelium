use crate::config::{NodeConfig, config_path, default_projects_root, default_state_dir};
use anyhow::{Context, Result, bail};
use mycelium_proto::api::{DeviceRequest, DeviceResponse, DeviceTokenRequest, DeviceTokenResponse, ErrorBody};
use mycelium_proto::conversation::DeviceKind;
use std::path::PathBuf;
use std::time::Duration;

const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);

pub struct JoinOptions {
    pub control_plane: String,
    pub join_token_file: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
}

pub fn hostname() -> String {
    let output = std::process::Command::new("hostname").output().ok();
    let name = output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string()).unwrap_or_default();

    if name.is_empty() {
        return "unknown".to_string();
    }

    name
}

pub async fn join(options: JoinOptions) -> Result<()> {
    if options.join_token_file.is_some() {
        bail!("Join tokens arrive in a later release. Join with a device code by leaving out --join-token-file.");
    }

    let control_plane = options.control_plane.trim_end_matches('/').to_string();
    let client = reqwest::Client::new();
    let hostname = hostname();

    let request = DeviceRequest {
        kind: DeviceKind::Node,
        label: hostname.clone(),
        hostname,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };
    let response = client
        .post(format!("{control_plane}/api/v1/device"))
        .json(&request)
        .send()
        .await
        .with_context(|| format!("Could not reach {control_plane}. Check the address and try again."))?;
    if !response.status().is_success() {
        bail!("{control_plane} refused the join request ({}). Check that the address is a Mycelium control plane.", response.status());
    }

    let device: DeviceResponse = response.json().await.context("The control plane sent an answer this node does not understand.")?;

    println!("To join this node, open {} and approve the code {}.", device.verification_uri_complete, device.user_code);
    println!("Or go to {} and enter the code by hand.", device.verification_uri);

    let granted = await_approval(&client, &control_plane, &device).await?;

    let config = NodeConfig {
        control_plane,
        node_id: granted.id,
        token: granted.token,
        state_dir: default_state_dir(),
        projects_root: default_projects_root(),
        max_sessions: 4,
    };
    let path = options.config_path.unwrap_or_else(config_path);
    config.save(&path)?;

    println!("Joined as node {}. Credential written to {}.", config.node_id, path.display());

    Ok(())
}

async fn await_approval(client: &reqwest::Client, control_plane: &str, device: &DeviceResponse) -> Result<DeviceTokenResponse> {
    let mut interval = Duration::from_secs(device.interval);
    let request = DeviceTokenRequest { device_code: device.device_code.clone() };

    loop {
        tokio::time::sleep(interval).await;

        let response = client.post(format!("{control_plane}/api/v1/device/token")).json(&request).send().await.context("Lost contact with the control plane while waiting for approval.")?;
        if response.status().is_success() {
            let granted = response.json().await.context("The control plane sent a credential this node does not understand.")?;

            return Ok(granted);
        }

        let body: ErrorBody = response.json().await.context("The control plane refused the join with an answer this node does not understand.")?;
        match body.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += SLOW_DOWN_STEP,
            "expired_token" => bail!("The code expired before it was approved. Run mycelium node join again."),
            "access_denied" => bail!("The join was denied. Ask whoever runs the control plane to approve it, then run mycelium node join again."),
            _ => bail!("{}", body.message),
        }
    }
}
