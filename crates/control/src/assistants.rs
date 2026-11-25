use crate::attachments::hex_digest;
use crate::auth::AuthUser;
use crate::chats;
use crate::continuation::{self, Target};
use crate::conversations;
use crate::idempotency;
use crate::error::{ApiError, ApiResult};
use crate::groups;
use crate::prompts;
use crate::state::{AppState, FolderAnswer};
use crate::store::{self, enum_text};
use mycelium_proto::routing::{ANY_NODE, Routing};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::body::Bytes;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use mycelium_proto::api::{ModelChoice, Assistant, AssistantBackup, AssistantFile, AssistantPreferences, AssistantSecret, AssistantSecretState, AssistantState, PutAssistantSecretRequest, CreateAssistantRequest, CreateChatRequest, CreateConversationRequest, PatchAssistantRequest, PromptRequest, NODE_COLOURS};
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
const ASSISTANT_COLUMNS: &str = "a.id, a.name, a.description, a.group_id, a.preferences, a.builtin_key, a.conversation_id, a.backup, a.pinned_at, a.archived_at, a.icon, a.colour, a.avatar_sha256, a.created_at,
    c.status as current_status,
    (select max(updated_at) from conversations where assistant_id = a.id) as last_active_at,
    (select count(*) from conversations where assistant_id = a.id) as session_count,
    (select string_agg(s.cron, '|' order by s.next_run_at) from schedules s where s.assistant_id = a.id and not s.proposed and not s.paused) as wake_cron,
    (select substring(content from 1 for $3) from assistant_files f where f.assistant_id = a.id and f.path = 'AGENTS.md') as instructions_preview";

#[derive(sqlx::FromRow)]
struct AssistantRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    group_id: Option<Uuid>,
    preferences: Jsonb<AssistantPreferences>,
    builtin_key: Option<String>,
    conversation_id: Option<Uuid>,
    backup: Option<Jsonb<AssistantBackup>>,
    pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    icon: Option<String>,
    colour: Option<String>,
    avatar_sha256: Option<String>,
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
            secrets: Vec::new(),
            backup: self.backup.map(|backup| backup.0),
            wakes: self.wake_cron.as_deref().map(|crons| crons.split('|').map(crate::schedules::describe_cron).collect::<Vec<_>>().join(", ")),
            builtin_key: self.builtin_key,
            icon: self.icon,
            colour: self.colour,
            avatar_sha256: self.avatar_sha256,
            created_at: self.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    archived: bool,
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<Assistant> {
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
    let assistant = find_owned(state, id, owner).await?;

    state.publish(Some(owner), StreamMessage::Assistant(assistant.clone()));

    Ok(assistant)
}

pub async fn instructions_of(state: &AppState, id: Uuid) -> ApiResult<String> {
    let content = sqlx::query_scalar::<_, Vec<u8>>("select content from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(INSTRUCTIONS_PATH).fetch_optional(&state.pool).await?;

    Ok(String::from_utf8_lossy(&content.unwrap_or_default()).into_owned())
}

fn check_file_size(path: &str, content: &[u8]) -> ApiResult<()> {
    if content.len() > FILE_LIMIT {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large", format!("{path} is over 1 MiB. Shorten it or split it into smaller files.")));
    }

    Ok(())
}

pub async fn store_file(state: &AppState, id: Uuid, path: &str, content: &[u8]) -> ApiResult<()> {
    check_file_size(path, content)?;

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

    if path == INSTRUCTIONS_PATH {
        crate::instructions::record_assistant_version(state, id, &String::from_utf8_lossy(content)).await?;
    }

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

pub fn valid_name(name: &str) -> ApiResult<String> {
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

pub async fn validate_node(state: &AppState, owner: Uuid, node_id: Uuid) -> ApiResult<()> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and created_by = $2 and revoked_at is null)").bind(node_id).bind(owner).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::bad_request("node_unknown", "That node is not one of yours"));
    }

    Ok(())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateAssistantRequest>) -> ApiResult<(StatusCode, Json<Assistant>)> {
    let assistant = create_for(&state, user.id, request).await?;

    Ok((StatusCode::CREATED, Json(assistant)))
}

