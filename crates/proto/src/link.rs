use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::conversation::{AgentId, AgentInfo, LoginInfo, SessionPosition};

pub const REPLACED_CLOSE_CODE: u16 = 4000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeToControl {
    Hello {
        version: String,
        hostname: String,
        os: String,
        arch: String,
        agents: Vec<AgentInfo>,
        sessions: Vec<SessionPosition>,
    },
    SessionStarted {
        conversation_id: Uuid,
        acp_session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modes: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        models: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config_options: Option<Value>,
    },
    PromptReceived {
        conversation_id: Uuid,
        prompt_id: Uuid,
    },
    AcpUpdate {
        conversation_id: Uuid,
        seq: i64,
        update: Value,
    },
    PermissionRequest {
        conversation_id: Uuid,
        seq: i64,
        request_id: String,
        request: Value,
    },
    TurnEnd {
        conversation_id: Uuid,
        seq: i64,
        stop_reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
    },
    SessionFailed {
        conversation_id: Uuid,
        seq: i64,
        message: String,
    },
    Logins {
        logins: Vec<LoginInfo>,
    },
    LoginOutput {
        login_id: Uuid,
        data: String,
    },
    LoginFinished {
        login_id: Uuid,
        ok: bool,
    },
    TerminalOutput {
        terminal_id: Uuid,
        data: String,
    },
    TerminalExited {
        terminal_id: Uuid,
    },
    Transcript {
        path: String,
        offset: i64,
        data: String,
    },
    Workflow {
        conversation_id: Uuid,
        run_id: String,
        line: Value,
    },
    Files {
        request_id: Uuid,
        paths: Vec<String>,
    },
    GitResult {
        request_id: Uuid,
        ok: bool,
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    InstructionsHeld {
        revision: String,
        unmanaged: Vec<String>,
    },
    Resources {
        sample: ResourceSample,
    },
    Pong,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskUsage {
    pub mount: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessUsage {
    pub pid: u32,
    pub name: String,
    pub command: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListeningPort {
    pub port: u16,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSample {
    pub at_ms: i64,
    pub available: bool,
    pub cpu_percent: f32,
    pub cores: u32,
    pub load: [f32; 3],
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub disks: Vec<DiskUsage>,
    pub processes: Vec<ProcessUsage>,
    pub ports: Vec<ListeningPort>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct GitCredentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for GitCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("GitCredentials").field("username", &self.username).field("password", &"<hidden>").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitCommand {
    Status,
    Log { limit: u32 },
    Diff { path: String, staged: bool },
    Stage { paths: Vec<String> },
    Unstage { paths: Vec<String> },
    StageAll,
    Commit { message: String },
    Fetch,
    UndoLastCommit,
    Clone {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials: Option<GitCredentials>,
    },
    Register,
    AddWorktree {
        branch: String,
    },
    MovePlan,
    Snapshot {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Apply {
        branch: String,
        head: String,
        patch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bundle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials: Option<GitCredentials>,
    },
    Undo {
        branch: String,
        previous: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch_before: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlToNode {
    Ack {
        conversation_id: Uuid,
        seq: i64,
    },
    StartSession {
        conversation_id: Uuid,
        agent: AgentId,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fork: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    Instructions {
        revision: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        global: Option<String>,
    },
    Prompt {
        conversation_id: Uuid,
        prompt_id: Uuid,
        text: String,
    },
    Cancel {
        conversation_id: Uuid,
    },
    PermissionResponse {
        conversation_id: Uuid,
        request_id: String,
        outcome: Value,
    },
    SetConfig {
        conversation_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<std::collections::BTreeMap<String, Value>>,
    },
    CloseSession {
        conversation_id: Uuid,
    },
    StartLogin {
        login_id: Uuid,
        agent: AgentId,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
    LoginInput {
        login_id: Uuid,
        text: String,
    },
    StopLogin {
        login_id: Uuid,
    },
    StartTerminal {
        terminal_id: Uuid,
        conversation_id: Uuid,
        cwd: String,
        cols: u16,
        rows: u16,
    },
    TerminalInput {
        terminal_id: Uuid,
        data: String,
    },
    ResizeTerminal {
        terminal_id: Uuid,
        cols: u16,
        rows: u16,
    },
    StopTerminal {
        terminal_id: Uuid,
    },
    TranscriptAck {
        path: String,
        bytes: i64,
    },
    RestoreTranscript {
        path: String,
        offset: i64,
        data: String,
    },
    ContinueSequence {
        conversation_id: Uuid,
        after: i64,
    },
    ListFiles {
        request_id: Uuid,
        cwd: String,
        query: String,
    },
    Git {
        request_id: Uuid,
        cwd: String,
        command: GitCommand,
    },
    Watch {
        seconds: u32,
    },
    Ping,
    Revoked,
}
