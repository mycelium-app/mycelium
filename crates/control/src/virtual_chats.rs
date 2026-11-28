use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateVirtualChatRequest, PatchVirtualChatRequest, PromptRequest, SaveTodosRequest, SendVirtualMessageRequest, VirtualChat, VirtualMessage};
use mycelium_proto::conversation::Attachment;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const VIRTUAL_ORIGIN: &str = "virtual_chat";
const CHAT_COLUMNS: &str = "c.id, c.assistant_id, c.title, c.description, c.closed_at is not null as closed,
    exists (select 1 from virtual_messages m where m.chat_id = c.id and not m.from_person and m.created_at > c.read_at) as unread,
    c.todos, c.created_at, c.updated_at";
const MESSAGE_COLUMNS: &str = "id, chat_id, from_person, text, attachments, sending, created_at";
pub const HISTORY_LIMIT: i64 = 50;

#[derive(sqlx::FromRow)]
struct ChatRow {
    id: Uuid,
    assistant_id: Uuid,
    title: String,
    description: Option<String>,
    closed: bool,
    unread: bool,
    todos: sqlx::types::Json<Value>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl ChatRow {
    fn to_proto(self) -> VirtualChat {
        VirtualChat { id: self.id, assistant_id: self.assistant_id, title: self.title, description: self.description, closed: self.closed, unread: self.unread, todos: self.todos.0, created_at: self.created_at, updated_at: self.updated_at }
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: Uuid,
    chat_id: Uuid,
    from_person: bool,
    text: String,
    attachments: sqlx::types::Json<Vec<Attachment>>,
    sending: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl MessageRow {
    fn to_proto(self) -> VirtualMessage {
        VirtualMessage { id: self.id, chat_id: self.chat_id, from_person: self.from_person, text: self.text, attachments: self.attachments.0, sending: self.sending, created_at: self.created_at }
    }
}

async fn chat(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<VirtualChat> {
    sqlx::query_as::<_, ChatRow>(sqlx::AssertSqlSafe(format!("select {CHAT_COLUMNS} from virtual_chats c where c.id = $1 and c.owner = $2")))
        .bind(id)
        .bind(owner)
        .fetch_optional(&state.pool)
        .await?
        .map(ChatRow::to_proto)
        .ok_or_else(|| ApiError::not_found("That virtual chat"))
}

async fn publish_chat(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<VirtualChat> {
    let chat = chat(state, owner, id).await?;
    state.publish(Some(owner), StreamMessage::VirtualChat(chat.clone()));

    Ok(chat)
}

pub async fn chats_of(state: &AppState, owner: Uuid, assistant_id: Uuid) -> ApiResult<Vec<VirtualChat>> {
    let rows = sqlx::query_as::<_, ChatRow>(sqlx::AssertSqlSafe(format!("select {CHAT_COLUMNS} from virtual_chats c where c.assistant_id = $1 and c.owner = $2 order by c.created_at, c.id")))
        .bind(assistant_id)
        .bind(owner)
        .fetch_all(&state.pool)
        .await?;

    Ok(rows.into_iter().map(ChatRow::to_proto).collect())
}

pub async fn find(state: &AppState, owner: Uuid, assistant_id: Uuid, named: &str) -> ApiResult<Option<VirtualChat>> {
    let named = named.trim();
    let chats = chats_of(state, owner, assistant_id).await?;

    Ok(chats.into_iter().find(|chat| chat.id.to_string() == named.to_lowercase() || chat.title.eq_ignore_ascii_case(named)))
}

pub async fn create_for(state: &AppState, owner: Uuid, assistant_id: Uuid, title: &str, description: Option<&str>) -> ApiResult<VirtualChat> {
    let title = title.trim();

    if title.is_empty() {
        return Err(ApiError::bad_request("title_required", "Enter a name for the virtual chat."));
    }

    crate::assistants::find_owned(state, assistant_id, owner).await?;
    let id = sqlx::query_scalar::<_, Uuid>("insert into virtual_chats (owner, assistant_id, title, description) values ($1, $2, $3, $4) returning id")
        .bind(owner)
        .bind(assistant_id)
        .bind(title)
        .bind(description.map(str::trim).filter(|description| !description.is_empty()))
        .fetch_one(&state.pool)
        .await?;

    publish_chat(state, owner, id).await
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Path(assistant_id): Path<Uuid>, Json(request): Json<CreateVirtualChatRequest>) -> ApiResult<(StatusCode, Json<VirtualChat>)> {
    let chat = create_for(&state, user.id, assistant_id, &request.title, request.description.as_deref()).await?;

    Ok((StatusCode::CREATED, Json(chat)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Path(assistant_id): Path<Uuid>) -> ApiResult<Json<Vec<VirtualChat>>> {
    Ok(Json(chats_of(&state, user.id, assistant_id).await?))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchVirtualChatRequest>) -> ApiResult<Json<VirtualChat>> {
    chat(&state, user.id, id).await?;

    if let Some(closed) = request.closed {
        sqlx::query("update virtual_chats set closed_at = case when $2 then coalesce(closed_at, now()) end, updated_at = now() where id = $1").bind(id).bind(closed).execute(&state.pool).await?;
    }

    if request.read {
        sqlx::query("update virtual_chats set read_at = now() where id = $1").bind(id).execute(&state.pool).await?;
    }

    Ok(Json(publish_chat(&state, user.id, id).await?))
}

pub async fn save_todos(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SaveTodosRequest>) -> ApiResult<Json<VirtualChat>> {
    let mut entries = Vec::with_capacity(request.todos.len());

    for item in &request.todos {
        let Some(entry) = crate::todos::entry(&item.content, &item.status, item.description.as_deref()) else {
            return Err(ApiError::bad_request("todo_content_required", "Enter what to do for every item on the to-do list, or dismiss the empty ones."));
        };

        entries.push(entry);
    }

    chat(&state, user.id, id).await?;
    set_todos(&state, user.id, id, entries).await.map(Json)
}

pub async fn set_todos(state: &AppState, owner: Uuid, id: Uuid, entries: Vec<Value>) -> ApiResult<VirtualChat> {
    sqlx::query("update virtual_chats set todos = $2 where id = $1").bind(id).bind(sqlx::types::Json(json!(entries))).execute(&state.pool).await?;

    publish_chat(state, owner, id).await
}

pub async fn messages_of(state: &AppState, chat_id: Uuid, limit: i64, before: Option<chrono::DateTime<chrono::Utc>>) -> ApiResult<Vec<VirtualMessage>> {
    let mut rows = sqlx::query_as::<_, MessageRow>(sqlx::AssertSqlSafe(format!(
        "select {MESSAGE_COLUMNS} from virtual_messages where chat_id = $1 and ($2::timestamptz is null or created_at < $2) order by created_at desc, id desc limit $3"
    )))
    .bind(chat_id)
    .bind(before)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    rows.reverse();

    Ok(rows.into_iter().map(MessageRow::to_proto).collect())
}

pub async fn list_messages(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<VirtualMessage>>> {
    chat(&state, user.id, id).await?;

    Ok(Json(messages_of(&state, id, i64::MAX, None).await?))
}

async fn insert_message(state: &AppState, chat_id: Uuid, from_person: bool, text: &str, attachments: &[Attachment], queue_id: Option<Uuid>) -> ApiResult<VirtualMessage> {
    let row = sqlx::query_as::<_, MessageRow>(sqlx::AssertSqlSafe(format!(
        "insert into virtual_messages (chat_id, from_person, text, attachments, queue_id, sending) values ($1, $2, $3, $4, $5, $5 is not null) returning {MESSAGE_COLUMNS}"
    )))
    .bind(chat_id)
    .bind(from_person)
    .bind(text)
    .bind(sqlx::types::Json(attachments))
    .bind(queue_id)
    .fetch_one(&state.pool)
    .await?;
    sqlx::query("update virtual_chats set closed_at = null, updated_at = now(), read_at = case when $2 then now() else read_at end where id = $1").bind(chat_id).bind(from_person).execute(&state.pool).await?;

    Ok(row.to_proto())
}

pub async fn post(state: &AppState, owner: Uuid, chat_id: Uuid, text: &str) -> ApiResult<VirtualMessage> {
    let message = insert_message(state, chat_id, false, text, &[], None).await?;
    state.publish(Some(owner), StreamMessage::VirtualMessage(message.clone()));
    publish_chat(state, owner, chat_id).await?;

    Ok(message)
}

fn prompt_text(chat: &VirtualChat, text: &str) -> String {
    format!("{text}\n\n(Sent in the virtual chat \"{}\", id {}. Answer there with post_virtual_message, chat {}.)", chat.title, chat.id, chat.id)
}

pub async fn create_message(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SendVirtualMessageRequest>) -> ApiResult<(StatusCode, Json<VirtualMessage>)> {
    let chat = chat(&state, user.id, id).await?;
    let text = request.text.trim();

    if text.is_empty() && request.attachments.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message to send."));
    }

    let host = sqlx::query_scalar::<_, Option<Uuid>>("select conversation_id from assistants where id = $1").bind(chat.assistant_id).fetch_one(&state.pool).await?;
    let attachments = crate::attachments::confirm(&state, user.id, &request.attachments).await?;
    let queued = match host {
        Some(host) => queue_on(&state, user.id, host, &prompt_text(&chat, text), &attachments).await?,
        None => None,
    };
    let message = insert_message(&state, id, true, text, &attachments, queued.map(|(_, queue_id)| queue_id)).await?;
    state.publish(Some(user.id), StreamMessage::VirtualMessage(message.clone()));
    publish_chat(&state, user.id, id).await?;

    match queued {
        Some((host, queue_id)) => deliver(&state, user.id, host, queue_id).await,
        None => {
            let request = PromptRequest { text: prompt_text(&chat, text), attachments, ..Default::default() };
            crate::assistants::wake(&state, user.id, chat.assistant_id, request, &crate::assistants::StartOverrides::default()).await?;
        }
    }

    let message = sqlx::query_as::<_, MessageRow>(sqlx::AssertSqlSafe(format!("select {MESSAGE_COLUMNS} from virtual_messages where id = $1"))).bind(message.id).fetch_one(&state.pool).await?;

    Ok((StatusCode::CREATED, Json(message.to_proto())))
}

async fn queue_on(state: &AppState, owner: Uuid, host: Uuid, text: &str, attachments: &[Attachment]) -> ApiResult<Option<(Uuid, Uuid)>> {
    let row = crate::store::find_owned(&state.pool, host, owner).await?;

    if crate::chats::is_chat(&row) || row.archived_at.is_some() {
        return Ok(None);
    }

    let mut tx = state.pool.begin().await?;
    let queue_id = crate::store::enqueue(&mut tx, host, text, attachments, Some(VIRTUAL_ORIGIN)).await?;
    tx.commit().await?;

    Ok(Some((host, queue_id)))
}

async fn deliver(state: &AppState, owner: Uuid, host: Uuid, queue_id: Uuid) {
    if let Err(error) = crate::prompts::pump(state, host, false).await {
        tracing::warn!("a virtual chat message for {host} is queued but could not be delivered yet: {}", error.message());
    }

    let waiting = sqlx::query_scalar::<_, bool>("select exists (select 1 from queued_prompts where id = $1)").bind(queue_id).fetch_one(&state.pool).await.unwrap_or(false);
    let node_version = sqlx::query_scalar::<_, String>("select n.version from nodes n join conversations c on c.node_id = n.id where c.id = $1").bind(host).fetch_optional(&state.pool).await.ok().flatten();
    let steerable = node_version.is_some_and(|version| crate::nodes::node_has(&version, crate::nodes::STEERING_SINCE));

    if waiting && steerable {
        let state = state.clone();

        tokio::spawn(async move {
            if let Err(error) = crate::prompts::send_queued_now(&state, owner, host, queue_id).await {
                tracing::info!("a virtual chat message for {host} waits for the turn to end: {}", error.message());
            }
        });
    }
}

pub async fn delivered(state: &AppState, owner: Uuid, queue_id: Uuid) {
    let row = sqlx::query_as::<_, MessageRow>(sqlx::AssertSqlSafe(format!("update virtual_messages set sending = false, queue_id = null where queue_id = $1 returning {MESSAGE_COLUMNS}")))
        .bind(queue_id)
        .fetch_optional(&state.pool)
        .await;

    match row {
        Ok(Some(row)) => state.publish(Some(owner), StreamMessage::VirtualMessage(row.to_proto())),
        Ok(None) => {}
        Err(error) => tracing::warn!("could not mark a virtual chat message delivered: {error}"),
    }
}

pub async fn delete_message(State(state): State<AppState>, user: AuthUser, Path((id, message_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    chat(&state, user.id, id).await?;
    let mut tx = state.pool.begin().await?;
    let queue_id = sqlx::query_scalar::<_, Option<Uuid>>("select queue_id from virtual_messages where id = $1 and chat_id = $2 and from_person and sending for update").bind(message_id).bind(id).fetch_optional(&mut *tx).await?.flatten();
    let Some(queue_id) = queue_id else {
        return Err(ApiError::conflict("already_read", "The assistant has already read this message, so it cannot be cancelled."));
    };
    let taken = sqlx::query("delete from queued_prompts where id = $1").bind(queue_id).execute(&mut *tx).await?.rows_affected();

    if taken == 0 {
        return Err(ApiError::conflict("already_read", "The assistant has already read this message, so it cannot be cancelled."));
    }

    sqlx::query("delete from virtual_messages where id = $1").bind(message_id).execute(&mut *tx).await?;
    tx.commit().await?;
    state.publish(Some(user.id), StreamMessage::VirtualMessageRemoved { chat_id: id, message_id });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn turn_ended(state: &AppState, conversation_id: Uuid) -> ApiResult<()> {
    let Some((owner, assistant_id)) = sqlx::query_as::<_, (Uuid, Uuid)>("select owner, id from assistants where conversation_id = $1").bind(conversation_id).fetch_optional(&state.pool).await? else {
        return Ok(());
    };
    let turn_started = sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "select max(created_at) from events where conversation_id = $1 and kind = 'turn_end' and seq < (select max(seq) from events where conversation_id = $1 and kind = 'turn_end')",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;
    let unanswered = sqlx::query_scalar::<_, Uuid>(
        "select c.id from virtual_chats c where c.assistant_id = $1
           and exists (select 1 from virtual_messages p where p.chat_id = c.id and p.from_person and not p.sending and ($2::timestamptz is null or p.created_at > $2)
                       and not exists (select 1 from virtual_messages r where r.chat_id = c.id and not r.from_person and r.created_at > p.created_at))",
    )
    .bind(assistant_id)
    .bind(turn_started)
    .fetch_all(&state.pool)
    .await?;

    if unanswered.is_empty() {
        return Ok(());
    }

    let text = sqlx::query_scalar::<_, String>(
        "select coalesce(string_agg(payload->'update'->'content'->>'text', '' order by seq), '') from events
         where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
           and seq > coalesce((select max(seq) from events where conversation_id = $1 and (kind = 'user_message' or (kind = 'acp_update' and payload->'update'->>'sessionUpdate' in ('tool_call', 'tool_call_update')))), 0)",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    if text.trim().is_empty() {
        return Ok(());
    }

    for chat_id in unanswered {
        post(state, owner, chat_id, text.trim()).await?;
    }

    Ok(())
}
