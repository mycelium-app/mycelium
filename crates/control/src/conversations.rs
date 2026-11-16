use crate::assistants;
use crate::auth::AuthUser;
use crate::chats;
use crate::continuation;
use crate::conversation_connections;
use crate::group_connections;
use crate::error::{ApiError, ApiResult};
use crate::idempotency;
use crate::instructions;
use crate::model_providers;
use crate::git::{pull_requests_of, relay};
use crate::nodes::{HEAD_AND_DIRECTORIES_SINCE, node_has, node_offline};
use crate::projects;
use crate::state::AppState;
use crate::worktrees;
use crate::store::{CONVERSATION_COLUMNS, append_event, enum_text, find_owned, lock_owned, parse_enum, publish_queue, set_status, ConversationRow};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateChatRequest, CreateConversationRequest, EventsQuery, PatchConversationRequest, StartSideChatRequest};
use mycelium_proto::conversation::{AgentId, Conversation, ConversationStatus, Event, EventKind, SideKind};
use mycelium_proto::link::{ControlToNode, GitCommand};
use mycelium_proto::routing::ANY_NODE;
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const TITLE_LIMIT: usize = 60;

fn forks_natively(parent: &ConversationRow) -> bool {
    parent.agent == enum_text(&AgentId::Claude) && parent.acp_session_id.is_some()
}

pub fn derived_title(text: &str, attachments: &[mycelium_proto::conversation::Attachment]) -> String {
    let first_line = text.lines().next().unwrap_or_default();
    let fallback = attachments.first().map(|attachment| attachment.name.as_str()).unwrap_or_default();

    let shown = if first_line.is_empty() { fallback } else { first_line };

    shown.chars().take(TITLE_LIMIT).collect()
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    archived: bool,
    view: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
    parent: Option<Uuid>,
    assistant: Option<Uuid>,
    #[serde(default)]
    managed: bool,
    q: Option<String>,
    ids: Option<String>,
    status: Option<String>,
}

const SIDEBAR_PAGE: i64 = 50;
const ARCHIVE_PAGE: i64 = 100;
const ARCHIVED_ROW: &str = "owner = $1 and archived_at is not null and side_kind is distinct from 'check'";
const LARGEST_PAGE: i64 = 200;
const NESTING_DEPTH: usize = 6;
const SIDEBAR_ROW: &str = "owner = $1 and archived_at is null and side_kind is distinct from 'check' and side_kind is distinct from 'subagent' and assistant_id is null and managed_by is null
     and (parent_id is null or not exists (select 1 from conversations shown_parent where shown_parent.id = conversations.parent_id and shown_parent.archived_at is null))";
const NEEDS_ATTENTION: &str = "owner = $1 and archived_at is null and side_kind is distinct from 'check'
     and (pinned_at is not null or flagged_at is not null or status in ('input', 'working') or resume_at is not null or snoozed_until is not null or conversation_action_items(id) <> '[]'::jsonb)";

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Value>> {
    if query.view.as_deref() == Some("sidebar") {
        return Ok(Json(sidebar_page(&state, user.id, &query).await?));
    }

    if query.view.as_deref() == Some("archive") {
        let (rows, next) = page_of(&state, user.id, ARCHIVED_ROW, &query, ARCHIVE_PAGE).await?;

        return Ok(Json(json!({"conversations": protos(rows)?, "next": next})));
    }

    let filtered = query.parent.is_some() || query.assistant.is_some() || query.managed || query.q.is_some() || query.ids.is_some() || query.status.is_some();

    if filtered {
        return Ok(Json(json!({"conversations": filtered_list(&state, user.id, &query).await?})));
    }

    let rows = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "select {CONVERSATION_COLUMNS}
         from conversations where owner = $1 and (archived_at is not null) = $2 and side_kind is distinct from 'check' order by updated_at desc",
    )),
    )
    .bind(user.id)
    .bind(query.archived)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(serde_json::to_value(protos(rows)?).map_err(ApiError::internal)?))
}

fn protos(rows: Vec<ConversationRow>) -> ApiResult<Vec<Conversation>> {
    rows.into_iter().map(|row| row.to_proto()).collect()
}

fn parse_cursor(cursor: &str) -> Option<(chrono::DateTime<chrono::Utc>, Uuid)> {
    let (at, id) = cursor.split_once('_')?;

    Some((chrono::DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&chrono::Utc), id.parse().ok()?))
}

fn cursor_of(query: &ListQuery) -> ApiResult<Option<(chrono::DateTime<chrono::Utc>, Uuid)>> {
    match query.cursor.as_deref() {
        Some(text) => Ok(Some(parse_cursor(text).ok_or_else(|| ApiError::bad_request("cursor_invalid", "Give the cursor exactly as the previous page returned it."))?)),
        None => Ok(None),
    }
}

