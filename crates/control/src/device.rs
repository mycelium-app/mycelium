use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, hash_token, new_token};
use crate::store::{enum_text, parse_enum, publish_node};
use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use mycelium_proto::api::{DeviceRequest, DeviceResponse, DeviceTokenRequest, DeviceTokenResponse, PendingDevice};
use mycelium_proto::conversation::{DeviceKind, DeviceState};
use mycelium_proto::stream::StreamMessage;
use crate::rate_limit::Limited;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;

const POLL_INTERVAL_SECONDS: u64 = 5;
const APP_SIGN_IN_CALLBACK: &str = "mycelium://signed-in";
const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKMNPQRSTVWXZ";

fn new_user_code() -> String {
    let pick_four = || (0..4).map(|_| USER_CODE_ALPHABET[rand::random_range(0..USER_CODE_ALPHABET.len())] as char).collect::<String>();

    format!("{}-{}", pick_four(), pick_four())
}

#[derive(Clone, sqlx::FromRow)]
struct DeviceIdentity {
    user_code: String,
    kind: String,
    label: String,
    hostname: String,
}

fn publish_device(state: &AppState, identity: &DeviceIdentity, device_state: DeviceState, result_id: Option<Uuid>) -> ApiResult<()> {
    state.publish(None, StreamMessage::Device { user_code: identity.user_code.clone(), kind: parse_enum(&identity.kind)?, label: identity.label.clone(), hostname: identity.hostname.clone(), state: device_state, result_id });

    Ok(())
}

async fn publish_expiry_when_due(state: AppState, identity: DeviceIdentity) {
    tokio::time::sleep(state.config.device_code_lifetime).await;

    let unfinished = sqlx::query_scalar::<_, bool>("select exists (select 1 from device_codes where user_code = $1 and consumed_at is null and denied_at is null)").bind(&identity.user_code).fetch_one(&state.pool).await;

    if unfinished.is_ok_and(|unfinished| unfinished) {
        let _ = publish_device(&state, &identity, DeviceState::Expired, None);
    }
}

fn normalise(user_code: &str) -> String {
    user_code.trim().to_uppercase()
}

pub fn client_address(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    let trusted = |address: IpAddr| state.config.trusted_proxies.iter().any(|range| range.contains(address));

    if !trusted(peer.ip()) {
        return peer.ip();
    }

    let forwarded = headers.get_all("x-forwarded-for").iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).filter_map(|value| value.trim().parse::<IpAddr>().ok());
    let hops: Vec<IpAddr> = forwarded.collect();

    hops.into_iter().rev().find(|address| !trusted(*address)).unwrap_or(peer.ip())
}

#[derive(serde::Deserialize)]
pub struct StartQuery {
    return_to: Option<String>,
}

pub async fn start(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(query): Query<StartQuery>, Json(request): Json<DeviceRequest>) -> Result<Json<DeviceResponse>, Response> {
    if query.return_to.as_deref().is_some_and(|target| target != APP_SIGN_IN_CALLBACK) {
        return Err(ApiError::bad_request("return_to_not_allowed", "Only the Mycelium app's own sign-in callback can be returned to.").into_response());
    }

    let address = client_address(&state, &headers, peer);
    state.rate_limiter.take(Limited::StartDevice, address).map_err(IntoResponse::into_response)?;

    start_allowed(state, address, request, query.return_to.is_some()).await.map_err(IntoResponse::into_response)
}

async fn start_allowed(state: AppState, address: IpAddr, request: DeviceRequest, returns_to_app: bool) -> ApiResult<Json<DeviceResponse>> {
    let label = if request.label.trim().is_empty() { request.hostname.trim().to_string() } else { request.label.trim().to_string() };

    if label.is_empty() {
        return Err(ApiError::bad_request("label_required", "Enter a name for this device."));
    }

    let device_code = new_token();
    let ip = address.to_string();

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
        .bind(state.config.device_code_lifetime.as_secs_f64())
        .execute(&state.pool)
        .await?;

        if inserted.rows_affected() == 1 {
            let verification_uri = format!("{}/approve", state.config.public_url);
            let identity = DeviceIdentity { user_code: user_code.clone(), kind: enum_text(&request.kind), label: label.clone(), hostname: request.hostname.clone() };

            publish_device(&state, &identity, DeviceState::Pending, None)?;
            tokio::spawn(publish_expiry_when_due(state.clone(), identity));

            let return_to = if returns_to_app { "&return_to=mycelium%3A%2F%2Fsigned-in" } else { "" };

            return Ok(Json(DeviceResponse {
                device_code,
                verification_uri_complete: format!("{verification_uri}?code={user_code}{return_to}"),
                verification_uri,
                user_code,
                interval: POLL_INTERVAL_SECONDS,
                expires_in: state.config.device_code_lifetime.as_secs(),
            }));
        }
    }

    Err(ApiError::internal("could not allocate a user code"))
}

