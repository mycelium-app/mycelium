use crate::assistants;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::groups;
use crate::instruction_proposals;
use crate::prompts;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{AssistantChange, AssistantPreferences, CreateAssistantRequest, ModelChoice};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use sqlx::types::Json as Jsonb;
use uuid::Uuid;

const CHANGE_COLUMNS: &str = "id, conversation_id, kind, assistant_id, name, description, group_id, instructions, preferences, state, created_at, decided_at";

#[derive(sqlx::FromRow)]
struct ChangeRow {
    id: Uuid,
    conversation_id: Option<Uuid>,
    kind: String,
    assistant_id: Option<Uuid>,
    name: String,
    description: Option<String>,
    group_id: Option<Uuid>,
    instructions: Option<String>,
    preferences: Option<Jsonb<AssistantPreferences>>,
    state: String,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ChangeRow {
    fn to_proto(self) -> AssistantChange {
        AssistantChange {
            id: self.id,
            conversation_id: self.conversation_id,
            kind: self.kind,
            assistant_id: self.assistant_id,
            name: self.name,
            description: self.description,
            group_id: self.group_id,
            instructions: self.instructions,
            preferences: self.preferences.map(|preferences| preferences.0),
            state: self.state,
            created_at: self.created_at,
            decided_at: self.decided_at,
        }
    }
}

fn publish(state: &AppState, owner: Uuid, row: ChangeRow) -> AssistantChange {
    let change = row.to_proto();

    state.publish(Some(owner), StreamMessage::AssistantChange(change.clone()));

    change
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ChangeRow> {
    let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!("select {CHANGE_COLUMNS} from assistant_changes where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That change"))
}

async fn group_named(state: &AppState, owner: Uuid, given: &str) -> ApiResult<Result<Uuid, String>> {
    let found = match given.parse::<Uuid>() {
        Ok(id) => sqlx::query_scalar::<_, Uuid>("select id from groups where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?,
        Err(_) => sqlx::query_scalar::<_, Uuid>("select id from groups where owner = $1 and lower(name) = lower($2)").bind(owner).bind(given).fetch_optional(&state.pool).await?,
    };

    Ok(found.ok_or_else(|| format!("No group is called {given}. Use the name of one of the person's groups.")))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<AssistantChange>>> {
    let rows = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!("select {CHANGE_COLUMNS} from assistant_changes where owner = $1 order by (state = 'proposed') desc, created_at desc limit 100"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(ChangeRow::to_proto).collect()))
}

pub async fn list_tool(state: &AppState, owner: Uuid) -> ApiResult<String> {
    let assistants = assistants::list_for(state, owner, false).await?;

    if assistants.is_empty() {
        return Ok("The person has no assistants.".to_string());
    }

    let groups = sqlx::query_as::<_, (Uuid, String)>("select id, name from groups where owner = $1").bind(owner).fetch_all(&state.pool).await?;
    let lines: Vec<String> = assistants
        .iter()
        .map(|assistant| {
            let group = assistant.group_id.and_then(|id| groups.iter().find(|(group_id, _)| *group_id == id)).map(|(_, name)| name.as_str()).unwrap_or("no group");
            let state = serde_json::to_value(assistant.state).ok().and_then(|value| value.as_str().map(str::to_string)).unwrap_or_default();

            format!("{} | {} | {} | group: {group} | {state} | built in: {}", assistant.id, assistant.name, assistant.description.as_deref().unwrap_or("no description"), assistant.builtin_key.is_some())
        })
        .collect();

    Ok(lines.join("\n"))
}

pub async fn propose_create(state: &AppState, owner: Uuid, conversation_id: Option<Uuid>, arguments: &Value) -> ApiResult<Result<AssistantChange, String>> {
    let text = |field: &str| arguments.get(field).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    let Some(name) = text("name") else {
        return Ok(Err("Nothing was proposed. Give the assistant a name.".to_string()));
    };
    let Some(instructions) = text("instructions") else {
        return Ok(Err("Nothing was proposed. Give the assistant's instructions.".to_string()));
    };
    let name = match assistants::valid_name(name) {
        Ok(name) => name,
        Err(error) => return Ok(Err(format!("Nothing was proposed. {}", error.message()))),
    };
    let taken = sqlx::query_scalar::<_, bool>("select exists (select 1 from assistants where owner = $1 and name = $2)").bind(owner).bind(&name).fetch_one(&state.pool).await?;

    if taken {
        return Ok(Err(format!("Nothing was proposed. The person already has an assistant called {name}. Choose another name.")));
    }

    let group_id = match text("group") {
        Some(given) => match group_named(state, owner, given).await? {
            Ok(id) => Some(id),
            Err(message) => return Ok(Err(format!("Nothing was proposed. {message}"))),
        },
        None => None,
    };
    let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!(
        "insert into assistant_changes (owner, conversation_id, kind, name, description, group_id, instructions) values ($1, $2, 'create', $3, $4, $5, $6) returning {CHANGE_COLUMNS}",
    )))
    .bind(owner)
    .bind(conversation_id)
    .bind(&name)
    .bind(text("description"))
    .bind(group_id)
    .bind(instructions)
    .fetch_one(&state.pool)
    .await?;

