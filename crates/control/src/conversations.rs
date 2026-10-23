use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::instructions;
use crate::nodes::node_offline;
use crate::projects;
use crate::state::AppState;
use crate::worktrees;
use crate::store::{CONVERSATION_COLUMNS, enum_text, find_owned, lock_owned, parse_enum, publish_queue, ConversationRow};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateConversationRequest, EventsQuery, PatchConversationRequest};
use mycelium_proto::conversation::{AgentId, Conversation, ConversationStatus, Event};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

const TITLE_LIMIT: usize = 60;

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    archived: bool,
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Conversation>>> {
    let rows = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "select {CONVERSATION_COLUMNS}
         from conversations where owner = $1 and (archived_at is not null) = $2 order by updated_at desc",
    )),
    )
    .bind(user.id)
    .bind(query.archived)
    .fetch_all(&state.pool)
    .await?;

    let mut conversations = Vec::new();

    for row in rows {
        conversations.push(row.to_proto()?);
    }

    Ok(Json(conversations))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateConversationRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let conversation = start(&state, &user, request).await?;

    Ok((StatusCode::CREATED, Json(conversation)))
}

pub async fn start(state: &AppState, user: &AuthUser, request: CreateConversationRequest) -> ApiResult<Conversation> {
    let project = match request.project_id {
        Some(project_id) => Some(projects::find_owned(state, project_id, user.id).await?),
        None => None,
    };

    if project.as_ref().is_some_and(|project| project.node_id != request.node_id) {
        return Err(ApiError::bad_request("project_not_on_node", "This project is not set up on that node. Choose a node it is set up on."));
    }

    if project.is_none() && request.branch.is_some() {
        return Err(ApiError::bad_request("project_required", "Choose a project to start a worktree from."));
    }

    let mut cwd = match &project {
        Some(project) => project.path.clone(),
        None => request.cwd.trim().to_string(),
    };

    if cwd.is_empty() {
        return Err(ApiError::bad_request("cwd_required", "Enter the folder the agent should work in."));
    }

    let draining = sqlx::query_scalar::<_, bool>("select draining from nodes where id = $1 and revoked_at is null").bind(request.node_id).fetch_optional(&state.pool).await?;

    match draining {
        None => return Err(ApiError::not_found("That node")),
        Some(true) => return Err(ApiError::conflict("node_draining", "This node is finishing its running conversations and is not starting new ones. Choose another node.")),
        Some(false) => {}
    }

    if !state.hub.is_online(request.node_id) {
        return Err(node_offline());
    }

    let available = sqlx::query_scalar::<_, bool>("select available from agents where node_id = $1 and agent = $2").bind(request.node_id).bind(enum_text(&request.agent)).fetch_optional(&state.pool).await?;

    if available == Some(false) {
        return Err(ApiError::conflict("agent_unavailable", format!("{} is not installed on this node. Install it or choose another agent.", enum_text(&request.agent))));
    }

    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty());

    if let Some(account) = account {
        require_signed_in_account(state, request.node_id, request.agent, account).await?;
    }

    if let (Some(project), Some(branch)) = (&project, &request.branch) {
        cwd = worktrees::create(state, project, branch, None).await?.path;
    }

    let prompt = request.prompt.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let title = match (request.title.as_deref().map(str::trim).filter(|title| !title.is_empty()), prompt) {
        (Some(title), _) => title.to_string(),
        (None, Some(prompt)) => prompt.lines().next().unwrap_or_default().chars().take(TITLE_LIMIT).collect(),
        (None, None) => "New conversation".to_string(),
    };
    let status = if prompt.is_some() { ConversationStatus::Working } else { ConversationStatus::Done };

    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "insert into conversations (owner, node_id, agent, cwd, title, status, account, project_id) values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning {CONVERSATION_COLUMNS}",
    )),
    )
    .bind(user.id)
    .bind(request.node_id)
    .bind(enum_text(&request.agent))
    .bind(&cwd)
    .bind(&title)
    .bind(enum_text(&status))
    .bind(account)
    .bind(project.as_ref().map(|project| project.id))
    .fetch_one(&mut *tx)
    .await?;

    if let Some(prompt) = prompt {
        sqlx::query("insert into queued_prompts (conversation_id, position, text) values ($1, 1, $2)").bind(row.id).bind(prompt).execute(&mut *tx).await?;
    }

    tx.commit().await?;

    let instructions = instructions::session_layer(state, row.id).await?;
    let started = state.hub.send(request.node_id, ControlToNode::StartSession { conversation_id: row.id, agent: request.agent, cwd, resume: None, account: account.map(str::to_string), instructions });

    if !started {
        sqlx::query("delete from conversations where id = $1").bind(row.id).execute(&state.pool).await?;

        return Err(node_offline());
    }

    let conversation = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));
    publish_queue(state, user.id, row.id).await?;

    Ok(conversation)
}

