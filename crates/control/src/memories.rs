use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Memory, MemoryBank, MemorySource, ModelClass, MoveMemoryRequest, SaveMemoryBankRequest, UpdateMemoryRequest};
use mycelium_proto::stream::StreamMessage;
use mycelium_proto::link::ControlToNode;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

pub const MEMORY_COLUMNS: &str = "id, text, scopes, created_by, conversation_id, model, agent, node_id, node_name, account, project_id, project_name, message_seq, tool_call_id, bank_id, created_at, updated_at";

#[derive(Deserialize)]
pub struct ListQuery {
    query: Option<String>,
}

#[derive(sqlx::FromRow)]
pub struct MemoryRow {
    pub id: Uuid,
    pub text: String,
    pub scopes: Vec<String>,
    pub created_by: String,
    pub conversation_id: Option<Uuid>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub node_id: Option<Uuid>,
    pub node_name: Option<String>,
    pub account: Option<String>,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub message_seq: Option<i64>,
    pub tool_call_id: Option<String>,
    pub bank_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl MemoryRow {
    pub fn to_proto(self) -> Memory {
        Memory {
            id: self.id,
            text: self.text,
            scopes: self.scopes,
            created_by: self.created_by,
            conversation_id: self.conversation_id,
            bank_id: self.bank_id,
            source: MemorySource {
                model: self.model,
                agent: self.agent,
                node_id: self.node_id,
                node_name: self.node_name,
                account: self.account,
                project_id: self.project_id,
                project_name: self.project_name,
                conversation_title: None,
                message_seq: self.message_seq,
                tool_call_id: self.tool_call_id,
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct BankRow {
    id: Uuid,
    name: String,
    model_class: String,
    group_ids: Vec<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl BankRow {
    fn to_proto(self) -> ApiResult<MemoryBank> {
        Ok(MemoryBank { id: self.id, name: self.name, model_class: parse_enum(&self.model_class)?, group_ids: self.group_ids, created_at: self.created_at })
    }
}

pub async fn usable_banks(pool: &PgPool, owner: Uuid, model_class: ModelClass, group_id: Option<Uuid>) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, Vec<Uuid>)>("select id, name, model_class, group_ids from memory_banks where owner = $1 order by name").bind(owner).fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .filter(|(_, _, class, group_ids)| {
            let allowed: ModelClass = parse_enum(class).unwrap_or(ModelClass::Private);

            model_class.rank() <= allowed.rank() && (group_ids.is_empty() || group_id.is_some_and(|group_id| group_ids.contains(&group_id)))
        })
        .map(|(id, name, _, _)| (id, name))
        .collect())
}

pub fn any_word(query: &str) -> String {
    query.split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty() && !word.eq_ignore_ascii_case("or")).collect::<Vec<&str>>().join(" or ")
}

pub async fn search(pool: &PgPool, owner: Uuid, query: Option<&str>, visible_scopes: Option<&[String]>, banks: Option<&[Uuid]>, limit: i64) -> Result<Vec<MemoryRow>, sqlx::Error> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());

    sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!(
        "select {MEMORY_COLUMNS} from memories
         where owner = $5 and ($1::text is null or to_tsvector('english', text) @@ websearch_to_tsquery('english', $1))
           and ($2::text[] is null or bank_id is not null or scopes && $2)
           and ($4::uuid[] is null or bank_id is null or bank_id = any($4))
         order by case when $1::text is null then 0 else ts_rank(to_tsvector('english', text), websearch_to_tsquery('english', $1)) end desc, created_at desc
         limit $3"
    )))
    .bind(query)
    .bind(visible_scopes)
    .bind(limit)
    .bind(banks)
    .bind(owner)
    .fetch_all(pool)
    .await
}

