use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, enum_text};
use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use mycelium_proto::api::{ImportRequest, ImportResult, ImportStatus};
use mycelium_proto::conversation::{AgentId, ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

const TITLE_LIMIT: usize = 80;
const AGENTS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];

enum Item {
    User(String),
    Agent(String),
    Tool { id: String, name: String },
}

struct Parsed {
    session_id: String,
    cwd: String,
    first_at: Option<DateTime<Utc>>,
    last_at: Option<DateTime<Utc>>,
    items: Vec<Item>,
}

fn root_of(agent: AgentId) -> &'static str {
    match agent {
        AgentId::Claude => ".claude/projects/",
        AgentId::Codex => ".codex/sessions/",
    }
}

fn session_id_of_path(agent: AgentId, path: &str) -> String {
    let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".jsonl");

    match agent {
        AgentId::Claude => stem.to_string(),
        AgentId::Codex => stem.chars().rev().take(36).collect::<Vec<_>>().into_iter().rev().collect(),
    }
}

fn parse_claude(text: &str) -> Option<Parsed> {
    let mut parsed = Parsed { session_id: String::new(), cwd: String::new(), first_at: None, last_at: None, items: Vec::new() };

    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if entry["isSidechain"].as_bool() == Some(true) {
            continue;
        }

        if parsed.session_id.is_empty() {
            parsed.session_id = entry["sessionId"].as_str().unwrap_or_default().to_string();
        }

        if parsed.cwd.is_empty() {
            parsed.cwd = entry["cwd"].as_str().unwrap_or_default().to_string();
        }

        let kind = entry["type"].as_str().unwrap_or_default();

        if kind != "user" && kind != "assistant" {
            continue;
        }

        if entry["isMeta"].as_bool() == Some(true) {
            continue;
        }

        if let Some(at) = entry["timestamp"].as_str().and_then(|at| DateTime::parse_from_rfc3339(at).ok()) {
            let at = at.with_timezone(&Utc);
            parsed.first_at = parsed.first_at.or(Some(at));
            parsed.last_at = Some(at);
        }

        let content = &entry["message"]["content"];

        if let Some(text) = content.as_str() {
            if kind == "user" {
                parsed.items.push(Item::User(text.to_string()));
            } else {
                parsed.items.push(Item::Agent(text.to_string()));
            }

            continue;
        }

        let blocks = content.as_array().cloned().unwrap_or_default();
        let texts: Vec<&str> = blocks.iter().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();

        if kind == "user" {
            if !texts.is_empty() {
                parsed.items.push(Item::User(texts.join("\n")));
            }

            continue;
        }

        for block in &blocks {
            match block["type"].as_str() {
                Some("text") => parsed.items.push(Item::Agent(block["text"].as_str().unwrap_or_default().to_string())),
                Some("tool_use") => parsed.items.push(Item::Tool { id: block["id"].as_str().unwrap_or_default().to_string(), name: block["name"].as_str().unwrap_or("Tool").to_string() }),
                _ => {}
            }
        }
    }

    (!parsed.session_id.is_empty()).then_some(parsed)
}

