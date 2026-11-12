use crate::auth::{AuthUser, client_token};
use crate::conversation_connections;
use crate::error::{ApiError, ApiResult};
use crate::group_connections;
use crate::integration_proxy;
use crate::prompts;
use crate::state::{AppState, hash_token, new_token};
use crate::token_delivery;
use axum::Json;
use axum::extract::{MatchedPath, Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{ApiAccessRequest, ApiAccessState, ApproveApiAccessRequest};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use std::sync::atomic::{AtomicI64, Ordering};
use uuid::Uuid;

pub const SCOPES: [&str; 6] = ["conversations:read", "conversations:write", "nodes:read", "settings:read", "assistants", FULL_SCOPE];

const FULL_SCOPE: &str = "full";

const DEFAULT_HOURS: i64 = 4;
const MIN_HOURS: i64 = 1;
const MAX_HOURS: i64 = 24;
const LISTED: i64 = 100;
const CALL_LOG_DAYS: i32 = 30;
const PURGE_INTERVAL_SECONDS: i64 = 3600;
static LAST_PURGE: AtomicI64 = AtomicI64::new(0);
const TOKEN_LABEL: &str = "Agent access";

const REQUEST_COLUMNS: &str = "id, conversation_id, scopes, hours, reason, state, created_at, decided_at, server_id, (select name from chat_tool_servers s where s.id = api_access_requests.server_id) as integration";

const FULL_ACCESS_ENDINGS: [(&str, &str); 5] = [
    ("/terminal", "Using a conversation's terminal"),
    ("/permission", "Changing a conversation's permission mode"),
    ("/config", "Changing a conversation's settings"),
    ("/move", "Moving a conversation"),
    ("/git/push", "Pushing from a conversation"),
];
const FULL_ACCESS_FALLBACK: &str = "That";

enum Need {
    Anyone,
    Scope(&'static str),
    Full { what: &'static str, hint: &'static str },
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
    server_id: Option<Uuid>,
    integration: Option<String>,
}

impl RequestRow {
    fn to_proto(self) -> ApiAccessRequest {
        let state = match self.state.as_str() {
            "approved" => ApiAccessState::Approved,
            "declined" => ApiAccessState::Declined,
            _ => ApiAccessState::Requested,
        };

        ApiAccessRequest { id: self.id, conversation_id: self.conversation_id, scopes: self.scopes, hours: self.hours, reason: self.reason, state, created_at: self.created_at, decided_at: self.decided_at, server_id: self.server_id, integration: self.integration }
    }
}

fn need(method: &Method, route: &str) -> Need {
    let reading = method == Method::GET || method == Method::HEAD;

    if reading && ["/healthz", "/readyz", "/api/v1/version", "/api/v1/me"].contains(&route) {
        return Need::Anyone;
    }

    if route == "/api/v1/conversations" || route.starts_with("/api/v1/conversations/") {
        if !reading && route == "/api/v1/conversations" {
            return Need::Full { what: "Starting a conversation", hint: " Use the start_conversation tool, which asks the person." };
        }

        if let Some((_, what)) = FULL_ACCESS_ENDINGS.iter().find(|(ending, _)| route.ends_with(ending)) {
            return Need::Full { what, hint: "" };
        }

        if route.contains("/responses/") {
            return Need::Full { what: "Answering a held response", hint: "" };
        }

        if route.contains("/environment") {
            return Need::Full { what: "Driving a conversation's environment", hint: "" };
        }

        return Need::Scope(if reading { "conversations:read" } else { "conversations:write" });
    }

    let assistant_route = route == "/api/v1/assistants" || route.strip_prefix("/api/v1/assistants/").is_some_and(|rest| rest == "{id}");

    if assistant_route && (reading || method == Method::POST || method == Method::PATCH) {
        return Need::Scope("assistants");
    }

    if !reading {
        return Need::Full { what: FULL_ACCESS_FALLBACK, hint: "" };
    }

    match route {
        "/api/v1/attachments/{sha256}" | "/api/v1/search" | "/api/v1/activity/conversations" => Need::Scope("conversations:read"),
        "/api/v1/nodes" | "/api/v1/worktrees" => Need::Scope("nodes:read"),
        "/api/v1/usage-limit" | "/api/v1/instructions" | "/api/v1/extensions" | "/api/v1/projects" | "/api/v1/groups" | "/api/v1/memories" | "/api/v1/memory-banks" => Need::Scope("settings:read"),
        _ => Need::Full { what: FULL_ACCESS_FALLBACK, hint: "" },
    }
}

pub async fn gate_scoped_tokens(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(token) = client_token(request.headers()) else {
        return next.run(request).await;
    };
    let grant = sqlx::query_as::<_, (Uuid, Uuid, Option<Vec<String>>, Option<Uuid>)>("select id, user_id, scopes, conversation_id from client_tokens where token_hash = $1 and revoked_at is null and (expires_at is null or expires_at > now())")
        .bind(hash_token(token))
        .fetch_optional(&state.pool)
        .await;
    let Ok(Some((token_id, owner, Some(scopes), bound_conversation))) = grant else {
        return next.run(request).await;
    };

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let route = request.extensions().get::<MatchedPath>().map(|matched| matched.as_str().to_string()).unwrap_or_else(|| request.uri().path().to_string());
    let unreachable = match (bound_conversation, conversation_in(&path)) {
        (Some(bound), Some(asked)) if asked != bound => {
            let linked = conversation_connections::active(&state, owner, bound, asked).await.unwrap_or(false);
            let reachable = linked || group_connections::connected(&state, owner, bound, asked).await.unwrap_or(false);

            (!reachable).then_some((bound, asked))
        }
        _ => None,
    };
    let refusal = match unreachable {
        Some((bound, asked)) => {
            let reached = group_name(&state, bound).await;
            let target = group_name(&state, asked).await;

            Some(ApiError::new(StatusCode::FORBIDDEN, "group_not_connected", format!("This token can reach conversations in {reached} and groups connected to it. {target} is not connected.")))
        }
        None => match integration_proxy::server_of(&path) {
            Some(server_id) if scopes.iter().any(|granted| granted.starts_with(&integration_proxy::token_scope(server_id, ""))) => None,
            Some(_) => Some(ApiError::new(StatusCode::FORBIDDEN, "scope_not_granted", "This token does not include that integration. Ask the person for access that includes it.")),
            None => match need(&method, &route) {
                Need::Anyone => None,
                Need::Scope(scope) if scopes.iter().any(|granted| granted == scope) => None,
                Need::Scope(scope) => Some(ApiError::new(StatusCode::FORBIDDEN, "scope_not_granted", format!("This token does not include {scope}. Ask the person for access that includes it."))),
                Need::Full { .. } if scopes.iter().any(|granted| granted == FULL_SCOPE) => None,
                Need::Full { what, hint } => Some(ApiError::new(StatusCode::FORBIDDEN, "full_access_required", format!("{what} needs full access, which this token does not include.{hint}"))),
            },
        },
    };

    let response = match refusal {
        Some(error) => error.into_response(),
        None => next.run(request).await,
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

    if !purge_is_due() {
        return response;
    }

    let purged = sqlx::query("delete from api_token_calls where called_at < now() - make_interval(days => $1)").bind(CALL_LOG_DAYS).execute(&state.pool).await;

    if let Err(error) = purged {
        tracing::warn!("could not purge old calls made with scoped tokens: {error}");
    }

    response
}

async fn group_name(state: &AppState, conversation: Uuid) -> String {
    let group = group_connections::group_of(state, conversation).await.unwrap_or(None);
    let Some(group) = group else {
        return "no group".to_string();
    };
    let name = sqlx::query_scalar::<_, String>("select name from groups where id = $1").bind(group).fetch_optional(&state.pool).await.unwrap_or(None);

    name.unwrap_or_else(|| "no group".to_string())
}

fn conversation_in(path: &str) -> Option<Uuid> {
    path.strip_prefix("/api/v1/conversations/")?.split('/').next()?.parse().ok()
}

fn purge_is_due() -> bool {
    let now = chrono::Utc::now().timestamp();
    let last = LAST_PURGE.load(Ordering::Relaxed);

    now - last >= PURGE_INTERVAL_SECONDS && LAST_PURGE.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
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

pub async fn request(state: &AppState, conversation_id: Uuid, scopes: &[String], hours: Option<i64>, reason: &str, integration: Option<&str>) -> ApiResult<Result<ApiAccessRequest, String>> {
    let hours = hours.unwrap_or(DEFAULT_HOURS);
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let server = match integration {
        Some(wanted) => match integration_proxy::find_with_api(state, owner, wanted).await? {
            Ok(server) => Some(server),
            Err(message) => return Ok(Err(message)),
        },
        None => None,
    };
    let allowed: Vec<&str> = if server.is_some() { integration_proxy::SCOPES.to_vec() } else { SCOPES.to_vec() };

    if !(MIN_HOURS..=MAX_HOURS).contains(&hours) {
        return Ok(Err(format!("Nothing was requested. hours is {MIN_HOURS} to {MAX_HOURS}.")));
    }

    let mut wanted: Vec<String> = Vec::new();

    for scope in scopes {
        let scope = scope.trim();

        if !allowed.contains(&scope) {
            return Ok(Err(format!("Nothing was requested. {scope} is not a scope. Use {}.", allowed.join(", "))));
        }

        if !wanted.iter().any(|known| known == scope) {
            wanted.push(scope.to_string());
        }
    }

    if wanted.is_empty() {
        return Ok(Err(format!("Nothing was requested. Give scopes, from {}.", allowed.join(", "))));
    }

    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("insert into api_access_requests (owner, conversation_id, scopes, hours, reason, server_id) values ($1, $2, $3, $4, $5, $6) returning {REQUEST_COLUMNS}")))
        .bind(owner)
        .bind(conversation_id)
        .bind(&wanted)
        .bind(hours as i32)
        .bind(crate::store::text_without_nul(reason))
        .bind(server)
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

    token_delivery::require_requested(&found.state)?;

    let scopes = match &body.scopes {
        None => found.scopes.clone(),
        Some(chosen) => found.scopes.iter().filter(|scope| chosen.contains(scope)).cloned().collect(),
    };

    if scopes.is_empty() {
        return Err(ApiError::bad_request("scopes_required", "Choose at least one kind of access to approve, or decline the request."));
    }

    let node_id = token_delivery::node_of(&state, found.conversation_id).await?;

    let granted: Vec<String> = match found.server_id {
        Some(server_id) => scopes.iter().map(|scope| integration_proxy::token_scope(server_id, scope)).collect(),
        None => scopes.clone(),
    };
    let secret = new_token();
    let mut tx = state.pool.begin().await?;
    let claimed = sqlx::query("update api_access_requests set state = 'approved', decided_at = now(), scopes = $2 where id = $1 and state = 'requested'").bind(id).bind(&scopes).execute(&mut *tx).await?;

    if claimed.rows_affected() == 0 {
        return Err(token_delivery::already_decided());
    }

    let token_id = sqlx::query_scalar::<_, Uuid>(
        "insert into client_tokens (user_id, label, token_hash, scopes, expires_at, conversation_id) values ($1, $2, $3, $4, now() + make_interval(hours => $5::int), $6) returning id",
    )
    .bind(user.id)
    .bind(TOKEN_LABEL)
    .bind(hash_token(&secret))
    .bind(&granted)
    .bind(found.hours)
    .bind(found.conversation_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("update api_access_requests set token_id = $2 where id = $1").bind(id).bind(token_id).execute(&mut *tx).await?;
    tx.commit().await?;

    let sent = token_delivery::send_token(&state, node_id, ControlToNode::ApiToken { conversation_id: found.conversation_id, token: format!("myc_client_{secret}") });

    if let Err(cause) = sent {
        sqlx::query("update client_tokens set revoked_at = now() where id = $1").bind(token_id).execute(&state.pool).await?;
        sqlx::query("update api_access_requests set state = 'requested', decided_at = null, token_id = null where id = $1").bind(id).execute(&state.pool).await?;

        return Err(cause);
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<ApiAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    token_delivery::require_requested(&found.state)?;

    sqlx::query("update api_access_requests set state = 'declined', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    prompts::tell_agent(&state, user.id, found.conversation_id, "I declined your request for API access. Do not get access to the API another way.".to_string()).await;

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn token_saved(state: &AppState, node_id: Uuid, conversation_id: Uuid, path: Option<String>) -> ApiResult<()> {
    let delivered = sqlx::query_as::<_, (Uuid, Uuid, Vec<String>, chrono::DateTime<chrono::Utc>, Option<Uuid>)>(
        "select r.owner, t.id, t.scopes, t.expires_at, r.server_id
         from api_access_requests r join client_tokens t on t.id = r.token_id join conversations c on c.id = r.conversation_id
         where r.conversation_id = $1 and c.node_id = $2 and r.state = 'approved' and t.revoked_at is null
         order by r.decided_at desc limit 1",
    )
    .bind(conversation_id)
    .bind(node_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((owner, token_id, scopes, expires_at, server_id)) = delivered else {
        return Ok(());
    };

    let text = match path {
        Some(path) => {
            let base = state.config.public_url.trim_end_matches('/');

            if let Some(server_id) = server_id {
                prompts::tell_agent(state, owner, conversation_id, integration_proxy::granted_text(base, server_id, &path, &scopes, expires_at)).await;

                return Ok(());
            }

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

    prompts::tell_agent(state, owner, conversation_id, text).await;

    Ok(())
}
