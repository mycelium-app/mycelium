use anyhow::{Context, Result};
use mycelium_proto::conversation::AgentId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const NODE_TOKEN_PREFIX: &str = "myc_node_";
pub const API_TOKEN_FILE: &str = "api-token";
pub const SSH_HOST_KEY_FILE: &str = "ssh_host_ed25519_key";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub control_plane: String,
    pub node_id: Uuid,
    pub token: String,
    pub state_dir: PathBuf,
    pub projects_root: PathBuf,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    #[serde(default, skip_serializing_if = "KubernetesConfig::is_default")]
    pub kubernetes: KubernetesConfig,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KubernetesResources {
    pub requests: BTreeMap<String, String>,
    pub limits: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KubernetesConfig {
    pub enabled: bool,
    pub namespace: String,
    pub image: String,
    pub image_pull_policy: Option<String>,
    pub resources: KubernetesResources,
    pub credentials_secret: String,
    pub storage_class: Option<String>,
    pub storage_size: String,
}

impl Default for KubernetesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            namespace: "mycelium".to_string(),
            image: DEFAULT_AGENT_IMAGE.to_string(),
            image_pull_policy: None,
            resources: KubernetesResources::default(),
            credentials_secret: "mycelium-agent-credentials".to_string(),
            storage_class: None,
            storage_size: "10Gi".to_string(),
        }
    }
}

impl KubernetesConfig {
    pub fn is_default(&self) -> bool {
        *self == KubernetesConfig::default()
    }

    pub fn with_environment(mut self, lookup: impl Fn(&str) -> Option<String>) -> Self {
        if let Some(enabled) = lookup("MYCELIUM_KUBERNETES_ENABLED") {
            self.enabled = matches!(enabled.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
        }

        if let Some(namespace) = lookup("MYCELIUM_KUBERNETES_NAMESPACE") {
            self.namespace = namespace;
        }

        if let Some(image) = lookup("MYCELIUM_KUBERNETES_IMAGE") {
            self.image = image;
        }

        if let Some(policy) = lookup("MYCELIUM_KUBERNETES_IMAGE_PULL_POLICY") {
            self.image_pull_policy = Some(policy);
        }

        if let Some(secret) = lookup("MYCELIUM_KUBERNETES_CREDENTIALS_SECRET") {
            self.credentials_secret = secret;
        }

        if let Some(class) = lookup("MYCELIUM_KUBERNETES_STORAGE_CLASS") {
            self.storage_class = Some(class);
        }

        if let Some(size) = lookup("MYCELIUM_KUBERNETES_STORAGE_SIZE") {
            self.storage_size = size;
        }

        self
    }
}

pub const DEFAULT_AGENT_IMAGE: &str = "ghcr.io/mycelium-app/mycelium-agent:latest";

fn default_max_sessions() -> usize {
    8
}

impl NodeConfig {
    pub fn load(path: &Path) -> Result<NodeConfig> {
        let text = std::fs::read_to_string(path).with_context(|| format!("This node has no credential at {}. Run mycelium node join first.", path.display()))?;
        let mut config: NodeConfig = toml::from_str(&text).with_context(|| format!("{} is not a valid node config.", path.display()))?;
        config.kubernetes = config.kubernetes.with_environment(|name| std::env::var(name).ok());

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let text = toml::to_string_pretty(self)?;
        let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        std::io::Write::write_all(&mut file, text.as_bytes())?;

        Ok(())
    }

    pub fn save_api_token(&self, conversation_id: Uuid, token: &str) -> Result<PathBuf> {
        let path = self.sessions_dir().join(conversation_id.to_string()).join(API_TOKEN_FILE);
        write_private_file(&path, token.as_bytes())?;

        Ok(path)
    }

    pub fn save_ssh_token(&self, request_id: Uuid, conversation_id: Uuid, token: &str) -> Result<PathBuf> {
        let path = self.sessions_dir().join(conversation_id.to_string()).join(format!("ssh-token-{request_id}"));
        write_private_file(&path, token.as_bytes())?;

        Ok(path)
    }

