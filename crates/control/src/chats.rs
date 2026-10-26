use crate::continuation;
use crate::attachments;
use crate::auth::AuthUser;
use crate::activity::record_turn;
use crate::chat_tools::{self, Toolbox};
use crate::conversations::derived_title;
use crate::error::{ApiError, ApiResult};
use crate::instructions::extensions_of;
use crate::model_providers::{self, ChatImage, ChatMessage, ProviderError, ProviderRow, ReportedUsage, ToolCall, ToolResult};
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, enqueue, enum_text, find_owned, lock_owned, publish_queue, set_status};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use mycelium_proto::api::CreateChatRequest;
use base64::Engine;
use mycelium_proto::conversation::{Attachment, Conversation, ConversationStatus, EventKind, SideKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

pub const CHAT_AGENT: &str = "chat";
const CHAT_HOST: &str = "control-plane";
const MAX_TOOL_ROUNDS: usize = 20;
const TOOLS_REJECTED_NOTE: &str = "This model does not take tools, so it answered without them.";
const NOT_APPROVED_TEXT: &str = "The person did not approve this call, so it did not run.";
const STOPPED_TEXT: &str = "The reply was stopped before this call ran.";
const CHAT_INSTRUCTIONS: &str = "You are chatting inside Mycelium. Reply in Markdown.
When an interface helps the person more than prose, you may include one fenced block whose language is mycelium-ui, containing valid JSON with no comments for a small component tree. Keep the explanation outside the block.
Components, each an object with a \"type\":
- {\"type\":\"column\",\"children\":[...]} and {\"type\":\"row\",\"children\":[...]} lay out other components.
- {\"type\":\"heading\",\"text\":\"...\"} and {\"type\":\"text\",\"text\":\"...\"}.
- {\"type\":\"table\",\"columns\":[\"...\"],\"rows\":[[\"...\"]]}.
- {\"type\":\"bar_chart\",\"title\":\"...\",\"bars\":[{\"label\":\"...\",\"value\":12,\"display\":\"12 ms\"}]}.
- {\"type\":\"progress\",\"label\":\"...\",\"value\":3,\"max\":8,\"display\":\"3 of 8 steps\"}; max defaults to 100 and display to the percentage.
- {\"type\":\"button\",\"label\":\"...\",\"action\":{...}} where the action is {\"type\":\"send_message\",\"text\":\"...\"}, which sends the text as the person's next message, or {\"type\":\"side_chat\",\"text\":\"...\"}, which opens a linked chat started with that text.
A button label says what happens when it is pressed.";

pub fn generated_ui_instructions() -> String {
    let schema = CHAT_INSTRUCTIONS.split_once("When an interface helps").map(|(_, rest)| rest).unwrap_or_default();

    format!("When an interface helps{schema}\n- In a workspace conversation a button may also use the action {{\"type\":\"open_file\",\"path\":\"...\"}}, which opens that file in the workspace.")
}

pub const RICH_MARKDOWN_INSTRUCTIONS: &str = "Mycelium renders these Markdown extensions, so use them where they help and not otherwise: task lists (- [ ] and - [x]), tables with column alignment, GitHub callouts (> [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]), footnotes ([^1]), superscript (^x^), subscript (~x~), strikethrough, and fenced code with a language for highlighting (kotlin, rust, ts, js, python, shell, json, yaml, toml, diff). Raw HTML is not rendered.";

pub async fn chat_node(state: &AppState, owner: Uuid) -> ApiResult<Uuid> {
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
    sqlx::query("update conversations set status = 'review' where agent = $1 and status in ('working', 'input')").bind(CHAT_AGENT).execute(&state.pool).await?;

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
    if !crate::boot::is_ready(state) {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "starting", "The control plane is still starting. Wait a moment, then send your message again."));
    }

    withdraw_waiting_call(state, owner, id).await?;

    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status == enum_text(&ConversationStatus::Working) {
        return Err(ApiError::conflict("chat_busy", "This chat is still replying. Wait for it to finish, or stop it, then send your message."));
    }

    let Some(provider_id) = row.provider_id else {
        return Err(ApiError::conflict("provider_removed", "This chat's model provider was removed. Choose another model in the message box to carry on."));
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
    let messages = history(state, id).await?;

    let turn = tokio::spawn(run_turn(state.clone(), owner, id, provider, model, messages));
    state.hub.track_chat_turn(id, turn.abort_handle());

    Ok(conversation)
}

pub async fn send_or_queue(state: &AppState, owner: Uuid, id: Uuid, text: &str) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status != enum_text(&ConversationStatus::Working) {
        drop(tx);
        send(state, owner, id, text, Vec::new()).await?;

        return Ok(());
    }

    enqueue(&mut tx, id, text, &[]).await?;
    tx.commit().await?;

    publish_queue(state, owner, id).await
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

async fn history(state: &AppState, id: Uuid) -> ApiResult<Vec<ChatMessage>> {
    let marker = continuation::last_handoff_marker(state, id).await?;
    let rows = sqlx::query_as::<_, (String, sqlx::types::Json<Value>)>("select kind, payload from events where conversation_id = $1 and seq > $2 and kind in ('user_message', 'acp_update', 'turn_end') order by seq").bind(id).bind(marker.unwrap_or(0)).fetch_all(&state.pool).await?;
    let mut messages: Vec<ChatMessage> = Vec::new();

    if let Some(marker) = marker {
        let content = continuation::handoff(state, id, marker).await?;

        messages.push(ChatMessage { role: "user", content, ..ChatMessage::default() });
    }

    let mut said = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut results: Vec<ToolResult> = Vec::new();

    for (kind, sqlx::types::Json(payload)) in rows {
        let update = &payload["update"];
        let update_kind = update["sessionUpdate"].as_str().unwrap_or_default();

        if kind == "acp_update" && update_kind == "tool_call_update" {
            let status = update["status"].as_str().unwrap_or_default();
            let call_id = update["toolCallId"].as_str().unwrap_or_default();

            if matches!(status, "completed" | "failed") && calls.iter().any(|call| call.id == call_id) {
                let content: Vec<&str> = update["content"].as_array().into_iter().flatten().filter_map(|block| block["content"]["text"].as_str()).collect();

                results.push(ToolResult { call_id: call_id.to_string(), content: content.join("\n"), is_error: status == "failed" });
            }

            continue;
        }

        if kind == "acp_update" && update_kind == "tool_call" {
            if !results.is_empty() {
                messages.extend(finished_round(&mut said, &mut calls, &mut results));
            }

            calls.push(ToolCall { id: update["toolCallId"].as_str().unwrap_or_default().to_string(), name: update["title"].as_str().unwrap_or_default().to_string(), input: update["rawInput"].clone() });

            continue;
        }

        if !calls.is_empty() {
            messages.extend(finished_round(&mut said, &mut calls, &mut results));
        }

        if kind == "acp_update" {
            if update_kind == "agent_message_chunk" {
                said.push_str(update["content"]["text"].as_str().unwrap_or_default());
            }

            continue;
        }

        if !said.trim().is_empty() {
            messages.push(ChatMessage { role: "assistant", content: said.trim().to_string(), ..ChatMessage::default() });
        }

        said.clear();

        if kind == "user_message" {
            let attachments: Vec<Attachment> = serde_json::from_value(payload["attachments"].clone()).unwrap_or_default();
            let text = payload["text"].as_str().unwrap_or_default().trim().to_string();
            let (content, images) = chat_content(state, text, attachments).await?;

            if content.is_empty() && images.is_empty() {
                continue;
            }

            match messages.last_mut() {
                Some(last) if last.role == "user" && last.tool_results.is_empty() => {
                    last.content = format!("{}\n\n{content}", last.content).trim().to_string();
                    last.images.extend(images);
                }
                _ => messages.push(ChatMessage { role: "user", content, images, ..ChatMessage::default() }),
            }
        }
    }

    if !calls.is_empty() {
        messages.extend(finished_round(&mut said, &mut calls, &mut results));
    }

    if !said.trim().is_empty() {
        messages.push(ChatMessage { role: "assistant", content: said.trim().to_string(), ..ChatMessage::default() });
    }

    Ok(messages)
}

fn finished_round(said: &mut String, calls: &mut Vec<ToolCall>, results: &mut Vec<ToolResult>) -> [ChatMessage; 2] {
    let answered: Vec<ToolResult> = calls
        .iter()
        .map(|call| results.iter().find(|result| result.call_id == call.id).cloned().unwrap_or_else(|| ToolResult { call_id: call.id.clone(), content: NOT_APPROVED_TEXT.to_string(), is_error: true }))
        .collect();
    let asked = ChatMessage { role: "assistant", content: said.trim().to_string(), tool_calls: std::mem::take(calls), ..ChatMessage::default() };

    said.clear();
    results.clear();

    [asked, ChatMessage { role: "user", tool_results: answered, ..ChatMessage::default() }]
}

fn without_tool_blocks(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .map(|mut message| {
            let called: Vec<String> = message.tool_calls.drain(..).map(|call| format!("[Called {} with {}]", call.name, call.input)).collect();
            let returned: Vec<String> = message.tool_results.drain(..).map(|result| format!("[Tool result: {}]", result.content)).collect();

            message.content = [vec![message.content.clone()], called, returned].concat().into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n");

            message
        })
        .collect()
}

async fn withdraw_waiting_call(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let waiting = sqlx::query_scalar::<_, bool>("select status = 'input' from conversations where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?.unwrap_or(false);

    if !waiting {
        return Ok(());
    }

    state.hub.abort_chat_turn(id);
    chat_tools::close_open_calls(state, owner, id, NOT_APPROVED_TEXT).await?;

    let mut tx = state.pool.begin().await?;
    let review = set_status(&mut tx, id, ConversationStatus::Review).await?;
    tx.commit().await?;

    state.publish(Some(owner), StreamMessage::Conversation(review.to_proto()?));

    Ok(())
}

pub async fn stop(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    find_owned(&state.pool, id, owner).await?;

    if state.hub.abort_chat_turn(id) {
        chat_tools::close_open_calls(state, owner, id, STOPPED_TEXT).await?;
        finish(state, owner, id, "cancelled", None).await?;
    }

    Ok(())
}

fn run_turn(state: AppState, owner: Uuid, id: Uuid, provider: ProviderRow, model: String, messages: Vec<ChatMessage>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let mut usage = ReportedUsage::default();
        let outcome = relay(&state, owner, id, &provider, &model, messages, &mut usage).await;
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
    })
}

async fn relay(state: &AppState, owner: Uuid, id: Uuid, provider: &ProviderRow, model: &str, messages: Vec<ChatMessage>, usage: &mut ReportedUsage) -> ApiResult<()> {
    let address = provider.address().unwrap_or_default();
    let extensions = extensions_of(state, owner).await?;
    let base_instructions = match extensions.rich_markdown {
        true => format!("{CHAT_INSTRUCTIONS}\n{RICH_MARKDOWN_INSTRUCTIONS}"),
        false => CHAT_INSTRUCTIONS.to_string(),
    };
    let mut toolbox = Toolbox::open(state, owner, id).await?;
    let mut messages = messages;
    let mut tools_on = true;
    let mut rounds = 0;

    loop {
        let definitions = if tools_on { toolbox.definitions() } else { Vec::new() };
        let instructions = if tools_on { format!("{base_instructions}\n{}", toolbox.instructions()) } else { base_instructions.clone() };
        let started = provider.stream_reply(&state.http, model, &instructions, &messages, &definitions).await?;
        let mut stream = match started {
            Ok(stream) => stream,
            Err(ProviderError::ToolsRejected) if tools_on => {
                tools_on = false;
                messages = without_tool_blocks(messages);
                record(state, owner, id, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "note", "content": {"type": "text", "text": TOOLS_REJECTED_NOTE}}})).await?;

                continue;
            }
            Err(error) => return Err(error.into_api(&provider.name, &address)),
        };
        let mut said = String::new();
        let mut calls = Vec::new();
        let previous_usage = usage.clone();

        loop {
            let next = stream.next().await.map_err(|error| error.into_api(&provider.name, &address))?;
            let Some(delta) = next else {
                break;
            };

            usage.absorb(&delta, &previous_usage);

            if delta.input_tokens.is_some() || delta.cache_read_tokens.is_some() || delta.cache_write_tokens.is_some() {
                let used = usage.input.unwrap_or(0) - previous_usage.input.unwrap_or(0)
                    + usage.cache_read.unwrap_or(0) - previous_usage.cache_read.unwrap_or(0)
                    + usage.cache_write.unwrap_or(0) - previous_usage.cache_write.unwrap_or(0);
                record(state, owner, id, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "usage_update", "used": used}})).await?;
            }

            calls.extend(delta.tool_calls);

            if !delta.text.is_empty() {
                said.push_str(&delta.text);
                record(state, owner, id, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": delta.text}}})).await?;
            }
        }

        if calls.is_empty() {
            return Ok(());
        }

        rounds += 1;
        let results = chat_tools::run_calls(state, owner, id, &mut toolbox, &calls).await?;
        messages.push(ChatMessage { role: "assistant", content: said, tool_calls: calls, ..ChatMessage::default() });
        messages.push(ChatMessage { role: "user", tool_results: results, ..ChatMessage::default() });

        if rounds == MAX_TOOL_ROUNDS {
            let note = format!("Stopped after {MAX_TOOL_ROUNDS} rounds of tool calls. Send a message to carry on.");

            return record(state, owner, id, EventKind::AcpUpdate, &json!({"update": {"sessionUpdate": "note", "content": {"type": "text", "text": note}}})).await;
        }
    }
}

