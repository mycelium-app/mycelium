use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::ssh_access;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "request_ssh_access",
            "description": "Ask the person for temporary SSH access to one of their nodes, for work such as debugging a machine this conversation does not run on. It never grants anything itself: it waits for the person to approve it in a card in this conversation, where they can shorten the duration. On approval you are told in a message the exact ssh command to run. The connection goes through Mycelium, so the node needs no open port. Never copy the token file it names into a message, a file or a commit. Access expires, and the person can revoke it, which ends open sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": {"type": "string", "description": "The node to reach: its name or id."},
                    "minutes": {"type": "number", "description": "How long the access lasts, 5 to 240. Defaults to 30."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["node", "reason"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let node_name = arguments.get("node").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let reason = arguments.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();

    if reason.is_empty() {
        return Ok(ToolReply::text("Nothing was requested. Give the reason.", true));
    }

    if node_name.is_empty() {
        return Ok(ToolReply::text("Nothing was requested. Give the node.", true));
    }

    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Nothing was requested. SSH access belongs to a conversation, and this call did not come from one.", true));
    };
    let minutes = arguments.get("minutes").and_then(Value::as_i64);

    Ok(match ssh_access::request(state, conversation_id, node_name, minutes, reason).await? {
        Ok(request) => ToolReply::text(&format!("Waiting for the person to approve SSH access to {} (request {}).", request.node_name, request.id), false),
        Err(message) => ToolReply::text(&message, true),
    })
}
