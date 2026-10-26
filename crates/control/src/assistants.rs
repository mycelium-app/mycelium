use crate::attachments::hex_digest;
use crate::auth::AuthUser;
use crate::chats;
use crate::continuation::{self, Target};
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::groups;
use crate::prompts;
use crate::state::AppState;
use crate::store::{ConversationRow, enum_text, find_owned, parse_enum};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::body::Bytes;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use mycelium_proto::api::{Assistant, AssistantBackup, AssistantFile, AssistantPreferences, AssistantSecret, AssistantSecretState, AssistantState, PutAssistantSecretRequest, CreateAssistantRequest, CreateChatRequest, CreateConversationRequest, PatchAssistantRequest, PromptRequest};
use mycelium_proto::conversation::{AgentId, Conversation};
use mycelium_proto::link::{ControlToNode, FolderFile};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Json as Jsonb;
use uuid::Uuid;

pub const INSTRUCTIONS_PATH: &str = "AGENTS.md";
const NAME_LIMIT: usize = 60;
const PATH_LIMIT: usize = 200;
pub const FILE_LIMIT: usize = 1024 * 1024;
const STORAGE_LIMIT: i64 = 25 * 1024 * 1024;
const DESCRIPTION_PREVIEW_BYTES: i32 = 400;
const FOLDER_ROOT: &str = ".mycelium-assistants";
const FOLDER_CHUNK_BYTES: usize = 1024 * 1024;
const FOLDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const SECRET_NAME_LIMIT: usize = 100;
const MODEL_ERROR_MINUTES: i64 = 10;
const FALL_BACK_CONDITIONS: [&str; 4] = ["usage_limit", "signed_out", "offline", "model_error"];
const ASSISTANT_COLUMNS: &str = "a.id, a.name, a.description, a.group_id, a.preferences, a.conversation_id, a.backup, a.pinned_at, a.archived_at, a.created_at,
    c.status as current_status,
    (select max(updated_at) from conversations where assistant_id = a.id) as last_active_at,
    (select count(*) from conversations where assistant_id = a.id) as session_count,
    (select s.cron from schedules s where s.assistant_id = a.id and not s.proposed and not s.paused order by s.next_run_at limit 1) as wake_cron,
    (select substring(content from 1 for $3) from assistant_files f where f.assistant_id = a.id and f.path = 'AGENTS.md') as instructions_preview";

#[derive(sqlx::FromRow)]
struct AssistantRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    group_id: Option<Uuid>,
    preferences: Jsonb<AssistantPreferences>,
    conversation_id: Option<Uuid>,
    backup: Option<Jsonb<AssistantBackup>>,
    pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    current_status: Option<String>,
    last_active_at: Option<chrono::DateTime<chrono::Utc>>,
    session_count: i64,
    wake_cron: Option<String>,
    instructions_preview: Option<Vec<u8>>,
}

impl AssistantRow {
    fn to_proto(self) -> Assistant {
        let described = self.description.filter(|text| !text.trim().is_empty());
        let first_line = || {
            let preview = String::from_utf8_lossy(self.instructions_preview.as_deref().unwrap_or_default()).into_owned();

            preview.lines().map(|line| line.trim_start_matches('#').trim()).find(|line| !line.is_empty()).map(str::to_string)
        };
        let description = described.or_else(first_line);
        let state = match self.current_status.as_deref() {
            Some("working") => AssistantState::Working,
            Some("input") => AssistantState::Input,
            _ => AssistantState::Idle,
        };

        Assistant {
            id: self.id,
            name: self.name,
            description,
            group_id: self.group_id,
            preferences: self.preferences.0,
            conversation_id: self.conversation_id,
            state,
            last_active_at: self.last_active_at,
            session_count: self.session_count,
            pinned_at: self.pinned_at,
            archived_at: self.archived_at,
            instructions: None,
            next_choice: None,
            secrets: Vec::new(),
            backup: self.backup.map(|backup| backup.0),
            wakes: self.wake_cron.as_deref().map(crate::schedules::describe_cron),
            created_at: self.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    archived: bool,
}

async fn load(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Assistant> {
    let row = sqlx::query_as::<_, AssistantRow>(sqlx::AssertSqlSafe(format!(
        "select {ASSISTANT_COLUMNS} from assistants a left join conversations c on c.id = a.conversation_id where a.id = $1 and a.owner = $2"
    )))
    .bind(id)
    .bind(owner)
    .bind(DESCRIPTION_PREVIEW_BYTES)
    .fetch_optional(&state.pool)
    .await?;

    let mut assistant = row.map(AssistantRow::to_proto).ok_or_else(|| ApiError::not_found("That assistant"))?;

    assistant.secrets = secrets_of(state, id).await?;

    Ok(assistant)
}

pub async fn publish(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Assistant> {
    let assistant = load(state, owner, id).await?;

    state.publish(Some(owner), StreamMessage::Assistant(assistant.clone()));

    Ok(assistant)
}

pub async fn instructions_of(state: &AppState, id: Uuid) -> ApiResult<String> {
    let content = sqlx::query_scalar::<_, Vec<u8>>("select content from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(INSTRUCTIONS_PATH).fetch_optional(&state.pool).await?;

    Ok(String::from_utf8_lossy(&content.unwrap_or_default()).into_owned())
}

pub async fn store_file(state: &AppState, id: Uuid, path: &str, content: &[u8]) -> ApiResult<()> {
    if content.len() > FILE_LIMIT {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large", format!("{path} is over 1 MiB. Shorten it or split it into smaller files.")));
    }

    let others = sqlx::query_scalar::<_, i64>("select coalesce(sum(length(content)), 0)::bigint from assistant_files where assistant_id = $1 and path <> $2").bind(id).bind(path).fetch_one(&state.pool).await?;

    if others + content.len() as i64 > STORAGE_LIMIT {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "storage_full", "This assistant's storage is full at 25 MiB. Delete a file before adding more."));
    }

    sqlx::query(
        "insert into assistant_files (assistant_id, path, content, sha256) values ($1, $2, $3, $4)
         on conflict (assistant_id, path) do update set content = excluded.content, sha256 = excluded.sha256, updated_at = now()",
    )
    .bind(id)
    .bind(path)
    .bind(content)
    .bind(hex_digest(content))
    .execute(&state.pool)
    .await?;

    Ok(())
}

pub fn valid_path(path: &str) -> ApiResult<String> {
    let path = path.trim();
    let wrong = path.is_empty() || path.len() > PATH_LIMIT || path.starts_with('/') || path.ends_with('/') || path.contains("..") || path.contains('\\') || path.contains('\0') || path.split('/').any(str::is_empty);

    if wrong {
        return Err(ApiError::bad_request("path_invalid", "Enter the path of the file inside the assistant's storage, for example notes/standup.md, with no leading slash and no \"..\"."));
    }

    Ok(path.to_string())
}

async fn files_of(state: &AppState, id: Uuid, prefix: &str) -> ApiResult<Vec<AssistantFile>> {
    let rows = sqlx::query_as::<_, (String, i64, chrono::DateTime<chrono::Utc>, bool)>("select path, length(content)::bigint, updated_at, coalesce(backed_up_sha256 = sha256, false) from assistant_files where assistant_id = $1 and starts_with(path, $2) order by path")
        .bind(id)
        .bind(prefix)
        .fetch_all(&state.pool)
        .await?;

    Ok(rows.into_iter().map(|(path, size, updated_at, backed_up)| AssistantFile { path, size, updated_at, backed_up }).collect())
}

fn valid_name(name: &str) -> ApiResult<String> {
    let name = name.trim();

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for the assistant."));
    }

    if name.chars().count() > NAME_LIMIT {
        return Err(ApiError::bad_request("name_invalid", "Use up to 60 characters in the assistant name."));
    }

    Ok(name.to_string())
}

fn name_taken(name: &str) -> ApiError {
    ApiError::conflict("name_taken", format!("You already have an assistant called {name}. Choose another name."))
}

async fn validate_preferences(state: &AppState, owner: Uuid, preferences: &AssistantPreferences) -> ApiResult<()> {
    for choice in &preferences.models {
        match (choice.agent, choice.provider_id) {
            (Some(_), None) => {}
            (None, Some(provider_id)) => {
                let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from model_providers where id = $1 and owner = $2)").bind(provider_id).bind(owner).fetch_one(&state.pool).await?;

                if !exists {
                    return Err(ApiError::bad_request("provider_unknown", "That model provider is not one of yours. Choose a model for this assistant."));
                }

                if choice.model.as_deref().is_none_or(|model| model.trim().is_empty()) {
                    return Err(ApiError::bad_request("model_required", "Choose a model for this assistant"));
                }
            }
            _ => return Err(ApiError::bad_request("model_required", "Choose a model for this assistant")),
        }
    }

    for node_id in &preferences.nodes {
        let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and created_by = $2 and revoked_at is null)").bind(node_id).bind(owner).fetch_one(&state.pool).await?;

        if !exists {
            return Err(ApiError::bad_request("node_unknown", "That node is not one of yours"));
        }
    }