    pub fn ssh_host_key_path(&self) -> PathBuf {
        self.state_dir.join(SSH_HOST_KEY_FILE)
    }

    pub fn bearer_token(&self) -> String {
        if self.token.starts_with(NODE_TOKEN_PREFIX) {
            return self.token.clone();
        }

        format!("{NODE_TOKEN_PREFIX}{}", self.token)
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.state_dir.join("sessions")
    }

    pub fn mcp_server(&self, session_token: &str) -> Value {
        let base = self.control_plane.trim_end_matches('/');

        json!({
            "type": "http",
            "name": "mycelium",
            "url": format!("{base}/api/v1/mcp"),
            "headers": [
                {"name": "Authorization", "value": format!("Bearer {session_token}")},
            ],
        })
    }

    pub fn connect_url(&self) -> Result<String> {
        let socket_base = self.socket_base()?;

        Ok(format!("{socket_base}/api/v1/node/connect"))
    }

    pub fn ssh_url(&self, node: &str) -> Result<String> {
        ssh_url_at(&self.control_plane, node)
    }

    fn socket_base(&self) -> Result<String> {
        socket_base_of(&self.control_plane)
    }
}

pub fn ssh_url_at(control_plane: &str, node: &str) -> Result<String> {
    let socket_base = socket_base_of(control_plane)?;

    Ok(format!("{socket_base}/api/v1/nodes/{}/ssh", percent_encode(node)))
}

fn socket_base_of(control_plane: &str) -> Result<String> {
    let base = control_plane.trim_end_matches('/');

    if let Some(rest) = base.strip_prefix("https://") {
        return Ok(format!("wss://{rest}"));
    }

    if let Some(rest) = base.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or_default();
        let loopback = ["localhost", "127.0.0.1", "[::1]"].iter().any(|name| host == *name || host.strip_prefix(name).is_some_and(|port| port.starts_with(':')));

        if !loopback {
            anyhow::bail!("The control plane address {base} sends the node token without encryption. Use an https:// address.");
        }

        return Ok(format!("ws://{rest}"));
    }

    anyhow::bail!("The control plane address {base} must start with https:// or http://.");
}

pub fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::io::Write::write_all(&mut file, bytes)?;

    Ok(())
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::new();

    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var("MYCELIUM_NODE_CONFIG") {
        return PathBuf::from(path);
    }

    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("mycelium").join("node.toml")
}

pub fn default_state_dir() -> PathBuf {
    dirs::state_dir().or_else(dirs::data_local_dir).unwrap_or_else(|| PathBuf::from(".")).join("mycelium")
}

pub fn default_projects_root() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn agent_command(agent: AgentId) -> Vec<String> {
    let (variable, default) = match agent {
        AgentId::Claude => ("MYCELIUM_CLAUDE_ACP", "claude-agent-acp"),
        AgentId::Codex => ("MYCELIUM_CODEX_ACP", "codex-acp"),
    };
    let value = std::env::var(variable).unwrap_or_else(|_| default.to_string());

    value.split_whitespace().map(str::to_string).collect()
}

pub fn codex_command() -> String {
    std::env::var("MYCELIUM_CODEX").unwrap_or_else(|_| "codex".to_string())
}

pub fn agent_env(agent: AgentId) -> Vec<(String, String)> {
    match agent {
        AgentId::Claude => vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "1".to_string())],
        AgentId::Codex => Vec::new(),
    }
}

