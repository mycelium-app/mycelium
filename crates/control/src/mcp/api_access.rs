use super::call::{ToolCall, ToolReply};
use crate::api_access;
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "request_api_access",
            "description": "Ask the person for temporary access to the Mycelium API, for work such as debugging that needs more than these tools. It never grants anything itself: it waits for the person to approve it in a card in this conversation, where they can untick scopes and see the duration. On approval you are told in a message the path of a file holding a token and the API address; read the token from that file when you need it and never copy it into a message, a file or a commit. It expires and the person can revoke it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scopes": {"type": "array", "items": {"type": "string", "enum": ["conversations:read", "conversations:write", "nodes:read", "settings:read", "assistants"]}, "description": "The access you need. Ask for the least that works."},
                    "hours": {"type": "number", "description": "How long the access lasts, 1 to 24. Defaults to 4."},
                    "integration": {"type": "string", "description": "Ask for API access to one integration account instead of Mycelium's own API: its name or id. Scopes are then read and write. On approval you are told a file holding a token and an address under /api/v1/integrations/{id}/proxy/ that forwards to the integration's API with its credentials added, so you never handle its key."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["scopes", "reason"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let scopes: Vec<String> = arguments.get("scopes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    let reason = arguments.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();

    if reason.is_empty() {
        return Ok(ToolReply::text("Nothing was requested. Give the reason.", true));
    }

    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Nothing was requested. API access belongs to a conversation, and this call did not come from one.", true));
    };
    let hours = arguments.get("hours").and_then(Value::as_i64);

    Ok(match api_access::request(state, conversation_id, &scopes, hours, reason, arguments.get("integration").and_then(Value::as_str)).await? {
        Ok(request) => ToolReply::text(&format!("Waiting for the person to approve API access (request {}).", request.id), false),
        Err(message) => ToolReply::text(&message, true),
    })
}
