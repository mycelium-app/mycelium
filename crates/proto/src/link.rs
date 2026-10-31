use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::api::{EnvironmentCapability, KubernetesNode, EnvironmentState, EnvironmentWindow};
use crate::conversation::{AgentId, AgentInfo, LoginInfo, SessionPosition};

pub const REPLACED_CLOSE_CODE: u16 = 4000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeTerminal {
    pub terminal_id: Uuid,
    pub conversation_id: Uuid,
    pub total: u64,
    pub cols: u16,
    pub rows: u16,
}

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kubernetes: Option<KubernetesNode>,
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
    SessionClosed {
        conversation_id: Uuid,
    },
    SessionFailed {
        conversation_id: Uuid,
        seq: i64,
        message: String,
    },
    Logins {
        logins: Vec<LoginInfo>,
    },
    NodeStatus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kubernetes: Option<KubernetesNode>,
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
    TerminalList {
        terminals: Vec<NodeTerminal>,
    },
    TerminalBuffer {
        request_id: Uuid,
        found: bool,
        data: String,
        offset: u64,
        total: u64,
    },
    EnvironmentCapability {
        capability: EnvironmentCapability,
    },
    EnvironmentChanged {
        conversation_id: Uuid,
        state: EnvironmentState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        browser: Option<String>,
    },
    EnvironmentWindows {
        conversation_id: Uuid,
        windows: Vec<EnvironmentWindow>,
    },
    EnvironmentLaunched {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    EnvironmentInputDone {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    EnvironmentScreenshotTaken {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        png: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    EnvironmentBrowserControls {
        request_id: Uuid,
        #[serde(default)]
        page: String,
        #[serde(default)]
        controls: Vec<BrowserControl>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    EnvironmentBrowserActed {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    EnvironmentStreamData {
        stream_id: Uuid,
        data: String,
    },
    EnvironmentStreamClosed {
        stream_id: Uuid,
    },
    Transcript {
        path: String,
        offset: i64,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<i64>,
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
    FolderFiles {
        request_id: Uuid,
        files: Vec<FolderFile>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    GitResult {
        request_id: Uuid,
        ok: bool,
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    GitChunk {
        request_id: Uuid,
        seq: u32,
        data: String,
    },
    Steered {
        request_id: Uuid,
        outcome: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    InstructionsHeld {
        revision: String,
        unmanaged: Vec<String>,
    },
    ApiTokenSaved {
        conversation_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    SshTokenSaved {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    SshData {
        session_id: Uuid,
        data: String,
    },
    SshClosed {
        session_id: Uuid,
    },
    Resources {
        sample: ResourceSample,
    },
    ProcessStopped {
        request_id: Uuid,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ResetUsed {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    UpdateResult {
        ok: bool,
        message: String,
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stoppable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockerContainer {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockerUsage {
    pub running: u32,
    pub containers: Vec<DockerContainer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reclaimable_bytes: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerUsage>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitOptions {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub amend: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signoff: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowRange {
    pub path: String,
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitCommand {
    Status,
    Log { limit: u32 },
    Graph { limit: u32, skip: u32 },
    Show { hash: String },
    Branches,
    Stack,
    Switch {
        branch: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        create: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        track: Option<String>,
    },
    Diff { path: String, staged: bool },
    ScopeDiff { scope: String },
    RunTests {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        timeout_secs: u32,
    },
    ChangesLayout,
    ChangesRows { ranges: Vec<RowRange> },
    Stage { paths: Vec<String> },
    Unstage { paths: Vec<String> },
    StageAll,
    Hunk { path: String, row: u32, fingerprint: String, action: String },
    File { path: String, content: Option<String>, expected: Option<String> },
    Remote { action: String, force: bool, url: String, allowed_urls: Vec<String>, credentials: GitCredentials },
    Commit {
        message: String,
        #[serde(default, flatten)]
        options: CommitOptions,
    },
    Fetch,
    Pull {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rebase: bool,
    },
    Push {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        force: bool,
    },
    UndoLastCommit,
    TownSync,
    TownPropose,
    Clone {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials: Option<GitCredentials>,
    },
    Register,
    Head,
    Directories,
    AddWorktree {
        branch: String,
    },
    AddRemoteWorktree {
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        approved_urls: Vec<String>,
    },
    Undo {
        branch: String,
        previous: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch_before: Option<String>,
    },
    ReadFile {
        path: String,
    },
    FindLogo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderFile {
    pub path: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptAttachment {
    pub name: String,
    pub media_type: String,
    pub data: String,
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reload: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        mcp_token: String,
    },
    SecretsChanged {
        conversation_id: Uuid,
        #[serde(default)]
        env: Vec<(String, String)>,
    },
    Instructions {
        revision: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        global: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        take_over: bool,
    },
    Prompt {
        conversation_id: Uuid,
        prompt_id: Uuid,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<PromptAttachment>,
    },
    Steer {
        conversation_id: Uuid,
        request_id: Uuid,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<PromptAttachment>,
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
    RemoveLogin {
        agent: AgentId,
        account: String,
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
    ReadTerminal {
        request_id: Uuid,
        terminal_id: Uuid,
        from: u64,
    },
    StartEnvironment {
        conversation_id: Uuid,
        width: u32,
        height: u32,
        #[serde(default)]
        apps: Vec<String>,
    },
    EnvironmentLaunch {
        request_id: Uuid,
        conversation_id: Uuid,
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    EnvironmentFocus {
        request_id: Uuid,
        conversation_id: Uuid,
        window: i64,
    },
    StopEnvironment {
        conversation_id: Uuid,
    },
    CheckEnvironment,
    EnvironmentInput {
        request_id: Uuid,
        conversation_id: Uuid,
        action: EnvironmentInputAction,
    },
    EnvironmentScreenshot {
        request_id: Uuid,
        conversation_id: Uuid,
    },
    EnvironmentBrowserRead {
        request_id: Uuid,
        conversation_id: Uuid,
    },
    EnvironmentBrowserAct {
        request_id: Uuid,
        conversation_id: Uuid,
        action: BrowserAction,
    },
    OpenEnvironmentStream {
        stream_id: Uuid,
        conversation_id: Uuid,
    },
    EnvironmentStreamInput {
        stream_id: Uuid,
        data: String,
    },
    CloseEnvironmentStream {
        stream_id: Uuid,
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
    WriteFolder {
        root: String,
        files: Vec<FolderFile>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        replace: bool,
    },
    ReadFolder {
        request_id: Uuid,
        root: String,
    },
    DeleteFolder {
        root: String,
    },
    Git {
        request_id: Uuid,
        cwd: String,
        command: GitCommand,
    },
    Watch {
        seconds: u32,
    },
    StopProcess {
        request_id: Uuid,
        pid: u32,
        name: String,
    },
    UseReset {
        request_id: Uuid,
        agent: AgentId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        idempotency_key: String,
    },
    ApiToken {
        conversation_id: Uuid,
        token: String,
    },
    SshToken {
        request_id: Uuid,
        conversation_id: Uuid,
        token: String,
    },
    SshOpen {
        session_id: Uuid,
    },
    SshData {
        session_id: Uuid,
        data: String,
    },
    SshClose {
        session_id: Uuid,
    },
    Update {
        releases_url: String,
        version: String,
    },
    Ping,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserControl {
    pub index: u32,
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserAction {
    Click {
        index: u32,
    },
    Type {
        index: u32,
        text: String,
    },
    Select {
        index: u32,
        text: String,
    },
    Scroll {
        direction: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvironmentInputAction {
    Click {
        x: u32,
        y: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button: Option<String>,
    },
    Type {
        text: String,
    },
    Scroll {
        direction: String,
        amount: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<u32>,
    },
    Key {
        keys: String,
    },
}
