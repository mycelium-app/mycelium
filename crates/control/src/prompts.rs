use crate::attachments;
use crate::auth::AuthUser;
use crate::chats;
use crate::continuation;
use crate::error::{ApiError, ApiResult};
use crate::conversations::derived_title;
use crate::instructions;
use crate::nodes::{STEERING_SINCE, node_has, node_offline};
use crate::state::{AppState, UnacknowledgedPrompt};
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, TURN_HAS_ENDED, append_event, enqueue, merge_queue, parse_enum, find_owned, lock_owned, publish_queue, queue_of, set_status};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{ConfigRequest, PatchQueueRequest, PermissionAnswer, PromptRequest};
use mycelium_proto::conversation::{Attachment, ConversationStatus, EventKind, QueuedPrompt};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const NODE_NOT_CONNECTED: &str = "The node is not connected. Your message is queued and goes as soon as it reconnects.";
const LATE_STEER_WINDOW: Duration = Duration::from_secs(120);

pub async fn pump(state: &AppState, conversation_id: Uuid, first_start: bool) -> ApiResult<()> {
    if state.hub.is_moving(conversation_id) {
        return Ok(());
    }

    let mut tx = state.pool.begin().await?;
    let locked = sqlx::query_as::<_, (Uuid, Uuid, bool, bool, String, String, Option<String>, Option<String>)>(sqlx::AssertSqlSafe(format!(
        "select owner, node_id, archived_at is not null,
                (acp_session_id is not null or exists (select 1 from events where conversation_id = conversations.id and kind = 'continued')) and (status in ('review', 'done') or (status = 'working' and {TURN_HAS_ENDED})), agent, cwd, acp_session_id, account from conversations where id = $1 for update",
    )))
    .bind(conversation_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((owner, node_id, archived, idle, agent, cwd, acp_session_id, account)) = locked else {
        return Ok(());
    };

    if archived || !(idle || first_start) {
        return Ok(());
    }

    if !state.hub.is_online(node_id) {
        let waiting = sqlx::query_scalar::<_, bool>("select exists (select 1 from queued_prompts where conversation_id = $1)").bind(conversation_id).fetch_one(&mut *tx).await?;

        if waiting {
            tracing::warn!("node {node_id} is not connected, so the queued messages of {conversation_id} wait for it to greet the control plane");
        }

        return Ok(());
    }

    let next = sqlx::query_as::<_, (Uuid, String, sqlx::types::Json<Vec<Attachment>>)>("select id, text, attachments from queued_prompts where conversation_id = $1 order by position limit 1 for update")
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.rollback().await?;

    let Some((prompt_id, text, sqlx::types::Json(attachments))) = next else {
        return Ok(());
    };

    let handoff_marker = continuation::pending_handoff(state, conversation_id).await?;
    let sent_text = match handoff_marker {
        Some(marker) => format!("{}\n\n{text}", continuation::handoff(state, conversation_id, marker).await?),
        None => text.clone(),
    };
    let loaded = attachments::for_node(state, &attachments).await?;
    let starts_session = !state.hub.has_session(node_id, conversation_id);
    let session = match starts_session {
        true => Some((instructions::session_layer(state, conversation_id).await?, crate::assistants::secret_env(state, conversation_id).await?, parse_enum(&agent)?)),
        false => None,
    };

    let mut tx = state.pool.begin().await?;
    sqlx::query("select 1 from conversations where id = $1 for update").bind(conversation_id).execute(&mut *tx).await?;
    let taken = sqlx::query("delete from queued_prompts where id = $1").bind(prompt_id).execute(&mut *tx).await?;

    if taken.rows_affected() == 0 {
        return Ok(());
    }

    let event = append_event(&mut tx, conversation_id, EventKind::UserMessage, &json!({"text": text, "prompt_id": prompt_id, "attachments": attachments}), None).await?;

    sqlx::query("update conversations set predicted_title = coalesce(predicted_title, $2), title = case when title_user_set then title else coalesce(predicted_title, $2) end where id = $1")
        .bind(conversation_id)
        .bind(derived_title(&text, &attachments))
        .execute(&mut *tx)
        .await?;

    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set status = 'working', agent_active = true, last_activity_at = now(), updated_at = now() where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(conversation_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    if let Some((instructions, env, agent)) = session {
        if let Err(error) = state.add_session(node_id, conversation_id).await {
            tracing::warn!("could not publish the new session of {conversation_id}: {}", error.message());
        }

        state.hub.send(node_id, ControlToNode::StartSession { conversation_id, agent, cwd, resume: acp_session_id, fork: false, account, instructions, env });
    }

    state.hub.await_prompt_acknowledgement(UnacknowledgedPrompt { node_id, conversation_id, prompt_id, text: sent_text.clone(), attachments });
    state.hub.send(node_id, ControlToNode::Prompt { conversation_id, prompt_id, text: sent_text, attachments: loaded });

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));
    publish_queue(state, owner, conversation_id).await?;

    Ok(())
}