#[derive(sqlx::FromRow)]
struct DeviceRow {
    id: Uuid,
    #[sqlx(flatten)]
    identity: DeviceIdentity,
    os: String,
    arch: String,
    expired: bool,
    approved_by: Option<Uuid>,
    denied: bool,
    consumed: bool,
}

pub async fn token(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(request): Json<DeviceTokenRequest>) -> Result<Json<DeviceTokenResponse>, Response> {
    let address = client_address(&state, &headers, peer);
    state.rate_limiter.take(Limited::PollDeviceToken, address).map_err(IntoResponse::into_response)?;

    token_allowed(state, request).await.map_err(IntoResponse::into_response)
}

async fn token_allowed(state: AppState, request: DeviceTokenRequest) -> ApiResult<Json<DeviceTokenResponse>> {
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, DeviceRow>(
        "select id, user_code, kind, label, hostname, os, arch, expires_at < now() as expired, approved_by,
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

    let kind: DeviceKind = parse_enum(&row.identity.kind)?;
    let secret = new_token();
    let id = Uuid::new_v4();

    match kind {
        DeviceKind::Node => {
            let base_name = if row.identity.label.is_empty() { row.identity.hostname.clone() } else { row.identity.label.clone() };
            let mut suffix = 1;

            loop {
                let name = if suffix == 1 { base_name.clone() } else { format!("{base_name}-{suffix}") };
                let inserted = sqlx::query(
                    "insert into nodes (id, name, hostname, os, arch, token_hash, created_by) values ($1, $2, $3, $4, $5, $6, $7)
                     on conflict (name) where revoked_at is null do nothing",
                )
                .bind(id)
                .bind(&name)
                .bind(&row.identity.hostname)
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
            sqlx::query("insert into client_tokens (id, user_id, label, hostname, token_hash) values ($1, $2, $3, $4, $5)")
                .bind(id)
                .bind(approver)
                .bind(&row.identity.label)
                .bind(&row.identity.hostname)
                .bind(hash_token(&secret))
                .execute(&mut *tx)
                .await?;
        }
    }

    sqlx::query("update device_codes set consumed_at = now(), result_id = $2 where id = $1").bind(row.id).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;

    if kind == DeviceKind::Node {
        publish_node(&state, id).await?;
    }

    publish_device(&state, &row.identity, DeviceState::Consumed, Some(id))?;

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
    let updated = sqlx::query_as::<_, DeviceIdentity>(sqlx::AssertSqlSafe(format!("update device_codes set approved_by = $2, approved_at = now() where {PENDING_FILTER} returning user_code, kind, label, hostname")))
        .bind(normalise(&user_code))
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;

    resolved(&state, updated, DeviceState::Approved)
}

pub async fn deny(State(state): State<AppState>, _user: AuthUser, Path(user_code): Path<String>) -> ApiResult<StatusCode> {
    let updated = sqlx::query_as::<_, DeviceIdentity>(sqlx::AssertSqlSafe(format!("update device_codes set denied_at = now() where {PENDING_FILTER} returning user_code, kind, label, hostname")))
        .bind(normalise(&user_code))
        .fetch_optional(&state.pool)
        .await?;

    resolved(&state, updated, DeviceState::Denied)
}

fn resolved(state: &AppState, updated: Option<DeviceIdentity>, device_state: DeviceState) -> ApiResult<StatusCode> {
    let identity = updated.ok_or_else(|| ApiError::not_found("That code"))?;

    publish_device(state, &identity, device_state, None)?;

    Ok(StatusCode::NO_CONTENT)
}