async fn page_ids(state: &AppState, owner: Uuid, rows: &str, query: &ListQuery, default_limit: i64) -> ApiResult<(Vec<Uuid>, Option<String>)> {
    let limit = query.limit.unwrap_or(default_limit).clamp(1, LARGEST_PAGE);
    let cursor = cursor_of(query)?;
    let page = sqlx::query_as::<_, (Uuid, chrono::DateTime<chrono::Utc>)>(sqlx::AssertSqlSafe(format!(
        "select id, updated_at from conversations where {rows}
         and ($2::timestamptz is null or (updated_at, id) < ($2, $3)) order by updated_at desc, id desc limit $4"
    )))
    .bind(owner)
    .bind(cursor.map(|(at, _)| at))
    .bind(cursor.map(|(_, id)| id))
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;
    let more = page.len() as i64 > limit;
    let page: Vec<_> = page.into_iter().take(limit as usize).collect();
    let next = page.last().filter(|_| more).map(|(id, at)| format!("{}_{id}", at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)));

    Ok((page.into_iter().map(|(id, _)| id).collect(), next))
}

async fn page_of(state: &AppState, owner: Uuid, rows: &str, query: &ListQuery, default_limit: i64) -> ApiResult<(Vec<ConversationRow>, Option<String>)> {
    let (ids, next) = page_ids(state, owner, rows, query, default_limit).await?;
    let rows = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where owner = $1 and id = any($2) order by updated_at desc, id desc"))).bind(owner).bind(&ids).fetch_all(&state.pool).await?;

    Ok((rows, next))
}

async fn sidebar_page(state: &AppState, owner: Uuid, query: &ListQuery) -> ApiResult<Value> {
    let cursor = cursor_of(query)?;
    let (mut ids, next) = page_ids(state, owner, SIDEBAR_ROW, query, SIDEBAR_PAGE).await?;

    if cursor.is_none() {
        let attention = sqlx::query_scalar::<_, Uuid>(sqlx::AssertSqlSafe(format!("select id from conversations where {NEEDS_ATTENTION}"))).bind(owner).fetch_all(&state.pool).await?;
        let assistants = sqlx::query_scalar::<_, Uuid>("select conversation_id from assistants where owner = $1 and conversation_id is not null").bind(owner).fetch_all(&state.pool).await?;
        ids.extend(attention);
        ids.extend(assistants);
    }

    ids = with_relatives(state, owner, ids).await?;

    let rows = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where owner = $1 and id = any($2) order by updated_at desc"))).bind(owner).bind(&ids).fetch_all(&state.pool).await?;
    let mut answer = json!({"conversations": protos(rows)?, "next": next});

    if cursor.is_none() {
        let settled = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "select count(*) from conversations where {SIDEBAR_ROW} and status = 'done' and pinned_at is null and snoozed_until is null and resume_at is null and agent <> $2 and not scratch"
        )))
        .bind(owner)
        .bind(chats::CHAT_AGENT)
        .fetch_one(&state.pool)
        .await?;
        answer["counts"] = json!({"settled": settled});
    }

    Ok(answer)
}

async fn with_relatives(state: &AppState, owner: Uuid, mut ids: Vec<Uuid>) -> ApiResult<Vec<Uuid>> {
    ids.sort();
    ids.dedup();
    let mut frontier = ids.clone();

    for _ in 0..NESTING_DEPTH {
        let children = sqlx::query_scalar::<_, Uuid>("select id from conversations where owner = $1 and parent_id = any($2) and archived_at is null and side_kind is distinct from 'subagent' and side_kind is distinct from 'check' and not (id = any($3))")
            .bind(owner)
            .bind(&frontier)
            .bind(&ids)
            .fetch_all(&state.pool)
            .await?;

        if children.is_empty() {
            break;
        }

        ids.extend(children.iter().copied());
        frontier = children;
    }

    let mut frontier = ids.clone();

    for _ in 0..NESTING_DEPTH {
        let related = sqlx::query_scalar::<_, Uuid>("select distinct related from conversations, unnest(array[parent_id, managed_by]) related where owner = $1 and id = any($2) and related is not null and not (related = any($3))")
            .bind(owner)
            .bind(&frontier)
            .bind(&ids)
            .fetch_all(&state.pool)
            .await?;

        if related.is_empty() {
            break;
        }

        ids.extend(related.iter().copied());
        frontier = related;
    }

    Ok(ids)
}

async fn filtered_list(state: &AppState, owner: Uuid, query: &ListQuery) -> ApiResult<Vec<Conversation>> {
    let limit = query.limit.unwrap_or(SIDEBAR_PAGE).clamp(1, LARGEST_PAGE);
    let ids: Option<Vec<Uuid>> = query.ids.as_deref().map(|ids| ids.split(',').filter_map(|id| id.trim().parse().ok()).collect());
    let pattern = query.q.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(|text| format!("%{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let rows = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "select {CONVERSATION_COLUMNS} from conversations where owner = $1 and side_kind is distinct from 'check'
         and ($2::uuid is null or parent_id = $2) and ($3::uuid is null or assistant_id = $3) and (not $4 or managed_by is not null)
         and ($5::text is null or title ilike $5) and ($6::uuid[] is null or id = any($6))
         and ($6::uuid[] is not null or (archived_at is not null) = $7) and ($9::text is null or status = $9)
         order by updated_at desc limit $8"
    )))
    .bind(owner)
    .bind(query.parent)
    .bind(query.assistant)
    .bind(query.managed)
    .bind(pattern)
    .bind(ids)
    .bind(query.archived)
    .bind(limit)
    .bind(query.status.as_deref())
    .fetch_all(&state.pool)
    .await?;

    protos(rows)
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateConversationRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let entry = idempotency::enter(&state, user.id, request.client_id).await?;
    let claim = match entry {
        idempotency::Entry::Repeat(Some(first)) => return Ok((StatusCode::CREATED, Json(find_owned(&state.pool, first, user.id).await?.to_proto()?))),
        idempotency::Entry::Repeat(None) => return Err(ApiError::not_found("That conversation")),
        idempotency::Entry::Fresh(claim) => claim,
    };
    let conversation = start(&state, user.id, request).await?;

    if let Some(claim) = claim {
        claim.done(Some(conversation.id)).await?;
    }

    Ok((StatusCode::CREATED, Json(conversation)))
}

