use super::call::{ToolCall, ToolReply};
use crate::error::{ApiError, ApiResult};
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "get_goal",
            "description": "Read this conversation's Mycelium goal, active/completed turns, reported token consumption, token/time limits and account usage windows. tokens_known=false means the agent has not reported usage; it does not mean zero consumption.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "report_goal",
            "description": "Report the outcome of this conversation's Mycelium goal and end the loop. Call it with outcome done when the goal is met, or blocked when you cannot go on without the person, and say why. Mycelium nudges you after every turn that ends without a report, so call this instead of stopping silently. Stopping a turn never needs a report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "outcome": {"type": "string", "enum": ["done", "blocked"], "description": "done: the goal is met. blocked: you cannot go on without the person."},
                    "reason": {"type": "string", "description": "Why the goal is done, or what you need from the person to go on."},
                },
                "required": ["outcome", "reason"],
            },
        },
        {
            "name": "get_account_usage",
            "description": "Read the signed-in agent account's reported usage windows and when they were read. This is account quota usage, separate from goal token consumption. Missing windows mean the node has not reported account usage.",
            "inputSchema": {"type": "object", "properties": {}},
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let Some((conversation, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Read goal and account usage from a conversation.", true));
    };
    let owner = caller.owner().await?;

    if tool == "report_goal" {
        let outcome = arguments.get("outcome").and_then(Value::as_str).map(str::trim);
        let reason = arguments.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();

        if !matches!(outcome, Some("done" | "blocked")) {
            return Ok(ToolReply::invalid("report_goal needs outcome, done or blocked."));
        }

        if reason.is_empty() {
            return Ok(ToolReply::text("Nothing was reported. Say why the goal is done, or what you need from the person to go on.", true));
        }

        let reported = crate::goals::report(state, owner, conversation, outcome.unwrap_or_default(), reason).await?;

        return Ok(if reported {
            ToolReply::text("Reported. Mycelium will not nudge you again. End your turn.", false)
        } else {
            ToolReply::text("Nothing was reported. This conversation has no active goal.", true)
        });
    }

    let value = if tool == "get_goal" {
        let goal = crate::goals::load(state, owner, conversation).await?;

        serde_json::to_value(goal).map_err(ApiError::internal)?
    } else {
        let row = crate::store::find_owned(&state.pool, conversation, owner).await?;
        let reading = sqlx::query_as::<_, (Value, Option<chrono::DateTime<chrono::Utc>>)>("select usage_windows, usage_read_at from logins where node_id = $1 and agent = $2 and ($3::text is null or coalesce(account,'') = $3) order by jsonb_array_length(usage_windows) > 0 desc, usage_read_at desc nulls last limit 1").bind(row.node_id).bind(row.agent).bind(row.account).fetch_optional(&state.pool).await?;

        match reading {
            Some((windows, read_at)) => json!({"windows":windows,"read_at":read_at}),
            None => json!({"windows":[],"read_at":null}),
        }
    };

    Ok(ToolReply::text(&value.to_string(), false))
}
