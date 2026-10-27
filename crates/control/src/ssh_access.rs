use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::state::{AppState, hash_token, new_token};
use crate::token_delivery;
use axum::Json;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::api::{ApproveSshAccessRequest, SshAccessRequest, SshAccessState, SshToken};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at, sleep_until};
use uuid::Uuid;

const DEFAULT_MINUTES: i64 = 30;
const MIN_MINUTES: i64 = 5;
const MAX_MINUTES: i64 = 240;
const LISTED: i64 = 100;
const TOKEN_PREFIX: &str = "myc_ssh_";
const PERSON_TOKEN_MINUTES: i64 = 10;
const PERSON_SESSION_HOURS: i64 = 12;
const NODE_WATCH: Duration = Duration::from_secs(5);

const REQUEST_COLUMNS: &str = "r.id, r.conversation_id, r.node_id, n.name as node_name, r.minutes, r.reason, r.state, r.created_at, r.decided_at, r.expires_at";
const REQUEST_SOURCE: &str = "ssh_access_requests r join nodes n on n.id = r.node_id";

enum Toward {
    Data(Vec<u8>),
    ClosedByNode,
    Cut,
}

struct Relay {
    node_id: Uuid,
    request_id: Uuid,
    to_agent: mpsc::UnboundedSender<Toward>,
}

