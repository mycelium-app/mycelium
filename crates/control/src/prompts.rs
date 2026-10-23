use crate::attachments;
use crate::auth::AuthUser;
use crate::chats;
use crate::error::{ApiError, ApiResult};
use crate::conversations::derived_title;
use crate::instructions;
use crate::nodes::node_offline;
use crate::state::{AppState, UnacknowledgedPrompt};
use crate::store::{CONVERSATION_COLUMNS, append_event, enqueue, parse_enum, find_owned, lock_owned, publish_queue, queue_of, set_status};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ConfigRequest, PatchQueueRequest, PermissionAnswer, PromptRequest};
use mycelium_proto::conversation::{Attachment, ConversationStatus, EventKind, QueuedPrompt};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn pump(state: &AppState, conversation_id: Uuid, first_start: bool) -> ApiResult<()> {
    if state.hub.is_moving(conversation_id) {
        return Ok(());
    }

    let mut tx = state.pool.begin().await?;
    let locked = sqlx::query_as::<_, (Uuid, Uuid, bool, bool, String, String, Option<String>, Option<String>)>(
        "select owner, node_id, archived_at is not null,
                acp_session_id is not null and status in ('review', 'done'), agent, cwd, acp_session_id, account from conversations where id = $1 for update",
    )
    .bind(conversation_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((owner, node_id, archived, idle, agent, cwd, acp_session_id, account)) = locked else {
        return Ok(());
    };

    if archived || !(idle || first_start) || !state.hub.is_online(node_id) {
        return Ok(());
    }

    let next = sqlx::query_as::<_, (Uuid, String, sqlx::types::Json<Vec<Attachment>>)>("delete from queued_prompts where id = (select id from queued_prompts where conversation_id = $1 order by position limit 1 for update) returning id, text, attachments")
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?;

    let Some((prompt_id, text, sqlx::types::Json(attachments))) = next else {
        return Ok(());
    };

    let event = append_event(&mut tx, conversation_id, EventKind::UserMessage, &json!({"text": text, "prompt_id": prompt_id, "attachments": attachments}), None).await?;

    sqlx::query("update conversations set predicted_title = coalesce(predicted_title, $2), title = case when title_user_set then title else coalesce(predicted_title, $2) end where id = $1")
        .bind(conversation_id)
        .bind(derived_title(&text, &attachments))
        .execute(&mut *tx)
        .await?;

    let row = set_status(&mut tx, conversation_id, ConversationStatus::Working).await?;
    tx.commit().await?;

    if !state.hub.has_session(node_id, conversation_id) {
        state.add_session(node_id, conversation_id).await?;
        let instructions = instructions::session_layer(state, conversation_id).await?;
        state.hub.send(node_id, ControlToNode::StartSession { conversation_id, agent: parse_enum(&agent)?, cwd, resume: acp_session_id, fork: false, account, instructions });
    }

    let loaded = attachments::for_node(state, &attachments).await?;
    state.hub.await_prompt_acknowledgement(UnacknowledgedPrompt { node_id, conversation_id, prompt_id, text: text.clone(), attachments });
    state.hub.send(node_id, ControlToNode::Prompt { conversation_id, prompt_id, text, attachments: loaded });

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));
    publish_queue(state, owner, conversation_id).await?;

    Ok(())
}