pub fn with_developer_instructions(mut env: Vec<(String, String)>, instructions: &str) -> Vec<(String, String)> {
    let position = env.iter().position(|(name, _)| name == "CODEX_CONFIG");
    let held = position.map(|position| env.remove(position).1).or_else(|| std::env::var("CODEX_CONFIG").ok());
    let mut config = held.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    let earlier = config["developer_instructions"].as_str().filter(|text| !text.trim().is_empty());
    let combined = match earlier {
        Some(earlier) => format!("{earlier}\n\n{instructions}"),
        None => instructions.to_string(),
    };

    config["developer_instructions"] = serde_json::Value::String(combined);
    env.push(("CODEX_CONFIG".to_string(), config.to_string()));

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NodeConfig {
        NodeConfig {
            control_plane: "https://mycelium.example/".into(),
            node_id: Uuid::nil(),
            token: "abc".into(),
            state_dir: "/tmp/state".into(),
            projects_root: "/tmp/projects".into(),
            max_sessions: 8,
            kubernetes: KubernetesConfig::default(),
        }
    }

    #[test]
    fn saved_config_is_private_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("node.toml");

        sample().save(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(NodeConfig::load(&path).unwrap(), sample());
    }

    #[test]
    fn saving_over_a_wider_file_tightens_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.toml");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        sample().save(&path).unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn connect_url_swaps_the_scheme() {
        assert_eq!(sample().connect_url().unwrap(), "wss://mycelium.example/api/v1/node/connect");

        let mut plain = sample();
        plain.control_plane = "http://localhost:8080".into();
        assert_eq!(plain.connect_url().unwrap(), "ws://localhost:8080/api/v1/node/connect");
    }

    #[test]
    fn connect_url_refuses_plain_http_except_to_this_machine() {
        let mut plain = sample();

        for address in ["http://mycelium.example", "http://localhost.example.com:8080", "http://127.0.0.1.example.com"] {
            plain.control_plane = address.into();
            let error = plain.connect_url().unwrap_err();

            assert!(format!("{error:#}").contains("Use an https:// address"), "{address}");
        }

        for address in ["http://localhost", "http://127.0.0.1:8080", "http://[::1]:8080"] {
            plain.control_plane = address.into();

            assert!(plain.connect_url().is_ok(), "{address}");
        }
    }

    #[test]
    fn ssh_url_swaps_the_scheme_and_encodes_the_node() {
        assert_eq!(sample().ssh_url("builder one/é").unwrap(), "wss://mycelium.example/api/v1/nodes/builder%20one%2F%C3%A9/ssh");

        let mut plain = sample();
        plain.control_plane = "http://localhost:8080".into();
        assert_eq!(plain.ssh_url("a-b_c.d~e").unwrap(), "ws://localhost:8080/api/v1/nodes/a-b_c.d~e/ssh");

        plain.control_plane = "ftp://nowhere".into();
        assert!(plain.ssh_url("a").is_err());
    }

    #[test]
    fn an_ssh_token_is_saved_privately_beside_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = sample();
        config.state_dir = dir.path().to_path_buf();

        let path = config.save_ssh_token(Uuid::from_u128(2), Uuid::from_u128(3), "myc_ssh_example").unwrap();

        assert_eq!(path, dir.path().join("sessions").join(Uuid::from_u128(3).to_string()).join(format!("ssh-token-{}", Uuid::from_u128(2))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "myc_ssh_example");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn bearer_token_is_prefixed_once() {
        let mut config = sample();
        assert_eq!(config.bearer_token(), "myc_node_abc");

        config.token = "myc_node_abc".into();
        assert_eq!(config.bearer_token(), "myc_node_abc");
    }

    #[test]
    fn the_mcp_server_carries_the_session_token_and_never_the_node_token() {
        let server = sample().mcp_server("myc_session_example");

        assert_eq!(
            server,
            json!({
                "type": "http",
                "name": "mycelium",
                "url": "https://mycelium.example/api/v1/mcp",
                "headers": [
                    {"name": "Authorization", "value": "Bearer myc_session_example"},
                ],
            })
        );
    }

    #[test]
    fn claude_runs_with_its_own_memory_off_and_codex_is_untouched() {
        assert_eq!(agent_env(AgentId::Claude), vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(), "1".to_string())]);
        assert!(agent_env(AgentId::Codex).is_empty());
    }

    #[test]
    fn a_config_without_a_kubernetes_section_has_it_off_and_does_not_write_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.toml");

        sample().save(&path).unwrap();

        assert!(!std::fs::read_to_string(&path).unwrap().contains("kubernetes"));
        assert!(!NodeConfig::load(&path).unwrap().kubernetes.enabled);
    }

    #[test]
    fn the_kubernetes_section_overrides_the_defaults_it_names() {
        let text = r#"
control_plane = "https://mycelium.example"
node_id = "00000000-0000-0000-0000-000000000000"
token = "abc"
state_dir = "/tmp/state"
projects_root = "/workspace"

[kubernetes]
enabled = true
namespace = "agents"
image = "registry.example.com/agent:1"
image_pull_policy = "Always"
credentials_secret = "agent-signins"
storage_class = "fast"

[kubernetes.resources.limits]
memory = "4Gi"
"#;

        let config: NodeConfig = toml::from_str(text).unwrap();

        assert!(config.kubernetes.enabled);
        assert_eq!(config.kubernetes.namespace, "agents");
        assert_eq!(config.kubernetes.image, "registry.example.com/agent:1");
        assert_eq!(config.kubernetes.image_pull_policy.as_deref(), Some("Always"));
        assert_eq!(config.kubernetes.credentials_secret, "agent-signins");
        assert_eq!(config.kubernetes.storage_class.as_deref(), Some("fast"));
        assert_eq!(config.kubernetes.storage_size, "10Gi");
        assert_eq!(config.kubernetes.resources.limits.get("memory").map(String::as_str), Some("4Gi"));
    }

    #[test]
    fn kubernetes_environment_variables_override_the_file() {
        let lookup = |name: &str| match name {
            "MYCELIUM_KUBERNETES_ENABLED" => Some("true".to_string()),
            "MYCELIUM_KUBERNETES_NAMESPACE" => Some("team-a".to_string()),
            "MYCELIUM_KUBERNETES_IMAGE" => Some("registry.example.com/agent:2".to_string()),
            _ => None,
        };

        let config = KubernetesConfig::default().with_environment(lookup);

        assert!(config.enabled);
        assert_eq!(config.namespace, "team-a");
        assert_eq!(config.image, "registry.example.com/agent:2");
        assert_eq!(config.credentials_secret, "mycelium-agent-credentials");
    }

    #[test]
    fn missing_config_says_to_join() {
        let error = NodeConfig::load(Path::new("/nonexistent/node.toml")).unwrap_err();

        assert!(format!("{error:#}").contains("mycelium node join"));
    }

    #[test]
    fn codex_gets_the_instructions_as_developer_instructions_beside_what_the_config_already_holds() {
        let held = vec![("CODEX_CONFIG".to_string(), r#"{"model":"gpt-5","developer_instructions":"Be brief."}"#.to_string()), ("OTHER".to_string(), "1".to_string())];

        let env = with_developer_instructions(held, "Use British English.");
        let config: serde_json::Value = serde_json::from_str(&env.iter().find(|(name, _)| name == "CODEX_CONFIG").unwrap().1).unwrap();

        assert_eq!(config, serde_json::json!({"model": "gpt-5", "developer_instructions": "Be brief.\n\nUse British English."}));
        assert!(env.contains(&("OTHER".to_string(), "1".to_string())));
        assert_eq!(env.iter().filter(|(name, _)| name == "CODEX_CONFIG").count(), 1);
    }

    #[test]
    fn codex_instructions_replace_a_config_that_is_not_json_object() {
        let env = with_developer_instructions(vec![("CODEX_CONFIG".to_string(), "not json".to_string())], "Rules.");

        assert_eq!(env, vec![("CODEX_CONFIG".to_string(), r#"{"developer_instructions":"Rules."}"#.to_string())]);
    }
}
