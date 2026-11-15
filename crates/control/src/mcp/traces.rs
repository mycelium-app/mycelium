use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::traces::{Filter, read};
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "read_traces",
            "description": "Read how long the control plane took to answer the person's requests, to find where time goes: each trace is one HTTP request or tool call with its route or tool, status and duration, and for traces over 200 ms or that failed, the database queries (statement, rows, time), node round-trips and outside calls (forge, identity provider, integrations) inside it. Newest first, kept for 48 hours. It never holds message content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {"type": "string", "description": "Only traces started at or after this RFC 3339 time, such as 2026-11-15T09:00:00Z."},
                    "name": {"type": "string", "description": "Only traces whose route or tool contains this text, such as /api/v1/conversations or send_to_conversation."},
                    "min_duration_ms": {"type": "number", "description": "Only traces that took at least this many milliseconds."},
                    "limit": {"type": "number", "description": "How many traces to return. Defaults to 20, at most 100."},
                },
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let owner = caller.owner().await?;
    let since = match arguments.get("since").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => match chrono::DateTime::parse_from_rfc3339(text) {
            Ok(time) => Some(time.with_timezone(&chrono::Utc)),
            Err(_) => return Ok(ToolReply::text("Nothing was read. since is a time like 2026-11-15T09:00:00Z.", true)),
        },
        None => None,
    };
    let filter = Filter {
        since,
        name: arguments.get("name").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string),
        min_duration_ms: arguments.get("min_duration_ms").and_then(Value::as_f64),
        limit: arguments.get("limit").and_then(Value::as_i64),
    };
    let traces = read(caller.state, owner, &filter).await?;

    Ok(ToolReply::text(&traces.to_string(), false))
}
