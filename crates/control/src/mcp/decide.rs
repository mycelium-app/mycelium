use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "decide",
            "description": "Ask a decisions server (OpenJev or Jev-compatible) typed questions about some state. The answer is read from the model's token probabilities, so it cannot go off-schema. Use it for a yes or no, a choice among named options, or a score, instead of reasoning it out in text. Returns, per question, the probability of yes for noul, or the choice or score with its probabilities and confidence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "state": {"description": "What the questions are about: text, or an object."},
                    "questions": {
                        "type": "object",
                        "description": "A name for each question mapped to its definition.",
                        "additionalProperties": {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["noul", "choice", "score"], "description": "noul is a yes or no question."},
                                "instructions": {"type": "string", "description": "The question to answer."},
                                "criteria": {"description": "For choice an object mapping each option to what it means; for score a list of level descriptions; for noul optionally {true, false}."},
                            },
                            "required": ["type", "instructions"],
                        },
                    },
                },
                "required": ["state", "questions"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let state = caller.state;

    let owner = caller.owner().await?;
    let outcome = crate::decision_providers::decide(state, owner, "decide", arguments.get("state").unwrap_or(&Value::Null), arguments.get("questions").unwrap_or(&Value::Null)).await?;

    Ok(match outcome {
        Ok(answers) => ToolReply::text(&answers.to_string(), false),
        Err(message) => ToolReply::text(&message, true),
    })
}
