use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::instruction_proposals;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "get_instructions",
            "description": "Read the current text and revision of one layer of the person's agent instructions (the AGENTS.md files). Call it before propose_instructions_edit: the proposal must be made on that revision. Without a layer it reads this conversation's group layer, or the global layer when the conversation has no group.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "layer": {"type": "string", "description": "global, or the name of a group. Defaults to this conversation's group, or global."},
                },
            },
        },
        {
            "name": "propose_instructions_edit",
            "description": "Propose an edit to the person's agent instructions (AGENTS.md) for them to approve. Use it when the person corrects how you work and the correction is a lasting rule, not a one-off: quote their words in reason. Never edit instruction files directly. Read the layer with get_instructions first, then send the complete new text of that layer and the revision you read. It waits in Settings, Instructions until the person approves or declines it, and a decline arrives in this conversation as a message. Layer defaults to this conversation's group, or global without one; pass layer global for a rule that applies everywhere.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The complete new text of the layer, with your edit applied to the text you read."},
                    "base_revision": {"type": "string", "description": "The revision get_instructions returned for the text you edited. A proposal on an older revision is refused."},
                    "reason": {"type": "string", "description": "One paragraph saying why, quoting the person's own words."},
                    "layer": {"type": "string", "description": "global, or the name of a group. Defaults to this conversation's group, or global."},
                },
                "required": ["text", "base_revision", "reason"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    Ok(match tool {
        "get_instructions" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                let latest = sqlx::query_as::<_, (String, String)>("select id, text from instruction_versions where layer = 'global' order by seq desc limit 1").fetch_optional(&state.pool).await?;
                let (revision, text) = latest.unwrap_or_else(|| (instruction_proposals::NO_REVISION.to_string(), String::new()));

                return Ok(ToolReply::text(&format!("Layer: Global\nRevision: {revision}\n\n{text}"), false));
            };

            match instruction_proposals::layer_text(state, conversation_id, arguments.get("layer").and_then(Value::as_str)).await? {
                Ok(layer) => ToolReply::text(&format!("Layer: {}\nRevision: {}\n\n{}", layer.name, layer.revision, layer.text), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        _ => {
            let field = |name: &str| arguments.get(name).and_then(Value::as_str);
            let (Some(text), Some(base_revision), Some(reason)) = (field("text"), field("base_revision"), field("reason").map(str::trim)) else {
                return Ok(ToolReply::invalid("propose_instructions_edit needs text, base_revision and reason, strings."));
            };

            if text.trim().is_empty() {
                return Ok(ToolReply::text("Nothing was proposed. Give the complete new text of the layer.", true));
            }

            if reason.is_empty() {
                return Ok(ToolReply::text("Nothing was proposed. Give the reason, quoting the person's own words.", true));
            }

            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was proposed. A proposal belongs to a conversation, and this call did not come from one.", true));
            };

            match instruction_proposals::propose(state, conversation_id, field("layer"), base_revision, text, reason).await? {
                Ok(proposal) => ToolReply::text(&format!("Proposal {} made for the {} instructions. It waits for the person to approve it in Settings, Instructions. If they decline it, the reason arrives here as a message.", proposal.id, proposal.layer_name), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
    })
}
