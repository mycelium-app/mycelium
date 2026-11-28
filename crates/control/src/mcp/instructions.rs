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
            "description": "Propose an edit to the person's agent instructions (AGENTS.md) for them to approve. Use it when the person corrects how you work and the correction is a lasting rule, not a one-off: quote their words in reason. Never edit instruction files directly. Read the layer with get_instructions first, then send the revision you read with either the complete new text of that layer, or edits: each the exact old text and its replacement, applied like a file edit. It waits in Settings, Instructions until the person approves or declines it, and a decline arrives in this conversation as a message. Layer defaults to this conversation's group, or global without one; pass layer global for a rule that applies everywhere.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The complete new text of the layer, with your edit applied to the text you read. Give this or edits."},
                    "edits": {
                        "type": "array",
                        "description": "Small edits instead of the whole text, applied in order to the text at base_revision. Each old_text must appear exactly once.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_text": {"type": "string", "description": "The exact text to replace, with enough around it to appear only once."},
                                "new_text": {"type": "string", "description": "What replaces it."},
                            },
                            "required": ["old_text", "new_text"],
                        },
                    },
                    "base_revision": {"type": "string", "description": "The revision get_instructions returned for the text you edited. A proposal on an older revision is refused."},
                    "reason": {"type": "string", "description": "One paragraph saying why, quoting the person's own words."},
                    "layer": {"type": "string", "description": "global, or the name of a group. Defaults to this conversation's group, or global."},
                },
                "required": ["base_revision", "reason"],
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
            let (Some(base_revision), Some(reason)) = (field("base_revision"), field("reason").map(str::trim)) else {
                return Ok(ToolReply::invalid("propose_instructions_edit needs base_revision and reason, strings, and text or edits."));
            };
            let edited;
            let text = match (field("text"), arguments.get("edits").and_then(Value::as_array)) {
                (Some(text), None) => text,
                (None, Some(edits)) => {
                    let Some((conversation_id, _)) = caller.conversation().await? else {
                        return Ok(ToolReply::text("Nothing was proposed. A proposal belongs to a conversation, and this call did not come from one.", true));
                    };
                    let layer = match instruction_proposals::layer_text(state, conversation_id, field("layer")).await? {
                        Ok(layer) => layer,
                        Err(message) => return Ok(ToolReply::text(&message, true)),
                    };

                    if base_revision.trim() != layer.revision {
                        return Ok(ToolReply::text(&format!("Nothing was proposed. The {} instructions are at revision {}, not {}. Read the current text with get_instructions and make your edits on it.", layer.name, layer.revision, base_revision.trim()), true));
                    }

                    let pairs: Option<Vec<(&str, &str)>> = edits.iter().map(|edit| Some((edit.get("old_text")?.as_str()?, edit.get("new_text")?.as_str()?))).collect();
                    let Some(pairs) = pairs.filter(|pairs| !pairs.is_empty()) else {
                        return Ok(ToolReply::invalid("Each edit needs old_text and new_text, strings."));
                    };

                    edited = match apply_edits(&layer.text, &pairs) {
                        Ok(text) => text,
                        Err(message) => return Ok(ToolReply::text(&format!("Nothing was proposed. {message}"), true)),
                    };
                    edited.as_str()
                }
                _ => return Ok(ToolReply::invalid("propose_instructions_edit needs either text, the whole new layer, or edits, not both.")),
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

fn apply_edits(text: &str, edits: &[(&str, &str)]) -> Result<String, String> {
    let mut edited = text.to_string();

    for (number, (old, new)) in edits.iter().enumerate() {
        let found = if old.is_empty() { 0 } else { edited.matches(old).count() };

        match found {
            0 => return Err(format!("Edit {} was not applied: its old_text is not in the text at that revision. Copy it exactly from get_instructions.", number + 1)),
            1 => edited = edited.replacen(old, new, 1),
            many => return Err(format!("Edit {} was not applied: its old_text appears {many} times. Include more of the text around it so it appears once.", number + 1)),
        }
    }

    Ok(edited)
}

#[cfg(test)]
mod tests {
    use super::apply_edits;

    #[test]
    fn edits_apply_in_order_and_a_missing_or_repeated_old_text_is_refused() {
        assert_eq!(apply_edits("# Rules\nBe brief.\n", &[("Be brief.", "Be brief.\nNever commit unasked.")]).unwrap(), "# Rules\nBe brief.\nNever commit unasked.\n");
        assert!(apply_edits("# Rules\nBe brief.", &[("Be kind.", "x")]).unwrap_err().contains("is not in the text"));
        assert!(apply_edits("- a\n- a", &[("- a", "- b")]).unwrap_err().contains("appears 2 times"));
    }
}
