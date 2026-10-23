use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::state::{AppState, hash_token, new_token};
use axum::Json;
use axum::extract::{MatchedPath, Path, Request, State};
use axum::http::{Method, StatusCode, header::AUTHORIZATION};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{ApiAccessRequest, ApiAccessState, ApproveApiAccessRequest, PromptRequest};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

pub const SCOPES: [&str; 4] = ["conversations:read", "conversations:write", "nodes:read", "settings:read"];

const DEFAULT_HOURS: i64 = 4;
const MIN_HOURS: i64 = 1;
const MAX_HOURS: i64 = 24;
const LISTED: i64 = 100;
const CALL_LOG_DAYS: i32 = 30;
const TOKEN_LABEL: &str = "Agent access";

const REQUEST_COLUMNS: &str = "id, conversation_id, scopes, hours, reason, state, created_at, decided_at";

enum Need {
    Anyone,
    Scope(&'static str),
    Full,
}

#[derive(sqlx::FromRow)]
struct RequestRow {
    id: Uuid,
    conversation_id: Uuid,
    scopes: Vec<String>,
    hours: i32,
    reason: String,
    state: String,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl RequestRow {
    fn to_proto(self) -> ApiAccessRequest {
        let state = match self.state.as_str() {
            "approved" => ApiAccessState::Approved,
            "declined" => ApiAccessState::Declined,
            _ => ApiAccessState::Requested,
        };

        ApiAccessRequest { id: self.id, conversation_id: self.conversation_id, scopes: self.scopes, hours: self.hours, reason: self.reason, state, created_at: self.created_at, decided_at: self.decided_at }
    }
}

fn need(method: &Method, route: &str) -> Need {
    let reading = method == Method::GET || method == Method::HEAD;

    if reading && ["/healthz", "/readyz", "/api/v1/version", "/api/v1/me"].contains(&route) {
        return Need::Anyone;
    }

    if route == "/api/v1/conversations" || route.starts_with("/api/v1/conversations/") {
        if route.ends_with("/terminal") || route.ends_with("/permission") {
            return Need::Full;
        }

        return Need::Scope(if reading { "conversations:read" } else { "conversations:write" });
    }

    if !reading {
        return Need::Full;
    }

    match route {
        "/api/v1/attachments/{sha256}" | "/api/v1/search" | "/api/v1/activity/conversations" => Need::Scope("conversations:read"),
        "/api/v1/nodes" | "/api/v1/worktrees" => Need::Scope("nodes:read"),
        "/api/v1/usage-limit" | "/api/v1/instructions" | "/api/v1/extensions" | "/api/v1/projects" | "/api/v1/groups" | "/api/v1/memories" => Need::Scope("settings:read"),
        _ => Need::Full,
    }
}

pub async fn gate_scoped_tokens(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let token = request.headers().get(AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).and_then(|value| value.strip_prefix("myc_client_"));
    let Some(token) = token else {
        return next.run(request).await;
    };
    let grant = sqlx::query_as::<_, (Uuid, Option<Vec<String>>)>("select id, scopes from client_tokens where token_hash = $1 and revoked_at is null and (expires_at is null or expires_at > now())")
        .bind(hash_token(token))
        .fetch_optional(&state.pool)
        .await;
    let Ok(Some((token_id, Some(scopes)))) = grant else {
        return next.run(request).await;
    };

    let method = request.method().clone();
    let route = request.extensions().get::<MatchedPath>().map(|matched| matched.as_str().to_string()).unwrap_or_else(|| request.uri().path().to_string());
    let allowed = match need(&method, &route) {
        Need::Anyone => true,
        Need::Scope(scope) => scopes.iter().any(|granted| granted == scope),
        Need::Full => false,
    };

    let response = if allowed {
        next.run(request).await
    } else {
        ApiError::new(StatusCode::FORBIDDEN, "scope_not_granted", "This token does not cover that. Ask the person for access that does.").into_response()
    };

    let logged = sqlx::query("insert into api_token_calls (token_id, method, route, status) values ($1, $2, $3, $4)")
        .bind(token_id)
        .bind(method.as_str())
        .bind(&route)
        .bind(i32::from(response.status().as_u16()))
        .execute(&state.pool)
        .await;

    if let Err(error) = logged {
        tracing::warn!("could not log a call made with a scoped token: {error}");
    }

    let purged = sqlx::query("delete from api_token_calls where called_at < now() - make_interval(days => $1)").bind(CALL_LOG_DAYS).execute(&state.pool).await;

    if let Err(error) = purged {
        tracing::warn!("could not purge old calls made with scoped tokens: {error}");
    }

    response
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<RequestRow> {
    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from api_access_requests where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That request"))
}

fn publish(state: &AppState, owner: Uuid, row: RequestRow) -> ApiAccessRequest {
    let request = row.to_proto();
    state.publish(Some(owner), StreamMessage::ApiAccessRequest(request.clone()));

    request
}

async fn tell_agent(state: &AppState, owner: Uuid, conversation_id: Uuid, text: String) {
    let user = AuthUser { id: owner, email: String::new(), name: String::new(), session: None, scopes: None, expires_at: None };
    let _ = prompts::prompt(State(state.clone()), user, Path(conversation_id), Json(PromptRequest { text, ..Default::default() })).await;
}

pub async fn request(state: &AppState, conversation_id: Uuid, scopes: &[String], hours: Option<i64>, reason: &str) -> ApiResult<Result<ApiAccessRequest, String>> {
    let hours = hours.unwrap_or(DEFAULT_HOURS);

    if !(MIN_HOURS..=MAX_HOURS).contains(&hours) {
        return Ok(Err(format!("Nothing was requested. hours is {MIN_HOURS} to {MAX_HOURS}.")));
    }

    let mut wanted: Vec<String> = Vec::new();

    for scope in scopes {
        let scope = scope.trim();

        if !SCOPES.contains(&scope) {
            return Ok(Err(format!("Nothing was requested. {scope} is not a scope. Use {}.", SCOPES.join(", "))));
        }

        if !wanted.iter().any(|known| known == scope) {
            wanted.push(scope.to_string());
        }
    }

    if wanted.is_empty() {
        return Ok(Err(format!("Nothing was requested. Give scopes, from {}.", SCOPES.join(", "))));
    }

    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("insert into api_access_requests (owner, conversation_id, scopes, hours, reason) values ($1, $2, $3, $4, $5) returning {REQUEST_COLUMNS}")))
        .bind(owner)
        .bind(conversation_id)
        .bind(&wanted)
        .bind(hours as i32)
        .bind(crate::store::text_without_nul(reason))
        .fetch_one(&state.pool)
        .await?;

