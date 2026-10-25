use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::token_hint;
use crate::mcp_client::{McpConnection, McpTool};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ChatToolInfo, ChatToolServer, CreateChatToolServerRequest, PatchChatToolServerRequest};
use serde_json::Value;
use uuid::Uuid;

pub const OWN_SERVER: &str = "mycelium";
const SERVER_COLUMNS: &str = "id, name, url, bearer_token, enabled, always_allow, tools, created_at";

#[derive(sqlx::FromRow)]
pub struct ServerRow {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub bearer_token: Option<String>,
    pub enabled: bool,
    pub always_allow: bool,
    pub tools: sqlx::types::Json<Vec<StoredTool>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct StoredTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn slug(name: &str) -> String {
    let mapped: String = name.trim().to_lowercase().chars().map(|character| if character.is_ascii_alphanumeric() { character } else { '_' }).collect();

    mapped.trim_matches('_').to_string()
}

impl ServerRow {
    pub fn to_proto(&self) -> ChatToolServer {
        ChatToolServer {
            id: self.id,
            name: self.name.clone(),
            url: self.url.clone(),
            token_hint: self.bearer_token.as_deref().map(token_hint),
            enabled: self.enabled,
            always_allow: self.always_allow,
            tools: self.tools.0.iter().map(|tool| ChatToolInfo { name: tool.name.clone(), description: tool.description.clone() }).collect(),
            created_at: self.created_at,
        }
    }
}

fn stored(tools: Vec<McpTool>) -> Vec<StoredTool> {
    tools.into_iter().map(|tool| StoredTool { name: tool.name, description: tool.description, input_schema: tool.input_schema }).collect()
}

async fn fetch_tools(state: &AppState, name: &str, url: &str, token: Option<&str>) -> ApiResult<Vec<StoredTool>> {
    let mut connection = McpConnection::new(&state.http, url, token);
    let tools = connection.list_tools().await.map_err(|error| ApiError::bad_request("tool_server_failed", error.describe(name)))?;

    Ok(stored(tools))
}

pub async fn list_owned(state: &AppState, owner: Uuid) -> ApiResult<Vec<ServerRow>> {
    let rows = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where owner = $1 order by created_at, id"))).bind(owner).fetch_all(&state.pool).await?;

    Ok(rows)
}

async fn check_name(state: &AppState, owner: Uuid, name: &str, except: Option<Uuid>) -> ApiResult<()> {
    if name.is_empty() || slug(name).is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for this server, using letters or numbers."));
    }

    if slug(name) == OWN_SERVER {
        return Err(ApiError::bad_request("name_taken", "Enter a different name. mycelium is the name of Mycelium's own tools."));
    }

    let taken = list_owned(state, owner).await?.iter().any(|row| Some(row.id) != except && slug(&row.name) == slug(name));

    if taken {
        return Err(ApiError::conflict("name_taken", "Enter a different name. Another tool server already has this one."));
    }

    Ok(())
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ChatToolServer>>> {
    let rows = list_owned(&state, user.id).await?;

    Ok(Json(rows.iter().map(ServerRow::to_proto).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateChatToolServerRequest>) -> ApiResult<(StatusCode, Json<ChatToolServer>)> {
    let name = request.name.trim();
    let token = request.bearer_token.as_deref().map(str::trim).filter(|token| !token.is_empty());
    let entered = request.url.trim();

    check_name(&state, user.id, name, None).await?;

    let parsed = url::Url::parse(entered).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("url_invalid", "Enter the server's address, starting with http:// or https://, for example https://example.com/mcp."));
    };

    let tools = fetch_tools(&state, name, parsed.as_str(), token).await?;
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("insert into chat_tool_servers (owner, name, url, bearer_token, tools) values ($1, $2, $3, $4, $5) returning {SERVER_COLUMNS}")))
        .bind(user.id)
        .bind(name)
        .bind(parsed.as_str())
        .bind(token)
        .bind(sqlx::types::Json(tools))
        .fetch_one(&state.pool)
        .await?;
    state.hub.tools_changed(user.id);

    Ok((StatusCode::CREATED, Json(row.to_proto())))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchChatToolServerRequest>) -> ApiResult<Json<ChatToolServer>> {
    let name = request.name.as_deref().map(str::trim);

    if let Some(name) = name {
        check_name(&state, user.id, name, Some(id)).await?;
    }

    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!(
        "update chat_tool_servers set enabled = coalesce($3, enabled), always_allow = coalesce($4, always_allow), name = coalesce($5, name) where id = $1 and owner = $2 returning {SERVER_COLUMNS}"
    )))
    .bind(id)
    .bind(user.id)
    .bind(request.enabled)
    .bind(request.always_allow)
    .bind(name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That tool server"))?;
    state.hub.tools_changed(user.id);

    Ok(Json(row.to_proto()))
}

pub async fn refresh(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<ChatToolServer>> {
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2"))).bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That tool server"))?;
    let tools = fetch_tools(&state, &row.name, &row.url, row.bearer_token.as_deref()).await?;
    let updated = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("update chat_tool_servers set tools = $2 where id = $1 returning {SERVER_COLUMNS}"))).bind(id).bind(sqlx::types::Json(tools)).fetch_one(&state.pool).await?;
    state.hub.tools_changed(user.id);

    Ok(Json(updated.to_proto()))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from chat_tool_servers where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That tool server"));
    }

    state.hub.tools_changed(user.id);

    Ok(StatusCode::NO_CONTENT)
}