pub async fn prompt(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PromptRequest>) -> ApiResult<StatusCode> {
    let text = request.text.trim();

    if text.is_empty() && request.attachments.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message to send."));
    }

    let attachments = attachments::confirm(&state, user.id, &request.attachments).await?;
    let mut tx = state.pool.begin().await?;
    let mut row = lock_owned(&mut tx, id, user.id).await?;

    if row.status == "failed" && !chats::is_chat(&row) {
        tx.commit().await?;
        retry_failed(&state, user.id, id).await?;
        tx = state.pool.begin().await?;
        row = lock_owned(&mut tx, id, user.id).await?;
    }

    if chats::is_chat(&row) {
        drop(tx);
        chats::send(&state, user.id, id, text, attachments).await?;

        return Ok(StatusCode::NO_CONTENT);
    }

    if row.archived_at.is_some() {
        let restored = sqlx::query_as::<_, crate::store::ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set archived_at = null where id = $1 returning {CONVERSATION_COLUMNS}")))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        state.publish(Some(user.id), StreamMessage::Conversation(restored.to_proto()?));
    }

    enqueue(&mut tx, id, text, &attachments).await?;
    tx.commit().await?;

    pump(&state, id, false).await?;
    publish_queue(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn retry(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;
    retry_failed(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn retry_failed(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status != "failed" {
        return Ok(());
    }

    let node_usable = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(row.node_id).fetch_one(&mut *tx).await?;

    if !node_usable {
        return Err(ApiError::conflict("node_removed", "This conversation's node was removed, so its session cannot start again. Start a new conversation on another node."));
    }

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let updated = set_status(&mut tx, id, ConversationStatus::Done).await?;
    tx.commit().await?;

    state.remove_session(row.node_id, id).await?;
    state.add_session(row.node_id, id).await?;
    let instructions = instructions::session_layer(state, id).await?;
    state.hub.send(row.node_id, ControlToNode::StartSession { conversation_id: id, agent: parse_enum(&row.agent)?, cwd: row.cwd.clone(), resume: row.acp_session_id.clone(), fork: false, account: row.account.clone(), instructions });
    state.publish(Some(owner), StreamMessage::Conversation(updated.to_proto()?));
    pump(state, id, true).await?;

    Ok(())
}

pub async fn queue(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<QueuedPrompt>>> {
    find_owned(&state.pool, id, user.id).await?;

    Ok(Json(queue_of(&state.pool, id).await?))
}

pub async fn edit_queued(State(state): State<AppState>, user: AuthUser, Path((id, queue_id)): Path<(Uuid, Uuid)>, Json(request): Json<PatchQueueRequest>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    let text = request.text.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message to send."));
    }

    let updated = sqlx::query("update queued_prompts set text = $3 where id = $2 and conversation_id = $1").bind(id).bind(queue_id).bind(text).execute(&state.pool).await?;

    if updated.rows_affected() == 0 {
        return Err(ApiError::not_found("That queued message"));
    }

    publish_queue(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_queued(State(state): State<AppState>, user: AuthUser, Path((id, queue_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    let deleted = sqlx::query("delete from queued_prompts where id = $2 and conversation_id = $1").bind(id).bind(queue_id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That queued message"));
    }

    publish_queue(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn send_now(State(state): State<AppState>, user: AuthUser, Path((id, queue_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, user.id).await?;
    let moved = sqlx::query(
        "update queued_prompts set position = (select coalesce(min(position), 1) - 1 from queued_prompts where conversation_id = $1)
         where id = $2 and conversation_id = $1",
    )
    .bind(id)
    .bind(queue_id)
    .execute(&mut *tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(ApiError::not_found("That queued message"));
    }

    tx.commit().await?;
    publish_queue(&state, user.id, id).await?;

    if row.is_idle() {
        return pump(&state, id, false).await.map(|()| StatusCode::NO_CONTENT);
    }

    if matches!(row.status.as_str(), "working" | "input") {
        state.hub.expect_send_now(id, row.node_id);

        if !state.hub.send(row.node_id, ControlToNode::Cancel { conversation_id: id }) {
            state.hub.take_send_now(id);

            return Err(node_offline());
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if chats::is_chat(&row) {
        chats::stop(&state, user.id, id).await?;

        return Ok(StatusCode::NO_CONTENT);
    }

    if !state.hub.send(row.node_id, ControlToNode::Cancel { conversation_id: id }) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn permission(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(answer): Json<PermissionAnswer>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, user.id).await?;

    let request = sqlx::query_scalar::<_, Value>("select payload from events where conversation_id = $1 and kind = 'permission_request' and payload->>'request_id' = $2")
        .bind(id)
        .bind(&answer.request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("That permission request"))?;

    if row.status == "failed" {
        return Err(ApiError::conflict("session_failed", "This conversation's session failed, so it is not waiting for an answer. Retry the conversation to continue."));
    }

    let answered = sqlx::query_scalar::<_, bool>("select exists (select 1 from events where conversation_id = $1 and kind = 'permission_resolved' and payload->>'request_id' = $2)")
        .bind(id)
        .bind(&answer.request_id)
        .fetch_one(&mut *tx)
        .await?;

    if answered || row.status != "input" {
        return Err(ApiError::conflict("already_resolved", "This permission request has already been answered."));
    }

    let offered = request["options"].as_array().map(|options| options.iter().any(|option| option["optionId"].as_str() == Some(answer.option_id.as_str())));

    if offered == Some(false) {
        return Err(ApiError::bad_request("unknown_option", "Choose one of the options the agent offered."));
    }

    let outcome = json!({"outcome": "selected", "optionId": answer.option_id});

    if !state.hub.send(row.node_id, ControlToNode::PermissionResponse { conversation_id: id, request_id: answer.request_id.clone(), outcome }) {
        return Err(node_offline());
    }

    let event = append_event(&mut tx, id, EventKind::PermissionResolved, &json!({"request_id": answer.request_id, "option_id": answer.option_id}), None).await?;
    let updated = set_status(&mut tx, id, ConversationStatus::Working).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(user.id), StreamMessage::Event(event));
    }

    state.publish(Some(user.id), StreamMessage::Conversation(updated.to_proto()?));

    Ok(StatusCode::NO_CONTENT)
}

pub async fn config(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<ConfigRequest>) -> ApiResult<StatusCode> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if row.acp_session_id.is_none() {
        return Err(ApiError::conflict("session_not_started", "The session is still starting. Try again in a moment."));
    }

    let message = ControlToNode::SetConfig { conversation_id: id, mode: request.mode.clone(), model: request.model.clone(), config: request.config.clone() };

    if !state.hub.send(row.node_id, message) {
        return Err(node_offline());
    }

    let updated = sqlx::query_as::<_, crate::store::ConversationRow>(
        sqlx::AssertSqlSafe(format!(
        "update conversations set mode = coalesce($2, mode), model = coalesce($3, model) where id = $1
         returning {CONVERSATION_COLUMNS}",
    )),
    )
    .bind(id)
    .bind(&request.mode)
    .bind(&request.model)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(user.id), StreamMessage::Conversation(updated.to_proto()?));

    Ok(StatusCode::NO_CONTENT)
}