    if let Some(unknown) = preferences.fall_back_on.iter().find(|condition| !FALL_BACK_CONDITIONS.contains(&condition.as_str())) {
        return Err(ApiError::bad_request("fall_back_invalid", format!("{unknown} is not a reason to move to the next choice.")));
    }

    Ok(())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateAssistantRequest>) -> ApiResult<(StatusCode, Json<Assistant>)> {
    let name = valid_name(&request.name)?;
    let preferences = request.preferences.unwrap_or_default();

    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    validate_preferences(&state, user.id, &preferences).await?;

    let description = request.description.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let inserted = sqlx::query_scalar::<_, Uuid>("insert into assistants (owner, name, description, group_id, preferences) values ($1, $2, $3, $4, $5) returning id")
        .bind(user.id)
        .bind(&name)
        .bind(description)
        .bind(request.group_id)
        .bind(Jsonb(&preferences))
        .fetch_one(&state.pool)
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(name_taken(&name)),
        Err(cause) => return Err(cause.into()),
    };

    store_file(&state, id, INSTRUCTIONS_PATH, request.instructions.unwrap_or_default().as_bytes()).await?;

    let assistant = publish(&state, user.id, id).await?;

    Ok((StatusCode::CREATED, Json(assistant)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Assistant>>> {
    let rows = sqlx::query_as::<_, AssistantRow>(sqlx::AssertSqlSafe(format!(
        "select {ASSISTANT_COLUMNS} from assistants a left join conversations c on c.id = a.conversation_id
         where a.owner = $1 and (a.archived_at is not null) = $2 order by lower(a.name), a.id"
    )))
    .bind(user.id)
    .bind(query.archived)
    .bind(DESCRIPTION_PREVIEW_BYTES)
    .fetch_all(&state.pool)
    .await?;

    let mut assistants: Vec<Assistant> = rows.into_iter().map(AssistantRow::to_proto).collect();

    for assistant in &mut assistants {
        assistant.secrets = secrets_of(&state, assistant.id).await?;
    }

    Ok(Json(assistants))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let mut assistant = load(&state, user.id, id).await?;

    assistant.instructions = Some(instructions_of(&state, id).await?);
    assistant.next_choice = next_choice_label(&state, user.id, &assistant).await?;

    Ok(Json(assistant))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchAssistantRequest>) -> ApiResult<Json<Assistant>> {
    load(&state, user.id, id).await?;

    let name = request.name.as_deref().map(valid_name).transpose()?;

    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    if let Some(preferences) = &request.preferences {
        validate_preferences(&state, user.id, preferences).await?;
    }

    let description = request.description.as_deref().map(str::trim);
    let updated = sqlx::query(
        "update assistants set name = coalesce($3, name),
             description = case when $4::text is null then description else nullif($4, '') end,
             group_id = case when $6 then null else coalesce($5, group_id) end,
             pinned_at = case when $7::boolean is null then pinned_at when $7 then coalesce(pinned_at, now()) else null end,
             archived_at = case when $8::boolean is null then archived_at when $8 then coalesce(archived_at, now()) else null end,
             preferences = coalesce($9, preferences),
             updated_at = now()
         where id = $1 and owner = $2",
    )
    .bind(id)
    .bind(user.id)
    .bind(name.as_deref())
    .bind(description)
    .bind(request.group_id)
    .bind(request.clear_group)
    .bind(request.pinned)
    .bind(request.archived)
    .bind(request.preferences.as_ref().map(Jsonb))
    .execute(&state.pool)
    .await;

    match updated {
        Ok(_) => {}
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(name_taken(name.as_deref().unwrap_or_default())),
        Err(cause) => return Err(cause.into()),
    }

    if let Some(instructions) = &request.instructions {
        store_file(&state, id, INSTRUCTIONS_PATH, instructions.as_bytes()).await?;
    }

    let mut assistant = publish(&state, user.id, id).await?;

    if request.instructions.is_some() {
        assistant.instructions = Some(instructions_of(&state, id).await?);
    }

    Ok(Json(assistant))
}

pub async fn prompt(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PromptRequest>) -> ApiResult<Json<Conversation>> {
    let assistant = load(&state, user.id, id).await?;
    let text = request.text.trim();

    if text.is_empty() && request.attachments.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message for the assistant."));
    }

    let current = match assistant.conversation_id {
        Some(conversation_id) => Some(find_owned(&state.pool, conversation_id, user.id).await?),
        None => None,
    };
    let conversation = match current {
        Some(row) if chats::is_chat(&row) && request.attachments.is_empty() => {
            chats::send_or_queue(&state, user.id, row.id, text).await?;

            find_owned(&state.pool, row.id, user.id).await?.to_proto()?
        }
        Some(row) if chats::is_chat(&row) => chats::send(&state, user.id, row.id, text, request.attachments).await?,
        Some(row) => {
            if let Some(reason) = unavailable_reason(&state, &row).await? {
                fall_back(&state, user.id, row.id, reason, None).await;
            }

            prompts::prompt(State(state.clone()), user.clone(), Path(row.id), Json(request.clone())).await?;

            find_owned(&state.pool, row.id, user.id).await?.to_proto()?
        }
        None => {
            if assistant.preferences.models.is_empty() {
                return Err(ApiError::bad_request("model_required", "Choose a model for this assistant in its settings, then send your message again."));
            }

            let facts = facts_of(&state, user.id, id).await?;
            let Some(choice) = choose(&assistant.preferences, &facts, false) else {
                return Err(ApiError::conflict("no_choice_available", "None of this assistant's models can run right now. Bring one of its nodes online and signed in, or add another model in its settings."));
            };

            match choice {
                Choice::Chat { provider_id, model, .. } => {
                    let started = chats::start(&state, &user, CreateChatRequest { provider_id, model, prompt: None, parent_id: None }, None, Some(id)).await?;

                    sqlx::query("update assistants set conversation_id = $2, updated_at = now() where id = $1").bind(id).bind(started.id).execute(&state.pool).await?;

                    chats::send(&state, user.id, started.id, text, request.attachments).await?
                }
                Choice::Workspace { agent, account, node_id, .. } => {
                    let started = CreateConversationRequest { node_id, agent, account, cwd: folder_of(id), project_id: None, branch: None, title: None, prompt: None, parent_id: None, side_kind: None };
                    let started = conversations::start_for(&state, &user, started, Some(id)).await?;

                    sqlx::query("update assistants set conversation_id = $2, updated_at = now() where id = $1").bind(id).bind(started.id).execute(&state.pool).await?;
                    prompts::prompt(State(state.clone()), user.clone(), Path(started.id), Json(request.clone())).await?;

                    find_owned(&state.pool, started.id, user.id).await?.to_proto()?
                }
            }
        }
    };

    publish(&state, user.id, id).await?;

    Ok(Json(conversation))
}

pub async fn clear(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let assistant = load(&state, user.id, id).await?;

    if let Some(conversation_id) = assistant.conversation_id {
        let row = find_owned(&state.pool, conversation_id, user.id).await?;

        if chats::is_chat(&row) {
            chats::stop(&state, user.id, conversation_id).await?;
        } else if let Err(error) = conversations::close(State(state.clone()), user.clone(), Path(conversation_id)).await {
            tracing::warn!("could not close the working session of assistant {id}: {}", error.message());
        }
    }

    sqlx::query("update assistants set conversation_id = null, updated_at = now() where id = $1").bind(id).execute(&state.pool).await?;

    Ok(Json(publish(&state, user.id, id).await?))
}

pub async fn list_files(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<AssistantFile>>> {
    load(&state, user.id, id).await?;

    Ok(Json(files_of(&state, id, "").await?))
}

pub async fn read_file(State(state): State<AppState>, user: AuthUser, Path((id, path)): Path<(Uuid, String)>) -> ApiResult<Response> {
    load(&state, user.id, id).await?;

    let content = sqlx::query_scalar::<_, Vec<u8>>("select content from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(&path).fetch_optional(&state.pool).await?;
    let Some(content) = content else {
        return Err(ApiError::not_found("That file"));
    };
    let kind = match std::str::from_utf8(&content) {
        Ok(_) => "text/plain; charset=utf-8",
        Err(_) => "application/octet-stream",
    };

    Ok(([(header::CONTENT_TYPE, kind)], content).into_response())
}

pub async fn write_file(State(state): State<AppState>, user: AuthUser, Path((id, path)): Path<(Uuid, String)>, body: Bytes) -> ApiResult<Json<AssistantFile>> {
    load(&state, user.id, id).await?;

    let path = valid_path(&path)?;

    store_file(&state, id, &path, &body).await?;
    publish(&state, user.id, id).await?;
    crate::assistant_backup::trigger(&state, user.id, id);

    let written = files_of(&state, id, &path).await?.into_iter().find(|file| file.path == path);

    written.map(Json).ok_or_else(|| ApiError::not_found("That file"))
}

pub async fn delete_file(State(state): State<AppState>, user: AuthUser, Path((id, path)): Path<(Uuid, String)>) -> ApiResult<StatusCode> {
    load(&state, user.id, id).await?;

    let deleted = sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(&path).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That file"));
    }

    publish(&state, user.id, id).await?;
    crate::assistant_backup::trigger(&state, user.id, id);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_secrets(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<AssistantSecret>>> {
    load(&state, user.id, id).await?;

    Ok(Json(secrets_of(&state, id).await?))
}

pub async fn put_secret(State(state): State<AppState>, user: AuthUser, Path((id, name)): Path<(Uuid, String)>, Json(request): Json<PutAssistantSecretRequest>) -> ApiResult<Json<AssistantSecret>> {
    load(&state, user.id, id).await?;

    let name = valid_secret_name(&name)?;
    let value = request.value.filter(|value| !value.is_empty());

    if let Some(value) = &value {
        sqlx::query(
            "insert into assistant_secrets (assistant_id, name, value, description, set_by) values ($1, $2, $3, $4, 'person')
             on conflict (assistant_id, name) do update set value = excluded.value, description = coalesce(excluded.description, assistant_secrets.description), set_by = 'person', updated_at = now()",
        )
        .bind(id)
        .bind(&name)
        .bind(value)
        .bind(request.description.as_deref())
        .execute(&state.pool)
        .await?;
        redact_stored(&state, id, value).await?;
    } else {
        sqlx::query(
            "insert into assistant_secrets (assistant_id, name, description) values ($1, $2, $3)
             on conflict (assistant_id, name) do update set description = coalesce(excluded.description, assistant_secrets.description), updated_at = now()",
        )
        .bind(id)
        .bind(&name)
        .bind(request.description.as_deref())
        .execute(&state.pool)
        .await?;
    }

    publish(&state, user.id, id).await?;

    let saved = secrets_of(&state, id).await?.into_iter().find(|secret| secret.name == name);

    saved.map(Json).ok_or_else(|| ApiError::not_found("That secret"))
}

pub async fn delete_secret(State(state): State<AppState>, user: AuthUser, Path((id, name)): Path<(Uuid, String)>) -> ApiResult<StatusCode> {
    load(&state, user.id, id).await?;

    let deleted = sqlx::query("delete from assistant_secrets where assistant_id = $1 and name = $2").bind(id).bind(&name).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That secret"));
    }

    publish(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn file_tool(state: &AppState, conversation_id: Option<Uuid>, tool: &str, arguments: &Value) -> ApiResult<(String, bool)> {
    let session = match conversation_id {
        Some(conversation_id) => sqlx::query_as::<_, (Option<Uuid>, String, Uuid)>("select assistant_id, agent, owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let Some((Some(id), agent, owner)) = session else {
        return Ok(("This conversation does not belong to an assistant.".to_string(), true));
    };

    if agent != chats::CHAT_AGENT {
        return Ok(("Use the files in your working folder; they are copied back after this turn.".to_string(), true));
    }

    let text_of = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::to_string);
    let path = match tool {
        "assistant_list_files" => String::new(),
        _ => match text_of("path").map(|path| valid_path(&path)) {
            Some(Ok(path)) => path,
            Some(Err(error)) => return Ok((error.message().to_string(), true)),
            None => return Ok(("Give the path of the file.".to_string(), true)),
        },
    };

    match tool {
        "assistant_list_files" => {
            let files = files_of(state, id, text_of("prefix").unwrap_or_default().trim()).await?;

            match files.is_empty() {
                true => Ok(("No files.".to_string(), false)),
                false => Ok((files.iter().map(|file| format!("{} ({} bytes)", file.path, file.size)).collect::<Vec<_>>().join("\n"), false)),
            }
        }
        "assistant_read_file" => {
            let content = sqlx::query_scalar::<_, Vec<u8>>("select content from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(&path).fetch_optional(&state.pool).await?;

            match content.map(String::from_utf8) {
                Some(Ok(text)) => Ok((text, false)),
                Some(Err(_)) => Ok((format!("{path} is not text, so it cannot be read."), true)),
                None => Ok((format!("There is no file called {path}."), true)),
            }
        }
        "assistant_write_file" => {
            let Some(content) = text_of("content") else {
                return Ok(("Give the content of the file.".to_string(), true));
            };

            match store_file(state, id, &path, content.as_bytes()).await {
                Ok(()) => {
                    publish(state, owner, id).await?;
                    crate::assistant_backup::trigger(state, owner, id);

                    Ok((format!("Wrote {path}."), false))
                }
                Err(error) => Ok((error.message().to_string(), true)),
            }
        }
        _ => {
            let deleted = sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(&path).execute(&state.pool).await?;

            publish(state, owner, id).await?;
            crate::assistant_backup::trigger(state, owner, id);

            match deleted.rows_affected() {
                0 => Ok((format!("There is no file called {path}."), true)),
                _ => Ok((format!("Deleted {path}."), false)),
            }
        }
    }
}

pub fn folder_of(id: Uuid) -> String {
    format!("{FOLDER_ROOT}/{id}")
}

pub struct NodeFacts {
    pub id: Uuid,
    pub agents: Vec<String>,
    pub accounts: Vec<(String, String)>,
}

pub struct Facts {
    pub nodes: Vec<NodeFacts>,
    pub limited: Vec<usize>,
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    Chat { index: usize, provider_id: Uuid, model: Option<String> },
    Workspace { index: usize, agent: AgentId, account: Option<String>, model: Option<String>, node_id: Uuid },
}

pub fn choose(preferences: &AssistantPreferences, facts: &Facts, workspace_only: bool) -> Option<Choice> {
    let chat_choice = |index: usize| {
        let choice = &preferences.models[index];

        choice.provider_id.filter(|_| !facts.limited.contains(&index)).map(|provider_id| Choice::Chat { index, provider_id, model: choice.model.clone() })
    };
    let workspace_choice = |index: usize| {
        let choice = &preferences.models[index];
        let agent = choice.agent.filter(|_| !facts.limited.contains(&index))?;
        let preferred = preferences.nodes.iter().filter_map(|id| facts.nodes.iter().find(|node| node.id == *id));
        let any = facts.nodes.iter();
        let mut candidates: Box<dyn Iterator<Item = &NodeFacts>> = if preferences.nodes.is_empty() { Box::new(any) } else { Box::new(preferred) };
        let node = candidates.find(|node| {
            node.agents.iter().any(|available| available == agent.as_str())
                && choice.account.as_ref().is_none_or(|account| node.accounts.iter().any(|(signed_in_agent, signed_in)| signed_in_agent == agent.as_str() && signed_in == account))
        })?;

        Some(Choice::Workspace { index, agent, account: choice.account.clone(), model: choice.model.clone(), node_id: node.id })
    };

    if preferences.chat_first && !workspace_only {
        let first_chat = (0..preferences.models.len()).find_map(chat_choice);

        if first_chat.is_some() {
            return first_chat;
        }
    }

    (0..preferences.models.len()).find_map(|index| match workspace_only {
        true => workspace_choice(index),
        false => chat_choice(index).or_else(|| workspace_choice(index)),
    })
}

pub async fn facts_of(state: &AppState, owner: Uuid, assistant_id: Uuid) -> ApiResult<Facts> {
    let available = sqlx::query_as::<_, (Uuid, String)>(
        "select n.id, a.agent from nodes n join agents a on a.node_id = n.id and a.available where n.created_by = $1 and n.revoked_at is null and not n.draining order by n.name, n.id",
    )
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;
    let mut nodes: Vec<NodeFacts> = Vec::new();

    for (node_id, agent) in available.into_iter().filter(|(node_id, _)| state.hub.is_online(*node_id)) {
        match nodes.iter_mut().find(|node| node.id == node_id) {
            Some(node) => node.agents.push(agent),
            None => nodes.push(NodeFacts { id: node_id, agents: vec![agent], accounts: Vec::new() }),
        }
    }

    let ids: Vec<Uuid> = nodes.iter().map(|node| node.id).collect();
    let logins = sqlx::query_as::<_, (Uuid, String, String)>("select node_id, agent, account from logins where node_id = any($1) and state = 'signed_in' and account is not null").bind(&ids).fetch_all(&state.pool).await?;

    for (node_id, agent, account) in logins {
        if let Some(node) = nodes.iter_mut().find(|node| node.id == node_id) {
            node.accounts.push((agent, account));
        }
    }

    let limited = sqlx::query_scalar::<_, Jsonb<Value>>("select limited from assistants where id = $1").bind(assistant_id).fetch_one(&state.pool).await?;
    let now = chrono::Utc::now();
    let limited = limited.0.as_object().into_iter().flatten().filter_map(|(index, until)| {
        let until = until.as_str()?.parse::<chrono::DateTime<chrono::Utc>>().ok()?;

        (until > now).then(|| index.parse::<usize>().ok()).flatten()
    });

    Ok(Facts { nodes, limited: limited.collect() })
}

pub async fn copy_down(state: &AppState, node_id: Uuid, assistant_id: Uuid) -> ApiResult<()> {
    let stored = sqlx::query_as::<_, (String, Vec<u8>)>("select path, content from assistant_files where assistant_id = $1 order by path").bind(assistant_id).fetch_all(&state.pool).await?;
    let root = folder_of(assistant_id);
    let mut chunks: Vec<Vec<FolderFile>> = Vec::new();
    let mut current: Vec<FolderFile> = Vec::new();
    let mut size = 0;

    for (path, content) in stored {
        if size + content.len() > FOLDER_CHUNK_BYTES && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            size = 0;
        }

        size += content.len();
        current.push(FolderFile { path, data_base64: base64::engine::general_purpose::STANDARD.encode(content) });
    }

    chunks.push(current);

    for (position, files) in chunks.into_iter().enumerate() {
        if !state.hub.send(node_id, ControlToNode::WriteFolder { root: root.clone(), files, replace: position == 0 }) {
            return Err(crate::nodes::node_offline());
        }
    }

    Ok(())
}

pub async fn copy_up(state: &AppState, owner: Uuid, node_id: Uuid, assistant_id: Uuid) -> ApiResult<()> {
    let Some((request_id, answered)) = state.hub.request_folder(node_id, folder_of(assistant_id)) else {
        return Ok(());
    };
    let answer = tokio::time::timeout(FOLDER_TIMEOUT, answered).await.ok().and_then(Result::ok);

    state.hub.forget_folder_request(request_id);

    let Some(answer) = answer else {
        tracing::warn!("node {node_id} did not return the folder of assistant {assistant_id} in time");

        return Ok(());
    };

    if let Some(error) = answer.error {
        tracing::warn!("node {node_id} could not read the folder of assistant {assistant_id}: {error}");

        return Ok(());
    }

    let stored = sqlx::query_as::<_, (String, String)>("select path, sha256 from assistant_files where assistant_id = $1").bind(assistant_id).fetch_all(&state.pool).await?;
    let mut kept = Vec::new();

    for file in answer.files {
        let Ok(content) = base64::engine::general_purpose::STANDARD.decode(&file.data_base64) else {
            continue;
        };
        let unchanged = stored.iter().any(|(path, digest)| *path == file.path && *digest == hex_digest(&content));

        kept.push(file.path.clone());

        if !unchanged && valid_path(&file.path).is_ok() && let Err(error) = store_file(state, assistant_id, &file.path, &content).await {
            tracing::warn!("could not keep {} from assistant {assistant_id}: {}", file.path, error.message());
        }
    }

    if !answer.truncated {
        for (path, _) in stored.iter().filter(|(path, _)| !kept.contains(path)) {
            sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(assistant_id).bind(path).execute(&state.pool).await?;
        }
    }

    publish(state, owner, assistant_id).await?;
    crate::assistant_backup::trigger(state, owner, assistant_id);

    Ok(())
}

pub fn workspace_turn_ended(state: &AppState, owner: Uuid, node_id: Uuid, conversation_id: Uuid) {
    let state = state.clone();

    tokio::spawn(async move {
        let assistant_id = sqlx::query_scalar::<_, Option<Uuid>>("select assistant_id from conversations where id = $1 and agent <> $2").bind(conversation_id).bind(chats::CHAT_AGENT).fetch_optional(&state.pool).await.ok().flatten().flatten();

        if let Some(assistant_id) = assistant_id && let Err(error) = copy_up(&state, owner, node_id, assistant_id).await {
            tracing::warn!("could not copy the folder of assistant {assistant_id} back: {}", error.message());
        }
    });
}

pub async fn chat_turn_ended(state: &AppState, owner: Uuid, id: Uuid, stop_reason: &str) -> bool {
    if !state.hub.take_workspace_ask(id) || stop_reason != "end_turn" {
        return false;
    }

    let state = state.clone();

    tokio::spawn(async move {
        if let Err(error) = move_to_workspace(&state, owner, id).await {
            tracing::warn!("could not move the thread of conversation {id} to a node: {}", error.message());
        }
    });

    true
}

async fn move_to_workspace(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let assistant_id = sqlx::query_scalar::<_, Option<Uuid>>("select assistant_id from conversations where id = $1").bind(id).fetch_one(&state.pool).await?;
    let Some(assistant_id) = assistant_id else {
        return Ok(());
    };
    let assistant = load(state, owner, assistant_id).await?;
    let facts = facts_of(state, owner, assistant_id).await?;
    let Some(Choice::Workspace { agent, account, node_id, .. }) = choose(&assistant.preferences, &facts, true) else {
        return Err(ApiError::conflict("no_choice_available", "No node can run this assistant right now."));
    };

    copy_down(state, node_id, assistant_id).await?;

    let target = Target { agent: enum_text(&agent), account, provider_id: None, model: None, node_id: Some(node_id), cwd: Some(folder_of(assistant_id)) };

    continuation::continue_on(state, owner, id, target, None).await?;
    prompts::continue_unless_queued(state, owner, id, "You now have a working folder on a node, and your files are in it. Carry on with the request above.").await?;
    prompts::pump(state, id, false).await
}

pub fn valid_secret_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let mut characters = name.chars();
    let starts_well = characters.next().is_some_and(|first| first.is_ascii_uppercase() || first == '_');
    let rest_well = characters.all(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_');

    if !starts_well || !rest_well || name.len() > SECRET_NAME_LIMIT {
        return Err(ApiError::bad_request("secret_name_invalid", "Name the secret like an environment variable: capital letters, digits and underscores, not starting with a digit, for example GITHUB_TOKEN."));
    }

    Ok(name.to_string())
}

async fn secrets_of(state: &AppState, id: Uuid) -> ApiResult<Vec<AssistantSecret>> {
    let rows = sqlx::query_as::<_, (String, Option<String>, bool, Option<String>)>("select name, description, value is not null, set_by from assistant_secrets where assistant_id = $1 order by name").bind(id).fetch_all(&state.pool).await?;

    Ok(rows.into_iter().map(|(name, description, set, set_by)| AssistantSecret { name, description, state: if set { AssistantSecretState::Set } else { AssistantSecretState::Waiting }, set_by }).collect())
}

pub async fn secret_env(state: &AppState, conversation_id: Uuid) -> ApiResult<Vec<(String, String)>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "select s.name, s.value from conversations c join assistant_secrets s on s.assistant_id = c.assistant_id where c.id = $1 and c.agent <> $2 and s.value is not null order by s.name",
    )
    .bind(conversation_id)
    .bind(chats::CHAT_AGENT)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows)
}

async fn redact_stored(state: &AppState, assistant_id: Uuid, value: &str) -> ApiResult<()> {
    if value.len() < crate::store::MIN_REDACTED_SECRET as usize {
        return Ok(());
    }

    let quoted = serde_json::to_string(value).unwrap_or_default();
    let escaped = quoted.trim_matches('"');

    sqlx::query(
        "update events set payload = replace(payload::text, $2, '[redacted]')::jsonb
         where conversation_id in (select id from conversations where assistant_id = $1) and position($2 in payload::text) > 0",
    )
    .bind(assistant_id)
    .bind(escaped)
    .execute(&state.pool)
    .await?;

    Ok(())
}

pub async fn secret_tool(state: &AppState, conversation_id: Option<Uuid>, tool: &str, arguments: &Value) -> ApiResult<(String, bool)> {
    let session = match conversation_id {
        Some(conversation_id) => sqlx::query_as::<_, (Option<Uuid>, Uuid)>("select assistant_id, owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let Some((Some(id), owner)) = session else {
        return Ok(("This conversation does not belong to an assistant.".to_string(), true));
    };
    let text_of = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    let name = match valid_secret_name(&text_of("name").unwrap_or_default()) {
        Ok(name) => name,
        Err(error) => return Ok((error.message().to_string(), true)),
    };
    let description = text_of("description");

    if tool == "assistant_set_secret" {
        let Some(value) = text_of("value") else {
            return Ok(("Give the value of the secret.".to_string(), true));
        };

        sqlx::query(
            "insert into assistant_secrets (assistant_id, name, value, description, set_by) values ($1, $2, $3, $4, 'assistant')
             on conflict (assistant_id, name) do update set value = excluded.value, description = coalesce(excluded.description, assistant_secrets.description), set_by = 'assistant', updated_at = now()",
        )
        .bind(id)
        .bind(&name)
        .bind(&value)
        .bind(description.as_deref())
        .execute(&state.pool)
        .await?;
        redact_stored(state, id, &value).await?;
        publish(state, owner, id).await?;

        return Ok((format!("Stored {name}. It reaches working folder sessions as an environment variable when they start."), false));
    }

    let existing = sqlx::query_scalar::<_, bool>("select value is not null from assistant_secrets where assistant_id = $1 and name = $2").bind(id).bind(&name).fetch_optional(&state.pool).await?;

    if existing == Some(true) {
        return Ok((format!("{name} is already set."), false));
    }

    sqlx::query(
        "insert into assistant_secrets (assistant_id, name, description) values ($1, $2, $3)
         on conflict (assistant_id, name) do update set description = coalesce(excluded.description, assistant_secrets.description), updated_at = now()",
    )
    .bind(id)
    .bind(&name)
    .bind(description.as_deref())
    .execute(&state.pool)
    .await?;
    publish(state, owner, id).await?;

    Ok((format!("Asked the person for {name}. It stays blank until they fill it in; ask again later."), false))
}

pub async fn use_workspace_tool(state: &AppState, conversation_id: Option<Uuid>) -> ApiResult<(String, bool)> {
    let session = match conversation_id {
        Some(conversation_id) => sqlx::query_as::<_, (Option<Uuid>, String, Uuid)>("select assistant_id, agent, owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?,
        None => None,
    };
    let (Some(conversation_id), Some((Some(assistant_id), agent, owner))) = (conversation_id, session) else {
        return Ok(("This conversation does not belong to an assistant.".to_string(), true));
    };

    if agent != chats::CHAT_AGENT {
        return Ok(("You already have a working folder.".to_string(), true));
    }

    let assistant = load(state, owner, assistant_id).await?;
    let facts = facts_of(state, owner, assistant_id).await?;

    let Some(Choice::Workspace { node_id, .. }) = choose(&assistant.preferences, &facts, true) else {
        return Ok(("No node can run this assistant right now. Tell the person to bring one of its nodes online and signed in, or to add one in the assistant's settings.".to_string(), true));
    };

    let name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(node_id).fetch_one(&state.pool).await?;

    state.hub.ask_workspace(conversation_id);

    Ok((format!("The thread moves to {name} when this reply ends, with your storage in a working folder. Finish this reply, then carry on there."), false))
}

pub async fn session_started(state: &AppState, owner: Uuid, conversation_id: Uuid) {
    let found = sqlx::query_as::<_, (Option<Uuid>, String, Option<String>)>("select assistant_id, agent, account from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await;
    let Ok(Some((Some(assistant_id), agent, account))) = found else {
        return;
    };
    let Ok(assistant) = load(state, owner, assistant_id).await else {
        return;
    };
    let Ok(facts) = facts_of(state, owner, assistant_id).await else {
        return;
    };
    let model = assistant.preferences.models.iter().enumerate().find(|(index, choice)| {
        choice.agent.is_some_and(|choice_agent| choice_agent.as_str() == agent) && (choice.account.is_none() || choice.account == account) && !facts.limited.contains(index)
    });

    if let Some((_, choice)) = model && let Some(model) = &choice.model && let Err(error) = prompts::apply_model(state, owner, conversation_id, Some(model), None).await {
        tracing::warn!("could not set the model {model} for assistant {assistant_id}: {}", error.message());
    }
}

async fn unavailable_reason(state: &AppState, row: &ConversationRow) -> ApiResult<Option<&'static str>> {
    if !state.hub.is_online(row.node_id) {
        return Ok(Some("offline"));
    }

    let signed_in = match (&row.account, parse_enum::<AgentId>(&row.agent)) {
        (Some(account), Ok(agent)) => sqlx::query_scalar::<_, bool>("select exists (select 1 from logins where node_id = $1 and agent = $2 and account = $3 and state = 'signed_in')").bind(row.node_id).bind(agent.as_str()).bind(account).fetch_one(&state.pool).await?,
        _ => true,
    };

    Ok((!signed_in).then_some("signed_out"))
}

fn current_index(preferences: &AssistantPreferences, row: &ConversationRow) -> Option<usize> {
    preferences.models.iter().position(|choice| match (choice.agent, choice.provider_id) {
        (Some(agent), _) => agent.as_str() == row.agent && (choice.account.is_none() || choice.account == row.account),
        (None, Some(provider_id)) => row.provider_id == Some(provider_id) && (choice.model.is_none() || choice.model == row.model),
        _ => false,
    })
}

async fn label_of(state: &AppState, choice: &Choice) -> ApiResult<String> {
    match choice {
        Choice::Chat { provider_id, model, .. } => {
            let name = sqlx::query_scalar::<_, String>("select name from model_providers where id = $1").bind(provider_id).fetch_optional(&state.pool).await?;

            Ok([name, model.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · "))
        }
        Choice::Workspace { agent, node_id, .. } => {
            let name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(node_id).fetch_one(&state.pool).await?;
            let agent = if *agent == AgentId::Codex { "Codex" } else { "Claude Code" };

            Ok(format!("{agent} on {name}"))
        }
    }
}

async fn next_choice_label(state: &AppState, owner: Uuid, assistant: &Assistant) -> ApiResult<Option<String>> {
    let facts = facts_of(state, owner, assistant.id).await?;

    match choose(&assistant.preferences, &facts, false) {
        Some(choice) => Ok(Some(label_of(state, &choice).await?)),
        None => Ok(None),
    }
}

pub async fn fall_back(state: &AppState, owner: Uuid, conversation_id: Uuid, reason: &str, until: Option<chrono::DateTime<chrono::Utc>>) -> bool {
    match try_fall_back(state, owner, conversation_id, reason, until).await {
        Ok(moved) => moved,
        Err(error) => {
            tracing::warn!("could not move conversation {conversation_id} to the assistant's next choice ({reason}): {}", error.message());

            false
        }
    }
}

async fn try_fall_back(state: &AppState, owner: Uuid, conversation_id: Uuid, reason: &str, until: Option<chrono::DateTime<chrono::Utc>>) -> ApiResult<bool> {
    let row = find_owned(&state.pool, conversation_id, owner).await?;
    let Some(assistant_id) = row.assistant_id else {
        return Ok(false);
    };
    let assistant = load(state, owner, assistant_id).await?;

    if !assistant.preferences.fall_back_on.iter().any(|condition| condition == reason) {
        return Ok(false);
    }

    if let (Some(until), Some(index)) = (until, current_index(&assistant.preferences, &row)) {
        sqlx::query("update assistants set limited = limited || jsonb_build_object($2::text, $3::text) where id = $1").bind(assistant_id).bind(index.to_string()).bind(until.to_rfc3339()).execute(&state.pool).await?;
    }

    let facts = facts_of(state, owner, assistant_id).await?;
    let target = match choose(&assistant.preferences, &facts, false) {
        Some(Choice::Chat { provider_id, model, .. }) => Target { agent: chats::CHAT_AGENT.to_string(), account: None, provider_id: Some(provider_id), model, node_id: None, cwd: None },
        Some(Choice::Workspace { agent, account, node_id, .. }) => {
            copy_down(state, node_id, assistant_id).await?;

            Target { agent: enum_text(&agent), account, provider_id: None, model: None, node_id: Some(node_id), cwd: Some(folder_of(assistant_id)) }
        }
        None => return Ok(false),
    };

    continuation::continue_on(state, owner, conversation_id, target, Some(reason)).await?;
    publish(state, owner, assistant_id).await?;

    Ok(true)
}

pub async fn fall_back_after_chat_error(state: &AppState, owner: Uuid, id: Uuid) {
    let until = chrono::Utc::now() + chrono::Duration::minutes(MODEL_ERROR_MINUTES);

    if !fall_back(state, owner, id, "model_error", Some(until)).await {
        return;
    }

    let resumed = match find_owned(&state.pool, id, owner).await {
        Ok(row) if chats::is_chat(&row) => chats::resume_turn(state, owner, id).await,
        Ok(_) => prompts::retry_failed(state, owner, id).await,
        Err(error) => Err(error),
    };

    if let Err(error) = resumed {
        tracing::warn!("could not carry on conversation {id} after its model failed: {}", error.message());
    }
}

pub async fn carry_on_after_limit(state: &AppState, owner: Uuid, id: Uuid, text: &str) -> ApiResult<()> {
    let row = find_owned(&state.pool, id, owner).await?;

    if chats::is_chat(&row) {
        chats::send(state, owner, id, text, Vec::new()).await?;

        return Ok(());
    }

    prompts::retry_failed(state, owner, id).await?;
    prompts::continue_unless_queued(state, owner, id, text).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_proto::api::ModelChoice;

    fn chat(model: &str) -> ModelChoice {
        ModelChoice { agent: None, provider_id: Some(Uuid::from_u128(1)), model: Some(model.into()), account: None }
    }

    fn harness(agent: AgentId, account: Option<&str>) -> ModelChoice {
        ModelChoice { agent: Some(agent), provider_id: None, model: None, account: account.map(str::to_string) }
    }

    fn node(id: u128, agents: &[&str], accounts: &[(&str, &str)]) -> NodeFacts {
        NodeFacts { id: Uuid::from_u128(id), agents: agents.iter().map(|agent| agent.to_string()).collect(), accounts: accounts.iter().map(|(agent, account)| (agent.to_string(), account.to_string())).collect() }
    }

    fn preferences(models: Vec<ModelChoice>, nodes: Vec<u128>, chat_first: bool) -> AssistantPreferences {
        AssistantPreferences { models, nodes: nodes.into_iter().map(Uuid::from_u128).collect(), chat_first, ..AssistantPreferences::default() }
    }

    #[test]
    fn secret_names_must_be_environment_variable_names() {
        assert!(valid_secret_name("GITHUB_TOKEN").is_ok());
        assert!(valid_secret_name("_X1").is_ok());
        assert!(valid_secret_name("github_token").is_err());
        assert!(valid_secret_name("1TOKEN").is_err());
        assert!(valid_secret_name("A-B").is_err());
        assert!(valid_secret_name("").is_err());
    }

    #[test]
    fn choose_prefers_a_chat_model_first_and_skips_limited_ones() {
        let preferences = preferences(vec![harness(AgentId::Claude, None), chat("a"), chat("b")], vec![], true);
        let facts = Facts { nodes: vec![node(10, &["claude"], &[])], limited: vec![1] };

        let choice = choose(&preferences, &facts, false);

        assert_eq!(choice, Some(Choice::Chat { index: 2, provider_id: Uuid::from_u128(1), model: Some("b".into()) }));
    }

    #[test]
    fn choose_takes_the_first_workable_node_when_chat_is_not_first() {
        let preferences = preferences(vec![harness(AgentId::Claude, Some("work")), chat("a")], vec![30, 20], false);
        let facts = Facts { nodes: vec![node(20, &["claude"], &[("claude", "personal")]), node(30, &["claude"], &[("claude", "work")])], limited: vec![] };

        let choice = choose(&preferences, &facts, false);

        assert_eq!(choice, Some(Choice::Workspace { index: 0, agent: AgentId::Claude, account: Some("work".into()), model: None, node_id: Uuid::from_u128(30) }));
    }

    #[test]
    fn choose_skips_offline_and_signed_out_choices_and_falls_to_the_next_model() {
        let preferences = preferences(vec![harness(AgentId::Claude, Some("work")), harness(AgentId::Codex, None)], vec![20], false);
        let facts = Facts { nodes: vec![node(20, &["claude", "codex"], &[])], limited: vec![] };

        let choice = choose(&preferences, &facts, false);

        assert_eq!(choice, Some(Choice::Workspace { index: 1, agent: AgentId::Codex, account: None, model: None, node_id: Uuid::from_u128(20) }));
        assert_eq!(choose(&preferences, &Facts { nodes: vec![], limited: vec![] }, false), None);
    }

    #[test]
    fn choosing_for_a_workspace_ignores_chat_models() {
        let preferences = preferences(vec![chat("a")], vec![], true);
        let facts = Facts { nodes: vec![node(10, &["claude"], &[])], limited: vec![] };

        assert_eq!(choose(&preferences, &facts, true), None);
    }
}