pub async fn start(state: &AppState, owner: Uuid, request: CreateConversationRequest) -> ApiResult<Conversation> {
    start_for(state, owner, request, None, false).await
}

pub async fn start_for(state: &AppState, owner: Uuid, mut request: CreateConversationRequest, assistant_id: Option<Uuid>, scratch: bool) -> ApiResult<Conversation> {
    let mut any_node_name = None;
    let project = match request.project_id {
        Some(project_id) => Some(projects::find_owned(state, project_id, owner).await?),
        None => None,
    };
    let any_node = request.node_id == ANY_NODE;

    if request.worktree_path.is_some() && request.branch.is_some() {
        return Err(ApiError::bad_request("worktree_or_branch", "Choose an existing worktree or a new branch, not both."));
    }

    if request.worktree_path.is_some() && any_node {
        return Err(ApiError::bad_request("node_required", "Choose the node the worktree is on."));
    }

    if any_node {
        let holding = project.as_ref().map(projects::ProjectRow::checkout_nodes);
        let picked = crate::node_pick::pick(state, owner, holding.as_deref()).await?;

        request.node_id = picked.node_id;
        any_node_name = Some(picked.name);
    }

    if project.as_ref().is_some_and(|project| project.checkout_on(request.node_id).is_none()) {
        return Err(projects::not_on_node());
    }

    if project.is_none() && (request.branch.is_some() || request.worktree_path.is_some()) {
        return Err(ApiError::bad_request("project_required", "Choose a project to start a worktree from."));
    }

    let scratch_id = (scratch && request.parent_id.is_none()).then(Uuid::new_v4);
    let mut cwd = match (&project, scratch_id) {
        (_, Some(id)) => assistants::folder_of(id),
        (Some(project), None) => project.checkout_on(request.node_id).map(|checkout| checkout.path.clone()).unwrap_or_default(),
        (None, None) => request.cwd.trim().to_string(),
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

    if let Some(routing) = request.routing {
        let picked = crate::routing::start_pick(state, owner, request.node_id, routing).await?;

        request.agent = picked.agent;
        request.account = Some(picked.account);
        request.model = request.model.take().or(picked.model);
    }

    let available = sqlx::query_scalar::<_, bool>("select available from agents where node_id = $1 and agent = $2").bind(request.node_id).bind(enum_text(&request.agent)).fetch_optional(&state.pool).await?;

    if available == Some(false) {
        return Err(ApiError::conflict("agent_unavailable", format!("{} is not installed on this node. Install it or choose another agent.", enum_text(&request.agent))));
    }

    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty());

    if let Some(account) = account {
        require_signed_in_account(state, request.node_id, request.agent, account).await?;
    }

    let mut worktree_branch = None;

    if let (Some(project), Some(branch)) = (&project, &request.branch) {
        let checkout = project.checkout_on(request.node_id).ok_or_else(projects::not_on_node)?;
        let worktree = worktrees::create(state, checkout, branch, None).await?;

        cwd = worktree.path;
        worktree_branch = Some(worktree.branch);
    }

    if let (Some(project), Some(path)) = (&project, &request.worktree_path) {
        let existing = worktrees::find_existing(state, project, request.node_id, path).await?;

        cwd = existing.path;
        worktree_branch = existing.branch;
    }

    if request.side_kind == Some(SideKind::Subagent) {
        return Err(ApiError::bad_request("parent_invalid", "A subagent chat is made by the agent that ran it."));
    }

    let parent = match request.parent_id {
        Some(parent_id) => Some(find_owned(&state.pool, parent_id, owner).await?),
        None => None,
    };

    if parent.as_ref().is_some_and(|parent| parent.node_id != request.node_id) || parent.is_some() != request.side_kind.is_some() {
        return Err(ApiError::bad_request("parent_invalid", "A side chat runs on the same node as the conversation it belongs to."));
    }

    let parent_project = match &parent {
        Some(parent) => sqlx::query_scalar::<_, Option<Uuid>>("select project_id from conversations where id = $1").bind(parent.id).fetch_one(&state.pool).await?,
        None => None,
    };
    let fork_of = match (&parent, request.side_kind) {
        (Some(parent), Some(SideKind::Fork)) if forks_natively(parent) => parent.acp_session_id.clone(),
        _ => None,
    };
    let prompt = request.prompt.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let chosen_title = request.title.as_deref().map(str::trim).filter(|title| !title.is_empty());
    let predicted_title = prompt.map(|text| derived_title(text, &[]));
    let title = match (chosen_title, &predicted_title) {
        (Some(title), _) => title.to_string(),
        (None, Some(predicted)) => predicted.clone(),
        (None, None) => "New conversation".to_string(),
    };
    let title_user_set = chosen_title.is_some() && parent.is_none();
    let status = if prompt.is_some() { ConversationStatus::Working } else { ConversationStatus::Done };

    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "insert into conversations (owner, node_id, agent, cwd, title, status, account, project_id, title_user_set, predicted_title, parent_id, side_kind, branch, assistant_id, model, routing, id, scratch, options, mode) values ($1, $2, $3, $4, $5, $6, $7, coalesce($8, project_for_folder($1, $2, $4)), $9, $10, $11, $12, $13, $14, $15, $16, coalesce($17, gen_random_uuid()), $18, $19, $20)
         returning {CONVERSATION_COLUMNS}",
    )),
    )
    .bind(owner)
    .bind(request.node_id)
    .bind(enum_text(&request.agent))
    .bind(&cwd)
    .bind(&title)
    .bind(enum_text(&status))
    .bind(account)
    .bind(project.as_ref().map(|project| project.id).or(parent_project))
    .bind(title_user_set)
    .bind(&predicted_title)
    .bind(request.parent_id)
    .bind(request.side_kind.as_ref().map(enum_text))
    .bind(&worktree_branch)
    .bind(assistant_id)
    .bind(request.model.as_deref().map(str::trim).filter(|model| !model.is_empty()))
    .bind(request.routing.map(|routing| routing.as_str()))
    .bind(scratch_id)
    .bind(scratch)
    .bind(request.config.as_ref().map(|config| json!({"initial_config": config})))
    .bind(&request.mode)
    .fetch_one(&mut *tx)
    .await?;

    let mut started_event = None;

    if scratch_id.is_some() {
        let node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(request.node_id).fetch_one(&mut *tx).await?;

        started_event = append_event(&mut tx, row.id, EventKind::Continued, &json!({"kind": "scratch", "node_name": node_name}), None).await?;
    }

    if let Some(node_name) = any_node_name {
        started_event = append_event(&mut tx, row.id, EventKind::Continued, &json!({"kind": "any_node", "node_name": node_name}), None).await?;
    }

    if let Some(prompt) = prompt {
        sqlx::query("insert into queued_prompts (conversation_id, position, text) values ($1, 1, $2)").bind(row.id).bind(prompt).execute(&mut *tx).await?;
    }

    tx.commit().await?;

    if let Some(assistant_id) = assistant_id {
        assistants::copy_down(state, request.node_id, assistant_id).await?;
    }

    if scratch_id.is_some() && !state.hub.send(request.node_id, ControlToNode::WriteFolder { root: cwd.clone(), files: Vec::new(), replace: true }) {
        sqlx::query("delete from conversations where id = $1").bind(row.id).execute(&state.pool).await?;

        return Err(node_offline());
    }

    let instructions = instructions::session_layer(state, row.id).await?;
    let env = assistants::secret_env(state, row.id).await?;
    let mcp_token = crate::mcp_tokens::mint(state, request.node_id, row.id).await?;
    let started = state.hub.send(request.node_id, ControlToNode::StartSession { conversation_id: row.id, agent: request.agent, cwd, resume: fork_of.clone(), fork: fork_of.is_some(), account: account.map(str::to_string), instructions, reload: false, env, mcp_token });

    if !started {
        sqlx::query("delete from conversations where id = $1").bind(row.id).execute(&state.pool).await?;

        return Err(node_offline());
    }

    let conversation = row.to_proto()?;
    state.publish(Some(owner), StreamMessage::Conversation(conversation.clone()));

    if let Some(event) = started_event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    publish_queue(state, owner, row.id).await?;

    Ok(conversation)
}

