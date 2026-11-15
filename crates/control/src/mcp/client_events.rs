use super::call::{ToolCall, ToolReply};
use crate::client_events::{Filter, KINDS, read};
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "read_client_events",
            "description": "Read what the person's Mycelium apps reported about themselves: uncaught errors and rejected promises with their stack, the notices the web page showed, lost WebGL graphics, and connection events such as disconnects with their close code, reconnect attempts and the page being hidden or shown. Each event names its client (web, desktop or android), version, user agent and session. Newest first, kept for 14 days. It never holds message content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {"type": "string", "description": "Only events at or after this RFC 3339 time, such as 2026-11-15T09:00:00Z."},
                    "until": {"type": "string", "description": "Only events before this RFC 3339 time."},
                    "client": {"type": "string", "enum": ["web", "desktop", "android"], "description": "Only events from this client."},
                    "kind": {"type": "string", "enum": KINDS, "description": "Only events of this kind."},
                    "limit": {"type": "number", "description": "How many events to return. Defaults to 100, at most 500."},
                },
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let owner = caller.owner().await?;
    let mut window = [None, None];

    for (slot, name) in window.iter_mut().zip(["since", "until"]) {
        if let Some(text) = arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()) {
            match chrono::DateTime::parse_from_rfc3339(text) {
                Ok(time) => *slot = Some(time.with_timezone(&chrono::Utc)),
                Err(_) => return Ok(ToolReply::text(&format!("Nothing was read. {name} is a time like 2026-11-15T09:00:00Z."), true)),
            }
        }
    }

    let text = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    let [since, until] = window;
    let filter = Filter { since, until, client: text("client"), kind: text("kind"), limit: arguments.get("limit").and_then(Value::as_i64) };
    let events = read(caller.state, owner, &filter).await?;

    Ok(ToolReply::text(&events.to_string(), false))
}
