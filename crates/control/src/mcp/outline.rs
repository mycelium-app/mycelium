use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "open_outline",
            "description": "Show an Outline document beside this conversation, in Outline's own page, so the person can read it as you work. It only opens a page and needs no approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document": {"type": "string", "description": "The document's id, urlId or address."},
                    "integration": {"type": "string", "description": "The Outline integration's name or id. Needed only when this conversation has more than one."},
                },
                "required": ["document"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let document = arguments.get("document").and_then(Value::as_str).map(str::trim).filter(|document| !document.is_empty());
    let Some(document) = document else {
        return Ok(ToolReply::invalid("open_outline needs the document's id, urlId or address."));
    };
    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Open a page from a conversation.", true));
    };
    let owner = caller.owner().await?;
    let group = caller.group_id().await?;
    let integration = arguments.get("integration").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());

    Ok(match crate::open_outline::open(state, owner, conversation_id, group, document, integration).await? {
        Ok(text) => ToolReply::text(&text, false),
        Err(message) => ToolReply::text(&message, true),
    })
}