    Ok(Ok(publish(state, owner, row)))
}

pub async fn update_from_agent(state: &AppState, owner: Uuid, conversation_id: Option<Uuid>, arguments: &Value) -> ApiResult<Result<String, String>> {
    let text = |field: &str| arguments.get(field).and_then(Value::as_str).map(str::trim);
    let Some(id) = text("id").and_then(|given| given.parse::<Uuid>().ok()) else {
        return Ok(Err("Nothing was changed. Give the id of the assistant, as assistant_list returned it.".to_string()));
    };
    let Ok(assistant) = assistants::load(state, owner, id).await else {
        return Ok(Err("Nothing was changed. The person has no assistant with that id.".to_string()));
    };
    let mut preferences = assistant.preferences.clone();

    if let Some(models) = arguments.get("models") {
        match serde_json::from_value::<Vec<ModelChoice>>(models.clone()) {
            Ok(models) => preferences.models = models,
            Err(_) => return Ok(Err("Nothing was changed. Each model needs an agent (claude or codex) or a provider_id with a model, and optionally an account.".to_string())),
        }
    }

    if let Some(nodes) = arguments.get("nodes") {
        match serde_json::from_value::<Vec<Uuid>>(nodes.clone()) {
            Ok(nodes) => preferences.nodes = nodes,
            Err(_) => return Ok(Err("Nothing was changed. nodes is a list of node ids.".to_string())),
        }
    }

    if let Err(error) = assistants::validate_preferences(state, owner, &preferences).await {
        return Ok(Err(format!("Nothing was changed. {}", error.message())));
    }

    let description = text("description").map(str::to_string);
    let group_id = match text("group").filter(|given| !given.is_empty()) {
        Some(given) => match group_named(state, owner, given).await? {
            Ok(id) => Some(id),
            Err(message) => return Ok(Err(format!("Nothing was changed. {message}"))),
        },
        None => None,
    };
    let instructions = arguments.get("instructions").and_then(Value::as_str);
    let settings_given = description.is_some() || group_id.is_some() || arguments.get("models").is_some() || arguments.get("nodes").is_some();

    if !settings_given && instructions.is_none() {
        return Ok(Err("Nothing was changed. Give at least one of description, group, models, nodes or instructions.".to_string()));
    }

    let mut said: Vec<String> = Vec::new();

    if settings_given {
        let before = serde_json::json!({"description": assistant.description, "group_id": assistant.group_id, "preferences": assistant.preferences});

        sqlx::query(
            "update assistants set description = case when $3::text is null then description else nullif($3, '') end, group_id = coalesce($4, group_id), preferences = $5, updated_at = now() where id = $1 and owner = $2",
        )
        .bind(id)
        .bind(owner)
        .bind(description.as_deref())
        .bind(group_id)
        .bind(Jsonb(&preferences))
        .execute(&state.pool)
        .await?;

        let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!(
            "insert into assistant_changes (owner, conversation_id, kind, assistant_id, name, description, group_id, preferences, before, state, decided_at) values ($1, $2, 'update', $3, $4, $5, $6, $7, $8, 'applied', now()) returning {CHANGE_COLUMNS}",
        )))
        .bind(owner)
        .bind(conversation_id)
        .bind(id)
        .bind(&assistant.name)
        .bind(description.as_deref())
        .bind(group_id)
        .bind(Jsonb(&preferences))
        .bind(Jsonb(before))
        .fetch_one(&state.pool)
        .await?;

        publish(state, owner, row);
        assistants::publish(state, owner, id).await?;
        said.push(format!("Updated {}. The person can undo it from the card.", assistant.name));
    }

    if let Some(instructions) = instructions {
        match instruction_proposals::propose_for_assistant(state, owner, id, conversation_id, instructions.trim(), text("reason").unwrap_or("Proposed by an agent.")).await? {
            Ok(_) => said.push("The instructions change waits for the person to approve it.".to_string()),
            Err(message) => return Ok(Err(message)),
        }
    }

    Ok(Ok(said.join(" ")))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<AssistantChange>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.kind != "create" || found.state != "proposed" {
        return Err(ApiError::conflict("change_not_proposed", "This change has already been decided."));
    }

    if let Some(group_id) = found.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let request = CreateAssistantRequest { name: found.name.clone(), description: found.description.clone(), group_id: found.group_id, instructions: found.instructions.clone(), preferences: None };
    let (_, Json(created)) = assistants::create(State(state.clone()), user.clone(), Json(request)).await?;
    let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!("update assistant_changes set state = 'approved', assistant_id = $2, decided_at = now() where id = $1 returning {CHANGE_COLUMNS}"))).bind(id).bind(created.id).fetch_one(&state.pool).await?;

    Ok(Json(publish(&state, user.id, row)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<AssistantChange>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.kind != "create" || found.state != "proposed" {
        return Err(ApiError::conflict("change_not_proposed", "This change has already been decided."));
    }

    let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!("update assistant_changes set state = 'declined', decided_at = now() where id = $1 returning {CHANGE_COLUMNS}"))).bind(id).fetch_one(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let text = format!("I declined the assistant you proposed, {}. Do not propose it again unless I ask.", found.name);

        let _ = prompts::continue_unless_queued(&state, user.id, conversation_id, &text).await;
    }

    Ok(Json(publish(&state, user.id, row)))
}

