use crate::error::ApiResult;
use crate::state::{AppState, hash_token, new_token};
use uuid::Uuid;

pub const SESSION_TOKEN_PREFIX: &str = "myc_session_";
const IDLE_DAYS: i32 = 30;

pub async fn mint(state: &AppState, node_id: Uuid, conversation_id: Uuid) -> ApiResult<String> {
    let secret = new_token();

    sqlx::query("insert into mcp_session_tokens (token_hash, conversation_id, node_id) values ($1, $2, $3)").bind(hash_token(&secret)).bind(conversation_id).bind(node_id).execute(&state.pool).await?;
    sqlx::query("delete from mcp_session_tokens where conversation_id = $1 and created_at < now() - make_interval(days => $2)").bind(conversation_id).bind(IDLE_DAYS).execute(&state.pool).await?;

    Ok(format!("{SESSION_TOKEN_PREFIX}{secret}"))
}
