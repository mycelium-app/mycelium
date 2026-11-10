use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use mycelium_proto::conversation::EventKind;
use crate::todos::STATUSES;
use serde_json::{Value, json};

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
                                "description": {"type": "string", "description": "Optional details the person sees when they open the item, in a sentence or two."},
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
        let content = todo.get("content").and_then(Value::as_str).unwrap_or_default();
        let status = todo.get("status").and_then(Value::as_str).unwrap_or_default();
        let Some(entry) = crate::todos::entry(content, status, todo.get("description").and_then(Value::as_str)) else {
            return Ok(ToolReply::invalid("Every item in todos needs content and a status of pending, in_progress or completed."));
        };

        entries.push(entry);
    }

    let Some((conversation, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Set the to-do list from a conversation.", true));
    };
    let owner = caller.owner().await?;

    let mut conn = caller.state.pool.acquire().await?;
    let (entries, kept) = crate::todos::from_agent(&mut conn, conversation, entries).await?;
    drop(conn);

    crate::chats::record(caller.state, owner, conversation, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "plan", "entries": entries}})).await?;

    let set = format!("To-do list set with {} items.", entries.len());
    let reply = match kept.is_empty() {
        true => set,
        false => format!("{set} The person changed the list since you last set it, and their changes are kept in it:\n{}", kept.join("\n")),
    };

    Ok(ToolReply::text(&reply, false))
}