pub async fn side(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<StartSideChatRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let parent = find_owned(&state.pool, id, user.id).await?;
    let text = request.text.as_deref().map(str::trim).filter(|text| !text.is_empty());

    if request.kind == SideKind::Subagent {
        return Err(ApiError::bad_request("parent_invalid", "A subagent chat is made by the agent that ran it."));
    }

    if chats::is_chat(&parent) {
        let Some(provider_id) = parent.provider_id else {
            return Err(ApiError::conflict("provider_removed", "This chat's model provider was removed. Choose another model in the message box to carry on."));
        };
        let started = CreateChatRequest { provider_id: Some(provider_id), agent: None, account: None, node_id: None, model: parent.model.clone(), prompt: text.map(str::to_string), parent_id: Some(id), client_id: None };
        let conversation = chats::start(&state, user.id, started, Some(request.kind), parent.assistant_id).await?;

        return Ok((StatusCode::CREATED, Json(conversation)));
    }

    if request.kind == SideKind::Btw && text.is_none() {
        return Err(ApiError::bad_request("text_required", "Enter a question to ask."));
    }

    let summary = if request.kind == SideKind::Fork && !forks_natively(&parent) {
        let lines = continuation::recent_turns(&state, id, i64::MAX, continuation::SUMMARY_MESSAGES).await?;

        Some(format!("Continuing from: {}\n\nThis conversation continues an earlier one. This is what was said most recently:\n\n{}\n\nContinue from here.", parent.title, lines.join("\n\n")))
    } else {
        None
    };

    let prompt = match (summary, text) {
        (Some(summary), Some(text)) => Some(format!("{summary}\n\n{text}")),
        (Some(summary), None) => Some(summary),
        (None, text) => text.map(str::to_string),
    };
    let title = match (prompt.is_some(), request.kind) {
        (true, _) => None,
        (false, SideKind::Fork) => Some(format!("{} (fork)", parent.title)),
        (false, _) => Some("Side chat".to_string()),
    };
    let started = CreateConversationRequest {
        mode: None,
        config: None,
        node_id: parent.node_id,
        agent: parse_enum(&parent.agent)?,
        account: parent.account.clone(),
        cwd: parent.cwd.clone(),
        project_id: None,
        branch: None,
        worktree_path: None,
        title,
        prompt,
        parent_id: Some(id),
        side_kind: Some(request.kind),
        model: None,
        routing: None,
        client_id: None,
    };
    let conversation = start_for(&state, user.id, started, parent.assistant_id, parent.scratch).await?;

    Ok((StatusCode::CREATED, Json(conversation)))
}

