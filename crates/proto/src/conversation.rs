use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    Claude,
    Codex,
}

impl AgentId {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentId::Claude => "claude",
            AgentId::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAgent {
    Claude,
    Codex,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Working,
    Input,
    Review,
    Failed,
    Done,
}

impl ConversationStatus {
    pub fn sidebar_rank(self) -> u8 {
        match self {
            ConversationStatus::Input => 0,
            ConversationStatus::Failed => 1,
            ConversationStatus::Review => 2,
            ConversationStatus::Working => 3,
            ConversationStatus::Done => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideKind {
    Btw,
    Side,
    Fork,
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    UserMessage,
    AcpUpdate,
    PermissionRequest,
    PermissionResolved,
    TurnEnd,
    Error,
    Workflow,
    ModelChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    SignedIn,
    SignedOut,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Node,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Pending,
    Approved,
    Denied,
    Expired,
    Consumed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: Uuid,
    pub owner: Uuid,
    pub node_id: Uuid,
    pub agent: ConversationAgent,
    pub cwd: String,
    pub title: String,
    #[serde(default)]
    pub title_user_set: bool,
    pub status: ConversationStatus,
    pub acp_session_id: Option<String>,
    pub mode: Option<String>,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_kind: Option<SideKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub conversation_id: Uuid,
    pub seq: i64,
    pub kind: EventKind,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub sha256: String,
    pub name: String,
    pub media_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedPrompt {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub position: i32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent: AgentId,
    pub version: Option<String>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_available: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginInfo {
    pub agent: AgentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub method: String,
    pub state: LoginState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_left: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<UsageWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_read_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub name: String,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_hour: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_out_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPosition {
    pub conversation_id: Uuid,
    pub last_seq: i64,
    #[serde(default = "session_is_live")]
    pub live: bool,
}

fn session_is_live() -> bool {
    true
}