async fn require_signed_in_account(state: &AppState, node_id: Uuid, agent: AgentId, account: &str) -> ApiResult<()> {
    let signed_in = sqlx::query_scalar::<_, bool>("select exists (select 1 from logins where node_id = $1 and agent = $2 and account = $3 and state = 'signed_in')")
        .bind(node_id)
        .bind(enum_text(&agent))
        .bind(account)
        .fetch_one(&state.pool)
        .await?;

    if !signed_in {
        return Err(ApiError::conflict("account_not_signed_in", format!("{account} is not signed in on this node. Sign in to it there, or choose another account.")));
    }

    Ok(())
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Conversation>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    Ok(Json(row.to_proto()?))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchConversationRequest>) -> ApiResult<Json<Conversation>> {
    let title = request.title.as_deref().map(str::trim);

    if title == Some("") {
        return Err(ApiError::bad_request("title_required", "Enter a title for this conversation."));
    }

    let mut tx = state.pool.begin().await?;
    let before = lock_owned(&mut tx, id, user.id).await?;
    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty());
    let use_default = request.use_default_account == Some(true);

    if use_default && before.acp_session_id.is_some() && before.account.is_some() {
        return Err(ApiError::conflict("account_locked", "This conversation has already started with its account. Start a new conversation to use another one."));
    }

    if let Some(account) = account {
        if before.acp_session_id.is_some() && before.account.as_deref() != Some(account) {
            return Err(ApiError::conflict("account_locked", "This conversation has already started with its account. Start a new conversation to use another one."));
        }

        require_signed_in_account(&state, before.node_id, parse_enum(&before.agent)?, account).await?;
    }

    let row = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "update conversations set title = coalesce($2, title), account = case when $5 then null else coalesce($4, account) end,
             archived_at = case when $3::bool is null then archived_at when $3 then coalesce(archived_at, now()) else null end
         where id = $1
         returning {CONVERSATION_COLUMNS}",
    )),
    )
    .bind(id)
    .bind(title)
    .bind(request.archived)
    .bind(account)
    .bind(use_default)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    if request.archived == Some(true) && before.is_idle() {
        state.hub.send(row.node_id, ControlToNode::CloseSession { conversation_id: id });
        state.remove_session(row.node_id, id).await?;
    }

    let conversation = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));

    Ok(Json(conversation))
}

pub async fn opened(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, user.id).await?;

    if row.status != enum_text(&ConversationStatus::Review) {
        return Ok(StatusCode::NO_CONTENT);
    }

    let updated = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set status = $2 where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .bind(enum_text(&ConversationStatus::Done))
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    state.publish(Some(user.id), StreamMessage::Conversation(updated.to_proto()?));

    Ok(StatusCode::NO_CONTENT)
}

pub async fn events(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<EventsQuery>) -> ApiResult<Json<Vec<Event>>> {
    find_owned(&state.pool, id, user.id).await?;

    let rows = sqlx::query_as::<_, (i64, String, Value)>("select seq, kind, payload from events where conversation_id = $1 and seq > $2 order by seq limit $3")
        .bind(id)
        .bind(query.after)
        .bind(query.limit.clamp(1, 1000))
        .fetch_all(&state.pool)
        .await?;

    let mut events = Vec::new();

    for (seq, kind, payload) in rows {
        events.push(Event { conversation_id: id, seq, kind: parse_enum(&kind)?, payload });
    }

    Ok(Json(events))
}

#[derive(Deserialize)]
pub struct FilesQuery {
    #[serde(default)]
    query: String,
}

pub async fn files(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<FilesQuery>) -> ApiResult<Json<Vec<String>>> {
    let row = find_owned(&state.pool, id, user.id).await?;
    let Some((request_id, answered)) = state.hub.request_files(row.node_id, row.cwd, query.query) else {
        return Err(node_offline());
    };

    let paths = tokio::time::timeout(FILES_TIMEOUT, answered).await.ok().and_then(Result::ok).unwrap_or_default();
    state.hub.forget_file_request(request_id);

    Ok(Json(paths))
}

const FILES_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
