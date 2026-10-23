use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{Goal, ImportJob, InstructionProposal, Memory, MoveProgress, NodeSummary, Schedule, Walkthrough};
use crate::link::ResourceSample;
use crate::conversation::{Conversation, DeviceKind, DeviceState, Event, QueuedPrompt};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    Conversation(Conversation),
    Event(Event),
    ConversationRemoved {
        conversation_id: Uuid,
    },
    Queue {
        conversation_id: Uuid,
        queue: Vec<QueuedPrompt>,
    },
    Move(MoveProgress),
    ImportJob(ImportJob),
    Goal(Goal),
    GoalCleared {
        conversation_id: Uuid,
    },
    Walkthrough(Walkthrough),
    Schedule(Schedule),
    ScheduleRemoved {
        schedule_id: Uuid,
    },
    InstructionProposal(InstructionProposal),
    Memory(Memory),
    MemoryRemoved {
        memory_id: Uuid,
    },
    Node(NodeSummary),
    NodeRemoved {
        node_id: Uuid,
    },
    NodeResources {
        node_id: Uuid,
        sample: ResourceSample,
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
    Device {
        user_code: String,
        kind: DeviceKind,
        label: String,
        hostname: String,
        state: DeviceState,
        result_id: Option<Uuid>,
    },
}
