use crate::auth::AuthUser;
use crate::device::client_address;
use crate::error::{ApiError, ApiResult};
use crate::rate_limit::Limited;
use crate::state::{AppState, hash_token, new_token};
use crate::store::publish_node;
use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{CreateJoinToken, DeviceTokenResponse, JoinToken, RedeemJoinToken};
use mycelium_proto::conversation::DeviceKind;
use std::net::SocketAddr;
use std::time::Duration;
use uuid::Uuid;

const JOIN_TOKEN_LIFETIME: Duration = Duration::from_secs(3600);
const JOIN_TOKEN_PREFIX: &str = "myc_join_";
const MAX_NAME: usize = 63;

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateJoinToken>) -> ApiResult<Json<JoinToken>> {
    user.require_full_access()?;

    let name = request.name.trim();

    if name.is_empty() || name.len() > MAX_NAME {
        return Err(ApiError::bad_request("name_required", format!("Enter a name for the node of up to {MAX_NAME} characters.")));
    }

    let secret = new_token();
    let expires_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>("insert into node_join_tokens (token_hash, name, created_by, expires_at) values ($1, $2, $3, now() + make_interval(secs => $4)) returning expires_at")
        .bind(hash_token(&secret))
        .bind(name)
        .bind(user.id)
        .bind(JOIN_TOKEN_LIFETIME.as_secs_f64())
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(JoinToken { token: format!("{JOIN_TOKEN_PREFIX}{secret}"), expires_at }))
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    name: String,
    created_by: Uuid,
    expired: bool,
    consumed: bool,
}

pub async fn redeem(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(request): Json<RedeemJoinToken>) -> Result<Json<DeviceTokenResponse>, Response> {
    let address = client_address(&state, &headers, peer);
    state.rate_limiter.take(Limited::RedeemJoinToken, address).map_err(IntoResponse::into_response)?;

    redeem_allowed(state, request).await.map_err(IntoResponse::into_response)
}

async fn redeem_allowed(state: AppState, request: RedeemJoinToken) -> ApiResult<Json<DeviceTokenResponse>> {
    let mut tx = state.pool.begin().await?;
    let presented = request.token.trim();
    let secret = presented.strip_prefix(JOIN_TOKEN_PREFIX).unwrap_or(presented);
    let row = sqlx::query_as::<_, TokenRow>("select id, name, created_by, expires_at < now() as expired, consumed_at is not null as consumed from node_join_tokens where token_hash = $1 for update")
        .bind(hash_token(secret))
        .fetch_optional(&mut *tx)
        .await?;
    let row = row.ok_or_else(|| ApiError::bad_request("join_token_unknown", "This join token is not recognised. Create a new one in Add an outpost and copy it again."))?;

    if row.consumed {
        return Err(ApiError::bad_request("join_token_used", "This join token has already been used. Create a new one in Add an outpost."));
    }

    if row.expired {
        return Err(ApiError::bad_request("join_token_expired", "This join token has expired. Create a new one in Add an outpost."));
    }

    let node_secret = new_token();
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "insert into nodes (id, name, hostname, os, arch, token_hash, created_by) values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (name) where revoked_at is null do nothing",
    )
    .bind(id)
    .bind(&row.name)
    .bind(&request.hostname)
    .bind(&request.os)
    .bind(&request.arch)
    .bind(hash_token(&node_secret))
    .bind(row.created_by)
    .execute(&mut *tx)
    .await?;

    if inserted.rows_affected() == 0 {
        return Err(ApiError::conflict("node_name_taken", format!("A node called {} already exists. Revoke it, or create a join token with another name.", row.name)));
    }

    sqlx::query("update node_join_tokens set consumed_at = now(), node_id = $2 where id = $1").bind(row.id).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;

    publish_node(&state, id).await?;

    Ok(Json(DeviceTokenResponse { kind: DeviceKind::Node, id, token: format!("myc_node_{node_secret}") }))
}