pub async fn omitted_groups(pool: &PgPool, owner: Uuid, query: &str, visible_scopes: &[String]) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as::<_, (String, i64)>(
        "select g.name, count(distinct m.id) from memories m
         join groups g on 'group:' || g.id = any(m.scopes)
         where to_tsvector('english', m.text) @@ websearch_to_tsquery('english', $1) and not (m.scopes && $2) and m.bank_id is null and m.owner = $3
         group by g.name order by g.name",
    )
    .bind(query.trim())
    .bind(visible_scopes)
    .bind(owner)
    .fetch_all(pool)
    .await
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Memory>>> {
    let rows = search(&state.pool, user.id, query.query.as_deref(), None, None, i64::MAX).await?;
    let titles: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>("select id, title from conversations where id = any($1)")
        .bind(rows.iter().filter_map(|row| row.conversation_id).collect::<Vec<Uuid>>())
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .collect();
    let memories = rows
        .into_iter()
        .map(|row| {
            let title = row.conversation_id.and_then(|conversation_id| titles.get(&conversation_id)).filter(|title| !title.trim().is_empty()).cloned();
            let mut memory = row.to_proto();
            memory.source.conversation_title = title;

            memory
        })
        .collect();

    Ok(Json(memories))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<UpdateMemoryRequest>) -> ApiResult<Json<Memory>> {
    let text = request.text.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter what to remember."));
    }

    let row = sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!("update memories set text = $2, updated_at = now() where id = $1 and owner = $3 returning {MEMORY_COLUMNS}")))
        .bind(id)
        .bind(text)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;

    let memory = row.ok_or_else(|| ApiError::not_found("That memory"))?.to_proto();
    state.publish(Some(user.id), StreamMessage::Memory(memory.clone()));

    Ok(Json(memory))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from memories where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That memory"));
    }

    state.publish(Some(user.id), StreamMessage::MemoryRemoved { memory_id: id });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_banks(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<MemoryBank>>> {
    let rows = sqlx::query_as::<_, BankRow>("select id, name, model_class, group_ids, created_at from memory_banks where owner = $1 order by name").bind(user.id).fetch_all(&state.pool).await?;
    let banks = rows.into_iter().map(BankRow::to_proto).collect::<ApiResult<Vec<MemoryBank>>>()?;

    Ok(Json(banks))
}

pub async fn create_bank(State(state): State<AppState>, user: AuthUser, Json(request): Json<SaveMemoryBankRequest>) -> ApiResult<(StatusCode, Json<MemoryBank>)> {
    user.require_full_access()?;
    let name = bank_name(&request)?;
    let saved = sqlx::query_as::<_, BankRow>("insert into memory_banks (owner, name, model_class, group_ids) values ($1, $2, $3, $4) returning id, name, model_class, group_ids, created_at")
        .bind(user.id)
        .bind(name)
        .bind(enum_text(&request.model_class))
        .bind(&request.group_ids)
        .fetch_one(&state.pool)
        .await;
    let row = match saved {
        Ok(row) => row,
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(bank_name_taken()),
        Err(error) => return Err(error.into()),
    };
    let bank = row.to_proto()?;
    state.publish(Some(user.id), StreamMessage::MemoryBank(bank.clone()));

    Ok((StatusCode::CREATED, Json(bank)))
}

pub async fn update_bank(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SaveMemoryBankRequest>) -> ApiResult<Json<MemoryBank>> {
    user.require_full_access()?;
    let name = bank_name(&request)?;
    let saved = sqlx::query_as::<_, BankRow>("update memory_banks set name = $3, model_class = $4, group_ids = $5 where id = $1 and owner = $2 returning id, name, model_class, group_ids, created_at")
        .bind(id)
        .bind(user.id)
        .bind(name)
        .bind(enum_text(&request.model_class))
        .bind(&request.group_ids)
        .fetch_optional(&state.pool)
        .await;
    let row = match saved {
        Ok(row) => row,
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(bank_name_taken()),
        Err(error) => return Err(error.into()),
    };
    let bank = row.ok_or_else(|| ApiError::not_found("That bank"))?.to_proto()?;
    state.publish(Some(user.id), StreamMessage::MemoryBank(bank.clone()));

    Ok(Json(bank))
}

pub async fn delete_bank(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    user.require_full_access()?;
    let owned = sqlx::query_scalar::<_, Uuid>("select id from memory_banks where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?;

    if owned.is_none() {
        return Err(ApiError::not_found("That bank"));
    }

    let held = sqlx::query_scalar::<_, i64>("select count(*) from memories where bank_id = $1").bind(id).fetch_one(&state.pool).await?;

    if held > 0 {
        return Err(ApiError::conflict("bank_not_empty", "Move or delete this bank's memories first."));
    }

    sqlx::query("delete from memory_banks where id = $1").bind(id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::MemoryBankRemoved { bank_id: id });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn move_to_bank(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<MoveMemoryRequest>) -> ApiResult<Json<Memory>> {
    user.require_full_access()?;

    if let Some(bank_id) = request.bank_id {
        let owned = sqlx::query_scalar::<_, Uuid>("select id from memory_banks where id = $1 and owner = $2").bind(bank_id).bind(user.id).fetch_optional(&state.pool).await?;

        if owned.is_none() {
            return Err(ApiError::not_found("That bank"));
        }
    }

    let row = sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!("update memories set bank_id = $2 where id = $1 and owner = $3 returning {MEMORY_COLUMNS}")))
        .bind(id)
        .bind(request.bank_id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;
    let memory = row.ok_or_else(|| ApiError::not_found("That memory"))?.to_proto();
    state.publish(Some(user.id), StreamMessage::Memory(memory.clone()));

    Ok(Json(memory))
}

fn bank_name(request: &SaveMemoryBankRequest) -> ApiResult<&str> {
    let name = request.name.trim();

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for this bank."));
    }

    Ok(name)
}

fn bank_name_taken() -> ApiError {
    ApiError::conflict("bank_name_taken", "You already have a bank with that name. Choose another name.")
}

const COMMAND_WRAPPERS: [&str; 8] = ["sudo", "env", "time", "nohup", "exec", "command", "heavy", "timeout"];
const COMMAND_BUILTINS: [&str; 8] = ["cd", "echo", "export", "true", "false", "set", "test", "printf"];
const FAILED_COMMAND_MEMORIES: i64 = 3;
const FAILURE_TEXT_LIMIT: usize = 2000;
const FAILURE_RANKING_WORDS: usize = 40;
pub const MEMORY_ORIGIN: &str = "memory";

pub fn programs_in(command: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();

    for segment in command.split(['&', '|', ';', '\n', '(', ')', '`']) {
        let program = segment
            .split_whitespace()
            .find(|word| !word.contains('=') && !COMMAND_WRAPPERS.contains(word) && !word.starts_with('-'))
            .map(|word| word.rsplit(['/', '\\']).next().unwrap_or(word).trim_matches(['"', '\'']).to_string());

        if let Some(program) = program.filter(|program| !program.is_empty() && !COMMAND_BUILTINS.contains(&program.as_str()) && !programs.contains(program)) {
            programs.push(program);
        }
    }

    programs
}

fn command_of(input: &Value) -> Option<String> {
    match &input["command"] {
        Value::String(command) => Some(command.clone()),
        Value::Array(words) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();

            match words.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => Some(script.to_string()),
                _ => Some(words.join(" ")),
            }
        }
        _ => None,
    }
}

