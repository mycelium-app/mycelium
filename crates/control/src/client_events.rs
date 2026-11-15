use crate::auth::AuthUser;
use crate::device::client_address;
use crate::error::{ApiError, ApiResult};
use crate::rate_limit::Limited;
use crate::state::AppState;
use axum::Json;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::header::USER_AGENT;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::net::SocketAddr;
use uuid::Uuid;

pub const RETENTION_DAYS: i32 = 14;
const PER_PERSON_CAP: i64 = 5000;
const LARGEST_BATCH: usize = 50;
const LONGEST_TEXT: usize = 2000;
const LARGEST_DETAIL: usize = 16 * 1024;
const DEFAULT_READ: i64 = 100;
const LARGEST_READ: i64 = 500;
pub const KINDS: [&str; 6] = ["error", "rejection", "notice", "webgl", "connection", "visibility"];
const CLIENTS: [&str; 3] = ["web", "desktop", "android"];
const REDACTED: &str = "[redacted]";
const SECRET_PREFIXES: [&str; 10] = ["myc_", "ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "glpat-", "sk-", "xoxb-", "xoxp-"];
const SECRET_NAMES: [&str; 10] = ["token", "access_token", "refresh_token", "code", "state", "key", "secret", "password", "authorization", "cookie"];

#[derive(Deserialize)]
pub struct Report {
    session_id: String,
    client: String,
    version: String,
    events: Vec<ReportedEvent>,
}

#[derive(Deserialize)]
struct ReportedEvent {
    at: DateTime<Utc>,
    kind: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    detail: Value,
}

pub async fn create(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, user: AuthUser, Json(report): Json<Report>) -> Result<StatusCode, Response> {
    state.rate_limiter.take(Limited::ClientEvents, client_address(&state, &headers, peer)).map_err(IntoResponse::into_response)?;
    let user_agent = headers.get(USER_AGENT).and_then(|value| value.to_str().ok()).unwrap_or_default();

    store(&state, user.id, &report, user_agent).await.map_err(IntoResponse::into_response)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn store(state: &AppState, owner: Uuid, report: &Report, user_agent: &str) -> ApiResult<()> {
    if report.events.len() > LARGEST_BATCH {
        return Err(ApiError::bad_request("client_events_too_many", format!("Send at most {LARGEST_BATCH} events at a time.")));
    }

    if !CLIENTS.contains(&report.client.as_str()) {
        return Err(ApiError::bad_request("client_event_client", format!("Give the client as one of {}.", CLIENTS.join(", "))));
    }

    if let Some(event) = report.events.iter().find(|event| !KINDS.contains(&event.kind.as_str())) {
        return Err(ApiError::bad_request("client_event_kind", format!("{} is not a kind of client event. Use one of {}.", event.kind, KINDS.join(", "))));
    }

    let mut tx = state.pool.begin().await?;

    for event in &report.events {
        sqlx::query("insert into client_events (owner, at, session_id, client, version, user_agent, kind, message, detail) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(owner)
            .bind(event.at)
            .bind(shortened(&report.session_id, 64))
            .bind(&report.client)
            .bind(shortened(&report.version, 64))
            .bind(shortened(user_agent, 512))
            .bind(&event.kind)
            .bind(shortened(&redacted_text(&event.message), LONGEST_TEXT))
            .bind(bounded(redacted_detail(&event.detail)))
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query("delete from client_events where owner = $1 and id < (select id from client_events where owner = $1 order by id desc offset $2 - 1 limit 1)").bind(owner).bind(PER_PERSON_CAP).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(())
}

#[derive(Deserialize, Default)]
pub struct Filter {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub client: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(filter): Query<Filter>) -> ApiResult<Json<Value>> {
    Ok(Json(read(&state, user.id, &filter).await?))
}

pub async fn read(state: &AppState, owner: Uuid, filter: &Filter) -> ApiResult<Value> {
    let limit = filter.limit.unwrap_or(DEFAULT_READ).clamp(1, LARGEST_READ);
    let rows = sqlx::query_as::<_, (DateTime<Utc>, String, String, String, String, String, String, Value)>(
        "select at, session_id, client, version, user_agent, kind, message, detail from client_events
         where owner = $1 and ($2::timestamptz is null or at >= $2) and ($3::timestamptz is null or at < $3) and ($4::text is null or client = $4) and ($5::text is null or kind = $5)
         order by at desc, id desc limit $6",
    )
    .bind(owner)
    .bind(filter.since)
    .bind(filter.until)
    .bind(filter.client.as_deref())
    .bind(filter.kind.as_deref())
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let events: Vec<Value> = rows
        .into_iter()
        .map(|(at, session_id, client, version, user_agent, kind, message, detail)| json!({"at": at, "session_id": session_id, "client": client, "version": version, "user_agent": user_agent, "kind": kind, "message": message, "detail": detail}))
        .collect();

    Ok(json!({"events": events}))
}

fn shortened(text: &str, longest: usize) -> String {
    text.chars().take(longest).collect()
}

fn bounded(detail: Value) -> Value {
    if detail.to_string().len() > LARGEST_DETAIL {
        return json!({"dropped": "the detail was larger than 16 KB"});
    }

    detail
}

fn redacted_detail(detail: &Value) -> Value {
    match detail {
        Value::String(text) => Value::String(shortened(&redacted_text(text), LONGEST_TEXT * 4)),
        Value::Array(items) => Value::Array(items.iter().map(redacted_detail).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(name, value)| (name.clone(), if secret_name(name) { Value::String(REDACTED.into()) } else { redacted_detail(value) })).collect::<Map<_, _>>()),
        Value::Null => json!({}),
        other => other.clone(),
    }
}

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();

    SECRET_NAMES.contains(&name.as_str())
}

fn redacted_text(text: &str) -> String {
    let mut after_bearer = false;

    text.split(' ')
        .map(|word| {
            let follows_bearer = after_bearer;
            after_bearer = word.eq_ignore_ascii_case("bearer");

            if follows_bearer || SECRET_PREFIXES.iter().any(|prefix| word.starts_with(prefix)) {
                return REDACTED.to_string();
            }

            redacted_query(word)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redacted_query(word: &str) -> String {
    let (address, query) = word.split_once('?').map_or(("", word), |(address, query)| (address, query));
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if secret_name(name) => format!("{name}={REDACTED}"),
            _ => pair.to_string(),
        })
        .collect();
    let joined = pairs.join("&");

    if word.contains('?') {
        format!("{address}?{joined}")
    } else {
        joined
    }
}
