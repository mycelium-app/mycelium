use super::call::Caller;
use crate::chat_tools::{self, Approval};
use crate::error::ApiResult;
use serde_json::Value;
use uuid::Uuid;

pub async fn approved(caller: &Caller<'_>, tool: &str, input: &Value) -> ApiResult<Result<(), String>> {
    let Some(conversation_id) = caller.conversation_id().await? else {
        return Ok(Err("Nothing was done. This change needs the person's approval, and approvals are asked in a conversation. Call it from inside one.".to_string()));
    };
    let owner = caller.owner().await?;
    let approval = Approval { title: tool, input, server: "Mycelium", account: None, filter: None, edits_by_setting: false, once_only: true };
    let answered = chat_tools::request_approval(caller.state, owner, conversation_id, &Uuid::new_v4().to_string(), &approval).await?;

    if chat_tools::is_approved(&answered) {
        Ok(Ok(()))
    } else {
        Ok(Err(format!("Nothing was done. {}", chat_tools::DECLINED_TEXT)))
    }
}
