use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use mycelium_proto::conversation::AgentId;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

const MAX_READ: u64 = 2 * 1024 * 1024;

pub async fn record(
    state: &AppState,
    node_id: Uuid,
    path: &str,
    start: i64,
    stored: i64,
) -> ApiResult<()> {
    if !path.ends_with(".jsonl") {
        return Ok(());
    }

    let sessions = [crate::imports::session_id_of_path(AgentId::Claude, path), crate::imports::session_id_of_path(AgentId::Codex, path)];
    let conversation = sqlx::query_scalar::<_, Uuid>("select id from conversations where node_id = $1 and acp_session_id = any($2) and exists(select 1 from goals where conversation_id = conversations.id or conversation_id = conversations.parent_id) limit 1").bind(node_id).bind(&sessions).fetch_optional(&state.pool).await?;
    let Some(conversation) = conversation else {
        return Ok(());
    };
    let relative = crate::transcripts::safe_relative(path)
        .ok_or_else(|| ApiError::bad_request("transcript_path", "Choose an agent transcript."))?;
    let full = state
        .config
        .data_dir
        .join("transcripts")
        .join(node_id.to_string())
        .join(relative);
    let previous = sqlx::query_scalar::<_, i64>(
        "select parsed_offset from transcript_usage_positions where node_id = $1 and path = $2",
    )
    .bind(node_id)
    .bind(path)
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(start);
    let offset = if previous > stored { 0 } else { previous };
    let mut file = tokio::fs::File::open(full)
        .await
        .map_err(ApiError::internal)?;
    file.seek(std::io::SeekFrom::Start(offset as u64))
        .await
        .map_err(ApiError::internal)?;
    let mut bytes = Vec::new();
    file.take(MAX_READ)
        .read_to_end(&mut bytes)
        .await
        .map_err(ApiError::internal)?;
    let complete = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        None if bytes.len() as u64 == MAX_READ => bytes.len(),
        None => return Ok(()),
    };
    let mut position = offset;
    let mut changed = false;
    let mut tx = state.pool.begin().await?;

    for line in bytes[..complete].split_inclusive(|byte| *byte == b'\n') {
        position += line.len() as i64;
        let Ok(row) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let Some(at) = row["timestamp"]
            .as_str()
            .and_then(|at| at.parse::<DateTime<Utc>>().ok())
        else {
            continue;
        };
        let (key, tokens, last, cumulative) =
            if row["type"] == "event_msg" && row["payload"]["type"] == "token_count" {
                let info = &row["payload"]["info"];
                let Some(total) = info["total_token_usage"]["total_tokens"].as_i64() else {
                    continue;
                };
                (
                    position.to_string(),
                    total.max(0),
                    info["last_token_usage"]["total_tokens"]
                        .as_i64()
                        .unwrap_or(0)
                        .max(0),
                    true,
                )
            } else if row["type"] == "assistant" {
                let message = &row["message"];
                let Some(key) = message["id"].as_str() else {
                    continue;
                };
                let usage = &message["usage"];

                if !usage.is_object() {
                    continue;
                }

                let tokens = [
                    "input_tokens",
                    "output_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .fold(0i64, |total, field| {
                    total.saturating_add(usage[*field].as_i64().unwrap_or(0).max(0))
                });
                (key.to_string(), tokens, tokens, false)
            } else {
                continue;
            };
        sqlx::query("insert into conversation_usage_samples (conversation_id, source, sample_key, position, recorded_at, tokens, last_tokens, cumulative) values ($1,$2,$3,$4,$5,$6,$7,$8) on conflict (conversation_id, source, sample_key) do update set tokens = excluded.tokens, last_tokens = excluded.last_tokens, recorded_at = excluded.recorded_at")
            .bind(conversation).bind(path).bind(key).bind(position).bind(at).bind(tokens).bind(last).bind(cumulative).execute(&mut *tx).await?;
        changed = true;
    }

    sqlx::query("insert into transcript_usage_positions (node_id, path, parsed_offset) values ($1,$2,$3) on conflict (node_id,path) do update set parsed_offset = excluded.parsed_offset").bind(node_id).bind(path).bind(offset + complete as i64).execute(&mut *tx).await?;
    tx.commit().await?;

    if changed {
        crate::goals::refresh(state, conversation).await?;
    }

    Ok(())
}
