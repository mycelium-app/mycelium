use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{ApiAccessRequest, ArtifactAccessRequest, Artifact, ArtifactComment, Goal, ImportJob, InstructionProposal, Memory, MessageProposal, MoveProgress, NodeSummary, Schedule, SshAccessRequest, Walkthrough, WalkthroughNote};
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
    WalkthroughNotes {
        conversation_id: Uuid,
        walkthrough_id: Uuid,
        notes: Vec<WalkthroughNote>,
    },
    Workflow { workflow: serde_json::Value },
    Schedule(Schedule),
    ScheduleRemoved {
        schedule_id: Uuid,
    },
    InstructionProposal(InstructionProposal),
    MessageProposal(MessageProposal),
    ConversationConnection(crate::api::ConversationConnection),
    ApiAccessRequest(ApiAccessRequest),
    SshAccessRequest(SshAccessRequest),
    Memory(Memory),
    MemoryRemoved {
        memory_id: Uuid,
    },
    Artifact(Artifact),
    ArtifactAccessRequest(ArtifactAccessRequest),
    ArtifactComment(ArtifactComment),
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