pub async fn prompt(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PromptRequest>) -> ApiResult<Response> {
    let text = request.text.trim();

    if text.is_empty() && request.attachments.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message to send."));
    }

    let attachments = attachments::confirm(&state, user.id, &request.attachments).await?;
    let mut tx = state.pool.begin().await?;
    let mut row = lock_owned(&mut tx, id, user.id).await?;

    if row.side_kind.as_deref() == Some("subagent") {
        return Err(ApiError::bad_request("read_only", "A subagent chat is read-only. Message the conversation it belongs to instead."));
    }

    if row.status == "failed" && !chats::is_chat(&row) {
        tx.commit().await?;
        retry_failed(&state, user.id, id).await?;
        tx = state.pool.begin().await?;
        row = lock_owned(&mut tx, id, user.id).await?;
    }

    if chats::is_chat(&row) {
        drop(tx);
        chats::send(&state, user.id, id, text, attachments).await?;

        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if row.archived_at.is_some() {
        let restored = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set archived_at = null where id = $1 returning {CONVERSATION_COLUMNS}")))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        state.publish(Some(user.id), StreamMessage::Conversation(restored.to_proto()?));
    }

    enqueue(&mut tx, id, text, &attachments).await?;
    tx.commit().await?;

    pump(&state, id, false).await?;
    publish_queue(&state, user.id, id).await?;

    if !state.hub.is_online(row.node_id) {
        return Ok((StatusCode::ACCEPTED, Json(json!({"queued": true, "message": NODE_NOT_CONNECTED}))).into_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn retry(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let row = find_owned(&state.pool, id, user.id).await?;
    retry_failed(&state, user.id, id).await?;

    if row.acp_session_id.is_some() {
        continue_unless_queued(&state, user.id, id, "Continue from where you stopped.").await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn continue_unless_queued(state: &AppState, owner: Uuid, id: Uuid, text: &str) -> ApiResult<()> {
    let waiting = sqlx::query_scalar::<_, bool>("select exists (select 1 from queued_prompts where conversation_id = $1)").bind(id).fetch_one(&state.pool).await?;

    if waiting {
        return Ok(());
    }

    let mut connection = state.pool.acquire().await?;
    enqueue(&mut connection, id, text, &[]).await?;
    pump(state, id, false).await?;
    publish_queue(state, owner, id).await
}

pub async fn retry_failed(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
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

    sqlx::query("update conversations set resume_at = null where id = $1").bind(id).execute(&mut *tx).await?;
    let updated = set_status(&mut tx, id, ConversationStatus::Done).await?;
    tx.commit().await?;

    state.remove_session(row.node_id, id).await?;
    state.add_session(row.node_id, id).await?;
    let instructions = instructions::session_layer(state, id).await?;
    let env = crate::assistants::secret_env(state, id).await?;
    state.hub.send(row.node_id, ControlToNode::StartSession { conversation_id: id, agent: parse_enum(&row.agent)?, cwd: row.cwd.clone(), resume: row.acp_session_id.clone(), fork: false, account: row.account.clone(), instructions, env });
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
        let node_version = sqlx::query_scalar::<_, String>("select version from nodes where id = $1").bind(row.node_id).fetch_optional(&state.pool).await?;
        let steerable = node_version.is_some_and(|version| node_has(&version, STEERING_SINCE));
        let queued = sqlx::query_as::<_, (String, sqlx::types::Json<Vec<Attachment>>)>("select text, attachments from queued_prompts where id = $1 and conversation_id = $2").bind(queue_id).bind(id).fetch_optional(&state.pool).await?;

        if let (true, Some((text, sqlx::types::Json(attachments)))) = (steerable, queued) {
            let loaded = attachments::for_node(&state, &attachments).await?;
            let requested = state.hub.request_steer(row.node_id, id, text.clone(), loaded);
            let answer = match requested {
                Some((request_id, mut answered)) => match tokio::time::timeout(state.config.steer_timeout, &mut answered).await {
                    Ok(answer) => {
                        state.hub.forget_steer_request(request_id);

                        answer.ok()
                    }
                    Err(_) => {
                        let late_state = state.clone();
                        tokio::spawn(async move {
                            let late = tokio::time::timeout(LATE_STEER_WINDOW, answered).await;
                            late_state.hub.forget_steer_request(request_id);

                            if let Ok(Ok(answer)) = late
                                && answer.outcome == "injected"
                                && let Err(error) = record_injected(&late_state, user.id, id, queue_id, &text, &attachments).await
                            {
                                tracing::warn!("could not record a late steered message for {id}: {error:?}");
                            }
                        });

                        return Ok(StatusCode::NO_CONTENT);
                    }
                },
                None => None,
            };

            match answer.as_ref().map(|answer| answer.outcome.as_str()) {
                Some("injected") => {
                    record_injected(&state, user.id, id, queue_id, &text, &attachments).await?;

                    return Ok(StatusCode::NO_CONTENT);
                }
                Some("prompt_required") => return pump(&state, id, false).await.map(|()| StatusCode::NO_CONTENT),
                _ => {}
            }
        }

        state.hub.expect_send_now(id, row.node_id);

        if !state.hub.send(row.node_id, ControlToNode::Cancel { conversation_id: id }) {
            state.hub.take_send_now(id);

            return Err(node_offline());
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn record_injected(state: &AppState, owner: Uuid, id: Uuid, queue_id: Uuid, text: &str, attachments: &[Attachment]) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let taken = sqlx::query_scalar::<_, Uuid>("delete from queued_prompts where id = $1 and conversation_id = $2 returning id").bind(queue_id).bind(id).fetch_optional(&mut *tx).await?;
    let prompt_id = taken.unwrap_or_else(Uuid::new_v4);
    let event = append_event(&mut tx, id, EventKind::UserMessage, &json!({"text": text, "prompt_id": prompt_id, "attachments": attachments}), None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    publish_queue(state, owner, id).await
}

pub async fn send_all_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    lock_owned(&mut tx, id, user.id).await?;
    let first = merge_queue(&mut tx, id).await?.ok_or_else(|| ApiError::not_found("A queued message"))?;
    tx.commit().await?;

    send_now(State(state), user, Path((id, first))).await
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

    if chats::is_chat(&row) {
        if !state.hub.answer_chat_approval(id, &answer.request_id, answer.option_id.clone()) {
            return Err(ApiError::conflict("already_resolved", "This permission request has already been answered."));
        }

        return Ok(StatusCode::NO_CONTENT);
    }

    if state.hub.answer_chat_approval(id, &answer.request_id, answer.option_id.clone()) {
        return Ok(StatusCode::NO_CONTENT);
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
    set_config(&state, user.id, id, request).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_config(state: &AppState, owner: Uuid, id: Uuid, request: ConfigRequest) -> ApiResult<()> {
    let row = find_owned(&state.pool, id, owner).await?;

    if row.acp_session_id.is_none() {
        return Err(ApiError::conflict("session_not_started", "The session is still starting. Try again in a moment."));
    }

    let message = ControlToNode::SetConfig { conversation_id: id, mode: request.mode.clone(), model: request.model.clone(), config: request.config.clone() };

    if !state.hub.send(row.node_id, message) {
        return Err(node_offline());
    }

    let updated = sqlx::query_as::<_, ConversationRow>(
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

    state.publish(Some(owner), StreamMessage::Conversation(updated.to_proto()?));

    Ok(())
}

pub async fn apply_model(state: &AppState, owner: Uuid, conversation: Uuid, model: Option<&str>, difficulty: Option<&str>) -> ApiResult<()> {
    let options: Option<Value> = sqlx::query_scalar("select options from conversations where id=$1").bind(conversation).fetch_one(&state.pool).await?;
    let config_options = options.as_ref().and_then(|options| options.get("config_options")).and_then(Value::as_array);
    let effort = config_options
        .and_then(|options| {
            options.iter().find(|option| {
                option.get("category").and_then(Value::as_str) == Some("thought_level")
                    || option.get("id").and_then(Value::as_str).is_some_and(|id| id.contains("effort") || id.contains("reasoning"))
            })
        })
        .and_then(|option| option.get("id"))
        .and_then(Value::as_str);

    if let Some(model) = model {
        let available = options.as_ref().and_then(|options| options.get("models")).and_then(|models| models.get("availableModels")).and_then(Value::as_array);

        if !available.is_some_and(|models| models.iter().any(|entry| entry.get("modelId").and_then(Value::as_str) == Some(model))) {
            return Err(ApiError::bad_request("workflow_model_unavailable", "The requested model is not offered by this agent. Choose one of its available models."));
        }
    }

    if let Some(difficulty) = difficulty {
        let option = config_options.and_then(|options| options.iter().find(|option| option.get("id").and_then(Value::as_str) == effort));
        let values = option.and_then(|option| option.get("options")).and_then(Value::as_array);
        let offered = values.is_some_and(|values| {
            values.iter().any(|value| {
                value.get("value").and_then(Value::as_str) == Some(difficulty)
                    || value.get("options").and_then(Value::as_array).is_some_and(|group| group.iter().any(|value| value.get("value").and_then(Value::as_str) == Some(difficulty)))
            })
        });

        if effort.is_none() || !offered {
            return Err(ApiError::bad_request("workflow_difficulty_unavailable", "The requested difficulty is not offered by this agent. Choose an available difficulty or use the agent default."));
        }
    }

    let config = difficulty.zip(effort).map(|(difficulty, key)| std::collections::BTreeMap::from([(key.to_string(), json!(difficulty))]));

    if model.is_some() || config.is_some() {
        set_config(state, owner, conversation, ConfigRequest { model: model.map(str::to_string), config, mode: None }).await?;
    }

    Ok(())
}
