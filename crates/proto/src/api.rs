use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::conversation::{AgentId, AgentInfo, Attachment, DeviceKind, LoginInfo, SideKind};
use crate::link::ResourceSample;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    #[serde(default)]
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_available: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_message: Option<String>,
    pub online: bool,
    pub draining: bool,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub agents: Vec<AgentInfo>,
    pub logins: Vec<LoginInfo>,
    #[serde(default)]
    pub sessions: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_revision: Option<String>,
    #[serde(default)]
    pub instructions_unmanaged: Vec<String>,
    #[serde(default)]
    pub sessions_on_older_instructions: u32,
    #[serde(default)]
    pub harness_models: Vec<HarnessModels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kubernetes: Option<KubernetesNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodState {
    Pending,
    Running,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPod {
    pub conversation_id: Uuid,
    pub name: String,
    pub state: PodState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KubernetesNode {
    pub namespace: String,
    pub image: String,
    pub credentials_secret: String,
    pub workspace_persistent: bool,
    pub pods: Vec<SessionPod>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessModels {
    pub agent: AgentId,
    #[serde(default)]
    pub account: Option<String>,
    pub models: Vec<HarnessModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub reported_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SshToken {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
}

pub const NODE_ICONS: &[&str] = &["server", "laptop", "desktop", "cloud", "terminal", "chip"];

pub const NODE_COLOURS: &[&str] = &["blue", "violet", "amber", "rose", "green"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartLoginRequest {
    pub agent: AgentId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UseResetRequest {
    pub agent: AgentId,
    #[serde(default)]
    pub account: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UseResetResponse {
    pub outcome: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_kind: Option<SideKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::Routing>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartSideChatRequest {
    pub kind: SideKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_title: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_default_account: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_project: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teach_me: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContinueRequest {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_default_account: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::Routing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Browser,
    Client,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub kind: SessionKind,
    pub label: String,
    pub hostname: String,
    pub address: String,
    pub last_used_at: chrono::DateTime<chrono::Utc>,
    pub current: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PromptRequest {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spoken: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchQueueRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveQueuedRequest {
    pub index: usize,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<i64>,
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
    #[serde(default)]
    pub bank_id: Option<Uuid>,
    #[serde(default)]
    pub source: MemorySource,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelClass {
    Private,
    Zdr,
    #[default]
    Cloud,
}

impl ModelClass {
    pub fn rank(self) -> u8 {
        match self {
            ModelClass::Private => 0,
            ModelClass::Zdr => 1,
            ModelClass::Cloud => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryBank {
    pub id: Uuid,
    pub name: String,
    pub model_class: ModelClass,
    #[serde(default)]
    pub group_ids: Vec<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveMemoryBankRequest {
    pub name: String,
    pub model_class: ModelClass,
    #[serde(default)]
    pub group_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveMemoryRequest {
    pub bank_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemorySource {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub node_name: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub conversation_title: Option<String>,
    #[serde(default)]
    pub message_seq: Option<i64>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportJobState {
    Running,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportProblem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportJob {
    pub id: Uuid,
    pub node_id: Uuid,
    pub agent: AgentId,
    pub state: ImportJobState,
    pub total: i64,
    pub done: i64,
    pub imported: i64,
    pub skipped: i64,
    pub waiting: i64,
    pub failed: Vec<ImportProblem>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    Github,
    Gitlab,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forge {
    pub id: Uuid,
    pub kind: ForgeKind,
    pub base_url: String,
    pub account: String,
    pub token_hint: String,
    pub organisations: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_error: Option<String>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectForgeRequest {
    pub kind: ForgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_mcp: Option<bool>,
}

impl std::fmt::Debug for ConnectForgeRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ConnectForgeRequest").field("kind", &self.kind).field("base_url", &self.base_url).field("token", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderKind {
    OpenaiCompatible,
    Anthropic,
}

pub const DECISION_USES: [&str; 2] = ["decide", "browser"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionProvider {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub key_hint: Option<String>,
    pub model: String,
    pub uses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateDecisionProviderRequest {
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub model: String,
    #[serde(default)]
    pub uses: Vec<String>,
}

impl std::fmt::Debug for CreateDecisionProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CreateDecisionProviderRequest").field("name", &self.name).field("base_url", &self.base_url).field("api_key", &"<hidden>").finish()
    }
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PatchDecisionProviderRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<Vec<String>>,
}

impl std::fmt::Debug for PatchDecisionProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PatchDecisionProviderRequest").field("name", &self.name).field("base_url", &self.base_url).field("api_key", &"<hidden>").finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TestDecisionProviderRequest {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl std::fmt::Debug for TestDecisionProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("TestDecisionProviderRequest").field("base_url", &self.base_url).field("key", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionProviderModels {
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProvider {
    pub id: Uuid,
    pub kind: ModelProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    pub key_hint: Option<String>,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub favourites: Vec<String>,
    #[serde(default)]
    pub model_class: ModelClass,
    #[serde(default)]
    pub speech_model: Option<String>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateModelProviderRequest {
    pub kind: ModelProviderKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl std::fmt::Debug for CreateModelProviderRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CreateModelProviderRequest").field("kind", &self.kind).field("name", &self.name).field("base_url", &self.base_url).field("api_key", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateModelProviderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model_class: Option<ModelClass>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetFavouriteModelRequest {
    pub model: String,
    pub favourite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatToolInfo {
    pub name: String,
    pub description: String,
    #[serde(default = "write_kind")]
    pub kind: ToolKind,
    #[serde(default)]
    pub asks: bool,
    #[serde(default)]
    pub edits: bool,
    #[serde(default)]
    pub ask_source: String,
    #[serde(default)]
    pub edit_source: String,
}

fn write_kind() -> ToolKind {
    ToolKind::Write
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PolicyLevel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<bool>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tools: std::collections::BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPreset {
    #[default]
    AskWrites,
    AskAll,
    AskAllEdit,
    DontAsk,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IntegrationPolicy {
    #[serde(default)]
    pub preset: ApprovalPreset,
    #[serde(default)]
    pub ask: PolicyLevel,
    #[serde(default)]
    pub edit: PolicyLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseToEdit {
    pub tool: String,
    pub server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_error: Option<String>,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_paths: Option<Vec<Vec<Value>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseFilterRequest {
    pub filter: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ResponseFilterResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_paths: Option<Vec<Vec<Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendResponseRequest {
    #[serde(default)]
    pub send: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStatus {
    Disabled,
    NeedsSignIn,
    Expired,
    ExpiresSoon,
    Connected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegrationCall {
    pub id: Uuid,
    pub server_id: Option<Uuid>,
    pub server_name: String,
    pub tool: String,
    pub kind: ToolKind,
    pub conversation_id: Option<Uuid>,
    pub surface: String,
    pub provider: Option<String>,
    pub decision: String,
    pub review: Option<String>,
    pub is_error: bool,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetIntegrationGroupsRequest {
    pub groups: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatToolServer {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "mcp_kind")]
    pub kind: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub settings: Value,
    pub token_hint: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oauth: bool,
    pub enabled: bool,
    pub tools: Vec<ChatToolInfo>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default = "connected")]
    pub status: IntegrationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub policy: IntegrationPolicy,
    #[serde(default)]
    pub groups: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastFmSignIn {
    pub authorize_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreeAgentSignIn {
    pub authorize_url: String,
}

fn connected() -> IntegrationStatus {
    IntegrationStatus::Connected
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartChatToolServerOAuthRequest {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthStart {
    pub authorize_url: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateChatToolServerRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<IntegrationPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Uuid>>,
}

fn mcp_kind() -> String {
    "mcp".to_string()
}

impl std::fmt::Debug for CreateChatToolServerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CreateChatToolServerRequest").field("name", &self.name).field("kind", &self.kind).field("url", &self.url).field("account", &self.account).field("bearer_token", &"<hidden>").finish()
    }
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PatchChatToolServerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_token_expiry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<IntegrationPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

impl std::fmt::Debug for PatchChatToolServerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PatchChatToolServerRequest").field("enabled", &self.enabled).field("name", &self.name).field("bearer_token", &self.bearer_token.as_ref().map(|_| "<hidden>")).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateChatRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    pub forge_id: Uuid,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub default_branch: Option<String>,
    #[serde(default)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgeOrganisation {
    pub forge_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgeFailure {
    pub forge_id: Uuid,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repositories {
    pub repositories: Vec<Repository>,
    pub organisations: Vec<ForgeOrganisation>,
    pub failures: Vec<ForgeFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub node_id: Uuid,
    pub name: String,
    pub path: String,
    pub forge_id: Option<Uuid>,
    pub repository: Option<String>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub avatar_sha256: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchProjectRequest {
    #[serde(default)]
    pub group_id: Option<Uuid>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub clear_icon: bool,
    #[serde(default)]
    pub test_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub folders: Vec<GroupFolder>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub hours: Hours,
    pub open_now: bool,
    #[serde(default)]
    pub closes_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub opens_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub opened_until: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRequest {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupFolder {
    pub id: Uuid,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupFolderRequest {
    #[serde(default)]
    pub node_id: Option<Uuid>,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extensions {
    pub generated_ui: bool,
    pub rich_markdown: bool,
    pub mycelium_tools: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnUsageLimit {
    Ask,
    Resume,
    Switch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageLimitSetting {
    pub on_usage_limit: OnUsageLimit,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ResumeRequest {
    #[serde(default)]
    pub at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionVersion {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionLayer {
    pub layer: String,
    pub name: String,
    pub path: Option<String>,
    pub text: String,
    pub version: Option<InstructionVersion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionSource {
    pub forge_id: Uuid,
    pub repository: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instructions {
    pub source: Option<InstructionSource>,
    pub revision: Option<String>,
    pub layers: Vec<InstructionLayer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveInstructionsRequest {
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionProposalState {
    Proposed,
    Approved,
    Declined,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstructionProposal {
    pub id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub conversation_title: Option<String>,
    pub layer: String,
    pub layer_name: String,
    pub base_revision: String,
    pub base_text: String,
    pub text: String,
    pub reason: String,
    pub state: InstructionProposalState,
    pub note: Option<String>,
    pub out_of_date: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeclineInstructionProposalRequest {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub redo: bool,
    #[serde(default = "tell_the_agent")]
    pub notify: bool,
}

fn tell_the_agent() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageProposalState {
    Proposed,
    Sent,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationConnection {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub source_title: String,
    pub target_title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateConversationConnectionRequest {
    pub source_id: Uuid,
    pub target_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupConnection {
    pub id: Uuid,
    pub source_group_id: Uuid,
    pub target_group_id: Uuid,
    pub source_name: String,
    pub target_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateGroupConnectionRequest {
    pub source_group_id: Uuid,
    pub target_group_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageProposal {
    pub id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub target_id: Uuid,
    pub target_title: String,
    pub target_node: String,
    pub target_project: Option<String>,
    pub message: String,
    pub reason: String,
    pub state: MessageProposalState,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub source_group: Option<String>,
    #[serde(default)]
    pub target_group: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartProposalState {
    Proposed,
    Started,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartProposalNode {
    pub id: Uuid,
    pub name: String,
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartProposalStep {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub gate: bool,
    pub node: Option<StartProposalNode>,
    pub folder: Option<String>,
    pub agent: Option<String>,
    pub account: Option<String>,
    pub model: Option<String>,
    pub first_message: String,
    #[serde(default)]
    pub node_choices: Vec<StartProposalNode>,
    #[serde(default)]
    pub account_choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartProposal {
    pub id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub kind: String,
    pub title: String,
    pub steps: Vec<StartProposalStep>,
    pub state: StartProposalState,
    pub connect: bool,
    pub workflow_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartProposalChoice {
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApproveStartProposalRequest {
    #[serde(default)]
    pub choices: std::collections::HashMap<String, StartProposalChoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub title: String,
    pub steps: Vec<WorkflowStep>,
    #[serde(default = "default_workflow_rounds")]
    pub max_rounds: u32,
    #[serde(default)]
    pub stop_when: Option<String>,
}

fn default_workflow_rounds() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub gate: bool,
    #[serde(default)]
    pub workflow: Option<Box<WorkflowPlan>>,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub agent: Option<AgentId>,
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRunStep {
    pub key: String,
    pub status: String,
    pub conversation_id: Option<Uuid>,
    pub workflow_id: Option<Uuid>,
    pub output: String,
    #[serde(default)]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub tokens: Option<i64>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub turns: Option<i32>,
    #[serde(default)]
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowActor {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowEndReason {
    pub reason: String,
    #[serde(default)]
    pub step: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub by: Option<WorkflowActor>,
    #[serde(default)]
    pub round: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDecision {
    pub round: i32,
    pub step: String,
    pub decision: String,
    pub by: WorkflowActor,
    pub at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRound {
    pub round: i32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub decided_by: Option<WorkflowActor>,
    pub steps: Vec<WorkflowRunStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub title: String,
    pub status: String,
    pub plan: WorkflowPlan,
    pub steps: Vec<WorkflowRunStep>,
    pub round: i32,
    pub output: String,
    #[serde(default = "default_workflow_kind")]
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub started_by: Option<WorkflowActor>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub end_reason: Option<WorkflowEndReason>,
    #[serde(default)]
    pub stop_rule: Option<String>,
    #[serde(default)]
    pub rerun_of: Option<Uuid>,
    #[serde(default)]
    pub rounds: Vec<WorkflowRound>,
    #[serde(default)]
    pub decisions: Vec<WorkflowDecision>,
    #[serde(default)]
    pub phases: Vec<String>,
    #[serde(default)]
    pub run_id: Option<String>,
}

fn default_workflow_kind() -> String {
    "mycelium".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowControlRequest {
    pub action: String,
    pub step: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiAccessState {
    Requested,
    Approved,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiAccessRequest {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub scopes: Vec<String>,
    pub hours: i32,
    pub reason: String,
    pub state: ApiAccessState,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegrationApiGrant {
    pub token_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub conversation_title: Option<String>,
    pub scopes: Vec<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub calls: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApproveApiAccessRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAccessState {
    Requested,
    Approved,
    Declined,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SshAccessRequest {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub node_id: Uuid,
    pub node_name: String,
    pub minutes: i32,
    pub reason: String,
    pub state: SshAccessState,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApproveSshAccessRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetInstructionSourceRequest {
    pub forge_id: Uuid,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub node_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub new_repository: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Worktree {
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch: String,
    pub path: String,
    pub issue_number: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub forge_id: Uuid,
    pub repository: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_requests: Option<Vec<PullRequest>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: i64,
    pub label: String,
    pub url: String,
    pub state: PullRequestState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issues {
    pub issues: Vec<Issue>,
    pub failures: Vec<ForgeFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartWorkspaceRequest {
    pub agent: AgentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Question,
    Proposed,
    Open,
    Claimed,
    Done,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub node_id: Uuid,
    pub agent: AgentId,
    pub cwd: String,
    pub project_id: Option<Uuid>,
    pub branch: Option<String>,
    pub title: String,
    pub prompt: Option<String>,
    pub status: TaskStatus,
    pub options: Vec<String>,
    pub answer: Option<String>,
    pub source_id: Option<Uuid>,
    pub source_title: Option<String>,
    pub conversation_id: Option<Uuid>,
    pub depends_on: Vec<Uuid>,
    pub waits_on: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerTaskRequest {
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartTaskRequest {
    pub conversation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub node_id: Uuid,
    pub agent: AgentId,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EachRun {
    New,
    Same,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Started,
    Skipped,
    Held,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::Routing>,
    pub cwd: String,
    pub project_id: Option<Uuid>,
    pub prompt: String,
    pub cron: String,
    pub each_run: EachRun,
    pub paused: bool,
    pub time_zone: String,
    pub proposed: bool,
    pub conversation_id: Option<Uuid>,
    pub next_run_at: chrono::DateTime<chrono::Utc>,
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub action_inputs: std::collections::BTreeMap<String, String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::Routing>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub action_inputs: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    pub prompt: String,
    pub cron: String,
    pub each_run: EachRun,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub each_run: Option<EachRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfirmScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub each_run: Option<EachRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetTimeZoneRequest {
    pub time_zone: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub id: Uuid,
    pub schedule_id: Uuid,
    pub ran_at: chrono::DateTime<chrono::Utc>,
    pub outcome: RunOutcome,
    pub reason: Option<String>,
    pub conversation_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuPoint {
    pub at_ms: i64,
    pub cpu_percent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeResources {
    pub latest: Option<ResourceSample>,
    pub history: Vec<CpuPoint>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActivityRow {
    pub key: String,
    pub name: String,
    pub turns: i64,
    pub reported_turns: i64,
    pub working_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub cost_estimated: bool,
    #[serde(default)]
    pub subagent_tokens: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConversationUsage {
    pub context_used: Option<i64>,
    pub context_size: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub range: String,
    pub totals: ActivityRow,
    pub days: Vec<ActivityRow>,
    #[serde(default)]
    pub hours: Vec<ActivityRow>,
    pub conversations: Vec<ActivityRow>,
    pub nodes: Vec<ActivityRow>,
    pub agents: Vec<ActivityRow>,
    pub projects: Vec<ActivityRow>,
    pub models: Vec<ActivityRow>,
    pub unreported_agents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveRequest {
    pub node_id: Uuid,
    #[serde(default)]
    pub fork: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovePlan {
    pub movable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub changed_files: u32,
    pub project_on_target: bool,
    pub clones_project: bool,
    pub agent_session: bool,
    pub stays_ports: Vec<u16>,
    pub stays_processes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveState {
    Waiting,
    Moving,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveProgress {
    pub conversation_id: Uuid,
    pub node_id: Uuid,
    pub fork: bool,
    pub state: MoveState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    Active,
    Paused,
    Met,
    Stuck,
    OverBudget,
    LimitReached,
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalWait {
    RepeatedOutcome,
    QuestionWaiting,
    BackgroundWork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalDelivery {
    Pending,
    #[default]
    Sent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub conversation_id: Uuid,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour_limit: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_limit: Option<u8>,
    pub tokens_used: i64,
    #[serde(default)]
    pub tokens_known: bool,
    pub turns: i64,
    #[serde(default)]
    pub turn_active: bool,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour_used: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_used: Option<f64>,
    pub state: GoalState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub delivery: GoalDelivery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_since: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub held_seconds: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumes_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub nudges: i64,
    #[serde(default)]
    pub repeats: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_nudge_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_reason: Option<GoalWait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetGoalRequest {
    pub text: String,
    #[serde(default)]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub time_limit_seconds: Option<i64>,
    #[serde(default)]
    pub five_hour_limit: Option<i64>,
    #[serde(default)]
    pub weekly_limit: Option<i64>,
    #[serde(default)]
    pub repeat_limit: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchGoalRequest {
    pub paused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContinueGoalRequest {
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughFile {
    pub path: String,
    pub diff: String,
    #[serde(default)]
    pub unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughStep {
    pub title: String,
    pub key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub explanation: String,
    pub files: Vec<WalkthroughFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughNote {
    pub id: Uuid,
    pub step: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<i32>,
    pub quote: String,
    pub comment: String,
    pub sent: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Walkthrough {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub steps: Vec<WalkthroughStep>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub dismissed: bool,
    #[serde(default = "default_walkthrough_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<WalkthroughCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reflection {
    pub id: Uuid,
    pub week: String,
    pub done: String,
    pub learned: String,
    pub waiting: String,
    pub next_week: String,
    pub edited: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafted_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatchReflectionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_week: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedraftReflection {
    pub conversation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughCheck {
    pub step: i32,
    pub question: String,
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughAnswerRequest {
    pub choice: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkthroughAnswer {
    pub step: i32,
    pub choice: i32,
    pub correct: bool,
    #[serde(default)]
    pub explanation: String,
}

fn default_walkthrough_scope() -> String {
    "branch".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Design,
    Page,
    Markdown,
    File,
    Drawing,
    Typst,
    Doc,
    Slides,
    Dashboard,
    Motion,
    DesignSystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAuthor {
    Agent,
    Person,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactVersion {
    pub version: i32,
    pub author: ArtifactAuthor,
    pub media_type: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactOrigin {
    pub conversation_id: Uuid,
    pub conversation_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub owner: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    #[serde(default)]
    pub conversation_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ArtifactOrigin>,
    pub kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_system_id: Option<Uuid>,
    pub title: String,
    pub slug: String,
    pub public: bool,
    pub latest_version: i32,
    #[serde(default)]
    pub thumbnail_version: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<ArtifactVersion>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactLink {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub project_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub permission: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkArtifactRequest {
    pub project_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub permission: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactAccessRequest {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub conversation_id: Uuid,
    pub permission: String,
    pub reason: String,
    pub state: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ApproveArtifactAccessRequest {
    #[serde(default)]
    pub project: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedArtifact {
    #[serde(flatten)]
    pub artifact: Artifact,
    pub media_type: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactComment {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub author_name: String,
    pub body: String,
    #[serde(default)]
    pub anchor: Option<Value>,
    pub resolved: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactDataEntry {
    pub key: String,
    pub value: Value,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateArtifactRequest {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub content_base64: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<Uuid>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub design_system_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddArtifactVersionRequest {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub content_base64: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchArtifactRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub public: Option<bool>,
    #[serde(default)]
    pub design_system_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateArtifactCommentRequest {
    pub body: String,
    #[serde(default)]
    pub anchor: Option<Value>,
    #[serde(default)]
    pub author_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchArtifactCommentRequest {
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantState {
    Working,
    Input,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub any_model: bool,
}

fn chat_first_by_default() -> bool {
    true
}

fn fall_back_by_default() -> Vec<String> {
    vec!["usage_limit".into(), "signed_out".into(), "offline".into()]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantPreferences {
    #[serde(default)]
    pub models: Vec<ModelChoice>,
    #[serde(default)]
    pub nodes: Vec<Uuid>,
    #[serde(default = "chat_first_by_default")]
    pub chat_first: bool,
    #[serde(default = "fall_back_by_default")]
    pub fall_back_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_speed: Option<String>,
}

impl Default for AssistantPreferences {
    fn default() -> Self {
        AssistantPreferences { models: Vec::new(), nodes: Vec::new(), chat_first: true, fall_back_on: fall_back_by_default(), voice_name: None, speech_speed: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantSecret {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub state: AssistantSecretState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantSecretState {
    Set,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PutAssistantSecretRequest {
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assistant {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    pub preferences: AssistantPreferences,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    pub state: AssistantState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub session_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_choice: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<AssistantSecret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<AssistantBackup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wakes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_key: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateAssistantRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub preferences: Option<AssistantPreferences>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PatchAssistantRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    #[serde(default)]
    pub clear_group: bool,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub preferences: Option<AssistantPreferences>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionInputKind {
    Text,
    Date,
    Choice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionInput {
    pub name: String,
    pub kind: ActionInputKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantAction {
    pub id: Uuid,
    pub assistant_id: Uuid,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub inputs: Vec<ActionInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
    pub pinned: bool,
    pub position: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_key: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveActionRequest {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub inputs: Vec<ActionInput>,
    #[serde(default)]
    pub model: Option<ModelChoice>,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunActionRequest {
    #[serde(default)]
    pub inputs: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionProposal {
    pub id: Uuid,
    pub assistant_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub inputs: Vec<ActionInput>,
    pub reason: String,
    pub state: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantChange {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<Uuid>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferences: Option<AssistantPreferences>,
    pub state: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantFile {
    pub path: String,
    pub size: i64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backed_up: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantBackup {
    pub forge_id: Uuid,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetAssistantBackupRequest {
    pub forge_id: Uuid,
    pub repository: String,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoursDay {
    pub weekday: u8,
    pub closed: bool,
    pub opens: String,
    pub closes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hours {
    pub always_open: bool,
    pub days: Vec<HoursDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenGroupRequest {
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupOpening {
    pub opened_until: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutOfHoursOpen {
    pub group_id: Uuid,
    pub group_name: String,
    pub reason: String,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub until: chrono::DateTime<chrono::Utc>,
    pub closed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WellnessSettings {
    pub break_reminders: bool,
    pub break_after_minutes: i32,
    pub snooze_minutes: i32,
    pub break_minutes: i32,
    pub hours: Hours,
    pub quiet_hold_notifications: bool,
    pub quiet_hold_schedules: bool,
    pub quiet_hold_pings: bool,
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WellnessHeartbeat {
    pub seconds: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySpan {
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityBreak {
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityDay {
    pub date: String,
    pub on_screen_minutes: i64,
    pub spans: Vec<ActivitySpan>,
    pub breaks: Vec<ActivityBreak>,
    pub recorded_since: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityHoursDay {
    pub date: String,
    pub minutes: Vec<i64>,
    pub top: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityHours {
    pub days: Vec<ActivityHoursDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartBreakRequest {
    #[serde(default)]
    pub minutes: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub id: Uuid,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ends_at: chrono::DateTime<chrono::Utc>,
    pub ended_early_at: Option<chrono::DateTime<chrono::Utc>>,
    pub planned_minutes: i32,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ended_early: bool,
    pub note: Option<String>,
    pub noted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakNoteRequest {
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WellnessDay {
    pub day: String,
    pub viewing_minutes: i64,
    pub agent_minutes: i64,
    pub breaks: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WellnessOverview {
    pub days: Vec<WellnessDay>,
    pub last_break_at: Option<chrono::DateTime<chrono::Utc>>,
    pub current_break: Option<Break>,
    #[serde(default)]
    pub awaiting_note: Option<Break>,
    #[serde(default)]
    pub recent_breaks: Vec<Break>,
    pub quiet_now: bool,
    pub held: i64,
    pub held_week: HeldWeek,
    #[serde(default)]
    pub opened_out_of_hours: Vec<OutOfHoursOpen>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeldWeek {
    pub notifications: i64,
    pub schedules: i64,
    pub pings: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnedItem {
    pub id: Uuid,
    pub text: String,
    pub source_kind: String,
    pub conversation_id: Uuid,
    pub conversation_title: String,
    pub message_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldRequest {
    pub kind: String,
    pub subject: String,
    #[serde(default)]
    pub group_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PuzzleResult {
    #[serde(default)]
    pub solved: Option<bool>,
    #[serde(default)]
    pub score: Option<i32>,
    #[serde(default)]
    pub seconds: Option<i32>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PuzzleDay {
    pub day: String,
    pub result: PuzzleResult,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PuzzleBucket {
    pub value: i32,
    pub count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PuzzleStats {
    pub played: i64,
    pub solved: i64,
    pub average: Option<f64>,
    #[serde(default)]
    pub average_seconds: Option<f64>,
    #[serde(default)]
    pub streak: i64,
    #[serde(default)]
    pub best_streak: i64,
    pub distribution: Vec<PuzzleBucket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PuzzleSummary {
    pub puzzle: String,
    pub day: String,
    pub today: Option<PuzzleResult>,
    pub history: Vec<PuzzleDay>,
    pub stats: PuzzleStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordleGuess {
    pub word: String,
    pub marks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordleGame {
    pub day: String,
    pub status: String,
    pub guesses: Vec<WordleGuess>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordleGuessRequest {
    pub word: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionsGroup {
    pub title: String,
    pub words: Vec<String>,
    pub level: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionsGame {
    pub day: String,
    pub status: String,
    pub words: Vec<String>,
    pub solved: Vec<ConnectionsGroup>,
    pub unsolved: Vec<ConnectionsGroup>,
    pub mistakes: i32,
    pub mistakes_allowed: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionsGuessRequest {
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionsGuessAnswer {
    pub outcome: String,
    pub game: ConnectionsGame,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub kind: String,
    pub name: String,
    pub own_lines: i64,
    pub agent_lines: i64,
    pub own_share: Option<f64>,
    pub weekly_own_lines: Vec<i64>,
    pub last_practised: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HiddenSkillRequest {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub path: String,
    pub line: i32,
    pub body: String,
    #[serde(default)]
    pub praise: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExerciseReview {
    pub id: Uuid,
    pub summary: String,
    pub comments: Vec<ReviewComment>,
    pub tests: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exercise {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub title: String,
    pub brief: String,
    pub minutes: Option<i32>,
    pub state: String,
    pub branch: Option<String>,
    pub conversation_id: Option<Uuid>,
    pub reviewing: bool,
    pub not_now_until: Option<chrono::DateTime<chrono::Utc>>,
    pub tests: Option<Value>,
    pub review: Option<ExerciseReview>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub source_conversation_id: Option<Uuid>,
    #[serde(default)]
    pub source_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposeExercisesRequest {
    pub node_id: Uuid,
    pub agent: AgentId,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolutionFile {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentVersions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compositor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wayvnc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentController {
    Agent,
    Person,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentState {
    Starting,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningEnvironment {
    pub conversation_id: Uuid,
    pub controller: EnvironmentController,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentAppKind {
    Browser,
    FileManager,
    Editor,
    Office,
    Mail,
    Terminal,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentApp {
    pub id: String,
    pub name: String,
    pub kind: EnvironmentAppKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentWindow {
    pub id: i64,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentCapability {
    pub available: bool,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub versions: EnvironmentVersions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub running: Vec<RunningEnvironment>,
    #[serde(default)]
    pub apps: Vec<EnvironmentApp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub conversation_id: Uuid,
    pub state: EnvironmentState,
    pub controller: EnvironmentController,
    pub node_id: Uuid,
    pub node_name: String,
    pub size: EnvironmentSize,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EnvironmentTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_task: Option<BrowserTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<EnvironmentWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed_with_conversation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTaskState {
    Running,
    Done,
    Blocked,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserTaskStep {
    pub label: String,
    pub millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserTask {
    pub goal: String,
    pub state: BrowserTaskState,
    pub provider: String,
    pub steps: Vec<BrowserTaskStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentTarget {
    pub x: u32,
    pub y: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentAction {
    pub at: chrono::DateTime<chrono::Utc>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartEnvironmentRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaunchEnvironmentAppRequest {
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechProvider {
    pub provider_id: Uuid,
    pub name: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacySettings {
    pub voice_enabled: bool,
    #[serde(default)]
    pub speech_to_text_provider: Option<SpeechProvider>,
    #[serde(default)]
    pub text_to_speech_provider: Option<SpeechProvider>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavePrivacyRequest {
    pub voice_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechRequest {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechVoices {
    pub voices: Vec<String>,
}
