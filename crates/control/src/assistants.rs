use crate::attachments::hex_digest;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::groups;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Assistant, AssistantPreferences, AssistantState, CreateAssistantRequest, PatchAssistantRequest};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use sqlx::types::Json as Jsonb;
use uuid::Uuid;

pub const INSTRUCTIONS_PATH: &str = "AGENTS.md";
const NAME_LIMIT: usize = 60;
const DESCRIPTION_PREVIEW_BYTES: i32 = 400;
const FALL_BACK_CONDITIONS: [&str; 4] = ["usage_limit", "signed_out", "offline", "model_error"];
const ASSISTANT_COLUMNS: &str = "a.id, a.name, a.description, a.group_id, a.preferences, a.conversation_id, a.pinned_at, a.archived_at, a.created_at,
    c.status as current_status,
    (select max(updated_at) from conversations where assistant_id = a.id) as last_active_at,
    (select count(*) from conversations where assistant_id = a.id) as session_count,
    (select substring(content from 1 for $3) from assistant_files f where f.assistant_id = a.id and f.path = 'AGENTS.md') as instructions_preview";

#[derive(sqlx::FromRow)]
struct AssistantRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    group_id: Option<Uuid>,
    preferences: Jsonb<AssistantPreferences>,
    conversation_id: Option<Uuid>,
    pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    current_status: Option<String>,
    last_active_at: Option<chrono::DateTime<chrono::Utc>>,
    session_count: i64,
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

    row.map(AssistantRow::to_proto).ok_or_else(|| ApiError::not_found("That assistant"))
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

    Ok(Json(rows.into_iter().map(AssistantRow::to_proto).collect()))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    let mut assistant = load(&state, user.id, id).await?;

    assistant.instructions = Some(instructions_of(&state, id).await?);

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
