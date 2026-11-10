use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use crate::inbox_feed::{self, LONGEST_POST};
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "post_to_feed",
            "description": "Post one short message to the person's Inbox feed. Post once when you release something or finish a feature the person asked for: one sentence on what now works. Do not post plans, progress, commits, test runs or fixes along the way. Anything that needs the person goes to Needs you, not here. It posts at once, and your name, the project and a link back to this chat are added for you. Write badges inside the sentence as [[kind:label]]. The kinds are project (drawn with that project's icon), type (release, feature, fix, docs, security, chore), outcome (live, shipped, passed, failed, blocked, rolled back) and custom (your own label, with an optional icon after a bar, such as [[custom:Nightly|moon]]). Example: [[project:Mycelium]] [[type:release]] v1.2.0 is [[outcome:live]].",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": format!("One sentence on what now works, up to {LONGEST_POST} characters. A badge counts as its label alone.")},
                },
                "required": ["message"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let Some((conversation, _)) = caller.conversation().await? else {
        return Ok(ToolReply::text("Post to the feed from a conversation.", true));
    };
    let owner = caller.owner().await?;

    match inbox_feed::post_from_agent(caller.state, owner, conversation, &arguments).await? {
        Ok(()) => Ok(ToolReply::text("Posted to the feed.", false)),
        Err(message) => Ok(ToolReply::text(&message, true)),
    }
}
