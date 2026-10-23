use crate::attachments;
use crate::auth::AuthUser;
use crate::activity::record_turn;
use crate::conversations::derived_title;
use crate::error::{ApiError, ApiResult};
use crate::model_providers::{self, ChatImage, ChatMessage, ProviderRow};
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, enum_text, find_owned, lock_owned, set_status};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use mycelium_proto::api::CreateChatRequest;
use base64::Engine;
use mycelium_proto::conversation::{Attachment, Conversation, ConversationStatus, EventKind, SideKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use uuid::Uuid;

pub const CHAT_AGENT: &str = "chat";
const CHAT_HOST: &str = "control-plane";
const CHAT_INSTRUCTIONS: &str = "You are chatting inside Mycelium. Reply in Markdown.
When an interface helps the person more than prose, you may include one fenced block whose language is mycelium-ui, containing valid JSON with no comments for a small component tree. Keep the explanation outside the block.
Components, each an object with a \"type\":
- {\"type\":\"column\",\"children\":[...]} and {\"type\":\"row\",\"children\":[...]} lay out other components.
- {\"type\":\"heading\",\"text\":\"...\"} and {\"type\":\"text\",\"text\":\"...\"}.
- {\"type\":\"table\",\"columns\":[\"...\"],\"rows\":[[\"...\"]]}.
- {\"type\":\"bar_chart\",\"title\":\"...\",\"bars\":[{\"label\":\"...\",\"value\":12,\"display\":\"12 ms\"}]}.
- {\"type\":\"button\",\"label\":\"...\",\"action\":{...}} where the action is {\"type\":\"send_message\",\"text\":\"...\"}, which sends the text as the person's next message, or {\"type\":\"side_chat\",\"text\":\"...\"}, which opens a linked chat started with that text.
A button label says what happens when it is pressed.";

async fn chat_node(state: &AppState, owner: Uuid) -> ApiResult<Uuid> {
    let existing = sqlx::query_scalar::<_, Uuid>("select id from nodes where created_by = $1 and hostname = $2 limit 1").bind(owner).bind(CHAT_HOST).fetch_optional(&state.pool).await?;

    if let Some(existing) = existing {
        return Ok(existing);
    }

    let id = sqlx::query_scalar::<_, Uuid>(
        "insert into nodes (name, hostname, os, arch, token_hash, created_by, revoked_at) values ('Chats', $2, 'linux', 'x86_64', $3, $1, now()) returning id",
    )
    .bind(owner)
    .bind(CHAT_HOST)
    .bind(rand::random::<[u8; 32]>().to_vec())
    .fetch_one(&state.pool)
    .await?;

    Ok(id)
}

pub async fn index_title(state: &AppState, conversation_id: Uuid, title: &str) -> ApiResult<()> {
    sqlx::query("insert into search_entries (conversation_id, seq, text) values ($1, 0, $2) on conflict (conversation_id, seq) do update set text = excluded.text")
        .bind(conversation_id)
        .bind(title)
        .execute(&state.pool)
        .await?;

    Ok(())
}

pub async fn end_interrupted(state: &AppState) -> ApiResult<()> {
    sqlx::query("update conversations set status = 'review' where agent = $1 and status = 'working'").bind(CHAT_AGENT).execute(&state.pool).await?;

    Ok(())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateChatRequest>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let side_kind = request.parent_id.map(|_| SideKind::Side);
    let conversation = start(&state, &user, request, side_kind).await?;

    Ok((StatusCode::CREATED, Json(conversation)))
}

pub async fn start(state: &AppState, user: &AuthUser, request: CreateChatRequest, side_kind: Option<SideKind>) -> ApiResult<Conversation> {
    let provider = model_providers::find_owned(state, request.provider_id, user.id).await?;
    let model = request.model.as_deref().map(str::trim).filter(|model| !model.is_empty()).map(str::to_string).or_else(|| provider.default_model.clone());
    let Some(model) = model else {
        return Err(ApiError::bad_request("model_required", "Choose a model for this chat."));
    };

    if let Some(parent_id) = request.parent_id {
        let parent = find_owned(&state.pool, parent_id, user.id).await?;

        if parent.agent != CHAT_AGENT {
            return Err(ApiError::bad_request("parent_invalid", "A side chat belongs to another chat."));
        }
    }

    let prompt = request.prompt.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let node_id = chat_node(state, user.id).await?;
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "insert into conversations (owner, node_id, agent, cwd, title, status, model, provider_id, parent_id, side_kind) values ($1, $2, $3, '', 'New chat', 'done', $4, $5, $6, $7)
         returning {CONVERSATION_COLUMNS}"
    )))
    .bind(user.id)
    .bind(node_id)
    .bind(CHAT_AGENT)
    .bind(&model)
    .bind(provider.id)
    .bind(request.parent_id)
    .bind(side_kind.as_ref().map(enum_text))
    .fetch_one(&state.pool)
    .await?;

    index_title(state, row.id, &row.title).await?;

    let conversation = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));

    match prompt {
        Some(text) => send(state, user.id, row.id, text, Vec::new()).await,
        None => Ok(conversation),
    }
}

pub async fn send(state: &AppState, owner: Uuid, id: Uuid, text: &str, attachments: Vec<Attachment>) -> ApiResult<Conversation> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status == enum_text(&ConversationStatus::Working) {
        return Err(ApiError::conflict("chat_busy", "This chat is still replying. Wait for it to finish, or stop it, then send your message."));
    }

    let Some(provider_id) = row.provider_id else {
        return Err(ApiError::conflict("provider_removed", "This chat's model provider was removed. Start a new chat with another provider."));
    };
    let model = row.model.clone().unwrap_or_default();
    let event = append_event(&mut tx, id, EventKind::UserMessage, &json!({"text": text, "attachments": attachments}), None).await?;
    let title = sqlx::query_scalar::<_, String>(
        "update conversations set predicted_title = coalesce(predicted_title, $2), archived_at = null,
             title = case when title_user_set then title else coalesce(predicted_title, $2) end where id = $1 returning title",
    )
    .bind(id)
    .bind(derived_title(text, &attachments))
    .fetch_one(&mut *tx)
    .await?;
    let working = set_status(&mut tx, id, ConversationStatus::Working).await?;
    tx.commit().await?;

    index_title(state, id, &title).await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    let conversation = working.to_proto()?;
    state.publish(Some(owner), StreamMessage::Conversation(conversation.clone()));

    let provider = model_providers::find_owned(state, provider_id, owner).await?;
    let said = sqlx::query_as::<_, (String, Option<String>, Option<sqlx::types::Json<Vec<Attachment>>>)>(
        "select e.kind, s.text, e.payload->'attachments' from events e left join search_entries s on s.conversation_id = e.conversation_id and s.seq = e.seq
         where e.conversation_id = $1 and e.kind in ('user_message', 'turn_end') and (s.text is not null or coalesce(jsonb_array_length(e.payload->'attachments'), 0) > 0) order by e.seq",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let mut messages: Vec<ChatMessage> = Vec::new();

    for (kind, text, said_attachments) in said {
        let role = if kind == "user_message" { "user" } else { "assistant" };
        let (content, images) = chat_content(state, text.unwrap_or_default(), said_attachments.map(|json| json.0).unwrap_or_default()).await?;

        match messages.last_mut() {
            Some(last) if last.role == role => {
                last.content = format!("{}\n\n{content}", last.content).trim().to_string();
                last.images.extend(images);
            }
            _ => messages.push(ChatMessage { role, content, images }),
        }
    }

    let turn = tokio::spawn(run_turn(state.clone(), owner, id, provider, model, messages));
    state.hub.track_chat_turn(id, turn.abort_handle());

    Ok(conversation)
}

async fn chat_content(state: &AppState, text: String, attachments: Vec<Attachment>) -> ApiResult<(String, Vec<ChatImage>)> {
    let mut content = text;
    let mut images = Vec::new();

    for attachment in attachments {
        let bytes = attachments::read(state, &attachment).await?;

        if attachment.media_type.starts_with("image/") {
            images.push(ChatImage { media_type: attachment.media_type, data: base64::engine::general_purpose::STANDARD.encode(bytes) });
        } else if let Ok(file_text) = String::from_utf8(bytes) {
            content = format!("{content}\n\n{}:\n```\n{file_text}\n```", attachment.name).trim().to_string();
        } else {
            content = format!("{content}\n\n[Attached file {} ({}), which cannot be read as text.]", attachment.name, attachment.media_type).trim().to_string();
        }
    }

    Ok((content, images))
}

pub async fn stop(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    find_owned(&state.pool, id, owner).await?;

    if state.hub.abort_chat_turn(id) {
        finish(state, owner, id, "cancelled", None).await?;
    }

    Ok(())
}

async fn run_turn(state: AppState, owner: Uuid, id: Uuid, provider: ProviderRow, model: String, messages: Vec<ChatMessage>) {
    let mut usage = (None, None);
    let outcome = relay(&state, owner, id, &provider, &model, &messages, &mut usage).await;
    let stop_reason = match outcome {
        Ok(()) => "end_turn",
        Err(error) => {
            if let Err(cause) = record_failure(&state, owner, id, error.message()).await {
                tracing::warn!("could not record the failure of chat {id}: {}", cause.message());
            }

            "error"
        }
    };

    if let Err(cause) = finish(&state, owner, id, stop_reason, Some(usage)).await {
        tracing::warn!("could not finish the reply of chat {id}: {}", cause.message());
    }
}

async fn relay(state: &AppState, owner: Uuid, id: Uuid, provider: &ProviderRow, model: &str, messages: &[ChatMessage], usage: &mut (Option<i64>, Option<i64>)) -> ApiResult<()> {
    let address = provider.address().unwrap_or_default();
    let started = provider.stream_reply(&state.http, model, CHAT_INSTRUCTIONS, messages).await?;
    let mut stream = started.map_err(|error| error.into_api(&provider.name, &address))?;

    loop {
        let next = stream.next().await.map_err(|error| error.into_api(&provider.name, &address))?;
        let Some(delta) = next else {
            return Ok(());
        };

        usage.0 = delta.input_tokens.or(usage.0);
        usage.1 = delta.output_tokens.or(usage.1);

        if !delta.text.is_empty() {
            let chunk = json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": delta.text}}});
            let mut tx = state.pool.begin().await?;
            let event = append_event(&mut tx, id, EventKind::AcpUpdate, &chunk, None).await?;
            tx.commit().await?;

            if let Some(event) = event {
                state.publish(Some(owner), StreamMessage::Event(event));
            }
        }
    }
}

async fn record_failure(state: &AppState, owner: Uuid, id: Uuid, message: &str) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, id, EventKind::Error, &json!({"message": message}), None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    Ok(())
}

async fn finish(state: &AppState, owner: Uuid, id: Uuid, stop_reason: &str, usage: Option<(Option<i64>, Option<i64>)>) -> ApiResult<()> {
    state.hub.forget_chat_turn(id);

    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status != enum_text(&ConversationStatus::Working) {
        return Ok(());
    }

    let event = append_event(&mut tx, id, EventKind::TurnEnd, &json!({"stop_reason": stop_reason}), None).await?;
    let review = set_status(&mut tx, id, ConversationStatus::Review).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(review.to_proto()?));

    let reported = usage.filter(|(input, output)| input.is_some() || output.is_some()).map(|(input, output)| json!({"inputTokens": input.unwrap_or(0), "outputTokens": output.unwrap_or(0)}));

    record_turn(state, id, reported.as_ref()).await
}

pub fn is_chat(row: &ConversationRow) -> bool {
    row.agent == CHAT_AGENT
}

