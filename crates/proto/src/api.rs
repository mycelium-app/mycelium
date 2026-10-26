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
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectForgeRequest {
    pub kind: ForgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub token: String,
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
    pub name: String,
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
    pub always_allow: bool,
    pub tools: Vec<ChatToolInfo>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartChatToolServerOAuthRequest {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
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
}

fn mcp_kind() -> String {
    "mcp".to_string()
}

impl std::fmt::Debug for CreateChatToolServerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CreateChatToolServerRequest").field("name", &self.name).field("kind", &self.kind).field("url", &self.url).field("account", &self.account).field("bearer_token", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PatchChatToolServerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_allow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateChatRequest {
    pub provider_id: Uuid,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub folders: Vec<GroupFolder>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRequest {
    pub name: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub node_id: Uuid,
    pub agent: AgentId,
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
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateScheduleRequest {
    pub node_id: Uuid,
    pub agent: AgentId,
    #[serde(default)]
    pub cwd: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchGoalRequest {
    pub paused: bool,
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
pub struct Artifact {
    pub id: Uuid,
    pub owner: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    #[serde(default)]
    pub conversation_ids: Vec<Uuid>,
    pub kind: ArtifactKind,
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
