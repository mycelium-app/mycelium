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
    Pong,
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
    ListFiles {
        request_id: Uuid,
        cwd: String,
        query: String,
    },
    Ping,
    Revoked,
}