static RELAYS: LazyLock<Mutex<HashMap<Uuid, Relay>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(sqlx::FromRow)]
struct RequestRow {
    id: Uuid,
    conversation_id: Uuid,
    node_id: Uuid,
    node_name: String,
    minutes: i32,
    reason: String,
    state: String,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl RequestRow {
    fn to_proto(self) -> SshAccessRequest {
        let state = match self.state.as_str() {
            "approved" => SshAccessState::Approved,
            "declined" => SshAccessState::Declined,
            "revoked" => SshAccessState::Revoked,
            _ => SshAccessState::Requested,
        };

        SshAccessRequest {
            id: self.id,
            conversation_id: self.conversation_id,
            node_id: self.node_id,
            node_name: self.node_name,
            minutes: self.minutes,
            reason: self.reason,
            state,
            created_at: self.created_at,
            decided_at: self.decided_at,
            expires_at: self.expires_at,
        }
    }
}

fn relays() -> MutexGuard<'static, HashMap<Uuid, Relay>> {
    RELAYS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn forward(node_id: Uuid, session_id: Uuid, toward: Toward) {
    let registry = relays();

    if let Some(relay) = registry.get(&session_id).filter(|relay| relay.node_id == node_id) {
        let _ = relay.to_agent.send(toward);
    }
}

#[derive(sqlx::FromRow)]
struct AgentGrant {
    id: Uuid,
    owner: Uuid,
    conversation_id: Uuid,
    node_id: Uuid,
    node_name: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<RequestRow> {
    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from {REQUEST_SOURCE} where r.id = $1 and r.owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That request"))
}

fn publish(state: &AppState, owner: Uuid, row: RequestRow) -> SshAccessRequest {
    let request = row.to_proto();
    state.publish(Some(owner), StreamMessage::SshAccessRequest(request.clone()));

    request
}

pub async fn request(state: &AppState, conversation_id: Uuid, node: &str, minutes: Option<i64>, reason: &str) -> ApiResult<Result<SshAccessRequest, String>> {
    let minutes = minutes.unwrap_or(DEFAULT_MINUTES);

    if !(MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        return Ok(Err(format!("Nothing was requested. minutes is {MIN_MINUTES} to {MAX_MINUTES}.")));
    }

    let node = node.trim();
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let candidates = sqlx::query_scalar::<_, Uuid>("select id from nodes where created_by = $1 and revoked_at is null and (id::text = $2 or lower(name) = lower($2))").bind(owner).bind(node).fetch_all(&state.pool).await?;

    let node_id = match candidates.as_slice() {
        [] => return Ok(Err(format!("Nothing was requested. There is no node called {node}. Give a node's name or id."))),
        [only] => *only,
        _ => return Ok(Err(format!("Nothing was requested. More than one node is called {node}. Give its id."))),
    };

    let id = sqlx::query_scalar::<_, Uuid>("insert into ssh_access_requests (owner, conversation_id, node_id, minutes, reason) values ($1, $2, $3, $4, $5) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(node_id)
        .bind(minutes as i32)
        .bind(crate::store::text_without_nul(reason))
        .fetch_one(&state.pool)
        .await?;

    Ok(Ok(publish(state, owner, find_owned(state, id, owner).await?)))
}

pub async fn mint_for_person(State(state): State<AppState>, user: AuthUser, Path(node_id): Path<Uuid>) -> ApiResult<Json<SshToken>> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    let secret = new_token();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(PERSON_TOKEN_MINUTES);
    sqlx::query("delete from person_ssh_tokens where expires_at < now()").execute(&state.pool).await?;
    sqlx::query("insert into person_ssh_tokens (owner, node_id, token_hash, expires_at) values ($1, $2, $3, $4)").bind(user.id).bind(node_id).bind(hash_token(&secret)).bind(expires_at).execute(&state.pool).await?;

    Ok(Json(SshToken { token: format!("{TOKEN_PREFIX}{secret}"), expires_at }))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<SshAccessRequest>>> {
    let rows = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from {REQUEST_SOURCE} where r.owner = $1 order by (r.state = 'requested') desc, r.created_at desc limit {LISTED}")))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows.into_iter().map(RequestRow::to_proto).collect()))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(body): Json<ApproveSshAccessRequest>) -> ApiResult<Json<SshAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    token_delivery::require_requested(&found.state)?;

    let minutes = body.minutes.unwrap_or(found.minutes);

    if minutes < MIN_MINUTES as i32 || minutes > found.minutes {
        return Err(ApiError::bad_request("minutes_out_of_range", format!("Choose between {MIN_MINUTES} and {} minutes.", found.minutes)));
    }

    let node_id = token_delivery::node_of(&state, found.conversation_id).await?;

    let secret = new_token();
    let claimed = sqlx::query("update ssh_access_requests set state = 'approved', decided_at = now(), minutes = $2, expires_at = now() + make_interval(mins => $2::int), token_hash = $3 where id = $1 and state = 'requested'")
        .bind(id)
        .bind(minutes)
        .bind(hash_token(&secret))
        .execute(&state.pool)
        .await?;

    if claimed.rows_affected() == 0 {
        return Err(token_delivery::already_decided());
    }

    let sent = token_delivery::send_token(&state, node_id, ControlToNode::SshToken { request_id: id, conversation_id: found.conversation_id, token: format!("{TOKEN_PREFIX}{secret}") });

    if let Err(cause) = sent {
        sqlx::query("update ssh_access_requests set state = 'requested', decided_at = null, expires_at = null, token_hash = null, minutes = $2 where id = $1").bind(id).bind(found.minutes).execute(&state.pool).await?;

        return Err(cause);
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<SshAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    token_delivery::require_requested(&found.state)?;

    sqlx::query("update ssh_access_requests set state = 'declined', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    prompts::tell_agent(&state, user.id, found.conversation_id, format!("I declined your request for SSH access to {}. Do not get access to that node another way.", found.node_name)).await;

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn revoke(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<SshAccessRequest>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "approved" {
        return Err(ApiError::conflict("request_not_approved", "Only approved access can be revoked."));
    }

    let revoked = sqlx::query("update ssh_access_requests set state = 'revoked', revoked_at = now() where id = $1 and state = 'approved'").bind(id).execute(&state.pool).await?;

    if revoked.rows_affected() == 0 {
        return Err(ApiError::conflict("request_not_approved", "Only approved access can be revoked."));
    }

    for relay in relays().values().filter(|relay| relay.request_id == id) {
        let _ = relay.to_agent.send(Toward::Cut);
    }

    prompts::tell_agent(&state, user.id, found.conversation_id, format!("I revoked your SSH access to {}. Its sessions have ended. Do not get access to that node another way.", found.node_name)).await;

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn token_saved(state: &AppState, node_id: Uuid, request_id: Uuid, path: Option<String>) -> ApiResult<()> {
    let delivered = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, Option<chrono::DateTime<chrono::Utc>>)>(
        "select r.owner, r.conversation_id, r.node_id, n.name, r.expires_at
         from ssh_access_requests r join nodes n on n.id = r.node_id join conversations c on c.id = r.conversation_id and c.node_id = $2
         where r.id = $1 and r.state = 'approved' and r.revoked_at is null",
    )
    .bind(request_id)
    .bind(node_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((owner, conversation_id, target_id, node_name, expires_at)) = delivered else {
        return Ok(());
    };

    let text = match path {
        Some(path) => {
            let directory = std::path::Path::new(&path).parent().map(|parent| parent.display().to_string()).unwrap_or_default();
            let expires = expires_at.unwrap_or_else(chrono::Utc::now);

            format!(
                "I approved SSH access to node {node_name} until {} UTC. Connect with: ssh -o ProxyCommand='mycelium ssh-proxy {target_id} --token-file {path}' -o UserKnownHostsFile={directory}/known_hosts -o StrictHostKeyChecking=accept-new mycelium@{target_id} The token is in the file {path}. Never copy it into a message, a file or a commit. Sessions end at that time, or sooner if I revoke it.",
                expires.format("%Y-%m-%d %H:%M")
            )
        }
        None => {
            sqlx::query("update ssh_access_requests set state = 'revoked', revoked_at = now() where id = $1").bind(request_id).execute(&state.pool).await?;
            publish(state, owner, find_owned(state, request_id, owner).await?);

            format!("I approved SSH access to {node_name}, but the node could not save the token, so there is none. Ask again if you still need it.")
        }
    };

    prompts::tell_agent(state, owner, conversation_id, text).await;

    Ok(())
}

pub fn node_data(node_id: Uuid, session_id: Uuid, data: String) -> ApiResult<()> {
    if let Ok(bytes) = STANDARD.decode(data) {
        forward(node_id, session_id, Toward::Data(bytes));
    }

    Ok(())
}

pub fn node_closed(node_id: Uuid, session_id: Uuid) -> ApiResult<()> {
    forward(node_id, session_id, Toward::ClosedByNode);

    Ok(())
}

pub async fn connect(State(state): State<AppState>, Path(node): Path<String>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    let admitted = async {
        let secret = headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).and_then(|value| value.strip_prefix(TOKEN_PREFIX)).ok_or_else(ApiError::unauthorised)?;
        let grant = sqlx::query_as::<_, AgentGrant>(
            "select r.id, r.owner, r.conversation_id, r.node_id, n.name as node_name, r.expires_at
             from ssh_access_requests r join nodes n on n.id = r.node_id
             where r.token_hash = $1 and r.state = 'approved' and r.revoked_at is null and r.expires_at > now()",
        )
        .bind(hash_token(secret))
        .fetch_optional(&state.pool)
        .await?;
        let person = match grant {
            Some(_) => None,
            None => sqlx::query_as::<_, (Uuid, Uuid, String)>("select t.id, t.node_id, n.name from person_ssh_tokens t join nodes n on n.id = t.node_id where t.token_hash = $1 and t.expires_at > now()").bind(hash_token(secret)).fetch_optional(&state.pool).await?,
        };
        let (request_id, agent_grant, target, target_name, expires_at) = match (grant, person) {
            (Some(AgentGrant { id: request_id, owner, conversation_id, node_id: target, node_name: target_name, expires_at }), _) => (request_id, Some((owner, conversation_id)), target, target_name, expires_at),
            (None, Some((token_id, target, target_name))) => (token_id, None, target, target_name, chrono::Utc::now() + chrono::Duration::hours(PERSON_SESSION_HOURS)),
            (None, None) => return Err(ApiError::unauthorised()),
        };

        if node != target.to_string() && !node.eq_ignore_ascii_case(&target_name) {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "ssh_wrong_node", "This access covers another node."));
        }

        if !state.hub.is_online(target) {
            return Err(ApiError::conflict("node_offline", "The node is offline, so there is nothing to connect to."));
        }

        let session_id = Uuid::new_v4();

        if let Some((owner, conversation_id)) = agent_grant {
            sqlx::query("insert into ssh_sessions (id, request_id, owner, conversation_id, node_id) values ($1, $2, $3, $4, $5)").bind(session_id).bind(request_id).bind(owner).bind(conversation_id).bind(target).execute(&state.pool).await?;
            tracing::info!("ssh session {session_id} started under request {request_id} from conversation {conversation_id} to node {target}");
        } else {
            tracing::info!("ssh session {session_id} started under a person's token {request_id} to node {target}");
        }

        let (to_agent, from_node) = mpsc::unbounded_channel::<Toward>();
        relays().insert(session_id, Relay { node_id: target, request_id, to_agent });

        if !state.hub.send(target, ControlToNode::SshOpen { session_id }) {
            relays().remove(&session_id);
            sqlx::query("update ssh_sessions set ended_at = now() where id = $1").bind(session_id).execute(&state.pool).await?;

            return Err(ApiError::conflict("node_offline", "The node is offline, so there is nothing to connect to."));
        }

        Ok((session_id, target, expires_at, from_node))
    }
    .await;

    let (session_id, target, expires_at, mut from_node) = match admitted {
        Ok(admitted) => admitted,
        Err(error) => return error.into_response(),
    };

    upgrade.on_upgrade(move |mut socket| async move {
        let remaining = (expires_at - chrono::Utc::now()).to_std().unwrap_or_default();
        let deadline = sleep_until(Instant::now() + remaining);
        tokio::pin!(deadline);
        let mut watch = interval_at(Instant::now() + NODE_WATCH, NODE_WATCH);
        let mut closed_by_node = false;

        loop {
            tokio::select! {
                frame = socket.recv() => match frame {
                    Some(Ok(Message::Binary(bytes))) => {
                        let data = STANDARD.encode(&bytes);

                        if !state.hub.send(target, ControlToNode::SshData { session_id, data }) {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => {}
                },
                toward = from_node.recv() => match toward {
                    Some(Toward::Data(bytes)) => {
                        if socket.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Toward::ClosedByNode) => {
                        closed_by_node = true;
                        break;
                    }
                    Some(Toward::Cut) | None => break,
                },
                _ = &mut deadline => break,
                _ = watch.tick() => {
                    if !state.hub.is_online(target) {
                        break;
                    }
                }
            }
        }

        relays().remove(&session_id);

        if !closed_by_node {
            state.hub.send(target, ControlToNode::SshClose { session_id });
        }

        let _ = socket.send(Message::Close(None)).await;
        let ended = sqlx::query("update ssh_sessions set ended_at = now() where id = $1").bind(session_id).execute(&state.pool).await;

        if let Err(error) = ended {
            tracing::warn!("could not record the end of ssh session {session_id}: {error}");
        }
    })
}
