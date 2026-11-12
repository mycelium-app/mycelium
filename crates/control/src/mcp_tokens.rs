use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, hash_token, new_token};
use axum::Json;
use axum::extract::State;
use mycelium_proto::conversation::EventKind;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub const SESSION_TOKEN_PREFIX: &str = "myc_session_";
const IDLE_DAYS: i32 = 30;
const NO_TOOLS_NOTICE: &str = "This session started without Mycelium's tools. Start a new session to get them back.";

#[derive(Debug, Deserialize)]
pub struct RevokeRequest {
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<Uuid>,
}

pub async fn mint(state: &AppState, node_id: Uuid, conversation_id: Uuid) -> ApiResult<String> {
    let secret = new_token();

    let token_id = sqlx::query_scalar::<_, Uuid>("insert into mcp_session_tokens (token_hash, conversation_id, node_id) values ($1, $2, $3) returning id").bind(hash_token(&secret)).bind(conversation_id).bind(node_id).fetch_one(&state.pool).await?;
    sqlx::query("delete from mcp_session_tokens where conversation_id = $1 and created_at < now() - make_interval(days => $2)").bind(conversation_id).bind(IDLE_DAYS).execute(&state.pool).await?;

    tokio::spawn(warn_if_tools_never_listed(state.clone(), node_id, conversation_id, token_id));

    Ok(format!("{SESSION_TOKEN_PREFIX}{secret}"))
}

pub async fn mark_tools_listed(state: &AppState, token_hash: &[u8]) -> ApiResult<()> {
    sqlx::query("update mcp_session_tokens set listed_tools_at = coalesce(listed_tools_at, now()) where token_hash = $1").bind(token_hash).execute(&state.pool).await?;

    Ok(())
}

async fn warn_if_tools_never_listed(state: AppState, node_id: Uuid, conversation_id: Uuid, token_id: Uuid) {
    tokio::time::sleep(state.config.tools_listing_grace).await;

    let waiting = sqlx::query_scalar::<_, bool>("select exists (select 1 from mcp_session_tokens where id = $1 and listed_tools_at is null)").bind(token_id).fetch_one(&state.pool).await;

    if !matches!(waiting, Ok(true)) {
        return;
    }

    tracing::warn!("the session of {conversation_id} never asked for its Mycelium tools");

    let payload = json!({"setting": "mycelium_tools", "requested": "listed", "message": NO_TOOLS_NOTICE});
    let recorded = crate::link::record(&state, node_id, conversation_id, None, EventKind::SessionNotice, payload, None).await;

    if let Err(error) = recorded {
        tracing::warn!("could not record the missing tools notice of {conversation_id}: {}", error.message());
    }
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