pub fn hosts_in(command: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();

    for word in command.split_whitespace() {
        let address = word.trim_matches(['"', '\'']);
        let address = address.split_once("://").map_or(address, |(_, rest)| rest);
        let address = address.rsplit_once('@').map_or(address, |(_, host)| host);
        let host = address.split([':', '/']).next().unwrap_or_default();
        let named = host.contains('.') && host.chars().any(char::is_alphabetic) && host.chars().all(|character| character.is_alphanumeric() || character == '.' || character == '-');
        let file_like = host.starts_with('.') || host.ends_with('.') || word.starts_with(['/', '.', '-', '$']) || host.rsplit('.').next().is_some_and(|extension| ["json", "yaml", "yml", "toml", "txt", "md", "rs", "kt", "ts", "js", "sh", "log", "conf", "env", "lock"].contains(&extension));

        if named && !file_like && !hosts.iter().any(|known| known == host) {
            hosts.push(host.to_string());
        }
    }

    hosts
}

fn words_of(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();

    for word in text.split(|character: char| !character.is_alphanumeric()).filter(|word| word.len() > 2 && !word.eq_ignore_ascii_case("or")) {
        let word = word.to_lowercase();

        if !words.contains(&word) {
            words.push(word);
        }
    }

    words
}

fn error_text_of(update: &Value) -> String {
    let content = update["content"].as_array().into_iter().flatten().filter_map(|item| item["content"]["text"].as_str().or_else(|| item["text"].as_str()));
    let raw = update["rawOutput"].as_str().into_iter().chain(update["rawOutput"]["output"].as_str()).chain(update["rawOutput"]["stderr"].as_str());

    content.chain(raw).collect::<Vec<&str>>().join("\n").chars().take(FAILURE_TEXT_LIMIT).collect()
}

