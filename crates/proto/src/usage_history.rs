use crate::conversation::AgentId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageHistory {
    pub accounts: Vec<UsageHistoryAccount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageHistoryAccount {
    pub agent: AgentId,
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_price_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_price_as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub your_price: Option<crate::routing::YourPrice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readings_since: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator_until: Option<chrono::DateTime<chrono::Utc>>,
    pub readings: Vec<UsageReading>,
    pub windows: Vec<UsageHistoryWindow>,
    pub tokens_per_point: TokensPerPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_equivalent_month_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_equivalent_per_window_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageReading {
    pub at: chrono::DateTime<chrono::Utc>,
    pub window: String,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageHistoryWindow {
    pub window: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub peak_percent: f64,
    pub tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_point: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_equivalent_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokensPerPoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<f64>,
    pub five_hour_samples: i64,
    pub seven_day_samples: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrchestratorReading {
    pub at: i64,
    pub five: f64,
    pub seven: f64,
    #[serde(default)]
    pub five_resets: Option<i64>,
    #[serde(default)]
    pub seven_resets: Option<i64>,
}
