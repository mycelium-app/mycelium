use crate::config::agent_command;
use crate::usage::{UsageCache, usage_left};
use mycelium_proto::conversation::{AgentId, AgentInfo, LoginInfo, LoginState};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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

        agents.push(AgentInfo { agent, version, available: resolved.is_some(), update_available: None });
    }

    agents
}

pub async fn detect_default_agents() -> Vec<AgentInfo> {
    detect_agents(agent_command).await
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountHome {
    pub profile: Option<String>,
    pub dir: PathBuf,
}

pub fn accounts_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("accounts")
}

pub fn account_homes(home: &Path, accounts_dir: &Path, agent: AgentId) -> Vec<AccountHome> {
    let default_dir = match agent {
        AgentId::Claude => home.join(".claude"),
        AgentId::Codex => home.join(".codex"),
    };
    let mut homes = vec![AccountHome { profile: None, dir: default_dir }];
    let Ok(entries) = std::fs::read_dir(accounts_dir.join(agent.as_str())) else {
        return homes;
    };

    let mut profiles: Vec<AccountHome> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(|name| AccountHome { profile: Some(name.to_string()), dir: entry.path() }))
        .collect();
    profiles.sort_by(|a, b| a.profile.cmp(&b.profile));
    homes.extend(profiles);

    homes
}

pub fn transcript_roots(home: &Path, accounts: &Path, configured: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = configured.to_vec();

    for agent in [AgentId::Claude, AgentId::Codex] {
        let folder = match agent {
            AgentId::Claude => "projects",
            AgentId::Codex => "sessions",
        };

        for account in account_homes(home, accounts, agent) {
            let root = account.dir.join(folder);

            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }

    roots
}

pub fn account_label(home: &Path, agent: AgentId, account: &AccountHome) -> Option<String> {
    let email = (|| -> Option<String> {
        match agent {
            AgentId::Claude => {
                let file = if account.profile.is_none() { home.join(".claude.json") } else { account.dir.join(".claude.json") };
                let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;

                document["oauthAccount"]["emailAddress"].as_str().map(str::to_string)
            }
            AgentId::Codex => {
                let document: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(account.dir.join("auth.json")).ok()?).ok()?;
                let payload = document["tokens"]["id_token"].as_str()?.split('.').nth(1)?;
                let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;

                claims["email"].as_str().map(str::to_string)
            }
        }
    })();

    email.filter(|email| !email.is_empty()).or_else(|| account.profile.clone())
}

pub fn find_account(home: &Path, accounts_dir: &Path, agent: AgentId, label: &str) -> Option<AccountHome> {
    account_homes(home, accounts_dir, agent).into_iter().find(|account| account_label(home, agent, account).as_deref() == Some(label) || account.profile.as_deref() == Some(label))
}

