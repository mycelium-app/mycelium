use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, hash_token, new_token};
use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub const SESSION_TOKEN_PREFIX: &str = "myc_session_";
const IDLE_DAYS: i32 = 30;

#[derive(Debug, Deserialize)]
pub struct RevokeRequest {
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<Uuid>,
}

pub async fn mint(state: &AppState, node_id: Uuid, conversation_id: Uuid) -> ApiResult<String> {
    let secret = new_token();

    sqlx::query("insert into mcp_session_tokens (token_hash, conversation_id, node_id) values ($1, $2, $3)").bind(hash_token(&secret)).bind(conversation_id).bind(node_id).execute(&state.pool).await?;
    sqlx::query("delete from mcp_session_tokens where conversation_id = $1 and created_at < now() - make_interval(days => $2)").bind(conversation_id).bind(IDLE_DAYS).execute(&state.pool).await?;

    Ok(format!("{SESSION_TOKEN_PREFIX}{secret}"))
}

pub async fn revoke(State(state): State<AppState>, user: AuthUser, Json(request): Json<RevokeRequest>) -> ApiResult<Json<Value>> {
    let revoked = match (request.sha256.as_deref(), request.conversation_id) {
        (Some(digest), None) => {
            let Some(token_hash) = decode_sha256(digest) else {
                return Err(ApiError::bad_request("invalid_sha256", "Enter the SHA-256 of the token without its myc_session_ prefix, as 64 hexadecimal characters."));
            };

            sqlx::query("delete from mcp_session_tokens t using conversations c where c.id = t.conversation_id and c.owner = $1 and t.token_hash = $2").bind(user.id).bind(token_hash).execute(&state.pool).await?
        }
        (None, Some(conversation_id)) => {
            crate::store::find_owned(&state.pool, conversation_id, user.id).await?;

            sqlx::query("delete from mcp_session_tokens where conversation_id = $1").bind(conversation_id).execute(&state.pool).await?
        }
        _ => return Err(ApiError::bad_request("revoke_target", "Give either sha256 or conversation_id, not both.")),
    };

    Ok(Json(json!({"revoked": revoked.rows_affected()})))
}

fn decode_sha256(digest: &str) -> Option<Vec<u8>> {
    if digest.len() != 64 {
        return None;
    }

    (0..64).step_by(2).map(|at| u8::from_str_radix(digest.get(at..at + 2)?, 16).ok()).collect()
}
