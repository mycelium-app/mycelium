use crate::api::ModelChoice;
use crate::conversation::AgentId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    ClaudeAny,
    CodexAny,
    AnyModel,
}

impl Routing {
    pub fn as_str(self) -> &'static str {
        match self {
            Routing::ClaudeAny => "claude_any",
            Routing::CodexAny => "codex_any",
            Routing::AnyModel => "any_model",
        }
    }

    pub fn parse(text: &str) -> Option<Routing> {
        [Routing::ClaudeAny, Routing::CodexAny, Routing::AnyModel].into_iter().find(|routing| routing.as_str() == text)
    }

    pub fn agent(self) -> Option<AgentId> {
        match self {
            Routing::ClaudeAny => Some(AgentId::Claude),
            Routing::CodexAny => Some(AgentId::Codex),
            Routing::AnyModel => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePreference {
    Preferred,
    #[default]
    Normal,
    Avoid,
    Never,
}

impl NodePreference {
    pub fn weight(self) -> Option<f64> {
        match self {
            NodePreference::Preferred => Some(2.0),
            NodePreference::Normal => Some(1.0),
            NodePreference::Avoid => Some(0.25),
            NodePreference::Never => None,
        }
    }
}

pub const ANY_NODE: Uuid = Uuid::nil();

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnyNodePick {
    pub node_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefaultModes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountMode {
    pub agent: AgentId,
    pub account: String,
    pub mode: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentSettings {
    #[serde(default)]
    pub default_modes: DefaultModes,
    #[serde(default)]
    pub account_modes: Vec<AccountMode>,
    #[serde(default)]
    pub any_model: Vec<ModelChoice>,
    #[serde(default)]
    pub node_preferences: std::collections::HashMap<Uuid, NodePreference>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveAgentSettingsRequest {
    #[serde(default)]
    pub default_modes: DefaultModes,
    #[serde(default)]
    pub any_model: Vec<ModelChoice>,
    #[serde(default)]
    pub node_preferences: std::collections::HashMap<Uuid, NodePreference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetAccountModeRequest {
    pub agent: AgentId,
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricePeriod {
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub amount: f64,
    pub currency: String,
    pub period: PricePeriod,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountPrice {
    pub agent: AgentId,
    pub account: String,
    pub amount: f64,
    pub currency: String,
    pub period: PricePeriod,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetAccountPriceRequest {
    pub agent: AgentId,
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YourPrice {
    pub amount: f64,
    pub currency: String,
    pub period: PricePeriod,
    pub monthly_amount: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_date: Option<String>,
}