pub async fn after_failed_tool_call(state: &AppState, owner: Uuid, node_id: Uuid, conversation_id: Uuid, update: &Value) -> ApiResult<()> {
    let Some(tool_call_id) = update["toolCallId"].as_str() else {
        return Ok(());
    };
    let earlier = sqlx::query_scalar::<_, Value>("select payload->'update' from events where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'toolCallId' = $2 order by seq")
        .bind(conversation_id)
        .bind(tool_call_id)
        .fetch_all(&state.pool)
        .await?;
    let calls: Vec<&Value> = earlier.iter().chain(std::iter::once(update)).collect();
    let command = calls.iter().rev().find_map(|call| command_of(&call["rawInput"]));
    let title = calls.iter().rev().find_map(|call| call["title"].as_str()).unwrap_or_default();
    let subjects = match &command {
        Some(command) => programs_in(command).into_iter().chain(hosts_in(command)).collect::<Vec<String>>(),
        None => words_of(title).into_iter().filter(|word| word != "mcp").collect(),
    };

    if subjects.is_empty() {
        return Ok(());
    }

    let error = error_text_of(update);
    let ranking: Vec<String> = subjects.iter().cloned().chain(words_of(&error).into_iter().take(FAILURE_RANKING_WORDS)).collect();
    let shown = sqlx::query_scalar::<_, Uuid>(
        "select (jsonb_array_elements_text(payload->'memories'))::uuid from events where conversation_id = $1 and kind = 'user_message' and payload->>'origin' = $2",
    )
    .bind(conversation_id)
    .bind(MEMORY_ORIGIN)
    .fetch_all(&state.pool)
    .await?;
    let rows = sqlx::query_as::<_, MemoryRow>(sqlx::AssertSqlSafe(format!(
        "select {MEMORY_COLUMNS} from memories
         where owner = $1 and bank_id is null and not (id = any($4)) and to_tsvector('english', text) @@ websearch_to_tsquery('english', $2)
         order by ts_rank(to_tsvector('english', text), websearch_to_tsquery('english', $3)) desc, created_at desc
         limit $5"
    )))
    .bind(owner)
    .bind(subjects.join(" or "))
    .bind(ranking.join(" or "))
    .bind(&shown)
    .bind(FAILED_COMMAND_MEMORIES)
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let node_version = sqlx::query_scalar::<_, String>("select version from nodes where id = $1").bind(node_id).fetch_optional(&state.pool).await?;

    if !node_version.is_some_and(|version| crate::nodes::node_has(&version, crate::nodes::STEERING_SINCE)) {
        return Ok(());
    }

    let Some(_claim) = state.hub.claim_steer(conversation_id) else {
        return Ok(());
    };
    let memory_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let names = subjects.iter().map(|subject| format!("`{subject}`")).collect::<Vec<String>>().join(", ");
    let text = format!("Mycelium: that call to {names} failed. Earlier conversations remembered this about it. Use it to fix the failure before asking the person:\n{}", crate::mcp::memory::memory_blocks(state, rows).await?);
    let Some((request_id, answered)) = state.hub.begin_request::<crate::state::SteerAnswer>(node_id, |request_id| ControlToNode::Steer { conversation_id, request_id, text: text.clone(), attachments: Vec::new() }) else {
        return Ok(());
    };
    let answer = tokio::time::timeout(state.config.steer_timeout, answered).await;
    state.hub.forget_request::<crate::state::SteerAnswer>(request_id);

    if !matches!(answer, Ok(Ok(ref answer)) if answer.outcome == "injected") {
        return Ok(());
    }

    let mut tx = state.pool.begin().await?;
    let event = crate::store::append_event(&mut tx, conversation_id, mycelium_proto::conversation::EventKind::UserMessage, &json!({"text": text, "prompt_id": Uuid::new_v4(), "attachments": [], "origin": MEMORY_ORIGIN, "memories": memory_ids}), None).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::programs_in;

    #[test]
    fn the_hosts_of_a_command_are_its_addresses_not_its_files() {
        assert_eq!(super::hosts_in("ssh deploy@atlas.example.com 'cat config.toml' && curl -s https://orion.example.org:8443/healthz"), ["atlas.example.com", "orion.example.org"]);
        assert!(super::hosts_in("cat ./notes.md /etc/hosts.allow").is_empty());
    }

    #[test]
    fn the_programs_of_a_command_skip_assignments_wrappers_paths_and_builtins() {
        assert_eq!(programs_in("PATH=/run/wrappers/bin:$PATH op read op://infra/kube"), ["op"]);
        assert_eq!(programs_in("cd /srv && sudo /usr/bin/kubectl get pods | grep web"), ["kubectl", "grep"]);
        assert_eq!(programs_in("heavy mise exec -- cargo test"), ["mise"]);
        assert!(programs_in("echo done").is_empty());
    }
}
