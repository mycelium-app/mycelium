use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::NodeSummary;
use crate::conversation::{Conversation, Event, QueuedPrompt};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    Conversation(Conversation),
    Event(Event),
    Queue {
        conversation_id: Uuid,
        queue: Vec<QueuedPrompt>,
    },
    Node(NodeSummary),
    NodeRemoved {
        node_id: Uuid,
    },
    LoginOutput {
        login_id: Uuid,
        data: String,
    },
    LoginFinished {
        login_id: Uuid,
        ok: bool,
    },
}
