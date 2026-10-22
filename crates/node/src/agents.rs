use crate::config::agent_command;
use mycelium_proto::conversation::{AgentId, AgentInfo, LoginInfo, LoginState};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const VERSION_TIMEOUT: Duration = Duration::from_secs(3);

pub const ALL_AGENTS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];

pub fn resolve_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);

        return is_executable(&path).then_some(path);
    }

    let search_path = std::env::var_os("PATH")?;

    std::env::split_paths(&search_path).map(|directory| directory.join(program)).find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

async fn report_version(program: &Path) -> Option<String> {
    let output = Command::new(program).arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true).output();
    let output = tokio::time::timeout(VERSION_TIMEOUT, output).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let first_line = text.lines().next()?.trim();

    (!first_line.is_empty()).then(|| first_line.to_string())
}

pub async fn detect_agents(commands: impl Fn(AgentId) -> Vec<String>) -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    for agent in ALL_AGENTS {
        let command = commands(agent);
        let resolved = command.first().and_then(|program| resolve_program(program));
        let version = match &resolved {
            Some(program) => report_version(program).await,
            None => None,
        };

        agents.push(AgentInfo { agent, version, available: resolved.is_some() });
    }

    agents
}

pub async fn detect_default_agents() -> Vec<AgentInfo> {
    detect_agents(agent_command).await
}

pub fn login_state(home: &Path, available: &[AgentInfo]) -> Vec<LoginInfo> {
    let mut logins = Vec::new();

    for info in available.iter().filter(|info| info.available) {
        let (method, credential_present) = match info.agent {
            AgentId::Claude => claude_credential(home),
            AgentId::Codex => ("device_auth", home.join(".codex").join("auth.json").exists()),
        };
        let state = if credential_present { LoginState::SignedIn } else { LoginState::SignedOut };

        logins.push(LoginInfo { agent: info.agent, account: None, method: method.to_string(), state });
    }

    logins
}

fn claude_credential(home: &Path) -> (&'static str, bool) {
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        return ("api_key", true);
    }

    if std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_some() {
        return ("oauth_token", true);
    }

    ("oauth_token", home.join(".claude").join(".credentials.json").exists())
}

pub fn login_command(agent: AgentId, method: &str) -> Option<Vec<String>> {
    let words: &[&str] = match (agent, method) {
        (AgentId::Claude, "oauth_token") => &["claude", "setup-token"],
        (AgentId::Codex, "device_auth") => &["codex", "login", "--device-auth"],
        _ => return None,
    };

    Some(words.iter().map(|word| word.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        path
    }

    #[tokio::test]
    async fn an_agent_is_available_when_its_command_resolves_and_reports_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let claude = write_script(dir.path(), "claude-acp", "echo 'claude-acp 1.2.3'; echo ignored");
        let missing = dir.path().join("nowhere").display().to_string();

        let agents = detect_agents(|agent| match agent {
            AgentId::Claude => vec![claude.display().to_string()],
            AgentId::Codex => vec![missing.clone()],
        })
        .await;

        assert_eq!(agents[0], AgentInfo { agent: AgentId::Claude, version: Some("claude-acp 1.2.3".into()), available: true });
        assert_eq!(agents[1], AgentInfo { agent: AgentId::Codex, version: None, available: false });
    }

    #[tokio::test]
    async fn an_agent_that_does_not_answer_version_is_still_available() {
        let dir = tempfile::tempdir().unwrap();
        let codex = write_script(dir.path(), "codex-acp", "exit 2");

        let agents = detect_agents(|_| vec![codex.display().to_string()]).await;

        assert_eq!(agents[1], AgentInfo { agent: AgentId::Codex, version: None, available: true });
    }

    #[test]
    fn codex_is_signed_in_when_its_auth_file_exists() {
        let home = tempfile::tempdir().unwrap();
        let available = [AgentInfo { agent: AgentId::Codex, version: None, available: true }];

        assert_eq!(login_state(home.path(), &available)[0].state, LoginState::SignedOut);

        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(home.path().join(".codex").join("auth.json"), "{}").unwrap();

        assert_eq!(login_state(home.path(), &available)[0].state, LoginState::SignedIn);
    }

    #[test]
    fn each_agent_signs_in_with_the_method_it_reports() {
        assert_eq!(login_command(AgentId::Claude, "oauth_token"), Some(vec!["claude".to_string(), "setup-token".to_string()]));
        assert_eq!(login_command(AgentId::Codex, "device_auth"), Some(vec!["codex".to_string(), "login".to_string(), "--device-auth".to_string()]));
    }

    #[test]
    fn a_method_the_agent_does_not_offer_has_no_command() {
        assert_eq!(login_command(AgentId::Claude, "device_auth"), None);
        assert_eq!(login_command(AgentId::Codex, "oauth_token"), None);
        assert_eq!(login_command(AgentId::Claude, "api_key"), None);
    }

    #[test]
    fn unavailable_agents_have_no_login_entry() {
        let home = tempfile::tempdir().unwrap();
        let unavailable = [AgentInfo { agent: AgentId::Claude, version: None, available: false }];

        assert!(login_state(home.path(), &unavailable).is_empty());
    }
}