pub async fn record(state: &AppState, owner: Uuid, id: Uuid, kind: EventKind, payload: &Value) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, id, kind, payload, None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    Ok(())
}

async fn record_failure(state: &AppState, owner: Uuid, id: Uuid, message: &str) -> ApiResult<()> {
    record(state, owner, id, EventKind::Error, &json!({"message": message})).await
}

async fn finish(state: &AppState, owner: Uuid, id: Uuid, stop_reason: &str, usage: Option<ReportedUsage>) -> ApiResult<()> {
    state.hub.forget_chat_turn(id);

    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, id, owner).await?;

    if row.status != enum_text(&ConversationStatus::Working) && row.status != enum_text(&ConversationStatus::Input) {
        return Ok(());
    }

    let event = append_event(&mut tx, id, EventKind::TurnEnd, &json!({"stop_reason": stop_reason}), None).await?;
    let review = set_status(&mut tx, id, ConversationStatus::Review).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(review.to_proto()?));

    let reported = usage.filter(ReportedUsage::is_reported).map(|usage| json!({"inputTokens": usage.input.unwrap_or(0), "outputTokens": usage.output.unwrap_or(0), "cachedReadTokens": usage.cache_read.unwrap_or(0), "cachedWriteTokens": usage.cache_write.unwrap_or(0)}));

    record_turn(state, id, reported.as_ref()).await?;

    let queued = sqlx::query_as::<_, (String, sqlx::types::Json<Vec<Attachment>>)>("delete from queued_prompts where id = (select id from queued_prompts where conversation_id = $1 order by position limit 1 for update) returning text, attachments")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let Some((text, sqlx::types::Json(attachments))) = queued else {
        return Ok(());
    };

    publish_queue(state, owner, id).await?;
    send(state, owner, id, &text, attachments).await?;

    Ok(())
}

pub fn is_chat(row: &ConversationRow) -> bool {
    row.agent == CHAT_AGENT
}

