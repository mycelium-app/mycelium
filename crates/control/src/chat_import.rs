use crate::auth::AuthUser;
use crate::chats::{CHAT_AGENT, chat_node, index_title};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, enum_text};
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use chrono::{DateTime, Utc};
use mycelium_proto::api::ImportResult;
use mycelium_proto::conversation::{ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::HashMap;

pub const MAX_EXPORT_BYTES: usize = 256 * 1024 * 1024;
const TOOL_RESULT_LIMIT: usize = 500;
const UNTITLED: &str = "Imported chat";

enum Part {
    Thought(String),
    Said(String),
}

fn not_an_export() -> ApiError {
    ApiError::bad_request("not_claude_export", "The file is not a Claude export. Choose the conversations.json from your Claude data export.")
}

fn conversations_in(body: &[u8]) -> ApiResult<Vec<Value>> {
    let parsed = serde_json::from_slice::<Value>(body).map_err(|_| not_an_export())?;
    let candidates = match parsed {
        Value::Array(items) => items,
        object => vec![object],
    };
    let conversations: Vec<Value> = candidates.into_iter().filter(|item| item["uuid"].is_string() && item["chat_messages"].is_array()).collect();

    match conversations.is_empty() {
        true => Err(not_an_export()),
        false => Ok(conversations),
    }
}

fn active_branch(conversation: &Value) -> Vec<&Value> {
    let messages: Vec<&Value> = conversation["chat_messages"].as_array().map(|all| all.iter().collect()).unwrap_or_default();
    let by_uuid: HashMap<&str, &Value> = messages.iter().filter_map(|message| message["uuid"].as_str().map(|uuid| (uuid, *message))).collect();
    let leaf = conversation["current_leaf_message_uuid"].as_str().and_then(|uuid| by_uuid.get(uuid));

    let Some(leaf) = leaf else {
        let mut ordered = messages;
        ordered.sort_by_key(|message| message["index"].as_i64().unwrap_or(0));

        return ordered;
    };

    let mut branch = vec![*leaf];

    while let Some(parent) = branch.last().and_then(|message| message["parent_message_uuid"].as_str()).and_then(|uuid| by_uuid.get(uuid)) {
        if branch.len() > messages.len() {
            break;
        }

        branch.push(*parent);
    }

    branch.reverse();

    branch
}

fn block_text(block: &Value) -> Option<String> {
    let text = block["text"].as_str().or_else(|| block["thinking"].as_str());

    text.map(str::to_string)
}

fn result_text(block: &Value) -> String {
    let content = match &block["content"] {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().filter_map(|item| item["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    let shortened: String = content.chars().take(TOOL_RESULT_LIMIT).collect();

    match content.chars().count() > TOOL_RESULT_LIMIT {
        true => format!("{shortened}..."),
        false => shortened,
    }
}

fn parts_of(message: &Value) -> Vec<Part> {
    let blocks = message["content"].as_array().cloned().unwrap_or_default();
    let mut parts = Vec::new();

    for block in &blocks {
        match block["type"].as_str().unwrap_or_default() {
            "thinking" => parts.extend(block_text(block).filter(|text| !text.trim().is_empty()).map(Part::Thought)),
            "tool_use" => parts.push(Part::Said(format!("Used the tool {}.", block["name"].as_str().unwrap_or("unknown")))),
            "tool_result" => {
                let output = result_text(block);

                if !output.trim().is_empty() {
                    parts.push(Part::Said(format!("Tool result: {output}")));
                }
            }
            _ => parts.extend(block_text(block).filter(|text| !text.trim().is_empty()).map(Part::Said)),
        }
    }

    if parts.is_empty() {
        let text = message["text"].as_str().unwrap_or_default();

        if !text.trim().is_empty() {
            parts.push(Part::Said(text.to_string()));
        }
    }

    parts
}

fn user_text(message: &Value) -> String {
    let said: Vec<String> = parts_of(message).into_iter().filter_map(|part| if let Part::Said(text) = part { Some(text) } else { None }).collect();
    let attachments = message["attachments"].as_array().cloned().unwrap_or_default();
    let mut text = said.join("\n\n");

    for attachment in &attachments {
        let name = attachment["file_name"].as_str().unwrap_or("attachment");

        if let Some(extracted) = attachment["extracted_content"].as_str().filter(|extracted| !extracted.trim().is_empty()) {
            text = format!("{text}\n\n{name}:\n```\n{extracted}\n```");
        }
    }

    text.trim().to_string()
}

fn moment(value: &Value) -> Option<DateTime<Utc>> {
    value.as_str().and_then(|text| DateTime::parse_from_rfc3339(text).ok()).map(|at| at.with_timezone(&Utc))
}

async fn record(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, conversation_id: uuid::Uuid, kind: EventKind, payload: Value, at: DateTime<Utc>) -> ApiResult<()> {
    let event = append_event(tx, conversation_id, kind, &payload, None).await?;

    if let Some(event) = event {
        sqlx::query("update events set created_at = $3 where conversation_id = $1 and seq = $2").bind(conversation_id).bind(event.seq).bind(at).execute(&mut **tx).await?;
    }

    Ok(())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, body: Bytes) -> ApiResult<Json<ImportResult>> {
    let conversations = conversations_in(&body)?;
    let node_id = chat_node(&state, user.id).await?;
    let mut imported = 0;
    let mut skipped = 0;

    for conversation in &conversations {
        let source = format!("claude:{}", conversation["uuid"].as_str().unwrap_or_default());
        let name = conversation["name"].as_str().map(str::trim).filter(|name| !name.is_empty());
        let created_at = moment(&conversation["created_at"]).unwrap_or_else(Utc::now);
        let updated_at = moment(&conversation["updated_at"]).unwrap_or(created_at);
        let model = conversation["model"].as_str().filter(|model| !model.is_empty());

        let mut tx = state.pool.begin().await?;
        let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
            "insert into conversations (owner, node_id, agent, cwd, title, title_user_set, status, model, import_source, created_at, updated_at)
             values ($1, $2, $3, '', $4, $5, $6, $7, $8, $9, $10)
             on conflict (owner, import_source) where import_source is not null do nothing
             returning {CONVERSATION_COLUMNS}"
        )))
        .bind(user.id)
        .bind(node_id)
        .bind(CHAT_AGENT)
        .bind(name.unwrap_or(UNTITLED))
        .bind(name.is_some())
        .bind(enum_text(&ConversationStatus::Done))
        .bind(model)
        .bind(&source)
        .bind(created_at)
        .bind(updated_at)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            skipped += 1;
            continue;
        };

        for message in active_branch(conversation) {
            let at = moment(&message["created_at"]).unwrap_or(created_at);

            if message["sender"] == "human" {
                let text = user_text(message);

                if !text.is_empty() {
                    record(&mut tx, row.id, EventKind::UserMessage, json!({"text": text}), at).await?;
                }

                continue;
            }

            let parts = parts_of(message);

            for part in &parts {
                let (update, text) = match part {
                    Part::Thought(text) => ("agent_thought_chunk", text),
                    Part::Said(text) => ("agent_message_chunk", text),
                };

                record(&mut tx, row.id, EventKind::AcpUpdate, json!({"update": {"sessionUpdate": update, "content": {"type": "text", "text": text}}}), at).await?;
            }

            if !parts.is_empty() {
                record(&mut tx, row.id, EventKind::TurnEnd, json!({"stop_reason": "end_turn"}), at).await?;
            }
        }

        sqlx::query("update conversations set updated_at = $2 where id = $1").bind(row.id).bind(updated_at).execute(&mut *tx).await?;
        tx.commit().await?;
        index_title(&state, row.id, &row.title).await?;
        imported += 1;
        state.publish(Some(user.id), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(Json(ImportResult { imported, skipped }))
}