pub async fn scratch_folder_in_use(state: &AppState, node_id: Uuid, folder: &str) -> ApiResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>("select exists (select 1 from conversations where node_id = $1 and cwd = $2 and scratch)").bind(node_id).bind(folder).fetch_one(&state.pool).await?)
}

pub async fn require_signed_in_account(state: &AppState, node_id: Uuid, agent: AgentId, account: &str) -> ApiResult<()> {
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

pub async fn resolve_action_item(State(state): State<AppState>, user: AuthUser, Path((id, item_id)): Path<(Uuid, Uuid)>) -> ApiResult<Json<Conversation>> {
    let Some(row) = crate::store::resolve_action_item(&state.pool, id, item_id, user.id).await? else {
        return Err(ApiError::not_found("That action item"));
    };
    let conversation = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));

    Ok(Json(conversation))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchConversationRequest>) -> ApiResult<Json<Conversation>> {
    let title = request.title.as_deref().map(str::trim);

    if title == Some("") {
        return Err(ApiError::bad_request("title_required", "Enter a title for this conversation."));
    }

    if request.snoozed_until.is_some_and(|until| until <= chrono::Utc::now()) {
        return Err(ApiError::bad_request("snooze_in_the_past", "Choose a time in the future to snooze this conversation until."));
    }

    let adopter = match request.manage_by_assistant {
        Some(assistant_id) => Some(adopting_conversation(&state, user.id, assistant_id, id).await?),
        None => None,
    };

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

    let chosen_model = request.model.as_deref().map(str::trim);
    let mut switched_to = None;
    let mut interrupted = false;

    if request.provider_id.is_some() || chosen_model.is_some() {
        if !chats::is_chat(&before) {
            return Err(ApiError::bad_request("not_a_chat", "Only a chat can change its model."));
        }

        let Some(provider_id) = request.provider_id else {
            return Err(ApiError::bad_request("provider_required", "Choose a model provider for this chat."));
        };
        let provider = model_providers::find_owned(&state, provider_id, user.id).await?;
        let Some(model) = chosen_model.filter(|model| !model.is_empty()) else {
            return Err(ApiError::bad_request("model_required", "Choose a model for this chat."));
        };

        switched_to = Some((provider, model.to_string()));
        interrupted = crate::prompts::interrupt(&state, user.id, id).await?;
    }

    if (request.project_id.is_some() || request.clear_project == Some(true)) && chats::is_chat(&before) {
        return Err(ApiError::bad_request("not_a_workspace", "Only a workspace conversation belongs to a project."));
    }

    if let Some(project_id) = request.project_id {
        let project = projects::find_owned(&state, project_id, user.id).await?;

        if project.checkout_on(before.node_id).is_none() {
            return Err(projects::not_on_node());
        }
    }

    let switched_provider = switched_to.as_ref().map(|(provider, _)| provider.id);
    let switched_model = switched_to.as_ref().map(|(_, model)| model.as_str());
    let row = sqlx::query_as::<_, ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "update conversations set title = case when $6 then coalesce(predicted_title, title) else coalesce($2, title) end,
             title_user_set = case when $6 then false when $2::text is not null then true else title_user_set end,
             account = case when $5 then null else coalesce($4, account) end,
             provider_id = coalesce($7, provider_id), model = coalesce($8, model),
             archived_at = case when $3::bool is null then archived_at when $3 then coalesce(archived_at, now()) else null end,
             agent_active = case when $3::bool is true then false else agent_active end,
             pinned_at = case when $9::bool is null then pinned_at when $9 then coalesce(pinned_at, now()) else null end,
             project_id = case when $10 then null else coalesce($11, project_id) end,
             teach_me = coalesce($12, teach_me),
             voice_mode = coalesce($16, voice_mode),
             snoozed_until = case when $13 then null else coalesce($14, snoozed_until) end,
             managed_by = case when $15 then null else managed_by end,
             flagged_at = case when $15 or $17 then null else flagged_at end,
             flag_reason = case when $15 or $17 then null else flag_reason end,
             pull_request_label = case when $10 or $11 is not null then null else pull_request_label end,
             pull_request_url = case when $10 or $11 is not null then null else pull_request_url end,
             pull_request_state = case when $10 or $11 is not null then null else pull_request_state end,
             pull_request_number = case when $10 or $11 is not null then null else pull_request_number end
         where id = $1
         returning {CONVERSATION_COLUMNS}",
    )),
    )
    .bind(id)
    .bind(title)
    .bind(request.archived)
    .bind(account)
    .bind(use_default)
    .bind(request.clear_title == Some(true))
    .bind(switched_provider)
    .bind(switched_model)
    .bind(request.pinned)
    .bind(request.clear_project == Some(true))
    .bind(request.project_id)
    .bind(request.teach_me)
    .bind(request.clear_snooze == Some(true))
    .bind(request.snoozed_until)
    .bind(request.take_over == Some(true))
    .bind(request.voice_mode)
    .bind(request.clear_flag == Some(true))
    .fetch_one(&mut *tx)
    .await?;
    let teach_change = match request.teach_me {
        Some(on) if on != before.teach_me => append_event(&mut tx, id, EventKind::TeachMode, &json!({"on": on}), None).await?,
        _ => None,
    };
    let voice_change = match request.voice_mode {
        Some(on) if on != before.voice_mode => append_event(&mut tx, id, EventKind::VoiceMode, &json!({"on": on}), None).await?,
        _ => None,
    };
    let model_change = match &switched_to {
        Some((provider, model)) if before.provider_id != Some(provider.id) || before.model.as_deref() != Some(model.as_str()) => {
            append_event(&mut tx, id, EventKind::ModelChanged, &json!({"provider_id": provider.id, "provider_name": provider.name, "model": model}), None).await?
        }
        _ => None,
    };
    tx.commit().await?;

    for event in model_change.into_iter().chain(teach_change).chain(voice_change) {
        state.publish(Some(user.id), StreamMessage::Event(event));
    }

    if request.teach_me.is_some_and(|on| on != before.teach_me) {
        state.hub.mark_secrets_stale(id);
    }

    if let Some(on) = request.voice_mode.filter(|on| *on != before.voice_mode && !chats::is_chat(&row)) {
        state.hub.note_for_agent(id, if on { instructions::VOICE_MODE_INSTRUCTIONS } else { instructions::VOICE_MODE_OFF_NOTE }.to_string());
    }

    if title.is_some() && chats::is_chat(&row) {
        chats::index_title(&state, id, &row.title).await?;
    }

    if request.archived == Some(true) {
        put_away(&state, &before).await?;
    }

    let row = match adopter {
        Some(manager) => manage(&state, user.id, manager, id).await?,
        None => row,
    };
    let conversation = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));

    if request.project_id.is_some() || request.clear_project == Some(true) {
        spawn_branch_refresh(&state, id);
    }

    if interrupted {
        crate::prompts::pump(&state, id, false).await?;
    }

    Ok(Json(conversation))
}