    Ok(Ok(publish(state, owner, row)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ApiAccessRequest>>> {
    let rows = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from api_access_requests where owner = $1 order by (state = 'requested') desc, created_at desc limit {LISTED}")))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows.into_iter().map(RequestRow::to_proto).collect()))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(body): Json<ApproveApiAccessRequest>) -> ApiResult<Json<ApiAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "requested" {
        return Err(ApiError::conflict("request_not_requested", "This request has already been decided."));
    }

    let scopes = match &body.scopes {
        None => found.scopes.clone(),
        Some(chosen) => found.scopes.iter().filter(|scope| chosen.contains(scope)).cloned().collect(),
    };

    if scopes.is_empty() {
        return Err(ApiError::bad_request("scopes_required", "Choose at least one kind of access to approve, or decline the request."));
    }

    let node_id = sqlx::query_scalar::<_, Option<Uuid>>("select node_id from conversations where id = $1").bind(found.conversation_id).fetch_one(&state.pool).await?;
    let Some(node_id) = node_id else {
        return Err(ApiError::conflict("no_node", "This conversation does not run on a node, so there is nowhere to save the token. Decline the request."));
    };

    let secret = new_token();
    let mut tx = state.pool.begin().await?;
    let claimed = sqlx::query("update api_access_requests set state = 'approved', decided_at = now(), scopes = $2 where id = $1 and state = 'requested'").bind(id).bind(&scopes).execute(&mut *tx).await?;

    if claimed.rows_affected() == 0 {
        return Err(ApiError::conflict("request_not_requested", "This request has already been decided."));
    }

    let token_id = sqlx::query_scalar::<_, Uuid>(
        "insert into client_tokens (user_id, label, token_hash, scopes, expires_at, conversation_id) values ($1, $2, $3, $4, now() + make_interval(hours => $5::int), $6) returning id",
    )
    .bind(user.id)
    .bind(TOKEN_LABEL)
    .bind(hash_token(&secret))
    .bind(&scopes)
    .bind(found.hours)
    .bind(found.conversation_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("update api_access_requests set token_id = $2 where id = $1").bind(id).bind(token_id).execute(&mut *tx).await?;
    tx.commit().await?;

    let sent = state.hub.send(node_id, ControlToNode::ApiToken { conversation_id: found.conversation_id, token: format!("myc_client_{secret}") });

    if !sent {
        sqlx::query("update client_tokens set revoked_at = now() where id = $1").bind(token_id).execute(&state.pool).await?;
        sqlx::query("update api_access_requests set state = 'requested', decided_at = null, token_id = null where id = $1").bind(id).execute(&state.pool).await?;

        return Err(ApiError::conflict("node_offline", "The node this conversation runs on is offline, so the token cannot be delivered. Approve it again once the node is back."));
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<ApiAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "requested" {
        return Err(ApiError::conflict("request_not_requested", "This request has already been decided."));
    }

    sqlx::query("update api_access_requests set state = 'declined', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    tell_agent(&state, user.id, found.conversation_id, "I declined your request for API access. Do not get access to the API another way.".to_string()).await;

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn token_saved(state: &AppState, node_id: Uuid, conversation_id: Uuid, path: Option<String>) -> ApiResult<()> {
    let delivered = sqlx::query_as::<_, (Uuid, Uuid, Vec<String>, chrono::DateTime<chrono::Utc>)>(
        "select r.owner, t.id, t.scopes, t.expires_at
         from api_access_requests r join client_tokens t on t.id = r.token_id join conversations c on c.id = r.conversation_id
         where r.conversation_id = $1 and c.node_id = $2 and r.state = 'approved' and t.revoked_at is null
         order by r.decided_at desc limit 1",
    )
    .bind(conversation_id)
    .bind(node_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((owner, token_id, scopes, expires_at)) = delivered else {
        return Ok(());
    };

    let text = match path {
        Some(path) => {
            let base = state.config.public_url.trim_end_matches('/');

            format!(
                "I approved API access for this conversation until {} UTC, for {}. The token is in the file {path}. Read it from there when you need it and never copy it into a message, a file or a commit. Send it as an Authorization: Bearer header to {base}/api/v1/. It stops working at that time, or sooner if I revoke it.",
                expires_at.format("%Y-%m-%d %H:%M"),
                scopes.join(", ")
            )
        }
        None => {
            sqlx::query("update client_tokens set revoked_at = now() where id = $1").bind(token_id).execute(&state.pool).await?;

            "I approved API access, but the node could not save the token, so there is none. Ask again if you still need it.".to_string()
        }
    };

    tell_agent(state, owner, conversation_id, text).await;

    Ok(())
}