pub async fn create_for(state: &AppState, owner: Uuid, request: CreateAssistantRequest) -> ApiResult<Assistant> {
    let name = valid_name(&request.name)?;
    let preferences = request.preferences.unwrap_or_default();
    let instructions = request.instructions.unwrap_or_default();

    check_file_size(INSTRUCTIONS_PATH, instructions.as_bytes())?;

    if let Some(group_id) = request.group_id {
        groups::find_owned(state, group_id, owner).await?;
    }

    let description = request.description.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let inserted = sqlx::query_scalar::<_, Uuid>("insert into assistants (owner, name, description, group_id, preferences) values ($1, $2, $3, $4, $5) returning id")
        .bind(owner)
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

    store_file(state, id, INSTRUCTIONS_PATH, instructions.as_bytes()).await?;

    publish(state, owner, id).await
}

pub async fn list_for(state: &AppState, owner: Uuid, archived: bool) -> ApiResult<Vec<Assistant>> {
    let rows = sqlx::query_as::<_, AssistantRow>(sqlx::AssertSqlSafe(format!(
        "select {ASSISTANT_COLUMNS} from assistants a left join conversations c on c.id = a.conversation_id
         where a.owner = $1 and (a.archived_at is not null) = $2 order by lower(a.name), a.id"
    )))
    .bind(owner)
    .bind(archived)
    .bind(DESCRIPTION_PREVIEW_BYTES)
    .fetch_all(&state.pool)
    .await?;

    let mut assistants: Vec<Assistant> = rows.into_iter().map(AssistantRow::to_proto).collect();

    for assistant in &mut assistants {
        assistant.secrets = secrets_of(state, assistant.id).await?;
    }

    Ok(assistants)
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Assistant>>> {
    if !query.archived {
        crate::voice::ensure_voice(&state, user.id).await?;
    }

    Ok(Json(list_for(&state, user.id, query.archived).await?))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let mut assistant = find_owned(&state, id, user.id).await?;

    assistant.instructions = Some(instructions_of(&state, id).await?);

    Ok(Json(assistant))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchAssistantRequest>) -> ApiResult<Json<Assistant>> {
    let current = find_owned(&state, id, user.id).await?;

    if current.builtin_key.is_some() {
        if request.name.as_deref().is_some_and(|name| name.trim() != current.name) {
            return Err(ApiError::conflict("assistant_built_in", format!("{} is built in, so it cannot be renamed. Change its instructions or actions instead.", current.name)));
        }

        if request.archived == Some(true) {
            return Err(ApiError::conflict("assistant_built_in", format!("{} is built in, so it cannot be archived. Pause its schedules instead.", current.name)));
        }
    }

    let name = request.name.as_deref().map(valid_name).transpose()?;

    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let icon = request.icon.as_deref().map(str::trim).filter(|icon| !icon.is_empty());

    if icon.is_some_and(|icon| !crate::projects::valid_icon(icon)) {
        return Err(ApiError::bad_request("icon_invalid", "Choose an icon from the list."));
    }

    if let Some(colour) = request.colour.as_deref().filter(|colour| !NODE_COLOURS.contains(colour)) {
        return Err(ApiError::bad_request("unknown_colour", format!("{colour} is not a colour Mycelium offers. Choose one of {}.", NODE_COLOURS.join(", "))));
    }

    let description = request.description.as_deref().map(str::trim);
    let updated = sqlx::query(
        "update assistants set name = coalesce($3, name),
             description = case when $4::text is null then description else nullif($4, '') end,
             group_id = case when $6 then null else coalesce($5, group_id) end,
             pinned_at = case when $7::boolean is null then pinned_at when $7 then coalesce(pinned_at, now()) else null end,
             archived_at = case when $8::boolean is null then archived_at when $8 then coalesce(archived_at, now()) else null end,
             preferences = coalesce($9, preferences),
             icon = case when $10 then null else coalesce($11, icon) end,
             avatar_sha256 = case when $10 or $11::text is not null then null else avatar_sha256 end,
             colour = case when $12 then null else coalesce($13, colour) end,
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
    .bind(request.clear_icon)
    .bind(icon)
    .bind(request.clear_colour)
    .bind(request.colour.as_deref())
    .execute(&state.pool)
    .await;

    match updated {
        Ok(_) => {}
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => return Err(name_taken(name.as_deref().unwrap_or_default())),
        Err(cause) => return Err(cause.into()),
    }

    if let Some(instructions) = &request.instructions {
        match user.scopes.is_some() {
            true => {
                let proposed = crate::instruction_proposals::propose_for_assistant(&state, user.id, id, None, instructions, "Proposed through the API.").await?;

                if let Err(message) = proposed {
                    tracing::debug!("{message}");
                }
            }
            false => store_file(&state, id, INSTRUCTIONS_PATH, instructions.as_bytes()).await?,
        }
    }

    let mut assistant = publish(&state, user.id, id).await?;

    if request.instructions.is_some() {
        assistant.instructions = Some(instructions_of(&state, id).await?);
    }

    Ok(Json(assistant))
}

pub async fn upload_avatar(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, headers: axum::http::HeaderMap, body: Bytes) -> ApiResult<Json<Assistant>> {
    find_owned(&state, id, user.id).await?;
    let media_type = crate::attachments::media_type_of(&headers);
    crate::projects::check_avatar(&media_type, &body)?;
    let (sha256, _, _) = crate::attachments::store(&state, user.id, &body, &media_type).await?;

    sqlx::query("update assistants set avatar_sha256 = $3, icon = null, updated_at = now() where id = $1 and owner = $2").bind(id).bind(user.id).bind(&sha256).execute(&state.pool).await?;

    Ok(Json(publish(&state, user.id, id).await?))
}

pub async fn prompt(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PromptRequest>) -> ApiResult<Json<Conversation>> {
    let entry = idempotency::enter(&state, id, request.client_id).await?;
    let claim = match entry {
        idempotency::Entry::Repeat(result) => {
            let conversation = result.ok_or_else(|| ApiError::not_found("That conversation"))?;

            return Ok(Json(store::find_owned(&state.pool, conversation, user.id).await?.to_proto()?));
        }
        idempotency::Entry::Fresh(claim) => claim,
    };
    let conversation = wake(&state, user.id, id, request, &StartOverrides::default()).await?;

    if let Some(claim) = claim {
        claim.done(Some(conversation.id)).await?;
    }

    Ok(Json(conversation))
}

#[derive(Default)]
pub struct StartOverrides {
    pub model: Option<ModelChoice>,
    pub node_id: Option<Uuid>,
}

pub async fn wake(state: &AppState, owner: Uuid, id: Uuid, request: PromptRequest, overrides: &StartOverrides) -> ApiResult<Conversation> {
    let assistant = find_owned(state, id, owner).await?;
    let text = request.text.trim();
    let spoken = request.spoken;

    if assistant.archived_at.is_some() {
        return Err(ApiError::conflict("assistant_archived", "This assistant is archived. Restore it, then send your message again."));
    }

    if text.is_empty() && request.attachments.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter a message for the assistant."));
    }

    let current = match assistant.conversation_id {
        Some(conversation_id) => Some(store::find_owned(&state.pool, conversation_id, owner).await?),
        None => None,
    };
    let conversation = match current {
        Some(row) if chats::is_chat(&row) && request.attachments.is_empty() => {
            chats::send_or_queue(state, owner, row.id, text).await?;

            store::find_owned(&state.pool, row.id, owner).await?.to_proto()?
        }
        Some(row) if chats::is_chat(&row) => chats::send(state, owner, row.id, text, request.attachments).await?,
        Some(row) => {
            prompts::send(state, owner, row.id, request.clone()).await?;

            store::find_owned(&state.pool, row.id, owner).await?.to_proto()?
        }
        None => {
            let chosen = overrides.model.clone().or(request.model.clone());
            let node_id = overrides.node_id.or(request.node_id).unwrap_or(ANY_NODE);

            match chosen {
                Some(ModelChoice { provider_id: Some(provider_id), model, .. }) => {
                    let started = chats::start(state, owner, CreateChatRequest { provider_id: Some(provider_id), agent: None, account: None, node_id: None, model, prompt: None, parent_id: None, client_id: None }, None, Some(id)).await?;

                    adopt(state, owner, id, started.id).await?;

                    chats::send(state, owner, started.id, text, request.attachments).await?
                }
                chosen => {
                    let (agent, account, model, routing) = match chosen {
                        Some(ModelChoice { agent: Some(agent), account: Some(account), model, any_model: false, .. }) => (agent, Some(account), model, None),
                        Some(ModelChoice { agent: Some(agent), model, any_model: false, .. }) => (agent, None, model, Some(Routing::for_agent(agent))),
                        _ => (AgentId::Claude, None, None, Some(Routing::AnyModel)),
                    };
                    let first = Some(text.to_string()).filter(|_| request.attachments.is_empty());
                    let started = CreateConversationRequest { mode: None, config: None, node_id, agent, account, cwd: folder_of(id), project_id: None, branch: None, worktree_path: None, title: None, prompt: first.clone(), parent_id: None, side_kind: None, model, routing, client_id: None };
                    let started = conversations::start_for(state, owner, started, Some(id), false).await?;

                    adopt(state, owner, id, started.id).await?;

                    if first.is_none() {
                        prompts::send(state, owner, started.id, request.clone()).await?;
                    }

                    store::find_owned(&state.pool, started.id, owner).await?.to_proto()?
                }
            }
        }
    };

    if spoken {
        prompts::mark_spoken(state, conversation.id).await?;
    }

    publish(state, owner, id).await?;

    Ok(conversation)
}

async fn adopt(state: &AppState, owner: Uuid, id: Uuid, started: Uuid) -> ApiResult<()> {
    sqlx::query("update assistants set conversation_id = $2, updated_at = now() where id = $1").bind(id).bind(started).execute(&state.pool).await?;
    crate::todos::carry(state, owner, id, started).await?;

    take_over(state, owner, id, started).await
}

async fn end_session(state: &AppState, owner: Uuid, id: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let row = store::find_owned(&state.pool, conversation_id, owner).await?;

    if chats::is_chat(&row) {
        return chats::stop(state, owner, conversation_id).await;
    }

    if let Err(error) = conversations::close_for(state, owner, conversation_id).await {
        tracing::warn!("could not close the working session of assistant {id}: {}", error.message());
    }

    Ok(())
}

pub async fn earlier_session_of(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<(Uuid, Option<Uuid>)>> {
    let found = sqlx::query_as::<_, (Uuid, Option<Uuid>)>("select a.id, a.conversation_id from conversations c join assistants a on a.id = c.assistant_id where c.id = $1 and c.parent_id is null and a.archived_at is null and a.conversation_id is distinct from c.id")
        .bind(conversation_id)
        .fetch_optional(&state.pool)
        .await?;

    Ok(found)
}

async fn take_over(state: &AppState, owner: Uuid, id: Uuid, current: Uuid) -> ApiResult<()> {
    let earlier = sqlx::query_as::<_, (Uuid, bool)>("select id, status in ('working', 'input') from conversations where assistant_id = $1 and parent_id is null and id <> $2").bind(id).bind(current).fetch_all(&state.pool).await?;
    let ids: Vec<Uuid> = earlier.iter().map(|(conversation_id, _)| *conversation_id).collect();

    for (conversation_id, _) in earlier.iter().filter(|(_, running)| *running) {
        end_session(state, owner, id, *conversation_id).await?;
    }

    sqlx::query("update watches w set ended_at = now() where w.watcher_id = any($2) and w.ended_at is null and w.kind = 'conversation' and exists (select 1 from watches other where other.ended_at is null and other.kind = 'conversation' and other.target_id = w.target_id and (other.watcher_id = $1 or (other.watcher_id = any($2) and other.id < w.id)))")
        .bind(current)
        .bind(&ids)
        .execute(&state.pool)
        .await?;
    sqlx::query("update watches set watcher_id = $1 where watcher_id = any($2) and ended_at is null").bind(current).bind(&ids).execute(&state.pool).await?;
    sqlx::query("update conversations set managed_by = $1 where managed_by = any($2)").bind(current).bind(&ids).execute(&state.pool).await?;
    crate::conversation_connections::carry(state, owner, &ids, current).await?;
    state.publish(Some(owner), StreamMessage::WatchesChanged { conversation_id: current });

    Ok(())
}

pub async fn clear(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let assistant = find_owned(&state, id, user.id).await?;

    if let Some(conversation_id) = assistant.conversation_id {
        end_session(&state, user.id, id, conversation_id).await?;
    }

    sqlx::query("update assistants set conversation_id = null, updated_at = now() where id = $1").bind(id).execute(&state.pool).await?;

    Ok(Json(publish(&state, user.id, id).await?))
}

pub async fn list_files(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<AssistantFile>>> {
    find_owned(&state, id, user.id).await?;

    Ok(Json(files_of(&state, id, "").await?))
}

pub async fn read_file(State(state): State<AppState>, user: AuthUser, Path((id, path)): Path<(Uuid, String)>) -> ApiResult<Response> {
    find_owned(&state, id, user.id).await?;

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
    find_owned(&state, id, user.id).await?;

    let path = valid_path(&path)?;

    store_file(&state, id, &path, &body).await?;
    publish(&state, user.id, id).await?;
    crate::assistant_backup::trigger(&state, user.id, id);

    let written = files_of(&state, id, &path).await?.into_iter().find(|file| file.path == path);

    written.map(Json).ok_or_else(|| ApiError::not_found("That file"))
}

pub async fn delete_file(State(state): State<AppState>, user: AuthUser, Path((id, path)): Path<(Uuid, String)>) -> ApiResult<StatusCode> {
    find_owned(&state, id, user.id).await?;

    let deleted = sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(id).bind(&path).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That file"));
    }

    publish(&state, user.id, id).await?;
    crate::assistant_backup::trigger(&state, user.id, id);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_secrets(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<AssistantSecret>>> {
    find_owned(&state, id, user.id).await?;

    Ok(Json(secrets_of(&state, id).await?))
}

pub async fn put_secret(State(state): State<AppState>, user: AuthUser, Path((id, name)): Path<(Uuid, String)>, Json(request): Json<PutAssistantSecretRequest>) -> ApiResult<Json<AssistantSecret>> {
    find_owned(&state, id, user.id).await?;

    let name = valid_secret_name(&name)?;
    let value = request.value.filter(|value| !value.is_empty());

    if let Some(value) = &value {
        sqlx::query(
            "insert into assistant_secrets (assistant_id, name, value, description, set_by) values ($1, $2, $3, $4, 'person')
             on conflict (assistant_id, name) do update set value = excluded.value, description = coalesce(excluded.description, assistant_secrets.description), set_by = 'person', updated_at = now()",
        )
        .bind(id)
        .bind(&name)
        .bind(crate::secrets::Sealed::from(value.as_str()))
        .bind(request.description.as_deref())
        .execute(&state.pool)
        .await?;
        redact_stored(&state, id, value).await?;
        push_secrets(&state, id).await?;
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
    find_owned(&state, id, user.id).await?;

    let deleted = sqlx::query("delete from assistant_secrets where assistant_id = $1 and name = $2").bind(id).bind(&name).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That secret"));
    }

    push_secrets(&state, id).await?;
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

    sqlx::query("update assistant_files set folder_sha256 = sha256 where assistant_id = $1").bind(assistant_id).execute(&state.pool).await?;

    Ok(())
}

pub async fn copy_between_nodes(state: &AppState, from_node: Uuid, to_node: Uuid, root: &str) -> ApiResult<()> {
    let asked = state.ask_node::<FolderAnswer>(from_node, FOLDER_TIMEOUT, |request_id| ControlToNode::ReadFolder { request_id, root: root.to_string() }).await;
    let Some(answer) = asked else {
        return Err(ApiError::conflict("folder_unavailable", "The folder could not be copied because its node did not answer in time. Bring the node online, then try again."));
    };

    if let Some(error) = answer.error {
        return Err(ApiError::conflict("folder_unavailable", format!("The folder could not be copied: {error}")));
    }

    let mut chunks: Vec<Vec<FolderFile>> = vec![Vec::new()];
    let mut size = 0;

    for file in answer.files {
        if size + file.data_base64.len() > FOLDER_CHUNK_BYTES && chunks.last().is_some_and(|chunk| !chunk.is_empty()) {
            chunks.push(Vec::new());
            size = 0;
        }

        size += file.data_base64.len();

        if let Some(chunk) = chunks.last_mut() {
            chunk.push(file);
        }
    }

    for (position, files) in chunks.into_iter().enumerate() {
        if !state.hub.send(to_node, ControlToNode::WriteFolder { root: root.to_string(), files, replace: position == 0 }) {
            return Err(crate::nodes::node_offline());
        }
    }

    Ok(())
}

pub async fn copy_up(state: &AppState, owner: Uuid, node_id: Uuid, assistant_id: Uuid) -> ApiResult<()> {
    if !state.hub.is_online(node_id) {
        return Ok(());
    }

    let asked = state.ask_node::<FolderAnswer>(node_id, FOLDER_TIMEOUT, |request_id| ControlToNode::ReadFolder { request_id, root: folder_of(assistant_id) }).await;

    let Some(answer) = asked else {
        tracing::warn!("node {node_id} did not return the folder of assistant {assistant_id} in time");

        return Ok(());
    };

    if let Some(error) = answer.error {
        tracing::warn!("node {node_id} could not read the folder of assistant {assistant_id}: {error}");

        return Ok(());
    }

    let stored = sqlx::query_as::<_, (String, String, Option<String>)>("select path, sha256, folder_sha256 from assistant_files where assistant_id = $1").bind(assistant_id).fetch_all(&state.pool).await?;
    let mut kept = Vec::new();

    for file in answer.files {
        let Ok(content) = base64::engine::general_purpose::STANDARD.decode(&file.data_base64) else {
            continue;
        };
        let digest = hex_digest(&content);
        let untouched = stored.iter().any(|(path, current, given)| *path == file.path && (*current == digest || given.as_deref() == Some(digest.as_str())));

        kept.push(file.path.clone());

        if untouched || valid_path(&file.path).is_err() {
            continue;
        }

        match store_file(state, assistant_id, &file.path, &content).await {
            Ok(()) => {
                sqlx::query("update assistant_files set folder_sha256 = sha256 where assistant_id = $1 and path = $2").bind(assistant_id).bind(&file.path).execute(&state.pool).await?;
            }
            Err(error) => tracing::warn!("could not keep {} from assistant {assistant_id}: {}", file.path, error.message()),
        }
    }

    if !answer.truncated {
        let removed_by_the_agent = stored.iter().filter(|(path, current, given)| !kept.contains(path) && given.as_deref() == Some(current.as_str()));

        for (path, _, _) in removed_by_the_agent {
            sqlx::query("delete from assistant_files where assistant_id = $1 and path = $2").bind(assistant_id).bind(path).execute(&state.pool).await?;
        }
    }

    let newer = sqlx::query_as::<_, (String, Vec<u8>)>("select path, content from assistant_files where assistant_id = $1 and folder_sha256 is distinct from sha256 order by path").bind(assistant_id).fetch_all(&state.pool).await?;

    if !newer.is_empty() {
        let files = newer.into_iter().map(|(path, content)| FolderFile { path, data_base64: base64::engine::general_purpose::STANDARD.encode(content) }).collect();

        if state.hub.send(node_id, ControlToNode::WriteFolder { root: folder_of(assistant_id), files, replace: false }) {
            sqlx::query("update assistant_files set folder_sha256 = sha256 where assistant_id = $1").bind(assistant_id).execute(&state.pool).await?;
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
    let node_id = crate::node_pick::pick(state, owner, None).await?.node_id;
    let start = crate::routing::start_pick(state, owner, node_id, Routing::AnyModel).await?;

    copy_down(state, node_id, assistant_id).await?;

    let target = Target { agent: enum_text(&start.agent), account: Some(start.account), provider_id: None, model: start.model, node_id: Some(node_id), cwd: Some(folder_of(assistant_id)) };

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
    let rows = sqlx::query_as::<_, (String, crate::secrets::Sealed)>(
        "select s.name, s.value from conversations c join assistant_secrets s on s.assistant_id = c.assistant_id where c.id = $1 and c.agent <> $2 and s.value is not null order by s.name",
    )
    .bind(conversation_id)
    .bind(chats::CHAT_AGENT)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows.into_iter().map(|(name, value)| (name, value.into_string())).collect())
}

async fn push_secrets(state: &AppState, assistant_id: Uuid) -> ApiResult<()> {
    let sessions = sqlx::query_as::<_, (Uuid, Uuid)>("select id, node_id from conversations where assistant_id = $1 and agent <> $2").bind(assistant_id).bind(chats::CHAT_AGENT).fetch_all(&state.pool).await?;

    for (conversation_id, node_id) in sessions {
        if !state.hub.has_session(node_id, conversation_id) {
            continue;
        }

        let env = secret_env(state, conversation_id).await?;
        state.hub.mark_secrets_stale(conversation_id);
        state.hub.send(node_id, ControlToNode::SecretsChanged { conversation_id, env });
    }

    Ok(())
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
        .bind(crate::secrets::Sealed::from(value.as_str()))
        .bind(description.as_deref())
        .execute(&state.pool)
        .await?;
        redact_stored(state, id, &value).await?;
        push_secrets(state, id).await?;
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
    let (Some(conversation_id), Some((Some(_), agent, owner))) = (conversation_id, session) else {
        return Ok(("This conversation does not belong to an assistant.".to_string(), true));
    };

    if agent != chats::CHAT_AGENT {
        return Ok(("You already have a working folder.".to_string(), true));
    }

    let Ok(picked) = crate::node_pick::pick(state, owner, None).await else {
        return Ok(("No node can run this assistant right now. Tell the person to bring a node online and signed in.".to_string(), true));
    };
    let name = picked.name;

    state.hub.ask_workspace(conversation_id);

    Ok((format!("The thread moves to {name} when this reply ends, with your storage in a working folder. Finish this reply, then carry on there."), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_must_be_environment_variable_names() {
        assert!(valid_secret_name("GITHUB_TOKEN").is_ok());
        assert!(valid_secret_name("_X1").is_ok());
        assert!(valid_secret_name("github_token").is_err());
        assert!(valid_secret_name("1TOKEN").is_err());
        assert!(valid_secret_name("A-B").is_err());
        assert!(valid_secret_name("").is_err());
    }
}
