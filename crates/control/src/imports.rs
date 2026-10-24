use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::activity::{ImportedTurn, record_imported_turns};
use crate::pricing::{Tokens, estimated_cost};
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, Tx, enum_text, parse_enum, text_without_nul, without_nul};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use mycelium_proto::api::{ImportJob, ImportJobState, ImportProblem, ImportRequest, ImportStatus};
use mycelium_proto::conversation::{AgentId, Conversation, ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};
use uuid::Uuid;

const TITLE_LIMIT: usize = 80;
const AGENTS: [AgentId; 2] = [AgentId::Claude, AgentId::Codex];
const WORKING_CAP_MS: i64 = 2 * 60 * 60 * 1000;
const RECENT_WRITE: Duration = Duration::from_secs(300);
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
const EVENTS_PER_INSERT: usize = 500;
const SEARCH_TEXT_LIMIT: usize = 100_000;
const JOB_COLUMNS: &str = "id, node_id, agent, state, total, done, imported, skipped, waiting, failed";

enum Outcome {
    Imported(Conversation),
    Skipped,
    Waiting,
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: Uuid,
    node_id: Uuid,
    agent: String,
    state: String,
    total: i32,
    done: i32,
    imported: i32,
    skipped: i32,
    waiting: i32,
    failed: sqlx::types::Json<Vec<ImportProblem>>,
}

impl JobRow {
    fn to_proto(self) -> ApiResult<ImportJob> {
        Ok(ImportJob {
            id: self.id,
            node_id: self.node_id,
            agent: parse_enum(&self.agent)?,
            state: parse_enum(&self.state)?,
            total: self.total.into(),
            done: self.done.into(),
            imported: self.imported.into(),
            skipped: self.skipped.into(),
            waiting: self.waiting.into(),
            failed: self.failed.0,
        })
    }
}

struct Usage {
    message_id: String,
    model: String,
    tokens: Tokens,
}

enum Item {
    At(DateTime<Utc>),
    Usage(Usage),
    User(String),
    Agent(String),
    Tool { id: String, title: String, kind: &'static str, input: Value },
    ToolResult { id: String, output: String, failed: bool },
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

fn subagent_session(path: &str) -> Option<&str> {
    let file = path.rsplit('/').next()?;

    if !file.starts_with("agent-") || !file.ends_with(".jsonl") {
        return None;
    }

    let folders: Vec<&str> = path.split('/').collect();
    let position = folders.iter().position(|folder| *folder == "subagents")?;

    position.checked_sub(1).map(|before| folders[before])
}

fn parent_transcript_path(path: &str) -> Option<String> {
    let session = subagent_session(path)?;
    let end = path.find(&format!("/{session}/subagents/"))?;

    Some(format!("{}/{session}.jsonl", &path[..end]))
}

fn claude_tool(id: &str, name: &str, input: &Value) -> Item {
    let kind = match name {
        "Bash" => "execute",
        "Read" => "read",
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => "edit",
        "Grep" | "Glob" => "search",
        "WebFetch" | "WebSearch" => "fetch",
        _ => "other",
    };
    let title = input["description"].as_str().unwrap_or(name).to_string();

    Item::Tool { id: id.to_string(), title, kind, input: input.clone() }
}

fn claude_usage(entry: &Value) -> Option<Usage> {
    let message = &entry["message"];
    let usage = message["usage"].as_object()?;
    let model = message["model"].as_str().unwrap_or_default();

    if model.starts_with('<') {
        return None;
    }

    let count = |field: &str| usage.get(field).and_then(Value::as_i64).unwrap_or(0).max(0);
    let cache_write = count("cache_creation_input_tokens");
    let write_1h = usage.get("cache_creation").and_then(|split| split["ephemeral_1h_input_tokens"].as_i64()).unwrap_or(0).clamp(0, cache_write);
    let tokens = Tokens { input: count("input_tokens"), output: count("output_tokens"), cache_read: count("cache_read_input_tokens"), cache_write_5m: cache_write - write_1h, cache_write_1h: write_1h };

    Some(Usage { message_id: message["id"].as_str().unwrap_or_default().to_string(), model: model.to_string(), tokens })
}

fn claude_result(block: &Value) -> Item {
    let content = &block["content"];
    let output = match content.as_array() {
        Some(parts) => parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n"),
        None => content.as_str().unwrap_or_default().to_string(),
    };

    Item::ToolResult { id: block["tool_use_id"].as_str().unwrap_or_default().to_string(), output, failed: block["is_error"].as_bool() == Some(true) }
}

fn codex_tool(id: &str, name: &str, arguments: &str) -> Item {
    let parsed: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let command = match &parsed["command"] {
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).last().map(str::to_string),
        Value::String(command) => Some(command.clone()),
        _ => parsed["cmd"].as_str().map(str::to_string),
    };

    match command {
        Some(command) => Item::Tool { id: id.to_string(), title: command.clone(), kind: "execute", input: json!({"command": command}) },
        None if name == "apply_patch" => Item::Tool { id: id.to_string(), title: name.to_string(), kind: "edit", input: json!({"patch": arguments}) },
        None => Item::Tool { id: id.to_string(), title: name.to_string(), kind: "other", input: parsed },
    }
}

fn codex_output(payload: &Value) -> Item {
    let raw = payload["output"].as_str().unwrap_or_default();
    let output = serde_json::from_str::<Value>(raw).ok().and_then(|value| value["output"].as_str().map(str::to_string)).unwrap_or_else(|| raw.to_string());

    Item::ToolResult { id: payload["call_id"].as_str().unwrap_or_default().to_string(), output, failed: false }
}

fn parse_claude(lines: impl Iterator<Item = impl AsRef<str>>, sidechain: bool) -> Option<Parsed> {
    let mut parsed = Parsed { session_id: String::new(), cwd: String::new(), first_at: None, last_at: None, items: Vec::new() };
    let mut usage_positions: HashMap<String, usize> = HashMap::new();

    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line.as_ref()) else {
            continue;
        };