async fn adopting_conversation(state: &AppState, owner: Uuid, assistant_id: Uuid, target: Uuid) -> ApiResult<Uuid> {
    let assistant = sqlx::query_as::<_, (String, Option<Uuid>)>("select name, conversation_id from assistants where id = $1 and owner = $2").bind(assistant_id).bind(owner).fetch_optional(&state.pool).await?;
    let Some((name, conversation_id)) = assistant else {
        return Err(ApiError::not_found("That assistant"));
    };
    let Some(conversation_id) = conversation_id else {
        return Err(ApiError::conflict("assistant_not_started", format!("{name} has no conversation yet. Start one with {name} first.")));
    };
    find_owned(&state.pool, target, owner).await?;

    if !group_connections::connected(state, owner, conversation_id, target).await? {
        return Err(ApiError::bad_request("assistant_not_in_group", format!("{name} is not in this conversation's group, or a group connected to it. Choose an assistant from this group.")));
    }

    Ok(conversation_id)
}

pub async fn manage(state: &AppState, owner: Uuid, manager: Uuid, target: Uuid) -> ApiResult<ConversationRow> {
    conversation_connections::connect(state, owner, manager, target).await?;

    set_manager(state, owner, manager, target).await
}

pub async fn set_manager(state: &AppState, owner: Uuid, manager: Uuid, target: Uuid) -> ApiResult<ConversationRow> {
    crate::watches::watch_conversation(state, owner, manager, target, false).await?;
    let managed = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set managed_by = $2 where id = $1 returning {CONVERSATION_COLUMNS}"))).bind(target).bind(manager).fetch_one(&state.pool).await?;

    Ok(managed)
}

pub async fn archive_for(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<ConversationRow> {
    let before = find_owned(&state.pool, id, owner).await?;
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set archived_at = coalesce(archived_at, now()), agent_active = false where id = $1 returning {CONVERSATION_COLUMNS}"
    )))
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    put_away(state, &before).await?;
    state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));

    Ok(row)
}

pub async fn put_away(state: &AppState, before: &ConversationRow) -> ApiResult<()> {
    state.stop_terminals_of_conversation(before.id);

    if before.is_idle() {
        state.hub.send(before.node_id, ControlToNode::CloseSession { conversation_id: before.id });
        state.remove_session(before.node_id, before.id).await?;
        crate::environments::stop_with_conversation(state, before.id, false).await;
    }

    Ok(())
}

