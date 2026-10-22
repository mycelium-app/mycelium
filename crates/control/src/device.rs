use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, hash_token, new_token};
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use mycelium_proto::api::{DeviceRequest, DeviceResponse, DeviceTokenRequest, DeviceTokenResponse, PendingDevice};
use mycelium_proto::conversation::DeviceKind;
use std::net::SocketAddr;
use uuid::Uuid;

const LIFETIME_SECONDS: u64 = 600;
const POLL_INTERVAL_SECONDS: u64 = 5;
const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKMNPQRSTVWXZ";

fn new_user_code() -> String {
    let pick_four = || (0..4).map(|_| USER_CODE_ALPHABET[rand::random_range(0..USER_CODE_ALPHABET.len())] as char).collect::<String>();

    format!("{}-{}", pick_four(), pick_four())
}

fn normalise(user_code: &str) -> String {
    user_code.trim().to_uppercase()
}

fn peer_address(headers: &HeaderMap, peer: SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| peer.ip().to_string())
}

pub async fn start(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(request): Json<DeviceRequest>) -> ApiResult<Json<DeviceResponse>> {
    let label = if request.label.trim().is_empty() { request.hostname.trim().to_string() } else { request.label.trim().to_string() };

    if label.is_empty() {
        return Err(ApiError::bad_request("label_required", "Enter a name for this device."));
    }

    let device_code = new_token();
    let ip = peer_address(&headers, peer);

    for _ in 0..5 {
        let user_code = new_user_code();
        let inserted = sqlx::query(
            "insert into device_codes (kind, device_code_hash, user_code, label, hostname, os, arch, ip, expires_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, now() + make_interval(secs => $9))
             on conflict (user_code) do nothing",
        )
        .bind(enum_text(&request.kind))
        .bind(hash_token(&device_code))
        .bind(&user_code)
        .bind(&label)
        .bind(&request.hostname)
        .bind(&request.os)
        .bind(&request.arch)
        .bind(&ip)
        .bind(LIFETIME_SECONDS as f64)
        .execute(&state.pool)
        .await?;

        if inserted.rows_affected() == 1 {
            let verification_uri = format!("{}/approve", state.config.public_url);

            return Ok(Json(DeviceResponse {
                device_code,
                verification_uri_complete: format!("{verification_uri}?code={user_code}"),
                verification_uri,
                user_code,
                interval: POLL_INTERVAL_SECONDS,
                expires_in: LIFETIME_SECONDS,
            }));
        }
    }

    Err(ApiError::internal("could not allocate a user code"))
}

#[derive(sqlx::FromRow)]
struct DeviceRow {
    id: Uuid,
    kind: String,
    label: String,
    hostname: String,
    os: String,
    arch: String,
    expired: bool,
    approved_by: Option<Uuid>,
    denied: bool,
    consumed: bool,
}

pub async fn token(State(state): State<AppState>, Json(request): Json<DeviceTokenRequest>) -> ApiResult<Json<DeviceTokenResponse>> {
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, DeviceRow>(
        "select id, kind, label, hostname, os, arch, expires_at < now() as expired, approved_by,
                denied_at is not null as denied, consumed_at is not null as consumed
         from device_codes where device_code_hash = $1 for update",
    )
    .bind(hash_token(&request.device_code))
    .fetch_optional(&mut *tx)
    .await?;

    let expired = || ApiError::bad_request("expired_token", "This code has expired. Start again to get a new one.");
    let row = row.ok_or_else(expired)?;

    if row.consumed || row.expired {
        return Err(expired());
    }

    if row.denied {
        return Err(ApiError::bad_request("access_denied", "This request was denied. Start again if you want to try again."));
    }

    let Some(approver) = row.approved_by else {
        return Err(ApiError::bad_request("authorization_pending", "Waiting for you to approve this code in the browser."));
    };

    let kind: DeviceKind = parse_enum(&row.kind)?;
    let secret = new_token();
    let id = Uuid::new_v4();

    match kind {
        DeviceKind::Node => {
            let base_name = if row.label.is_empty() { row.hostname.clone() } else { row.label.clone() };
            let mut suffix = 1;

            loop {
                let name = if suffix == 1 { base_name.clone() } else { format!("{base_name}-{suffix}") };
                let inserted = sqlx::query(
                    "insert into nodes (id, name, hostname, os, arch, token_hash, created_by) values ($1, $2, $3, $4, $5, $6, $7)
                     on conflict (name) do nothing",
                )
                .bind(id)
                .bind(&name)
                .bind(&row.hostname)
                .bind(&row.os)
                .bind(&row.arch)
                .bind(hash_token(&secret))
                .bind(approver)
                .execute(&mut *tx)
                .await?;

                if inserted.rows_affected() == 1 {
                    break;
                }

                suffix += 1;
            }
        }
        DeviceKind::Client => {
            sqlx::query("insert into client_tokens (id, user_id, label, token_hash) values ($1, $2, $3, $4)")
                .bind(id)
                .bind(approver)
                .bind(&row.label)
                .bind(hash_token(&secret))
                .execute(&mut *tx)
                .await?;
        }
    }

    sqlx::query("update device_codes set consumed_at = now(), result_id = $2 where id = $1").bind(row.id).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;

    let prefix = match kind {
        DeviceKind::Node => "myc_node_",
        DeviceKind::Client => "myc_client_",
    };

    Ok(Json(DeviceTokenResponse { kind, id, token: format!("{prefix}{secret}") }))
}

#[derive(sqlx::FromRow)]
struct PendingRow {
    user_code: String,
    kind: String,
    label: String,
    hostname: String,
    os: String,
    arch: String,
    ip: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

pub async fn pending(State(state): State<AppState>, _user: AuthUser, Path(user_code): Path<String>) -> ApiResult<Json<PendingDevice>> {
    let row = sqlx::query_as::<_, PendingRow>(
        sqlx::AssertSqlSafe(format!("select user_code, kind, label, hostname, os, arch, ip, expires_at from device_codes where {PENDING_FILTER}")),
    )
    .bind(normalise(&user_code))
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or_else(|| ApiError::not_found("That code"))?;

    Ok(Json(PendingDevice {
        user_code: row.user_code,
        kind: parse_enum(&row.kind)?,
        label: row.label,
        hostname: row.hostname,
        os: row.os,
        arch: row.arch,
        ip: row.ip,
        expires_at: row.expires_at,
    }))
}

const PENDING_FILTER: &str = "user_code = $1 and approved_at is null and denied_at is null and consumed_at is null and expires_at > now()";

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(user_code): Path<String>) -> ApiResult<StatusCode> {
    let updated = sqlx::query(sqlx::AssertSqlSafe(format!("update device_codes set approved_by = $2, approved_at = now() where {PENDING_FILTER}")))
        .bind(normalise(&user_code))
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    resolved(updated.rows_affected())
}

pub async fn deny(State(state): State<AppState>, _user: AuthUser, Path(user_code): Path<String>) -> ApiResult<StatusCode> {
    let updated = sqlx::query(sqlx::AssertSqlSafe(format!("update device_codes set denied_at = now() where {PENDING_FILTER}"))).bind(normalise(&user_code)).execute(&state.pool).await?;

    resolved(updated.rows_affected())
}

fn resolved(rows_affected: u64) -> ApiResult<StatusCode> {
    if rows_affected == 0 {
        return Err(ApiError::not_found("That code"));
    }

    Ok(StatusCode::NO_CONTENT)
}
