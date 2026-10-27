use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "read_earlier_turns",
            "description": "Read earlier messages of this conversation, including turns from before it moved to this model or account. Search with query, or page back with before.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to search for in the earlier messages."},
                    "before": {"type": "number", "description": "Only messages before this number, as given in the last answer, to page back."},
                    "limit": {"type": "number", "description": "How many messages to return. Defaults to 10, at most 50."},
                },
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let Some((conversation_id, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("There is nothing to read. Earlier turns belong to a conversation, and this call did not come from one.", true));
    };
    let query = arguments.get("query").and_then(Value::as_str).map(str::trim).filter(|query| !query.is_empty());
    let before = arguments.get("before").and_then(Value::as_i64).unwrap_or(i64::MAX);
    let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(10).clamp(1, 50);
    let mut found = sqlx::query_as::<_, (i64, String, String)>(
        "select s.seq, e.kind, s.text from search_entries s join events e on e.conversation_id = s.conversation_id and e.seq = s.seq
         where s.conversation_id = $1 and s.seq < $2 and ($3::text is null or s.tsv @@ websearch_to_tsquery('english', $3))
         order by s.seq desc limit $4",
    )
    .bind(conversation_id)
    .bind(before)
    .bind(query)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;
    let more = found.len() as i64 > limit;

    found.truncate(limit as usize);
    found.reverse();

    let Some(oldest) = found.first().map(|(seq, _, _)| *seq) else {
        return Ok(ToolReply::text("No earlier messages match.", false));
    };
    let mut lines: Vec<String> = found.iter().map(|(seq, kind, text)| format!("#{seq} {}: {}", if kind == "user_message" { "User" } else { "Agent" }, text.chars().take(4000).collect::<String>())).collect();

    if more {
        lines.push(format!("Call again with before={oldest} for older turns."));
    }

    Ok(ToolReply::text(&lines.join("\n\n"), false))
}