fn parse_codex(text: &str) -> Option<Parsed> {
    let mut parsed = Parsed { session_id: String::new(), cwd: String::new(), first_at: None, last_at: None, items: Vec::new() };

    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let payload = &entry["payload"];

        match entry["type"].as_str() {
            Some("session_meta") => {
                parsed.session_id = payload["id"].as_str().unwrap_or_default().to_string();
                parsed.cwd = payload["cwd"].as_str().unwrap_or_default().to_string();
            }
            Some("response_item") => {
                let at = entry["timestamp"].as_str().and_then(|at| DateTime::parse_from_rfc3339(at).ok()).map(|at| at.with_timezone(&Utc));
                parsed.first_at = parsed.first_at.or(at);
                parsed.last_at = at.or(parsed.last_at);

                match payload["type"].as_str() {
                    Some("message") => {
                        let blocks = payload["content"].as_array().cloned().unwrap_or_default();
                        let text: Vec<&str> = blocks.iter().filter_map(|block| block["text"].as_str()).collect();
                        let text = text.join("\n");

                        match payload["role"].as_str() {
                            Some("user") if !text.starts_with("<environment_context>") && !text.starts_with("<user_instructions>") && !text.starts_with("# AGENTS.md instructions") => {
                                parsed.items.push(Item::User(text));
                            }
                            Some("assistant") => parsed.items.push(Item::Agent(text)),
                            _ => {}
                        }
                    }
                    Some("function_call") | Some("custom_tool_call") => {
                        parsed.items.push(Item::Tool { id: payload["call_id"].as_str().unwrap_or_default().to_string(), name: payload["name"].as_str().unwrap_or("Tool").to_string() });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    (!parsed.session_id.is_empty()).then_some(parsed)
}

fn events_of(items: &[Item]) -> Vec<(EventKind, Value)> {
    let mut events = Vec::new();
    let mut turn_open = false;

    for item in items {
        match item {
            Item::User(text) if text.trim().is_empty() => {}
            Item::User(text) => {
                if turn_open {
                    events.push((EventKind::TurnEnd, json!({"stop_reason": "end_turn"})));
                    turn_open = false;
                }

                events.push((EventKind::UserMessage, json!({"text": text})));
            }
            Item::Agent(text) if text.trim().is_empty() => {}
            Item::Agent(text) => {
                turn_open = true;
                events.push((EventKind::AcpUpdate, json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}})));
            }
            Item::Tool { id, name } => {
                turn_open = true;
                events.push((EventKind::AcpUpdate, json!({"update": {"sessionUpdate": "tool_call", "toolCallId": id, "title": name, "kind": "other", "status": "completed"}})));
            }
        }
    }

    if turn_open {
        events.push((EventKind::TurnEnd, json!({"stop_reason": "end_turn"})));
    }

    events
}

async fn importable_paths(state: &AppState, node_id: Uuid, agent: AgentId) -> ApiResult<(i64, Vec<String>)> {
    let paths = sqlx::query_scalar::<_, String>("select path from transcripts where node_id = $1 and starts_with(path, $2) and position('/subagents/' in path) = 0 order by path")
        .bind(node_id)
        .bind(root_of(agent))
        .fetch_all(&state.pool)
        .await?;
    let known = sqlx::query_as::<_, (Option<String>, Option<String>)>("select imported_from, acp_session_id from conversations where node_id = $1")
        .bind(node_id)
        .fetch_all(&state.pool)
        .await?;
    let imported_paths: HashSet<&str> = known.iter().filter_map(|(path, _)| path.as_deref()).collect();
    let session_ids: HashSet<&str> = known.iter().filter_map(|(_, session)| session.as_deref()).collect();
    let found = paths.len() as i64;
    let pending = paths.into_iter().filter(|path| !imported_paths.contains(path.as_str()) && !session_ids.contains(session_id_of_path(agent, path).as_str())).collect();

    Ok((found, pending))
}

pub async fn list(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<ImportStatus>>> {
    let nodes = sqlx::query_scalar::<_, Uuid>("select id from nodes where revoked_at is null order by name").fetch_all(&state.pool).await?;
    let mut statuses = Vec::new();

    for node_id in nodes {
        for agent in AGENTS {
            let (found, pending) = importable_paths(&state, node_id, agent).await?;
            statuses.push(ImportStatus { node_id, agent, found, imported: found - pending.len() as i64 });
        }
    }

    Ok(Json(statuses))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<ImportRequest>) -> ApiResult<Json<ImportResult>> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(request.node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    let (_, pending) = importable_paths(&state, request.node_id, request.agent).await?;
    let mut imported = 0;

    for path in pending {
        let full = state.config.data_dir.join("transcripts").join(request.node_id.to_string()).join(&path);
        let Ok(bytes) = tokio::fs::read(&full).await else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let parsed = match request.agent {
            AgentId::Claude => parse_claude(&text),
            AgentId::Codex => parse_codex(&text),
        };
        let Some(parsed) = parsed else {
            continue;
        };

        let taken = sqlx::query_scalar::<_, bool>("select exists (select 1 from conversations where node_id = $1 and acp_session_id = $2)").bind(request.node_id).bind(&parsed.session_id).fetch_one(&state.pool).await?;

        if taken {
            continue;
        }

        let events = events_of(&parsed.items);
        let title = parsed
            .items
            .iter()
            .find_map(|item| match item {
                Item::User(text) if !text.trim().is_empty() => Some(text.trim().lines().next().unwrap_or_default().trim().chars().take(TITLE_LIMIT).collect::<String>()),
                _ => None,
            })
            .unwrap_or_else(|| "Untitled conversation".to_string());
        let created_at = parsed.first_at.unwrap_or_else(Utc::now);
        let updated_at = parsed.last_at.unwrap_or(created_at);

        let mut tx = state.pool.begin().await?;
        let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
            "insert into conversations (owner, node_id, agent, cwd, title, status, acp_session_id, imported_from, created_at, updated_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             on conflict (node_id, imported_from) where imported_from is not null do nothing
             returning {CONVERSATION_COLUMNS}"
        )))
        .bind(user.id)
        .bind(request.node_id)
        .bind(enum_text(&request.agent))
        .bind(&parsed.cwd)
        .bind(title)
        .bind(enum_text(&ConversationStatus::Done))
        .bind(&parsed.session_id)
        .bind(&path)
        .bind(created_at)
        .bind(updated_at)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            continue;
        };

        for (kind, payload) in &events {
            append_event(&mut tx, row.id, *kind, payload, None).await?;
        }

        tx.commit().await?;
        imported += 1;
        state.publish(Some(user.id), StreamMessage::Conversation(row.to_proto()?));
    }

    Ok(Json(ImportResult { imported }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_transcripts_become_messages_tool_calls_and_turn_ends() {
        let text = [
            r#"{"type":"user","sessionId":"s1","cwd":"/work/app","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"Fix the build\nplease"}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:05Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"}]}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:06Z","message":{"role":"assistant","content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:08Z","message":{"role":"assistant","content":[{"type":"text","text":"Fixed."}]}}"#,
            r#"{"type":"user","sessionId":"s1","isMeta":true,"timestamp":"2026-10-01T10:00:09Z","message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-01T10:01:00Z","message":{"role":"user","content":[{"type":"text","text":"Thanks"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"sessionId":"s1","timestamp":"2026-10-01T10:01:01Z","message":{"role":"assistant","content":"side"}}"#,
        ]
        .join("\n");

        let parsed = parse_claude(&text).unwrap();
        let events = events_of(&parsed.items);
        let kinds: Vec<EventKind> = events.iter().map(|(kind, _)| *kind).collect();

        assert_eq!((parsed.session_id.as_str(), parsed.cwd.as_str()), ("s1", "/work/app"));
        assert_eq!(kinds, [EventKind::UserMessage, EventKind::AcpUpdate, EventKind::AcpUpdate, EventKind::AcpUpdate, EventKind::TurnEnd, EventKind::UserMessage]);
        assert_eq!(events[2].1["update"]["toolCallId"], "t1");
        assert_eq!(parsed.last_at.unwrap().to_rfc3339(), "2026-10-01T10:01:00+00:00");
    }

    #[test]
    fn codex_rollouts_skip_injected_context_and_read_function_calls() {
        let text = [
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/work/app"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
            r#"{"timestamp":"2026-10-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Add a test"}]}}"#,
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{}"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:04Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Added."}]}}"#,
        ]
        .join("\n");

        let parsed = parse_codex(&text).unwrap();
        let events = events_of(&parsed.items);

        assert_eq!((parsed.session_id.as_str(), parsed.cwd.as_str()), ("abc", "/work/app"));
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].1["text"], "Add a test");
        assert_eq!(events[1].1["update"]["title"], "shell");
        assert_eq!(events[3].0, EventKind::TurnEnd);
    }

    #[test]
    fn session_ids_are_read_from_transcript_file_names() {
        assert_eq!(session_id_of_path(AgentId::Claude, ".claude/projects/p/abc-123.jsonl"), "abc-123");
        assert_eq!(session_id_of_path(AgentId::Codex, ".codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-0199aaaa-bbbb-cccc-dddd-eeeeffff0000.jsonl"), "0199aaaa-bbbb-cccc-dddd-eeeeffff0000");
    }
}
