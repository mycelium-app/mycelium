use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::conversation::{AgentId, AgentInfo, DeviceKind, LoginInfo};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Me {
    pub id: Uuid,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceRequest {
    pub kind: DeviceKind,
    pub label: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceTokenRequest {
    pub device_code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceTokenResponse {
    pub kind: DeviceKind,
    pub id: Uuid,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingDevice {
    pub user_code: String,
    pub kind: DeviceKind,
    pub label: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub ip: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSummary {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    pub online: bool,
    pub draining: bool,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub agents: Vec<AgentInfo>,
    pub logins: Vec<LoginInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrainRequest {
    pub draining: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateNodeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

pub const NODE_ICONS: &[&str] = &["server", "laptop", "desktop", "cloud", "terminal", "chip"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartLoginRequest {
    pub agent: AgentId,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartLoginResponse {
    pub login_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginInputRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartTerminalRequest {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartTerminalResponse {
    pub terminal_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalInputRequest {
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResizeTerminalRequest {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateConversationRequest {
    pub node_id: Uuid,
    pub agent: AgentId,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchQueueRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionAnswer {
    pub request_id: String,
    pub option_id: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ConfigRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<std::collections::BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    pub after: i64,
    #[serde(default = "EventsQuery::default_limit")]
    pub limit: i64,
}

impl EventsQuery {
    pub fn default_limit() -> i64 {
        500
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub text: String,
    pub scopes: Vec<String>,
    pub created_by: String,
    pub conversation_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateMemoryRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportStatus {
    pub node_id: Uuid,
    pub agent: AgentId,
    pub found: i64,
    pub imported: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub node_id: Uuid,
    pub agent: AgentId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: i64,
    pub skipped: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeIndexing {
    pub node_id: Uuid,
    pub transcripts: i64,
    pub imported: i64,
    pub conversations: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indexing {
    pub conversations: i64,
    pub entries: i64,
    pub nodes: Vec<NodeIndexing>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "SearchQuery::default_limit")]
    pub limit: i64,
}

impl SearchQuery {
    pub fn default_limit() -> i64 {
        20
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub conversation_id: Uuid,
    pub title: String,
    pub node_id: Uuid,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub snippet: String,
}
