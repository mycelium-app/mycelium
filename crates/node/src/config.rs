use anyhow::{Context, Result};
use mycelium_proto::conversation::AgentId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const NODE_TOKEN_PREFIX: &str = "myc_node_";
pub const API_TOKEN_FILE: &str = "api-token";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub control_plane: String,
    pub node_id: Uuid,
    pub token: String,
    pub state_dir: PathBuf,
    pub projects_root: PathBuf,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
}

fn default_max_sessions() -> usize {
    8
}

impl NodeConfig {
    pub fn load(path: &Path) -> Result<NodeConfig> {
        let text = std::fs::read_to_string(path).with_context(|| format!("This node has no credential at {}. Run mycelium node join first.", path.display()))?;
        let config = toml::from_str(&text).with_context(|| format!("{} is not a valid node config.", path.display()))?;

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
        let dir = self.sessions_dir().join(conversation_id.to_string());
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(API_TOKEN_FILE);
        let mut file = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        std::io::Write::write_all(&mut file, token.as_bytes())?;

        Ok(path)
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

    pub fn mcp_server(&self, conversation_id: Uuid) -> Value {
        let base = self.control_plane.trim_end_matches('/');

        json!({
            "type": "http",
            "name": "mycelium",
            "url": format!("{base}/api/v1/mcp"),
            "headers": [
                {"name": "Authorization", "value": format!("Bearer {}", self.bearer_token())},
                {"name": "X-Mycelium-Conversation", "value": conversation_id.to_string()},
            ],
        })
    }

    pub fn connect_url(&self) -> Result<String> {
        let base = self.control_plane.trim_end_matches('/');
        let socket_base = if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            anyhow::bail!("The control plane address {base} must start with https:// or http://.");
        };

        Ok(format!("{socket_base}/api/v1/node/connect"))
    }
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
    fn bearer_token_is_prefixed_once() {
        let mut config = sample();
        assert_eq!(config.bearer_token(), "myc_node_abc");

        config.token = "myc_node_abc".into();
        assert_eq!(config.bearer_token(), "myc_node_abc");
    }

    #[test]
    fn the_mcp_server_points_at_the_control_plane_with_the_node_token_and_conversation() {
        let server = sample().mcp_server(Uuid::from_u128(9));

        assert_eq!(
            server,
            json!({
                "type": "http",
                "name": "mycelium",
                "url": "https://mycelium.example/api/v1/mcp",
                "headers": [
                    {"name": "Authorization", "value": "Bearer myc_node_abc"},
                    {"name": "X-Mycelium-Conversation", "value": "00000000-0000-0000-0000-000000000009"},
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