pub fn new_account(accounts_dir: &Path, agent: AgentId, label: &str) -> std::io::Result<AccountHome> {
    let slug: String = label.chars().map(|character| if character.is_ascii_alphanumeric() || character == '.' || character == '@' { character.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.trim_matches(|character| character == '-' || character == '.');
    let name = if slug.is_empty() { "account" } else { slug };
    let dir = accounts_dir.join(agent.as_str()).join(name);
    std::fs::create_dir_all(&dir)?;

    Ok(AccountHome { profile: Some(name.to_string()), dir })
}

pub fn account_env(agent: AgentId, account: &AccountHome) -> Vec<(String, String)> {
    if account.profile.is_none() {
        return Vec::new();
    }

    let variable = match agent {
        AgentId::Claude => "CLAUDE_CONFIG_DIR",
        AgentId::Codex => "CODEX_HOME",
    };

    vec![(variable.to_string(), account.dir.display().to_string())]
}

pub fn login_state(home: &Path, accounts_dir: &Path, available: &[AgentInfo], usage: &UsageCache) -> Vec<LoginInfo> {
    let mut logins = Vec::new();

    for info in available.iter().filter(|info| info.available) {
        for account in account_homes(home, accounts_dir, info.agent) {
            let (method, credential_present) = match (info.agent, &account.profile) {
                (AgentId::Claude, None) => claude_credential(home),
                (AgentId::Claude, Some(_)) => ("oauth_token", account.dir.join(".credentials.json").exists()),
                (AgentId::Codex, _) => ("device_auth", account.dir.join("auth.json").exists()),
            };
            let state = if credential_present { LoginState::SignedIn } else { LoginState::SignedOut };

            let reading = usage.reading(&account.dir).filter(|_| credential_present);
            let usage_left = reading.as_ref().and_then(|reading| usage_left(&reading.windows));
            let (windows, usage_read_at) = reading.map_or((Vec::new(), None), |reading| (reading.windows, Some(reading.read_at)));

            logins.push(LoginInfo { agent: info.agent, account: account_label(home, info.agent, &account), method: method.to_string(), state, usage_left, windows, usage_read_at });
        }
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
        (AgentId::Claude, "oauth_token") => &["claude", "auth", "login"],
        (AgentId::Codex, "device_auth") => &["codex", "login", "--device-auth"],
        _ => return None,
    };

    Some(words.iter().map(|word| word.to_string()).collect())
}

#[cfg(test)]
mod tests {

    #[test]
    fn transcript_discovery_includes_new_managed_accounts_and_keeps_configured_roots() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join(".local/state/mycelium/accounts");
        let configured = vec![home.path().join("extra-transcripts")];
        let first = super::transcript_roots(home.path(), &accounts, &configured);
        assert!(first.contains(&home.path().join(".claude/projects")));
        assert!(first.contains(&home.path().join(".codex/sessions")));
        assert!(first.contains(&configured[0]));
        let codex = accounts.join("codex/account-test");
        let claude = accounts.join("claude/account-test");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::create_dir_all(&claude).unwrap();
        let updated = super::transcript_roots(home.path(), &accounts, &configured);
        assert!(updated.contains(&codex.join("sessions")));
        assert!(updated.contains(&claude.join("projects")));
    }
    use super::*;
    use crate::usage::{CODEX_USAGE_ENDPOINT, USAGE_ENDPOINT, USAGE_MIN_AGE};

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

        assert_eq!(agents[0], AgentInfo { agent: AgentId::Claude, version: Some("claude-acp 1.2.3".into()), available: true, update_available: None });
        assert_eq!(agents[1], AgentInfo { agent: AgentId::Codex, version: None, available: false, update_available: None });
    }

    #[tokio::test]
    async fn an_agent_that_does_not_answer_version_is_still_available() {
        let dir = tempfile::tempdir().unwrap();
        let codex = write_script(dir.path(), "codex-acp", "exit 2");

        let agents = detect_agents(|_| vec![codex.display().to_string()]).await;

        assert_eq!(agents[1], AgentInfo { agent: AgentId::Codex, version: None, available: true, update_available: None });
    }

    #[test]
    fn codex_is_signed_in_when_its_auth_file_exists() {
        let home = tempfile::tempdir().unwrap();
        let available = [AgentInfo { agent: AgentId::Codex, version: None, available: true, update_available: None }];

        assert_eq!(login_state(home.path(), &home.path().join("accounts"), &available, &no_usage())[0].state, LoginState::SignedOut);

        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(home.path().join(".codex").join("auth.json"), "{}").unwrap();

        assert_eq!(login_state(home.path(), &home.path().join("accounts"), &available, &no_usage())[0].state, LoginState::SignedIn);
    }

    #[test]
    fn each_agent_signs_in_with_the_method_it_reports() {
        assert_eq!(login_command(AgentId::Claude, "oauth_token"), Some(vec!["claude".to_string(), "auth".to_string(), "login".to_string()]));
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
        let unavailable = [AgentInfo { agent: AgentId::Claude, version: None, available: false, update_available: None }];

        assert!(login_state(home.path(), &home.path().join("accounts"), &unavailable, &no_usage()).is_empty());
    }

    fn available(agent: AgentId) -> [AgentInfo; 1] {
        [AgentInfo { agent, version: None, available: true, update_available: None }]
    }

    fn no_usage() -> UsageCache {
        UsageCache::new(USAGE_ENDPOINT, CODEX_USAGE_ENDPOINT, USAGE_MIN_AGE)
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn claude_accounts_are_the_default_home_and_each_account_folder_named_by_their_email() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&home.path().join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "sam@example.com"}}"#);
        write(&home.path().join(".claude").join(".credentials.json"), "{}");
        write(&accounts.join("claude").join("work").join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "work@acme.example"}}"#);
        write(&accounts.join("claude").join("work").join(".credentials.json"), "{}");
        std::fs::create_dir_all(accounts.join("claude").join("side")).unwrap();

        let logins = login_state(home.path(), &accounts, &available(AgentId::Claude), &no_usage());

        let listed: Vec<(Option<&str>, LoginState)> = logins.iter().map(|login| (login.account.as_deref(), login.state)).collect();
        assert_eq!(listed, vec![(Some("sam@example.com"), LoginState::SignedIn), (Some("side"), LoginState::SignedOut), (Some("work@acme.example"), LoginState::SignedIn)]);
    }

    #[test]
    fn a_codex_account_is_named_by_the_email_in_its_identity_token() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        let payload = URL_SAFE_NO_PAD.encode(r#"{"email": "sam@example.com"}"#);
        write(&accounts.join("codex").join("sam").join("auth.json"), &format!(r#"{{"tokens": {{"id_token": "e30.{payload}.sig"}}}}"#));

        let logins = login_state(home.path(), &accounts, &available(AgentId::Codex), &no_usage());

        assert_eq!(logins[0].account, None);
        assert_eq!(logins[0].state, LoginState::SignedOut);
        assert_eq!(logins[1].account.as_deref(), Some("sam@example.com"));
        assert_eq!(logins[1].state, LoginState::SignedIn);
    }

    #[test]
    fn an_account_runs_with_its_own_config_folder_and_the_default_account_with_none() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&accounts.join("claude").join("work").join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "work@acme.example"}}"#);
        write(&accounts.join("codex").join("work").join("auth.json"), "{}");

        let claude = find_account(home.path(), &accounts, AgentId::Claude, "work@acme.example").unwrap();
        let codex = find_account(home.path(), &accounts, AgentId::Codex, "work").unwrap();

        assert_eq!(account_env(AgentId::Claude, &claude), vec![("CLAUDE_CONFIG_DIR".to_string(), accounts.join("claude").join("work").display().to_string())]);
        assert_eq!(account_env(AgentId::Codex, &codex), vec![("CODEX_HOME".to_string(), accounts.join("codex").join("work").display().to_string())]);
        assert!(account_env(AgentId::Claude, &account_homes(home.path(), &accounts, AgentId::Claude)[0]).is_empty());
    }

    #[test]
    fn an_account_that_is_not_on_the_node_is_not_found() {
        let home = tempfile::tempdir().unwrap();

        assert_eq!(find_account(home.path(), &home.path().join("accounts"), AgentId::Claude, "nobody@example.com"), None);
    }

    #[test]
    fn a_new_account_gets_a_folder_named_from_its_label() {
        let state = tempfile::tempdir().unwrap();

        let account = new_account(state.path(), AgentId::Claude, "Work@Acme.example").unwrap();

        assert_eq!(account.profile.as_deref(), Some("work@acme.example"));
        assert!(account.dir.is_dir());
        assert_eq!(new_account(state.path(), AgentId::Codex, "!!!").unwrap().profile.as_deref(), Some("account"));
    }
}