        if !sidechain && entry["isSidechain"].as_bool() == Some(true) {
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
            parsed.items.push(Item::At(at));
        }

        if let Some(usage) = claude_usage(&entry) {
            match usage_positions.get(&usage.message_id) {
                Some(position) if !usage.message_id.is_empty() => parsed.items[*position] = Item::Usage(usage),
                _ => {
                    usage_positions.insert(usage.message_id.clone(), parsed.items.len());
                    parsed.items.push(Item::Usage(usage));
                }
            }
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
            for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
                parsed.items.push(claude_result(block));
            }

            if !texts.is_empty() {
                parsed.items.push(Item::User(texts.join("\n")));
            }

            continue;
        }

        for block in &blocks {
            match block["type"].as_str() {
                Some("text") => parsed.items.push(Item::Agent(block["text"].as_str().unwrap_or_default().to_string())),
                Some("tool_use") => parsed.items.push(claude_tool(block["id"].as_str().unwrap_or_default(), block["name"].as_str().unwrap_or("Tool"), &block["input"])),
                _ => {}
            }
        }
    }

    (!parsed.session_id.is_empty()).then_some(parsed)
}

fn parse_codex(lines: impl Iterator<Item = impl AsRef<str>>) -> Option<Parsed> {
    let mut parsed = Parsed { session_id: String::new(), cwd: String::new(), first_at: None, last_at: None, items: Vec::new() };
    let mut model = String::new();
    let mut counted = [0i64; 3];

    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line.as_ref()) else {
            continue;
        };
        let payload = &entry["payload"];

        match entry["type"].as_str() {
            Some("session_meta") => {
                parsed.session_id = payload["id"].as_str().unwrap_or_default().to_string();
                parsed.cwd = payload["cwd"].as_str().unwrap_or_default().to_string();
            }
            Some("turn_context") => model = payload["model"].as_str().unwrap_or_default().to_string(),
            Some("event_msg") if payload["type"] == "token_count" => {
                let total = &payload["info"]["total_token_usage"];
                let current = ["input_tokens", "cached_input_tokens", "output_tokens"].map(|field| total[field].as_i64().unwrap_or(0).max(0));

                if total.is_object() && current != counted {
                    let fresh = current.iter().zip(counted).any(|(now, before)| *now < before);
                    let delta = if fresh { current } else { [current[0] - counted[0], current[1] - counted[1], current[2] - counted[2]] };
                    let cached = delta[1].min(delta[0]);

                    counted = current;
                    parsed.items.push(Item::Usage(Usage { message_id: String::new(), model: model.clone(), tokens: Tokens { input: delta[0] - cached, output: delta[2], cache_read: cached, ..Tokens::default() } }));
                }
            }
            Some("response_item") => {
                let at = entry["timestamp"].as_str().and_then(|at| DateTime::parse_from_rfc3339(at).ok()).map(|at| at.with_timezone(&Utc));
                parsed.first_at = parsed.first_at.or(at);
                parsed.last_at = at.or(parsed.last_at);

                if let Some(at) = at {
                    parsed.items.push(Item::At(at));
                }

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
                    Some("function_call") => {
                        parsed.items.push(codex_tool(payload["call_id"].as_str().unwrap_or_default(), payload["name"].as_str().unwrap_or("Tool"), payload["arguments"].as_str().unwrap_or_default()));
                    }
                    Some("custom_tool_call") => {
                        parsed.items.push(codex_tool(payload["call_id"].as_str().unwrap_or_default(), payload["name"].as_str().unwrap_or("Tool"), payload["input"].as_str().unwrap_or_default()));
                    }
                    Some("function_call_output") | Some("custom_tool_call_output") => parsed.items.push(codex_output(payload)),
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
            Item::At(_) | Item::Usage(_) => {}
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
            Item::Tool { id, title, kind, input } => {
                turn_open = true;
                events.push((EventKind::AcpUpdate, json!({"update": {"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "completed", "rawInput": input}})));
            }
            Item::ToolResult { id, output, failed } => {
                let status = if *failed { "failed" } else { "completed" };
                events.push((EventKind::AcpUpdate, json!({"update": {"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status, "content": [{"type": "content", "content": {"type": "text", "text": output}}]}})));
            }
        }
    }

    if turn_open {
        events.push((EventKind::TurnEnd, json!({"stop_reason": "end_turn"})));
    }

    events
}

fn finished_turn(mut turn: ImportedTurn, cost_unknown: bool, started_at: Option<DateTime<Utc>>, last_activity: Option<DateTime<Utc>>) -> ImportedTurn {
    turn.cost_usd = turn.cost_usd.filter(|_| !cost_unknown);
    turn.ended_at = last_activity;
    turn.working_ms = started_at.zip(last_activity).map(|(start, end)| (end - start).num_milliseconds().clamp(0, WORKING_CAP_MS));

    turn
}

fn turns_of(items: &[Item]) -> Vec<ImportedTurn> {
    let mut turns = Vec::new();
    let mut turn_open = false;
    let mut now = None;
    let mut started_at = None;
    let mut last_activity = None;
    let mut turn = ImportedTurn::default();
    let mut cost_unknown = false;

    for item in items {
        match item {
            Item::At(at) => now = Some(*at),
            Item::Usage(usage) => {
                turn.reported = true;
                turn.model = Some(usage.model.clone()).filter(|model| !model.is_empty());
                turn.tokens.add(&usage.tokens);

                match estimated_cost(&usage.model, &usage.tokens) {
                    Some(cost) => turn.cost_usd = Some(turn.cost_usd.unwrap_or(0.0) + cost),
                    None => cost_unknown = true,
                }
            }
            Item::User(text) if text.trim().is_empty() => {}
            Item::User(_) => {
                if turn_open {
                    turns.push(finished_turn(std::mem::take(&mut turn), cost_unknown, started_at, last_activity));
                    turn_open = false;
                }

                turn = ImportedTurn::default();
                cost_unknown = false;
                started_at = now;
                last_activity = now;
            }
            Item::Agent(text) if text.trim().is_empty() => {}
            Item::Agent(_) | Item::Tool { .. } => {
                turn_open = true;
                last_activity = now;
            }
            Item::ToolResult { .. } => last_activity = now,
        }
    }

    if turn_open {
        turns.push(finished_turn(turn, cost_unknown, started_at, last_activity));
    }

    turns
}

async fn importable_paths(state: &AppState, node_id: Uuid, agent: AgentId) -> ApiResult<(i64, Vec<String>)> {
    let paths = sqlx::query_scalar::<_, String>("select path from transcripts where node_id = $1 and starts_with(path, $2) order by path")
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
    let (subagents, sessions): (Vec<String>, Vec<String>) = paths.into_iter().filter(|path| !path.contains("/subagents/") || subagent_session(path).is_some()).partition(|path| subagent_session(path).is_some());
    let parent_sessions: HashSet<String> = sessions.iter().map(|path| session_id_of_path(agent, path)).chain(session_ids.iter().map(|session| session.to_string())).collect();
    let subagents: Vec<String> = subagents.into_iter().filter(|path| subagent_session(path).is_some_and(|session| parent_sessions.contains(session))).collect();
    let found = (sessions.len() + subagents.len()) as i64;
    let mut pending: Vec<String> = sessions
        .into_iter()
        .filter(|path| !imported_paths.contains(path.as_str()) && !session_ids.contains(session_id_of_path(agent, path).as_str()))
        .chain(subagents.into_iter().filter(|path| !imported_paths.contains(path.as_str())))
        .collect();
    pending.sort();

    Ok((found, pending))
}

async fn parsed_pure_imports(state: &AppState) -> anyhow::Result<Vec<(Uuid, Parsed)>> {
    let imports = sqlx::query_as::<_, (Uuid, Uuid, String, String)>(
        "select c.id, c.node_id, c.agent, c.imported_from from conversations c
         where c.imported_from is not null and c.side_kind is null
           and not exists (select 1 from events e where e.conversation_id = c.id and (e.node_seq is not null or (e.kind = 'user_message' and e.payload ? 'prompt_id')))",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut found = Vec::new();

    for (id, node_id, agent, path) in imports {
        let full = state.config.data_dir.join("transcripts").join(node_id.to_string()).join(&path);
        let codex = agent == enum_text(&AgentId::Codex);
        let Ok(parsed) = tokio::task::spawn_blocking(move || parse_file(&full, if codex { AgentId::Codex } else { AgentId::Claude }, false)).await else {
            continue;
        };
        let Ok(parsed) = parsed else {
            continue;
        };

        if let Some(parsed) = parsed {
            found.push((id, parsed));
        }
    }

    Ok(found)
}

pub async fn repair_activity_dates(state: &AppState) -> anyhow::Result<()> {
    let claimed = sqlx::query("insert into boot_repairs (name) values ('import_activity_dates') on conflict do nothing").execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    for (id, parsed) in parsed_pure_imports(state).await? {
        if let Some(last_at) = parsed.last_at {
            sqlx::query("update conversations set updated_at = $2 where id = $1").bind(id).bind(last_at).execute(&state.pool).await?;
        }
    }

    Ok(())
}

pub async fn repair_import_usage(state: &AppState) -> anyhow::Result<()> {
    let claimed = sqlx::query("insert into boot_repairs (name) values ('import_usage') on conflict do nothing").execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    for (id, parsed) in parsed_pure_imports(state).await? {
        let mut tx = state.pool.begin().await?;

        sqlx::query("delete from usage_turns where conversation_id = $1").bind(id).execute(&mut *tx).await?;
        record_imported_turns(&mut tx, id, &turns_of(&parsed.items)).await.map_err(|cause| anyhow::anyhow!("{cause:?}"))?;
        tx.commit().await?;
    }

    Ok(())
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

pub async fn jobs(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Vec<ImportJob>>> {
    let rows = sqlx::query_as::<_, JobRow>(sqlx::AssertSqlSafe(format!(
        "select distinct on (node_id, agent) {JOB_COLUMNS} from import_jobs where node_id in (select id from nodes where revoked_at is null) order by node_id, agent, started_at desc"
    )))
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(JobRow::to_proto).collect::<ApiResult<Vec<_>>>()?))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<ImportRequest>) -> ApiResult<(StatusCode, Json<ImportJob>)> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(request.node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    let (_, pending) = importable_paths(&state, request.node_id, request.agent).await?;
    let started = sqlx::query_as::<_, JobRow>(sqlx::AssertSqlSafe(format!(
        "insert into import_jobs (owner, node_id, agent, total) values ($1, $2, $3, $4)
         on conflict (node_id, agent) where state = 'running' do nothing
         returning {JOB_COLUMNS}"
    )))
    .bind(user.id)
    .bind(request.node_id)
    .bind(enum_text(&request.agent))
    .bind(pending.len() as i32)
    .fetch_optional(&state.pool)
    .await?;

    let Some(started) = started else {
        let running = sqlx::query_as::<_, JobRow>(sqlx::AssertSqlSafe(format!("select {JOB_COLUMNS} from import_jobs where node_id = $1 and agent = $2 and state = 'running'")))
            .bind(request.node_id)
            .bind(enum_text(&request.agent))
            .fetch_one(&state.pool)
            .await?;

        return Ok((StatusCode::ACCEPTED, Json(running.to_proto()?)));
    };

    let job = started.to_proto()?;
    tokio::spawn(run_job(state, user.id, job.clone(), pending));

    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn run_job(state: AppState, owner: Uuid, mut job: ImportJob, pending: Vec<String>) {
    let mut saved_at = Instant::now();

    for path in pending {
        let node_id = job.node_id;
        let agent = job.agent;
        let outcome = tokio::spawn(import_file(state.clone(), owner, node_id, agent, path.clone())).await;

        job.done += 1;

        match outcome {
            Ok(Ok(Outcome::Imported(conversation))) => {
                job.imported += 1;
                state.publish(Some(owner), StreamMessage::Conversation(conversation));
            }
            Ok(Ok(Outcome::Skipped)) => job.skipped += 1,
            Ok(Ok(Outcome::Waiting)) => job.waiting += 1,
            Ok(Err(reason)) => job.failed.push(ImportProblem { path, reason }),
            Err(_) => job.failed.push(ImportProblem { path, reason: "The file could not be read.".to_string() }),
        }

        if saved_at.elapsed() >= PROGRESS_EVERY {
            save_progress(&state, owner, &job).await;
            saved_at = Instant::now();
        }
    }

    job.state = ImportJobState::Finished;
    save_progress(&state, owner, &job).await;
}

async fn save_progress(state: &AppState, owner: Uuid, job: &ImportJob) {
    let saved = sqlx::query(
        "update import_jobs set state = $2, done = $3, imported = $4, skipped = $5, waiting = $6, failed = $7, finished_at = case when $2 = 'finished' then now() end where id = $1",
    )
    .bind(job.id)
    .bind(enum_text(&job.state))
    .bind(job.done as i32)
    .bind(job.imported as i32)
    .bind(job.skipped as i32)
    .bind(job.waiting as i32)
    .bind(sqlx::types::Json(&job.failed))
    .execute(&state.pool)
    .await;

    if let Err(cause) = saved {
        tracing::warn!("could not record the progress of import {}: {cause}", job.id);
    }

    state.publish(Some(owner), StreamMessage::ImportJob(job.clone()));
}

pub async fn end_interrupted(state: &AppState) -> anyhow::Result<()> {
    sqlx::query("update import_jobs set state = 'finished', finished_at = now() where state = 'running'").execute(&state.pool).await?;

    Ok(())
}

fn store_failure(path: &str, cause: impl std::fmt::Display) -> String {
    tracing::warn!("could not import {path}: {cause}");

    "The database could not store it. Try again.".to_string()
}

fn parse_file(path: &Path, agent: AgentId, sidechain: bool) -> std::io::Result<Option<Parsed>> {
    let file = std::fs::File::open(path)?;
    let lines = BufReader::new(file).split(b'\n').map_while(Result::ok).map(|line| String::from_utf8_lossy(&line).into_owned());

    Ok(match agent {
        AgentId::Claude => parse_claude(lines, sidechain),
        AgentId::Codex => parse_codex(lines),
    })
}

fn searchable(text: &str) -> String {
    text.chars().take(SEARCH_TEXT_LIMIT).collect()
}

async fn insert_events(tx: &mut Tx<'_>, conversation_id: Uuid, events: &[(EventKind, Value)], from: usize) -> sqlx::Result<()> {
    let mut seq = 0i64;
    let mut agent_text = String::new();
    let mut entries: Vec<(i64, String)> = Vec::new();

    for batch in events.chunks(EVENTS_PER_INSERT) {
        let mut seqs = Vec::new();
        let mut kinds = Vec::new();
        let mut payloads = Vec::new();

        for (kind, payload) in batch {
            seq += 1;
            let is_new = seq as usize > from;

            match kind {
                EventKind::UserMessage => {
                    agent_text.clear();

                    if let Some(text) = payload["text"].as_str().map(str::trim).filter(|text| !text.is_empty()).filter(|_| is_new) {
                        entries.push((seq, searchable(text)));
                    }
                }
                EventKind::TurnEnd if is_new && !agent_text.is_empty() => entries.push((seq, searchable(&agent_text))),
                EventKind::AcpUpdate if payload["update"]["sessionUpdate"] == "agent_message_chunk" => {
                    if agent_text.chars().count() < SEARCH_TEXT_LIMIT {
                        agent_text.push_str(payload["update"]["content"]["text"].as_str().unwrap_or_default());
                    }
                }
                _ => {}
            }

            if is_new {
                seqs.push(seq);
                kinds.push(enum_text(kind));
                payloads.push(payload.clone());
            }
        }

        if seqs.is_empty() {
            continue;
        }

        sqlx::query("insert into events (conversation_id, seq, kind, payload) select $1, * from unnest($2::bigint[], $3::text[], $4::jsonb[])")
            .bind(conversation_id)
            .bind(seqs)
            .bind(kinds)
            .bind(payloads)
            .execute(&mut **tx)
            .await?;
    }

    for batch in entries.chunks(EVENTS_PER_INSERT) {
        sqlx::query("insert into search_entries (conversation_id, seq, text) select $1, * from unnest($2::bigint[], $3::text[]) on conflict do nothing")
            .bind(conversation_id)
            .bind(batch.iter().map(|(seq, _)| *seq).collect::<Vec<_>>())
            .bind(batch.iter().map(|(_, text)| text.clone()).collect::<Vec<_>>())
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

async fn import_file(state: AppState, owner: Uuid, node_id: Uuid, agent: AgentId, path: String) -> Result<Outcome, String> {
    let full = state.config.data_dir.join("transcripts").join(node_id.to_string()).join(&path);
    let metadata = tokio::fs::metadata(&full).await.map_err(|_| "The control plane no longer has this file.".to_string())?;
    let idle = metadata.modified().ok().and_then(|at| at.elapsed().ok()).unwrap_or_default();

    if idle < RECENT_WRITE {
        return Ok(Outcome::Waiting);
    }

    let still_uploading = sqlx::query_scalar::<_, bool>("select coalesce(bytes < size_bytes, false) from transcripts where node_id = $1 and path = $2")
        .bind(node_id)
        .bind(&path)
        .fetch_optional(&state.pool)
        .await
        .map_err(|cause| store_failure(&path, cause))?;

    if still_uploading == Some(true) {
        return Err("Still uploading.".to_string());
    }

    if subagent_session(&path).is_some() {
        return store_subagent(&state, node_id, &path).await;
    }

    let parsed = tokio::task::spawn_blocking(move || parse_file(&full, agent, false)).await.map_err(|_| "The file could not be read.".to_string())?;
    let parsed = parsed.map_err(|_| "The file could not be read.".to_string())?;
    let Some(parsed) = parsed else {
        return Err("There is no conversation in this file.".to_string());
    };
    let Parsed { session_id, cwd, first_at, last_at, items } = parsed;
    let session_id = text_without_nul(&session_id);

    let taken = sqlx::query_scalar::<_, bool>("select exists (select 1 from conversations where node_id = $1 and acp_session_id = $2)")
        .bind(node_id)
        .bind(&session_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|cause| store_failure(&path, cause))?;

    if taken {
        return Ok(Outcome::Skipped);
    }

    let mut events = events_of(&items);
    events.iter_mut().for_each(|(_, payload)| without_nul(payload));
    let title = items
        .iter()
        .find_map(|item| match item {
            Item::User(text) if !text.trim().is_empty() => Some(text.trim().lines().next().unwrap_or_default().trim().chars().take(TITLE_LIMIT).collect::<String>()),
            _ => None,
        })
        .unwrap_or_else(|| "Untitled conversation".to_string());
    let title = text_without_nul(&title);
    let turns = turns_of(&items);
    drop(items);

    let cwd = text_without_nul(&cwd);
    let created_at = first_at.unwrap_or_else(Utc::now);
    let updated_at = last_at.unwrap_or(created_at);

    let mut tx = state.pool.begin().await.map_err(|cause| store_failure(&path, cause))?;
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "insert into conversations (owner, node_id, agent, cwd, title, status, acp_session_id, imported_from, created_at, updated_at, archived_at, project_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, (select id from projects where owner = $1 and node_id = $2 and ($4 = path or left($4, length(path) + 1) = path || '/') order by length(path) desc limit 1))
         on conflict (node_id, imported_from) where imported_from is not null do nothing
         returning {CONVERSATION_COLUMNS}"
    )))
    .bind(owner)
    .bind(node_id)
    .bind(enum_text(&agent))
    .bind(&cwd)
    .bind(title)
    .bind(enum_text(&ConversationStatus::Done))
    .bind(&session_id)
    .bind(&path)
    .bind(created_at)
    .bind(updated_at)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|cause| store_failure(&path, cause))?;

    let Some(row) = row else {
        return Ok(Outcome::Skipped);
    };

    insert_events(&mut tx, row.id, &events, 0).await.map_err(|cause| store_failure(&path, cause))?;
    record_imported_turns(&mut tx, row.id, &turns).await.map_err(|cause| store_failure(&path, cause.message()))?;
    tx.commit().await.map_err(|cause| store_failure(&path, cause))?;

    let conversation = row.to_proto().map_err(|cause| store_failure(&path, cause.message()))?;

    Ok(Outcome::Imported(conversation))
}

async fn store_subagent(state: &AppState, node_id: Uuid, path: &str) -> Result<Outcome, String> {
    let Some(session) = subagent_session(path) else {
        return Ok(Outcome::Skipped);
    };
    let parent = sqlx::query_as::<_, (Uuid, Uuid, String, Option<Uuid>, Option<DateTime<Utc>>)>(
        "select id, owner, cwd, project_id, archived_at from conversations where node_id = $1 and acp_session_id = $2 and parent_id is null limit 1",
    )
    .bind(node_id)
    .bind(session)
    .fetch_optional(&state.pool)
    .await
    .map_err(|cause| store_failure(path, cause))?;

    let Some((parent_id, owner, cwd, project_id, archived_at)) = parent else {
        return Ok(Outcome::Waiting);
    };

    let existing = sqlx::query_as::<_, (Uuid, i64)>("select id, (select count(*) from events where conversation_id = conversations.id) from conversations where node_id = $1 and imported_from = $2")
        .bind(node_id)
        .bind(path)
        .fetch_optional(&state.pool)
        .await
        .map_err(|cause| store_failure(path, cause))?;
    let transcripts = state.config.data_dir.join("transcripts").join(node_id.to_string());
    let full = transcripts.join(path);
    let parent_full = parent_transcript_path(path).map(|parent_path| transcripts.join(parent_path));
    let needs_description = existing.is_none();
    let read = tokio::task::spawn_blocking(move || {
        let parsed = parse_file(&full, AgentId::Claude, true)?;
        let prompt = parsed.as_ref().and_then(|parsed| {
            parsed.items.iter().find_map(|item| match item {
                Item::User(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
                _ => None,
            })
        });
        let description = parent_full.filter(|_| needs_description).zip(prompt).and_then(|(parent_full, prompt)| {
            let parent = parse_file(&parent_full, AgentId::Claude, false).ok().flatten()?;

            parent.items.iter().find_map(|item| match item {
                Item::Tool { input, .. } if input["prompt"].as_str().is_some_and(|candidate| candidate.trim() == prompt) => input["description"].as_str().map(|description| description.trim().to_string()),
                _ => None,
            })
        });

        Ok::<_, std::io::Error>((parsed, description))
    })
    .await
    .map_err(|_| "The file could not be read.".to_string())?;
    let (parsed, description) = read.map_err(|_| "The file could not be read.".to_string())?;
    let Some(parsed) = parsed else {
        return Err("There is no conversation in this file.".to_string());
    };

    let mut events = events_of(&parsed.items);
    events.iter_mut().for_each(|(_, payload)| without_nul(payload));

    if let Some((id, stored)) = existing {
        if stored as usize == events.len() {
            return Ok(Outcome::Skipped);
        }

        let mut tx = state.pool.begin().await.map_err(|cause| store_failure(path, cause))?;
        let appended = events.len() > stored as usize;

        if !appended {
            sqlx::query("delete from events where conversation_id = $1").bind(id).execute(&mut *tx).await.map_err(|cause| store_failure(path, cause))?;
            sqlx::query("delete from search_entries where conversation_id = $1").bind(id).execute(&mut *tx).await.map_err(|cause| store_failure(path, cause))?;
        }

        insert_events(&mut tx, id, &events, if appended { stored as usize } else { 0 }).await.map_err(|cause| store_failure(path, cause))?;
        sqlx::query("delete from usage_turns where conversation_id = $1").bind(id).execute(&mut *tx).await.map_err(|cause| store_failure(path, cause))?;
        record_imported_turns(&mut tx, id, &turns_of(&parsed.items)).await.map_err(|cause| store_failure(path, cause.message()))?;
        let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set updated_at = $2 where id = $1 returning {CONVERSATION_COLUMNS}")))
            .bind(id)
            .bind(parsed.last_at.unwrap_or_else(Utc::now))
            .fetch_one(&mut *tx)
            .await
            .map_err(|cause| store_failure(path, cause))?;
        tx.commit().await.map_err(|cause| store_failure(path, cause))?;
        state.publish(Some(owner), StreamMessage::Conversation(row.to_proto().map_err(|cause| store_failure(path, cause.message()))?));

        return Ok(Outcome::Skipped);
    }

    let title = description
        .filter(|description| !description.is_empty())
        .or_else(|| {
            parsed.items.iter().find_map(|item| match item {
                Item::User(text) if !text.trim().is_empty() => Some(text.trim().lines().next().unwrap_or_default().trim().to_string()),
                _ => None,
            })
        })
        .map(|title| text_without_nul(&title.chars().take(TITLE_LIMIT).collect::<String>()))
        .unwrap_or_else(|| "Subagent".to_string());
    let created_at = parsed.first_at.unwrap_or_else(Utc::now);
    let updated_at = parsed.last_at.unwrap_or(created_at);

    let mut tx = state.pool.begin().await.map_err(|cause| store_failure(path, cause))?;
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "insert into conversations (owner, node_id, agent, cwd, title, status, imported_from, parent_id, side_kind, project_id, created_at, updated_at, archived_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8, 'subagent', $9, $10, $11, $12)
         on conflict (node_id, imported_from) where imported_from is not null do nothing
         returning {CONVERSATION_COLUMNS}"
    )))
    .bind(owner)
    .bind(node_id)
    .bind(enum_text(&AgentId::Claude))
    .bind(&cwd)
    .bind(title)
    .bind(enum_text(&ConversationStatus::Done))
    .bind(path)
    .bind(parent_id)
    .bind(project_id)
    .bind(created_at)
    .bind(updated_at)
    .bind(archived_at)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|cause| store_failure(path, cause))?;

    let Some(row) = row else {
        return Ok(Outcome::Skipped);
    };

    insert_events(&mut tx, row.id, &events, 0).await.map_err(|cause| store_failure(path, cause))?;
    record_imported_turns(&mut tx, row.id, &turns_of(&parsed.items)).await.map_err(|cause| store_failure(path, cause.message()))?;
    tx.commit().await.map_err(|cause| store_failure(path, cause))?;

    Ok(Outcome::Imported(row.to_proto().map_err(|cause| store_failure(path, cause.message()))?))
}

pub async fn sync_subagent(state: &AppState, node_id: Uuid, path: &str) {
    if subagent_session(path).is_none() {
        return;
    }

    match store_subagent(state, node_id, path).await {
        Ok(Outcome::Imported(conversation)) => state.publish(Some(conversation.owner), StreamMessage::Conversation(conversation)),
        Ok(_) => {}
        Err(reason) => tracing::warn!("could not follow subagent {path}: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_transcripts_become_messages_tool_calls_and_turn_ends() {
        let text = [
            r#"{"type":"user","sessionId":"s1","cwd":"/work/app","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"Fix the build\nplease"}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:05Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"}]}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:06Z","message":{"role":"assistant","content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo build","description":"Build the crate"}}]}}"#,
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-01T10:00:08Z","message":{"role":"assistant","content":[{"type":"text","text":"Fixed."}]}}"#,
            r#"{"type":"user","sessionId":"s1","isMeta":true,"timestamp":"2026-10-01T10:00:09Z","message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-01T10:01:00Z","message":{"role":"user","content":[{"type":"text","text":"Thanks"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"sessionId":"s1","timestamp":"2026-10-01T10:01:01Z","message":{"role":"assistant","content":"side"}}"#,
        ]
        .join("\n");

        let parsed = parse_claude(text.lines(), false).unwrap();
        let events = events_of(&parsed.items);
        let kinds: Vec<EventKind> = events.iter().map(|(kind, _)| *kind).collect();

        assert_eq!((parsed.session_id.as_str(), parsed.cwd.as_str()), ("s1", "/work/app"));
        assert_eq!(kinds, [EventKind::UserMessage, EventKind::AcpUpdate, EventKind::AcpUpdate, EventKind::AcpUpdate, EventKind::AcpUpdate, EventKind::TurnEnd, EventKind::UserMessage]);
        assert_eq!(events[2].1["update"]["toolCallId"], "t1");
        assert_eq!(events[2].1["update"]["kind"], "execute");
        assert_eq!(events[2].1["update"]["rawInput"]["command"], "cargo build");
        assert_eq!(events[3].1["update"]["sessionUpdate"], "tool_call_update");
        assert_eq!(events[3].1["update"]["content"][0]["content"]["text"], "ok");
        assert_eq!(parsed.last_at.unwrap().to_rfc3339(), "2026-10-01T10:01:00+00:00");
    }

    #[test]
    fn codex_rollouts_skip_injected_context_and_read_function_calls() {
        let text = [
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/work/app"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
            r#"{"timestamp":"2026-10-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Add a test"}]}}"#,
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"{\"output\":\"1 passed\",\"metadata\":{}}"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:04Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Added."}]}}"#,
        ]
        .join("\n");

        let parsed = parse_codex(text.lines()).unwrap();
        let events = events_of(&parsed.items);

        assert_eq!((parsed.session_id.as_str(), parsed.cwd.as_str()), ("abc", "/work/app"));
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].1["text"], "Add a test");
        assert_eq!(events[1].1["update"]["kind"], "execute");
        assert_eq!(events[1].1["update"]["rawInput"]["command"], "cargo test");
        assert_eq!(events[2].1["update"]["content"][0]["content"]["text"], "1 passed");
        assert_eq!(events[4].0, EventKind::TurnEnd);
    }

    #[test]
    fn session_ids_are_read_from_transcript_file_names() {
        assert_eq!(session_id_of_path(AgentId::Claude, ".claude/projects/p/abc-123.jsonl"), "abc-123");
        assert_eq!(session_id_of_path(AgentId::Codex, ".codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-0199aaaa-bbbb-cccc-dddd-eeeeffff0000.jsonl"), "0199aaaa-bbbb-cccc-dddd-eeeeffff0000");
    }

    fn usage_entry(id: &str, at: &str, model: &str, output: i64) -> String {
        json!({"type": "assistant", "sessionId": "s1", "timestamp": at, "message": {"id": id, "role": "assistant", "model": model, "content": [{"type": "text", "text": "Working."}], "usage": {"input_tokens": 10, "output_tokens": output, "cache_creation_input_tokens": 300, "cache_read_input_tokens": 2000, "cache_creation": {"ephemeral_5m_input_tokens": 100, "ephemeral_1h_input_tokens": 200}}}}).to_string()
    }

    fn user_entry(at: &str, text: &str) -> String {
        json!({"type": "user", "sessionId": "s1", "timestamp": at, "message": {"role": "user", "content": text}}).to_string()
    }

    #[test]
    fn claude_usage_is_counted_once_per_message_and_priced_from_the_list() {
        let text = [
            user_entry("2026-10-01T10:00:00Z", "Go"),
            usage_entry("m1", "2026-10-01T10:00:05Z", "claude-opus-5", 4),
            usage_entry("m1", "2026-10-01T10:00:06Z", "claude-opus-5", 40),
            usage_entry("m2", "2026-10-01T10:00:09Z", "claude-opus-5", 60),
        ]
        .join("\n");

        let turns = turns_of(&parse_claude(text.lines(), false).unwrap().items);
        let expected = (20.0 * 5.0 + 100.0 * 25.0 + 4000.0 * 0.5 + 200.0 * 6.25 + 400.0 * 10.0) / 1_000_000.0;

        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tokens, Tokens { input: 20, output: 100, cache_read: 4000, cache_write_5m: 200, cache_write_1h: 400 });
        assert_eq!(turns[0].model.as_deref(), Some("claude-opus-5"));
        assert!((turns[0].cost_usd.unwrap() - expected).abs() < 1e-12);
        assert!(turns[0].reported);
    }

    #[test]
    fn a_turn_with_an_unpriced_model_keeps_its_tokens_and_has_no_cost() {
        let text = [user_entry("2026-10-01T10:00:00Z", "Go"), usage_entry("m1", "2026-10-01T10:00:05Z", "some-local-model", 5), usage_entry("m2", "2026-10-01T10:00:06Z", "<synthetic>", 0)].join("\n");

        let turns = turns_of(&parse_claude(text.lines(), false).unwrap().items);

        assert_eq!(turns[0].tokens.output, 5);
        assert_eq!(turns[0].cost_usd, None);
    }

    #[test]
    fn each_turn_carries_its_own_usage_and_end_time() {
        let text = [
            user_entry("2026-10-01T10:00:00Z", "One"),
            usage_entry("m1", "2026-10-01T10:00:05Z", "claude-opus-5", 1),
            user_entry("2026-10-02T09:00:00Z", "Two"),
            usage_entry("m2", "2026-10-02T09:00:05Z", "claude-opus-5", 2),
        ]
        .join("\n");

        let turns = turns_of(&parse_claude(text.lines(), false).unwrap().items);

        assert_eq!(turns.iter().map(|turn| turn.tokens.output).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(turns.iter().map(|turn| turn.ended_at.unwrap().to_rfc3339()).collect::<Vec<_>>(), ["2026-10-01T10:00:05+00:00", "2026-10-02T09:00:05+00:00"]);
    }

    #[test]
    fn time_working_runs_from_the_prompt_to_the_last_agent_or_tool_entry_and_is_capped() {
        let tool_result = json!({"type": "user", "sessionId": "s1", "timestamp": "2026-10-01T10:00:40Z", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}).to_string();
        let text = [
            user_entry("2026-10-01T10:00:00Z", "Short"),
            usage_entry("m1", "2026-10-01T10:00:05Z", "claude-opus-5", 1),
            tool_result,
            user_entry("2026-10-01T12:00:00Z", "Long"),
            usage_entry("m2", "2026-10-01T18:00:00Z", "claude-opus-5", 1),
        ]
        .join("\n");

        let turns = turns_of(&parse_claude(text.lines(), false).unwrap().items);

        assert_eq!(turns[0].working_ms, Some(40_000));
        assert_eq!(turns[1].working_ms, Some(WORKING_CAP_MS));
    }

    #[test]
    fn codex_usage_is_the_growth_of_the_running_total_with_cached_input_split_out() {
        let token_count = |total: Value| json!({"timestamp": "2026-10-01T10:00:04Z", "type": "event_msg", "payload": {"type": "token_count", "info": {"total_token_usage": total}}}).to_string();
        let text = [
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/work/app"}}"#.to_string(),
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5.4"}}"#.to_string(),
            r#"{"timestamp":"2026-10-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Add a test"}]}}"#.to_string(),
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Added."}]}}"#.to_string(),
            token_count(json!({"input_tokens": 1000, "cached_input_tokens": 600, "output_tokens": 50, "total_tokens": 1050})),
            token_count(json!({"input_tokens": 1000, "cached_input_tokens": 600, "output_tokens": 50, "total_tokens": 1050})),
            token_count(json!({"input_tokens": 1500, "cached_input_tokens": 900, "output_tokens": 80, "total_tokens": 1580})),
            r#"{"timestamp":"2026-10-01T10:00:05Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#.to_string(),
        ]
        .join("\n");

        let turns = turns_of(&parse_codex(text.lines()).unwrap().items);

        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tokens, Tokens { input: 600, output: 80, cache_read: 900, ..Tokens::default() });
        assert_eq!(turns[0].model.as_deref(), Some("gpt-5.4"));
        assert!((turns[0].cost_usd.unwrap() - (600.0 * 2.5 + 80.0 * 15.0 + 900.0 * 0.25) / 1_000_000.0).abs() < 1e-12);
        assert_eq!(turns[0].working_ms, Some(1000));
    }
}