pub async fn wake_regularly(state: AppState) {
    crate::boot::run_while_ready(&state, "wake the snoozed conversations", state.config.scheduler_tick, || wake_due(&state)).await;
}

async fn wake_due(state: &AppState) -> ApiResult<()> {
    let woken = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set snoozed_until = null where snoozed_until <= now() returning {CONVERSATION_COLUMNS}",
    )))
    .fetch_all(&state.pool)
    .await?;

    for row in woken {
        let owner = row.owner;
        state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(())
}

pub fn spawn_branch_refresh(state: &AppState, conversation_id: Uuid) {
    let state = state.clone();

    tokio::spawn(async move {
        if let Err(error) = refresh_branch(&state, conversation_id).await {
            tracing::warn!("recording the branch of {conversation_id} failed: {}", error.message());
        }
    });
}

async fn refresh_branch(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where id = $1"))).bind(conversation_id).fetch_optional(&state.pool).await?;
    let Some(row) = row else {
        return Ok(());
    };

    if chats::is_chat(&row) || !state.hub.is_online(row.node_id) {
        return Ok(());
    }

    let version = sqlx::query_scalar::<_, String>("select version from nodes where id = $1").bind(row.node_id).fetch_one(&state.pool).await?;

    if !node_has(&version, HEAD_AND_DIRECTORIES_SINCE) {
        return Ok(());
    }

    let Ok(answer) = relay(state, row.node_id, row.cwd.clone(), GitCommand::Head).await else {
        return Ok(());
    };
    let branch = answer["branch"].as_str().map(str::to_string);
    let lookup = match &branch {
        Some(_) => pull_requests_of(state, row.owner, row.project_id).await,
        None => Err(None),
    };
    let pull_request = match lookup {
        Ok(found) => branch.as_ref().and_then(|name| found.get(name)).map(|request| (request.label.clone(), request.url.clone(), request.state.to_string(), Some(request.number))),
        Err(_) if row.branch == branch => row.pull_request_label.clone().zip(row.pull_request_url.clone()).zip(row.pull_request_state.clone()).map(|((label, url), state)| (label, url, state, row.pull_request_number)),
        Err(_) => None,
    };
    let (label, url, pull_request_state, number) = match pull_request {
        Some((label, url, state, number)) => (Some(label), Some(url), Some(state), number),
        None => (None, None, None, None),
    };
    let just_merged = pull_request_state.as_deref() == Some("merged") && row.pull_request_state.as_deref() != Some("merged");

    let changed = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set branch = $2, pull_request_label = $3, pull_request_url = $4, pull_request_state = $5, pull_request_number = $7,
             archived_at = case when $6 and status not in ('working', 'input') then coalesce(archived_at, now()) else archived_at end
         where id = $1 and (branch, pull_request_label, pull_request_url, pull_request_state, pull_request_number) is distinct from ($2::text, $3::text, $4::text, $5::text, $7::bigint)
         returning {CONVERSATION_COLUMNS}",
    )))
    .bind(conversation_id)
    .bind(&branch)
    .bind(&label)
    .bind(&url)
    .bind(&pull_request_state)
    .bind(just_merged)
    .bind(number)
    .fetch_optional(&state.pool)
    .await?;

    if let Some(changed) = changed {
        state.publish(Some(changed.owner), StreamMessage::Conversation(changed.to_proto()?));
    }

    Ok(())
}

