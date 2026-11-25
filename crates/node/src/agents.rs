use crate::config::agent_command;
use crate::usage::{UsageCache, usage_left};
use mycelium_proto::conversation::{AgentId, AgentInfo, LoginInfo, LoginState};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const VERSION_TIMEOUT: Duration = Duration::from_secs(3);

const POD_AGENTS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];

#[cfg(unix)]
pub fn resolve_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);

        return is_executable(&path).then_some(path);
    }

    let search_path = std::env::var_os("PATH")?;

    std::env::split_paths(&search_path).map(|directory| directory.join(program)).find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(windows)]
pub use mycelium_keeper::windows::resolve_program;

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

    for agent in AgentId::ALL {
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

pub fn pod_agents() -> Vec<AgentInfo> {
    POD_AGENTS.into_iter().map(|agent| AgentInfo { agent, version: None, available: true, update_available: None }).collect()
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
        AgentId::Vibe => home.join(".vibe"),
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

pub fn transcript_root(agent: AgentId, account: &AccountHome) -> PathBuf {
    match agent {
        AgentId::Claude => account.dir.join("projects"),
        AgentId::Codex => account.dir.join("sessions"),
        AgentId::Vibe => account.dir.join("logs").join("session"),
    }
}

pub fn transcript_roots(home: &Path, accounts: &Path, configured: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = configured.to_vec();

    for agent in AgentId::ALL {
        for account in account_homes(home, accounts, agent) {
            let root = transcript_root(agent, &account);

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
            AgentId::Vibe => account.profile.is_none().then(|| "default".to_string()),
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

pub fn sign_out(home: &Path, agent: AgentId, account: &AccountHome) -> std::io::Result<()> {
    let removed = match (agent, &account.profile) {
        (AgentId::Claude, None) => std::fs::remove_file(home.join(".claude").join(".credentials.json")),
        (AgentId::Claude, Some(_)) => std::fs::remove_file(account.dir.join(".credentials.json")),
        (AgentId::Codex, _) => std::fs::remove_file(account.dir.join("auth.json")),
        (AgentId::Vibe, _) => {
            let env = account.dir.join(".env");
            let kept: Vec<String> = std::fs::read_to_string(&env)?.lines().filter(|line| !line.trim().starts_with("MISTRAL_API_KEY=")).map(str::to_string).collect();

            std::fs::write(&env, kept.iter().map(|line| format!("{line}\n")).collect::<String>())
        }
    };

    match removed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub fn mark_rejected(home: &Path, agent: AgentId, account: &AccountHome) -> std::io::Result<()> {
    let (_, Some(stored)) = credential(home, agent, account) else {
        return Ok(());
    };

    std::fs::create_dir_all(&account.dir)?;
    std::fs::write(account.dir.join(REJECTED_MARKER), fingerprint(&stored))
}

pub fn account_env(agent: AgentId, account: &AccountHome) -> Vec<(String, String)> {
    if account.profile.is_none() {
        return Vec::new();
    }

    let variable = match agent {
        AgentId::Claude => "CLAUDE_CONFIG_DIR",
        AgentId::Codex => "CODEX_HOME",
        AgentId::Vibe => "VIBE_HOME",
    };

    vec![(variable.to_string(), account.dir.display().to_string())]
}

const REJECTED_MARKER: &str = ".mycelium-rejected";

pub fn login_state(home: &Path, accounts_dir: &Path, available: &[AgentInfo], usage: &UsageCache) -> Vec<LoginInfo> {
    let mut logins = Vec::new();

    for info in available.iter().filter(|info| info.available) {
        for account in account_homes(home, accounts_dir, info.agent) {
            let (method, stored) = credential(home, info.agent, &account);
            let credential_present = stored.is_some();
            let state = match stored {
                None => LoginState::SignedOut,
                Some(stored) if rejected(&account, &stored) || claude_login_ended(info.agent, &stored, chrono::Utc::now().timestamp_millis()) => LoginState::Expired,
                Some(_) => LoginState::SignedIn,
            };

            let reading = usage.reading(&account.dir).filter(|_| credential_present);
            let usage_left = reading.as_ref().and_then(|reading| usage_left(&reading.windows));
            let (windows, usage_read_at, reset_credits, plan, usage_note) = reading.map_or((Vec::new(), None, None, None, None), |reading| (reading.windows, Some(reading.read_at), reading.reset_credits, reading.plan, reading.usage_note));

            logins.push(LoginInfo { agent: info.agent, account: account_label(home, info.agent, &account), method: method.to_string(), state, usage_left, windows, usage_read_at, reset_credits, managed: account.profile.is_some(), plan, usage_note });
        }
    }

    logins
}

fn credential(home: &Path, agent: AgentId, account: &AccountHome) -> (&'static str, Option<Vec<u8>>) {
    match (agent, &account.profile) {
        (AgentId::Claude, None) => claude_credential(home),
        (AgentId::Claude, Some(_)) => ("oauth_token", std::fs::read(account.dir.join(".credentials.json")).ok().filter(|stored| holds_token(stored, &[&["claudeAiOauth", "accessToken"], &["claudeAiOauth", "refreshToken"]]))),
        (AgentId::Codex, _) => ("device_auth", std::fs::read(account.dir.join("auth.json")).ok().filter(|stored| holds_token(stored, &[&["tokens", "access_token"], &["tokens", "refresh_token"], &["tokens", "id_token"], &["OPENAI_API_KEY"]]))),
        (AgentId::Vibe, profile) => ("browser", vibe_key(&account.dir, profile.is_none()).map(String::into_bytes)),
    }
}

fn holds_token(stored: &[u8], paths: &[&[&str]]) -> bool {
    let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(stored) else {
        return false;
    };

    paths.iter().any(|path| path.iter().fold(&parsed, |value, key| &value[*key]).as_str().is_some_and(|token| !token.is_empty()))
}

fn claude_credential(home: &Path) -> (&'static str, Option<Vec<u8>>) {
    if let Some(key) = std::env::var_os("ANTHROPIC_API_KEY") {
        return ("api_key", Some(key.into_encoded_bytes()));
    }

    if let Some(token) = std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN") {
        return ("oauth_token", Some(token.into_encoded_bytes()));
    }

    ("oauth_token", std::fs::read(home.join(".claude").join(".credentials.json")).ok().filter(|stored| holds_token(stored, &[&["claudeAiOauth", "accessToken"], &["claudeAiOauth", "refreshToken"]])))
}

fn claude_login_ended(agent: AgentId, stored: &[u8], now_millis: i64) -> bool {
    let Some(oauth) = (agent == AgentId::Claude).then(|| serde_json::from_slice::<serde_json::Value>(stored).ok()).flatten().map(|parsed| parsed["claudeAiOauth"].clone()).filter(serde_json::Value::is_object) else {
        return false;
    };
    let access_ended = oauth["expiresAt"].as_i64().is_some_and(|at| at <= now_millis);
    let refresh_ended = oauth["refreshToken"].as_str().is_none_or(str::is_empty) || oauth["refreshTokenExpiresAt"].as_i64().is_some_and(|at| at <= now_millis);

    access_ended && refresh_ended
}

fn rejected(account: &AccountHome, stored: &[u8]) -> bool {
    std::fs::read_to_string(account.dir.join(REJECTED_MARKER)).is_ok_and(|marked| marked.trim() == fingerprint(stored))
}

fn fingerprint(stored: &[u8]) -> String {
    use sha2::Digest;

    sha2::Sha256::digest(stored).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn vibe_key(dir: &Path, default: bool) -> Option<String> {
    let stored = std::fs::read_to_string(dir.join(".env")).ok().and_then(|env| env.lines().find_map(|line| line.trim().strip_prefix("MISTRAL_API_KEY=").map(|key| key.trim().trim_matches(['"', '\'']).to_string()))).filter(|key| !key.is_empty());

    stored.or_else(|| default.then(|| std::env::var("MISTRAL_API_KEY").ok()).flatten().filter(|key| !key.is_empty()))
}

pub fn update_command(agent: AgentId, version: &str, installed: &Path) -> Result<Vec<String>, String> {
    let name = agent.display_name();
    let real = std::fs::canonicalize(installed).unwrap_or_else(|_| installed.to_path_buf());
    let path = real.to_string_lossy();

    if path.starts_with("/nix/store/") {
        return Err(format!("{name} on this node comes from Nix, so Mycelium cannot update it. Update it in the node's Nix configuration."));
    }

    if let Some(package) = path.split("/node_modules/").nth(1).map(|rest| rest.split('/').take(if rest.starts_with('@') { 2 } else { 1 }).collect::<Vec<_>>().join("/")) {
        return Ok(vec!["npm".to_string(), "install".to_string(), "--global".to_string(), format!("{package}@{version}")]);
    }

    if let Some(tool) = path.split("/uv/tools/").nth(1).and_then(|rest| rest.split('/').next()) {
        return Ok(vec!["uv".to_string(), "tool".to_string(), "upgrade".to_string(), tool.to_string()]);
    }

    Err(format!("Mycelium cannot tell how {name} was installed at {}. Update it there.", installed.display()))
}

pub struct Tools<'a> {
    pub npm: Option<&'a Path>,
    pub uv: Option<&'a Path>,
    pub node_download: Option<&'a str>,
    pub node_folder: &'a Path,
}

pub fn install_command(agent: AgentId, version: &str, tools: &Tools) -> Result<Vec<String>, String> {
    let name = agent.display_name();
    let from_nix = |program: &Path| std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf()).starts_with("/nix/store");
    let script = match agent {
        AgentId::Claude | AgentId::Codex => {
            let packages = match agent {
                AgentId::Claude => format!("@agentclientprotocol/claude-agent-acp@{version} @anthropic-ai/claude-code"),
                _ => format!("@agentclientprotocol/codex-acp@{version} @openai/codex"),
            };
            let npm = match (tools.npm, tools.node_download) {
                (Some(npm), _) if from_nix(npm) => return Err(format!("Node.js on this node comes from Nix, so Mycelium cannot install {name} with it. Add {name} to the node's Nix configuration.")),
                (Some(npm), _) => quoted(&npm.display().to_string()),
                (None, Some(url)) => {
                    let folder = quoted(&tools.node_folder.display().to_string());

                    return Ok(shell(format!("set -e\necho 'Installing Node.js'\nmkdir -p {folder}\ncurl -fsSL {} | tar -xz -C {folder} --strip-components=1\n{folder}/bin/npm install --global {packages}", quoted(url))));
                }
                (None, None) => return Err(format!("Node.js is not installed on this node and Mycelium could not find a Node.js build for it. Install Node.js, then install {name} again.")),
            };

            format!("set -e\n{npm} install --global {packages}")
        }
        AgentId::Vibe => match tools.uv {
            Some(uv) if from_nix(uv) => return Err(format!("uv on this node comes from Nix, so Mycelium cannot install {name} with it. Add {name} to the node's Nix configuration.")),
            Some(uv) => format!("set -e\n{} tool install mistral-vibe", quoted(&uv.display().to_string())),
            None => "set -e\necho 'Installing uv'\ncurl -LsSf https://astral.sh/uv/install.sh | env UV_NO_MODIFY_PATH=1 sh\n\"$HOME/.local/bin/uv\" tool install mistral-vibe".to_string(),
        },
    };

    Ok(shell(script))
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn shell(script: String) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script]
}

pub fn login_command(agent: AgentId, method: &str) -> Option<Vec<String>> {
    if (agent, method) == (AgentId::Vibe, "browser") {
        let node = std::env::current_exe().ok()?;

        return Some(vec![node.display().to_string(), "node".to_string(), "vibe-sign-in".to_string()]);
    }

    let words: &[&str] = match (agent, method) {
        (AgentId::Claude, "oauth_token") => &["claude", "auth", "login"],
        (AgentId::Codex, "device_auth") => &["codex", "login", "--device-auth"],
        _ => return None,
    };

    Some(words.iter().map(|word| word.to_string()).collect())
}

#[cfg(all(test, unix))]
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
            AgentId::Codex | AgentId::Vibe => vec![missing.clone()],
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
    fn codex_is_signed_in_when_its_auth_file_holds_a_token() {
        let home = tempfile::tempdir().unwrap();
        let available = [AgentInfo { agent: AgentId::Codex, version: None, available: true, update_available: None }];

        assert_eq!(login_state(home.path(), &home.path().join("accounts"), &available, &no_usage())[0].state, LoginState::SignedOut);

        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(home.path().join(".codex").join("auth.json"), r#"{"tokens": {"access_token": "eyJ-example"}}"#).unwrap();

        assert_eq!(login_state(home.path(), &home.path().join("accounts"), &available, &no_usage())[0].state, LoginState::SignedIn);
    }

    #[test]
    fn each_agent_signs_in_with_the_method_it_reports() {
        assert_eq!(login_command(AgentId::Claude, "oauth_token"), Some(vec!["claude".to_string(), "auth".to_string(), "login".to_string()]));
        assert_eq!(login_command(AgentId::Codex, "device_auth"), Some(vec!["codex".to_string(), "login".to_string(), "--device-auth".to_string()]));
        assert_eq!(login_command(AgentId::Vibe, "browser").unwrap()[1..], ["node".to_string(), "vibe-sign-in".to_string()]);
        assert_eq!(login_command(AgentId::Vibe, "api_key"), None);
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
        UsageCache::new(USAGE_ENDPOINT, CODEX_USAGE_ENDPOINT, "codex", USAGE_MIN_AGE)
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn an_agent_installed_with_npm_updates_its_package_to_the_version_offered() {
        let prefix = tempfile::tempdir().unwrap();
        let script = prefix.path().join("lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js");
        write(&script, "");
        std::fs::create_dir_all(prefix.path().join("bin")).unwrap();
        std::os::unix::fs::symlink(&script, prefix.path().join("bin/claude-agent-acp")).unwrap();

        let command = update_command(AgentId::Claude, "0.108.1", &prefix.path().join("bin/claude-agent-acp")).unwrap();

        assert_eq!(command, ["npm", "install", "--global", "@agentclientprotocol/claude-agent-acp@0.108.1"]);
    }

    #[test]
    fn an_agent_installed_as_a_uv_tool_is_upgraded_with_uv() {
        let home = tempfile::tempdir().unwrap();
        let program = home.path().join(".local/share/uv/tools/mistral-vibe/bin/vibe-acp");
        write(&program, "");

        assert_eq!(update_command(AgentId::Vibe, "2.1.0", &program).unwrap(), ["uv", "tool", "upgrade", "mistral-vibe"]);
    }

    #[test]
    fn an_agent_installed_some_other_way_is_refused_with_where_to_update_it() {
        let home = tempfile::tempdir().unwrap();
        let program = home.path().join("bin/codex-acp");
        write(&program, "");

        let refused = update_command(AgentId::Codex, "2.7.0", &program).unwrap_err();

        assert!(refused.starts_with("Mycelium cannot tell how Codex was installed at "), "{refused}");
        assert!(update_command(AgentId::Codex, "2.7.0", Path::new("/nix/store/abc-codex-acp/bin/codex-acp")).unwrap_err().contains("comes from Nix"));
    }

    #[test]
    fn claude_accounts_are_the_default_home_and_each_account_folder_named_by_their_email() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&home.path().join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "sam@example.com"}}"#);
        write(&home.path().join(".claude").join(".credentials.json"), r#"{"claudeAiOauth": {"accessToken": "sk-ant-oat01-example"}}"#);
        write(&accounts.join("claude").join("work").join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "work@acme.example"}}"#);
        write(&accounts.join("claude").join("work").join(".credentials.json"), r#"{"claudeAiOauth": {"accessToken": "sk-ant-oat01-example"}}"#);
        std::fs::create_dir_all(accounts.join("claude").join("side")).unwrap();

        let logins = login_state(home.path(), &accounts, &available(AgentId::Claude), &no_usage());

        let listed: Vec<(Option<&str>, LoginState)> = logins.iter().map(|login| (login.account.as_deref(), login.state)).collect();
        assert_eq!(listed, vec![(Some("sam@example.com"), LoginState::SignedIn), (Some("side"), LoginState::SignedOut), (Some("work@acme.example"), LoginState::SignedIn)]);
    }

    #[test]
    fn a_vibe_account_signs_in_through_the_browser_and_counts_a_key_in_its_env_file() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&accounts.join("vibe").join("blank").join(".env"), "MISTRAL_API_KEY=\n");
        write(&accounts.join("vibe").join("work").join(".env"), "OTHER=1\nMISTRAL_API_KEY='placeholder-key'\n");
        std::fs::create_dir_all(accounts.join("vibe").join("fresh")).unwrap();

        let logins = login_state(home.path(), &accounts, &available(AgentId::Vibe), &no_usage());

        let listed: Vec<(Option<&str>, &str, LoginState)> = logins.iter().skip(1).map(|login| (login.account.as_deref(), login.method.as_str(), login.state)).collect();
        assert_eq!(logins[0].account.as_deref(), Some("default"));
        assert_eq!(listed, vec![(Some("blank"), "browser", LoginState::SignedOut), (Some("fresh"), "browser", LoginState::SignedOut), (Some("work"), "browser", LoginState::SignedIn)]);
        assert!(!format!("{logins:?}").contains("placeholder-key"));
    }

    #[test]
    fn signing_out_a_vibe_account_drops_its_key_and_keeps_its_other_settings() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&home.path().join(".vibe").join(".env"), "OTHER=1\nMISTRAL_API_KEY='placeholder-key'\n");
        let account = find_account(home.path(), &accounts, AgentId::Vibe, "default").unwrap();

        sign_out(home.path(), AgentId::Vibe, &account).unwrap();

        let logins = login_state(home.path(), &accounts, &available(AgentId::Vibe), &no_usage());
        assert_eq!((logins[0].account.as_deref(), logins[0].state), (Some("default"), LoginState::SignedOut));
        assert_eq!(std::fs::read_to_string(home.path().join(".vibe").join(".env")).unwrap(), "OTHER=1\n");
    }

    #[test]
    fn installing_claude_code_uses_the_nodes_npm_for_the_adapter_and_the_cli() {
        let npm = std::path::PathBuf::from("/usr/local/bin/npm");
        let tools = Tools { npm: Some(&npm), uv: None, node_download: None, node_folder: Path::new("/var/lib/mycelium/tools/node") };

        let command = install_command(AgentId::Claude, "0.13.0", &tools).unwrap();

        assert_eq!(command[..2], ["sh".to_string(), "-c".to_string()]);
        assert!(command[2].ends_with("'/usr/local/bin/npm' install --global @agentclientprotocol/claude-agent-acp@0.13.0 @anthropic-ai/claude-code"), "{}", command[2]);
    }

    #[test]
    fn without_npm_codex_installs_node_js_into_mycelium_first() {
        let tools = Tools { npm: None, uv: None, node_download: Some("https://nodejs.example.org/v24.1.0/node-v24.1.0-linux-x64.tar.gz"), node_folder: Path::new("/var/lib/mycelium/tools/node") };

        let script = install_command(AgentId::Codex, "2.7.0", &tools).unwrap().remove(2);

        assert!(script.contains("curl -fsSL 'https://nodejs.example.org/v24.1.0/node-v24.1.0-linux-x64.tar.gz' | tar -xz -C '/var/lib/mycelium/tools/node' --strip-components=1"), "{script}");
        assert!(script.ends_with("'/var/lib/mycelium/tools/node'/bin/npm install --global @agentclientprotocol/codex-acp@2.7.0 @openai/codex"), "{script}");
    }

    #[test]
    fn without_uv_mistral_vibe_installs_uv_first_and_with_it_uses_it() {
        let folder = Path::new("/var/lib/mycelium/tools/node");
        let uv = std::path::PathBuf::from("/opt/uv/bin/uv");

        let without = install_command(AgentId::Vibe, "latest", &Tools { npm: None, uv: None, node_download: None, node_folder: folder }).unwrap().remove(2);
        let with = install_command(AgentId::Vibe, "latest", &Tools { npm: None, uv: Some(&uv), node_download: None, node_folder: folder }).unwrap().remove(2);

        assert!(without.contains("https://astral.sh/uv/install.sh") && without.ends_with("uv\" tool install mistral-vibe"), "{without}");
        assert!(with.ends_with("'/opt/uv/bin/uv' tool install mistral-vibe"), "{with}");
    }

    #[test]
    fn an_agent_is_not_installed_with_tools_from_nix_or_without_node_js_to_fetch() {
        let folder = Path::new("/var/lib/mycelium/tools/node");
        let nix = std::path::PathBuf::from("/nix/store/abc-nodejs/bin/npm");

        let from_nix = install_command(AgentId::Claude, "0.13.0", &Tools { npm: Some(&nix), uv: None, node_download: None, node_folder: folder }).unwrap_err();
        let nothing = install_command(AgentId::Claude, "0.13.0", &Tools { npm: None, uv: None, node_download: None, node_folder: folder }).unwrap_err();

        assert!(from_nix.contains("comes from Nix"), "{from_nix}");
        assert!(nothing.starts_with("Node.js is not installed on this node"), "{nothing}");
    }

    #[test]
    fn an_account_folder_whose_credentials_hold_no_token_is_signed_out() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&accounts.join("claude/account-4916/.credentials.json"), "{}");
        write(&accounts.join("claude/work/.credentials.json"), r#"{"claudeAiOauth": {"accessToken": ""}}"#);
        write(&accounts.join("codex/empty/auth.json"), r#"{"OPENAI_API_KEY": null, "tokens": null}"#);
        write(&accounts.join("codex/signed/auth.json"), r#"{"tokens": {"access_token": "eyJ-example", "refresh_token": "rt-example"}}"#);

        let claude = login_state(home.path(), &accounts, &available(AgentId::Claude), &no_usage());
        let codex = login_state(home.path(), &accounts, &available(AgentId::Codex), &no_usage());

        assert!(claude.iter().filter(|login| login.managed).all(|login| login.state == LoginState::SignedOut), "{claude:?}");
        assert_eq!(codex.iter().filter(|login| login.managed).map(|login| login.state).collect::<Vec<_>>(), vec![LoginState::SignedOut, LoginState::SignedIn]);
    }

    #[test]
    fn a_claude_login_whose_token_and_refresh_have_both_ended_is_expired() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        let now = chrono::Utc::now().timestamp_millis();
        let hour = 3_600_000;
        let credential = |expires: i64, refresh: &str, refresh_expires: i64| format!(r#"{{"claudeAiOauth": {{"accessToken": "sk-ant-oat01-example", "expiresAt": {expires}, "refreshToken": "{refresh}", "refreshTokenExpiresAt": {refresh_expires}}}}}"#);
        write(&accounts.join("claude/cleared/.credentials.json"), &credential(0, "", now - hour));
        write(&accounts.join("claude/lapsed/.credentials.json"), &credential(now - hour, "sk-ant-ort01-example", now - 1));
        write(&accounts.join("claude/refreshes/.credentials.json"), &credential(now - hour, "sk-ant-ort01-example", now + hour));
        write(&accounts.join("claude/current/.credentials.json"), &credential(now + hour, "", now - hour));

        let logins = login_state(home.path(), &accounts, &available(AgentId::Claude), &no_usage());

        let listed: Vec<(Option<&str>, LoginState)> = logins.iter().skip(1).map(|login| (login.account.as_deref(), login.state)).collect();
        assert_eq!(listed, vec![(Some("cleared"), LoginState::Expired), (Some("current"), LoginState::SignedIn), (Some("lapsed"), LoginState::Expired), (Some("refreshes"), LoginState::SignedIn)]);
    }

    #[test]
    fn a_login_the_agent_rejected_is_expired_until_it_signs_in_again() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&home.path().join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "sam@example.com"}}"#);
        write(&home.path().join(".claude/.credentials.json"), r#"{"claudeAiOauth": {"accessToken": "first"}}"#);
        write(&accounts.join("claude/work/.credentials.json"), r#"{"claudeAiOauth": {"accessToken": "work"}}"#);
        let states = || login_state(home.path(), &accounts, &available(AgentId::Claude), &no_usage()).iter().map(|login| login.state).collect::<Vec<_>>();

        mark_rejected(home.path(), AgentId::Claude, &find_account(home.path(), &accounts, AgentId::Claude, "sam@example.com").unwrap()).unwrap();

        assert_eq!(states(), vec![LoginState::Expired, LoginState::SignedIn]);

        write(&home.path().join(".claude/.credentials.json"), r#"{"claudeAiOauth": {"accessToken": "second"}}"#);

        assert_eq!(states(), vec![LoginState::SignedIn, LoginState::SignedIn]);
    }

    #[test]
    fn signing_out_a_claude_or_codex_account_removes_its_credentials_and_only_its_own() {
        let home = tempfile::tempdir().unwrap();
        let accounts = home.path().join("accounts");
        write(&home.path().join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "sam@example.com"}}"#);
        write(&home.path().join(".claude").join(".credentials.json"), r#"{"claudeAiOauth": {"accessToken": "sk-ant-oat01-example"}}"#);
        write(&accounts.join("claude").join("work").join(".credentials.json"), r#"{"claudeAiOauth": {"accessToken": "sk-ant-oat01-example"}}"#);
        write(&accounts.join("codex").join("side").join("auth.json"), "{}");

        sign_out(home.path(), AgentId::Claude, &find_account(home.path(), &accounts, AgentId::Claude, "sam@example.com").unwrap()).unwrap();
        sign_out(home.path(), AgentId::Codex, &find_account(home.path(), &accounts, AgentId::Codex, "side").unwrap()).unwrap();
        sign_out(home.path(), AgentId::Codex, &find_account(home.path(), &accounts, AgentId::Codex, "side").unwrap()).unwrap();

        assert!(!home.path().join(".claude").join(".credentials.json").exists());
        assert!(accounts.join("claude").join("work").join(".credentials.json").exists());
        assert!(!accounts.join("codex").join("side").join("auth.json").exists());
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
