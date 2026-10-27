use crate::boot::run_while_ready;
use crate::error::ApiResult;
use crate::state::AppState;
use crate::store::{enum_text, node_summaries};
use anyhow::{Context, Result, bail};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::stream::StreamMessage;
use semver::Version;
use std::collections::HashMap;
use std::sync::Mutex;

const MYCELIUM: &str = "mycelium";
const AGENTS: [(AgentId, &str); 2] = [(AgentId::Claude, "@agentclientprotocol/claude-agent-acp"), (AgentId::Codex, "@agentclientprotocol/codex-acp")];

#[derive(Default)]
pub struct LatestVersions {
    known: Mutex<HashMap<String, String>>,
}

impl LatestVersions {
    pub fn get(&self, source: &str) -> Option<String> {
        self.known.lock().ok().and_then(|known| known.get(source).cloned())
    }

    fn keep(&self, source: &str, latest: String) -> bool {
        let Ok(mut known) = self.known.lock() else {
            return false;
        };

        known.insert(source.to_string(), latest.clone()).is_none_or(|previous| previous != latest)
    }
}

pub fn newer(installed: &str, latest: Option<String>) -> Option<String> {
    let latest = latest?;

    (parse(&latest)? > parse(installed)?).then_some(latest)
}

fn parse(text: &str) -> Option<Version> {
    Version::parse(text.split_whitespace().next_back()?.trim_start_matches('v')).ok()
}

async fn fetch(state: &AppState, url: &str) -> Result<String> {
    let response = state.http.get(url).send().await.context("the request failed")?;

    if !response.status().is_success() {
        bail!("it answered {}", response.status());
    }

    Ok(response.text().await?)
}

async fn fetch_agent_version(state: &AppState, package: &str) -> Result<String> {
    let body = fetch(state, &format!("{}/{package}/latest", state.config.npm_registry_url.trim_end_matches('/'))).await?;
    let document: serde_json::Value = serde_json::from_str(&body).context("the answer was not JSON")?;

    document["version"].as_str().map(str::to_string).context("the answer has no version")
}

async fn refresh(state: &AppState) -> bool {
    let mut changed = false;
    let release = fetch(state, &state.config.release_url).await.map(|text| text.trim().to_string());
    let mut found = vec![(MYCELIUM.to_string(), "the Mycelium release marker".to_string(), release)];

    for (agent, package) in AGENTS {
        found.push((enum_text(&agent), package.to_string(), fetch_agent_version(state, package).await));
    }

    for (source, name, result) in found {
        match result {
            Ok(latest) if parse(&latest).is_some() => changed |= state.latest.keep(&source, latest),
            Ok(latest) => tracing::warn!("could not read a version from {name}: {latest:?} is not a version. The last known version stays."),
            Err(cause) => tracing::warn!("could not check the latest version of {name}: {cause:#}. The last known version stays."),
        }
    }

    changed
}

async fn announce_refreshed(state: &AppState) -> ApiResult<()> {
    if refresh(state).await {
        for summary in node_summaries(state, None).await? {
            state.publish(None, StreamMessage::Node(summary));
        }
    }

    Ok(())
}

pub async fn refresh_regularly(state: AppState) {
    run_while_ready(&state, "announce the latest versions", state.config.version_check_interval, || announce_refreshed(&state)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_newer_by_number_not_by_text() {
        assert_eq!(newer("v0.1.9", Some("v0.1.10".into())).as_deref(), Some("v0.1.10"));
        assert_eq!(newer("v0.1.10", Some("v0.1.9".into())), None);
        assert_eq!(newer("v0.1.10", Some("v0.1.10".into())), None);
    }

    #[test]
    fn the_installed_version_may_carry_a_package_name_or_a_v() {
        assert_eq!(newer("@agentclientprotocol/codex-acp 2.2.2", Some("2.3.0".into())).as_deref(), Some("2.3.0"));
        assert_eq!(newer("0.94.0", Some("v0.95.0".into())).as_deref(), Some("v0.95.0"));
    }

    #[test]
    fn a_release_is_newer_than_its_own_prerelease() {
        assert_eq!(newer("1.0.0-rc.1", Some("1.0.0".into())).as_deref(), Some("1.0.0"));
        assert_eq!(newer("1.0.0", Some("1.0.0-rc.1".into())), None);
    }

    #[test]
    fn nothing_is_newer_when_either_side_is_not_a_version() {
        assert_eq!(newer("a1b2c3d", Some("v0.1.16".into())), None);
        assert_eq!(newer("v0.1.0", Some("<html>".into())), None);
        assert_eq!(newer("v0.1.0", None), None);
    }
}