pub async fn close(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    close_for(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn close_for(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    let active = matches!(row.status.as_str(), "working" | "input");
    if chats::is_chat(&row) || !(active || state.hub.has_session(row.node_id, id)) {
        tx.commit().await?;
        crate::environments::stop_with_conversation(state, id, false).await;

        return Ok(());
    }

    if !state.hub.send(row.node_id, ControlToNode::CloseSession { conversation_id: id }) {
        return Err(node_offline());
    }

    state.hub.forget_prompts_of(id);
    if active {
        let updated = set_status(&mut tx, id, ConversationStatus::Done).await?;
        state.publish(Some(owner), StreamMessage::Conversation(updated.to_proto()?));
    }

    tx.commit().await?;
    state.remove_session(row.node_id, id).await?;
    crate::environments::stop_with_conversation(state, id, false).await;

    Ok(())
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, user.id).await?;

    if row.archived_at.is_none() {
        return Err(ApiError::conflict("not_archived", "Archive this conversation before deleting it."));
    }

    let removed = sqlx::query_as::<_, (Uuid, Uuid)>(
        "with recursive family as (
             select id, node_id from conversations where id = $1
             union all
             select c.id, c.node_id from conversations c join family f on c.parent_id = f.id
         ) select id, node_id from family",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let ids: Vec<Uuid> = removed.iter().map(|(removed_id, _)| *removed_id).collect();
    let attachments = sqlx::query_scalar::<_, String>(
        "select distinct sha from (
             select a->>'sha256' as sha from events e, jsonb_array_elements(case when jsonb_typeof(e.payload->'attachments') = 'array' then e.payload->'attachments' else '[]'::jsonb end) a where e.conversation_id = any($1)
             union all
             select a->>'sha256' from queued_prompts q, jsonb_array_elements(q.attachments) a where q.conversation_id = any($1)
         ) found where sha is not null",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("delete from conversations where id = $1").bind(id).execute(&mut *tx).await?;

    let mut unreferenced = Vec::new();

    for sha256 in attachments {
        let still_used = sqlx::query_scalar::<_, bool>(
            "select exists (select 1 from events e join conversations c on c.id = e.conversation_id where c.owner = $1 and e.kind = 'user_message' and jsonb_typeof(e.payload->'attachments') = 'array' and e.payload->'attachments' @> jsonb_build_array(jsonb_build_object('sha256', $2::text)))
                 or exists (select 1 from queued_prompts q join conversations c on c.id = q.conversation_id where c.owner = $1 and q.attachments @> jsonb_build_array(jsonb_build_object('sha256', $2::text)))",
        )
        .bind(user.id)
        .bind(&sha256)
        .fetch_one(&mut *tx)
        .await?;

        if still_used {
            continue;
        }

        sqlx::query("delete from attachments where owner = $1 and sha256 = $2").bind(user.id).bind(&sha256).execute(&mut *tx).await?;

        let held_by_another_owner = sqlx::query_scalar::<_, bool>("select exists (select 1 from attachments where sha256 = $1)").bind(&sha256).fetch_one(&mut *tx).await?;

        if !held_by_another_owner {
            unreferenced.push(sha256);
        }
    }

    tx.commit().await?;

    for sha256 in unreferenced {
        let _ = std::fs::remove_file(state.config.data_dir.join("attachments").join(sha256));
    }

    for (removed_id, node_id) in removed {
        state.stop_terminals_of_conversation(removed_id);
        state.hub.send(node_id, ControlToNode::CloseSession { conversation_id: removed_id });
        state.remove_session(node_id, removed_id).await?;
        crate::environments::stop_with_conversation(&state, removed_id, true).await;
        state.hub.forget_prompts_of(removed_id);
        state.hub.take_send_now(removed_id);
        state.publish(Some(user.id), StreamMessage::ConversationRemoved { conversation_id: removed_id });
    }

    if row.scratch && !scratch_folder_in_use(&state, row.node_id, &row.cwd).await? {
        state.hub.send(row.node_id, ControlToNode::DeleteFolder { root: row.cwd });
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn opened(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, user.id).await?;

    let reviewing = row.status == enum_text(&ConversationStatus::Review);

    if !reviewing && !row.unread {
        return Ok(StatusCode::NO_CONTENT);
    }

    sqlx::query(
        "insert into conversation_reads (user_id, conversation_id, seq) select $1, id, last_message_seq from conversations where id = $2
         on conflict (user_id, conversation_id) do update set seq = greatest(conversation_reads.seq, excluded.seq)",
    )
    .bind(user.id)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let status = if reviewing { ConversationStatus::Done } else { row.parsed_status()? };
    let updated = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set status = $2 where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .bind(enum_text(&status))
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    state.publish(Some(user.id), StreamMessage::Conversation(updated.to_proto()?));

    Ok(StatusCode::NO_CONTENT)
}

pub async fn events(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<EventsQuery>) -> ApiResult<Json<Vec<Event>>> {
    find_owned(&state.pool, id, user.id).await?;

    let limit = query.limit.clamp(1, 1000);

    let rows = match query.before {
        Some(before) => {
            sqlx::query_as::<_, (i64, String, Value, chrono::DateTime<chrono::Utc>)>(
                "with page_start as (select min(seq) as seq from (select seq from events where conversation_id = $1 and seq < $2 order by seq desc limit $3) newest)
                 select seq, kind, payload, created_at from events
                 where conversation_id = $1 and seq < $2
                   and seq >= greatest(coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message' and seq <= (select seq from page_start)), 0), (select seq from page_start) - $3)
                 order by seq",
            )
            .bind(id)
            .bind(before)
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, (i64, String, Value, chrono::DateTime<chrono::Utc>)>("select seq, kind, payload, created_at from events where conversation_id = $1 and seq > $2 order by seq limit $3")
                .bind(id)
                .bind(query.after)
                .bind(limit)
                .fetch_all(&state.pool)
                .await?
        }
    };

    let mut events = Vec::new();

    for (seq, kind, payload, created_at) in rows {
        events.push(Event { conversation_id: id, seq, kind: parse_enum(&kind)?, payload, created_at: Some(created_at) });
    }

    Ok(Json(events))
}

pub async fn session(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<Value>>> {
    find_owned(&state.pool, id, user.id).await?;

    let updates = sqlx::query_scalar::<_, Value>("select update from conversation_session_state where conversation_id = $1 order by update_kind").bind(id).fetch_all(&state.pool).await?;

    Ok(Json(updates))
}

#[derive(Deserialize)]
pub struct FilesQuery {
    #[serde(default)]
    query: String,
}

pub async fn files(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<FilesQuery>) -> ApiResult<Json<Vec<String>>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let asked = state.ask_node::<Vec<String>>(row.node_id, FILES_TIMEOUT, |request_id| ControlToNode::ListFiles { request_id, cwd: row.cwd, query: query.query }).await;
    let paths = asked.unwrap_or_default();

    Ok(Json(paths))
}

const FILES_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
