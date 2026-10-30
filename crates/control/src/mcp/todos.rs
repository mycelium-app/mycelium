use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use mycelium_proto::conversation::EventKind;
use serde_json::{Value, json};

const STATUSES: [&str; 3] = ["pending", "in_progress", "completed"];

pub fn definitions() -> Value {
    json!([
        {
            "name": "set_todos",
            "description": "Set this conversation's to-do list, shown to the person above the message box and handed to whoever continues the conversation. Send the whole list every time: it replaces the previous one. Keep one item in_progress while you work and mark items completed as you finish them.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "Every item on the list, in order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {"type": "string", "description": "What to do, in a short phrase."},
                                "status": {"type": "string", "enum": STATUSES, "description": "pending, in_progress or completed."},
                            },
                            "required": ["content", "status"],
                        },
                    },
                },
                "required": ["todos"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let Some(todos) = arguments.get("todos").and_then(Value::as_array) else {
        return Ok(ToolReply::invalid("set_todos needs todos, the whole list as an array of content and status."));
    };

    let mut entries = Vec::new();

    for todo in todos {
        let content = todo.get("content").and_then(Value::as_str).map(str::trim).filter(|content| !content.is_empty());
        let status = todo.get("status").and_then(Value::as_str).filter(|status| STATUSES.contains(status));

        let (Some(content), Some(status)) = (content, status) else {
            return Ok(ToolReply::invalid("Every item in todos needs content and a status of pending, in_progress or completed."));
        };

        entries.push(json!({"content": content, "status": status}));
    }

    let Some((conversation, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Set the to-do list from a conversation.", true));
    };
    let owner = caller.owner().await?;

    crate::chats::record(caller.state, owner, conversation, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "plan", "entries": entries}})).await?;

    Ok(ToolReply::text(&format!("To-do list set with {} items.", entries.len()), false))
}