pub async fn undo(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<AssistantChange>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.kind != "update" || found.state != "applied" {
        return Err(ApiError::conflict("change_not_applied", "This change cannot be undone."));
    }

    let before = sqlx::query_scalar::<_, Option<Value>>("select before from assistant_changes where id = $1").bind(id).fetch_one(&state.pool).await?.unwrap_or(Value::Null);
    let assistant_id = found.assistant_id.ok_or_else(|| ApiError::not_found("That assistant"))?;
    let description = before.get("description").and_then(Value::as_str).map(str::to_string);
    let group_id = before.get("group_id").and_then(Value::as_str).and_then(|given| given.parse::<Uuid>().ok());
    let preferences: AssistantPreferences = serde_json::from_value(before.get("preferences").cloned().unwrap_or(Value::Null)).unwrap_or_default();

    sqlx::query("update assistants set description = $3, group_id = $4, preferences = $5, updated_at = now() where id = $1 and owner = $2")
        .bind(assistant_id)
        .bind(user.id)
        .bind(description)
        .bind(group_id)
        .bind(Jsonb(&preferences))
        .execute(&state.pool)
        .await?;

    let row = sqlx::query_as::<_, ChangeRow>(sqlx::AssertSqlSafe(format!("update assistant_changes set state = 'undone', decided_at = now() where id = $1 returning {CHANGE_COLUMNS}"))).bind(id).fetch_one(&state.pool).await?;

    assistants::publish(&state, user.id, assistant_id).await?;

    Ok(Json(publish(&state, user.id, row)))
}
