use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::token_hint;
use crate::integrations::{Credentials, Integration};
use crate::mcp_client::{McpConnection, McpError, McpTool};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ChatToolInfo, ChatToolServer, CreateChatToolServerRequest, IntegrationCall, IntegrationPolicy, PatchChatToolServerRequest, SetIntegrationGroupsRequest, ToolKind};
use serde_json::{Value, json};
use uuid::Uuid;

pub const OWN_SERVER: &str = "mycelium";

pub const DEFAULT_KEY_HEADER: &str = "authorization";

pub struct KnownApp {
    pub id: &'static str,
    pub stored_kind: &'static str,
    pub stored_in_settings: bool,
    pub key_prompt: &'static str,
    pub refused: &'static str,
    pub default_path: &'static str,
    pub keeps_path: fn(&str) -> bool,
    pub key_header: &'static str,
}

pub const KNOWN_APPS: [KnownApp; 2] = [KnownApp {
    id: "outline",
    stored_kind: "mcp",
    stored_in_settings: true,
    key_prompt: "Enter your Outline API key. Create one in Outline under Settings, API & Apps.",
    refused: "Outline refused that API key. Check it was copied whole and has not been revoked.",
    default_path: "/mcp",
    keeps_path: |path| path.trim_end_matches('/').ends_with("/mcp"),
    key_header: DEFAULT_KEY_HEADER,
}, KnownApp {
    id: "litellm",
    stored_kind: "litellm",
    stored_in_settings: false,
    key_prompt: "Enter your LiteLLM key.",
    refused: "LiteLLM refused that key. Check it was copied whole and has not been revoked.",
    default_path: "/mcp/",
    keeps_path: |path| !matches!(path, "" | "/"),
    key_header: "x-litellm-api-key",
}];

impl KnownApp {
    pub fn address(&self, parsed: &url::Url) -> String {
        if (self.keeps_path)(parsed.path()) {
            return parsed.as_str().to_string();
        }

        let mut resolved = parsed.clone();
        resolved.set_path(self.default_path);
        resolved.set_query(None);
        resolved.set_fragment(None);

        resolved.as_str().to_string()
    }
}

pub fn known_app(id: &str) -> Option<&'static KnownApp> {
    KNOWN_APPS.iter().find(|app| app.id == id)
}
pub const SERVER_COLUMNS: &str = "id, name, kind, url, account, settings, bearer_token, oauth_refresh_token, oauth_expires_at, oauth, enabled, always_allow, tools, created_at, token_expires_at, last_used_at, last_error, policy, session_key, forge_id";

#[derive(sqlx::FromRow, Clone)]
pub struct ServerRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub account: Option<String>,
    pub settings: sqlx::types::Json<Value>,
    pub bearer_token: Option<String>,
    pub oauth_refresh_token: Option<String>,
    pub oauth_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub oauth: Option<sqlx::types::Json<Value>>,
    pub enabled: bool,
    pub always_allow: bool,
    pub tools: sqlx::types::Json<Vec<StoredTool>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
    pub policy: sqlx::types::Json<IntegrationPolicy>,
    pub session_key: Option<String>,
    pub forge_id: Option<Uuid>,
    #[sqlx(skip)]
    pub groups: Vec<Uuid>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct StoredTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default)]
    pub read_only: Option<bool>,
}

pub fn slug(name: &str) -> String {
    let mapped: String = name.trim().to_lowercase().chars().map(|character| if character.is_ascii_alphanumeric() { character } else { '_' }).collect();

    mapped.trim_matches('_').to_string()
}

impl ServerRow {
    fn known_app(&self) -> Option<&'static KnownApp> {
        known_app(&self.kind).or_else(|| self.settings["app"].as_str().and_then(known_app))
    }

    pub fn key_header(&self) -> &'static str {
        self.known_app().map_or(DEFAULT_KEY_HEADER, |app| app.key_header)
    }

    pub fn to_proto(&self) -> ChatToolServer {
        ChatToolServer {
            id: self.id,
            name: self.name.clone(),
            kind: self.kind.clone(),
            url: self.url.clone(),
            account: self.account.clone(),
            settings: self.settings.0.clone(),
            token_hint: self.bearer_token.as_deref().map(token_hint),
            oauth: self.oauth.is_some(),
            enabled: self.enabled,
            always_allow: self.always_allow,
            tools: self.tool_list().iter().map(|tool| ChatToolInfo { name: tool.name.clone(), description: tool.description.clone(), kind: self.tool_kind(&tool.name), asks: self.effective(&tool.name).ask }).collect(),
            created_at: self.created_at,
            status: self.status(),
            token_expires_at: self.token_expires_at,
            last_used_at: self.last_used_at,
            last_error: self.last_error.clone(),
            policy: self.policy.0.clone(),
            locked: self.integration() == Some(Integration::Zulip),
            groups: self.groups.clone(),
            signed_in: self.session_key.is_some(),
            forge_id: self.forge_id,
        }
    }
}

fn stored(tools: Vec<McpTool>) -> Vec<StoredTool> {
    tools.into_iter().map(|tool| StoredTool { name: tool.name, description: tool.description, input_schema: tool.input_schema, read_only: tool.read_only }).collect()
}

pub async fn fetch_tools(state: &AppState, name: &str, url: &str, token: Option<&str>, key_header: &'static str, app: Option<&KnownApp>) -> ApiResult<Vec<StoredTool>> {
    let mut connection = McpConnection::new(&state.http, url, token, key_header);
    let tools = connection.list_tools().await.map_err(|error| match (&error, app) {
        (McpError::Rejected, Some(app)) => ApiError::bad_request("tool_server_failed", app.refused),
        _ => ApiError::bad_request("tool_server_failed", error.describe(name)),
    })?;

    Ok(stored(tools))
}

const REFRESH_MARGIN: chrono::Duration = chrono::Duration::seconds(60);

pub async fn bearer(state: &AppState, row: &ServerRow) -> ApiResult<Option<String>> {
    if row.oauth.is_none() {
        return Ok(row.bearer_token.clone());
    }

    let current = reload(state, row).await?;
    let expiring = current.oauth_expires_at.is_some_and(|expires_at| expires_at - chrono::Utc::now() < REFRESH_MARGIN);

    if !expiring {
        return Ok(current.bearer_token);
    }

    renew(state, row).await
}

pub fn refreshable_after_rejection(row: &ServerRow) -> bool {
    row.oauth.is_some() && row.oauth_refresh_token.is_some() && row.oauth_expires_at.is_none()
}

pub async fn renew(state: &AppState, row: &ServerRow) -> ApiResult<Option<String>> {
    let current = reload(state, row).await?;
    let (Some(refresh_token), Some(oauth)) = (current.oauth_refresh_token.as_deref(), current.oauth.as_ref()) else {
        return Ok(current.bearer_token);
    };
    let tokens = crate::mcp_oauth::refresh(state, &oauth.0, refresh_token).await.map_err(|_| ApiError::conflict("sign_in_again", format!("Sign in to {} again in Settings, Integrations.", current.name)))?;

    sqlx::query("update chat_tool_servers set bearer_token = $2, oauth_refresh_token = coalesce($3, oauth_refresh_token), oauth_expires_at = $4 where id = $1")
        .bind(current.id)
        .bind(&tokens.access_token)
        .bind(&tokens.refresh_token)
        .bind(tokens.expires_at)
        .execute(&state.pool)
        .await?;

    Ok(Some(tokens.access_token))
}

async fn reload(state: &AppState, row: &ServerRow) -> ApiResult<ServerRow> {
    sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1"))).bind(row.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That tool server"))
}

pub async fn list_owned(state: &AppState, owner: Uuid) -> ApiResult<Vec<ServerRow>> {
    let mut rows = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where owner = $1 order by created_at, id"))).bind(owner).fetch_all(&state.pool).await?;
    let memberships = sqlx::query_as::<_, (Uuid, Uuid)>("select g.server_id, g.group_id from integration_groups g join chat_tool_servers s on s.id = g.server_id where s.owner = $1 order by g.group_id").bind(owner).fetch_all(&state.pool).await?;

    for row in &mut rows {
        row.groups = memberships.iter().filter(|(server, _)| *server == row.id).map(|(_, group)| *group).collect();
    }

    Ok(rows)
}

pub async fn check_name(state: &AppState, owner: Uuid, name: &str, except: Option<Uuid>) -> ApiResult<()> {
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

fn check_expiry(expires_at: Option<chrono::DateTime<chrono::Utc>>) -> ApiResult<()> {
    match expires_at {
        Some(expires_at) if expires_at <= chrono::Utc::now() => Err(ApiError::bad_request("expiry_in_past", "Choose an expiry date in the future.")),
        _ => Ok(()),
    }
}

async fn replace_groups(state: &AppState, owner: Uuid, id: Uuid, groups: &[Uuid]) -> ApiResult<()> {
    let owned = sqlx::query_scalar::<_, i64>("select count(*) from groups where owner = $1 and id = any($2)").bind(owner).bind(groups).fetch_one(&state.pool).await?;

    if owned != groups.iter().collect::<std::collections::BTreeSet<_>>().len() as i64 {
        return Err(ApiError::bad_request("group_invalid", "Choose a group you own."));
    }

    let mut tx = state.pool.begin().await?;
    sqlx::query("delete from integration_groups where server_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("insert into integration_groups (server_id, group_id) select $1, unnest($2::uuid[]) on conflict do nothing").bind(id).bind(groups).execute(&mut *tx).await?;
    tx.commit().await?;

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
    check_expiry(request.token_expires_at)?;

    let kind = request.kind.as_deref().map(str::trim).filter(|kind| !kind.is_empty()).unwrap_or("mcp");
    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty());
    let app = known_app(kind);
    let integration = match kind {
        "mcp" => None,
        _ if app.is_some() => None,
        other => match Integration::parse(other) {
            Some(integration) => Some(integration),
            None => return Err(ApiError::bad_request("kind_invalid", "Choose MCP or one of the built-in integrations, such as Zulip, Email, Stoat or Last.fm.")),
        },
    };
    let address = match integration {
        Some(integration) if !integration.needs_address() => String::new(),
        Some(integration) if entered.is_empty() && integration.default_address().is_some() => integration.default_address().unwrap_or_default().to_string(),
        _ => {
            let parsed = url::Url::parse(entered).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
            let Some(parsed) = parsed else {
                return Err(ApiError::bad_request("url_invalid", "Enter the server's address, starting with http:// or https://, for example https://example.com/mcp, or https://example.zulipchat.com for Zulip."));
            };

            match app {
                Some(app) => app.address(&parsed),
                None => parsed.as_str().to_string(),
            }
        }
    };

    if let (Some(app), None) = (app, token) {
        return Err(ApiError::bad_request("api_key_required", app.key_prompt));
    }

    let mut settings = Value::Object(Default::default());
    let tools = match integration {
        Some(integration) => {
            let account = match (integration.account_prompt(), account) {
                (Some(prompt), None) => return Err(ApiError::bad_request("account_required", prompt)),
                (_, account) => account,
            };
            let Some(secret) = token else {
                return Err(ApiError::bad_request("api_key_required", integration.secret_prompt()));
            };
            settings = integration.normalise_settings(request.settings.as_ref().unwrap_or(&Value::Null)).map_err(|reason| ApiError::bad_request("settings_invalid", reason))?;

            if settings.is_null() {
                settings = Value::Object(Default::default());
            }

            let credentials = Credentials { address: address.clone(), account: account.unwrap_or_default().to_string(), secret: secret.to_string(), settings: settings.clone(), session: None };

            let discovered = integration.verify(&state.http, &credentials).await.map_err(|reason| ApiError::bad_request("tool_server_failed", reason))?;

            if let (Some(settings), Some(discovered)) = (settings.as_object_mut(), discovered.as_object()) {
                settings.extend(discovered.clone());
            }

            Vec::new()
        }
        None => {
            if app.is_some_and(|app| app.stored_in_settings) {
                settings = json!({"app": kind});
            }

            fetch_tools(&state, name, &address, token, app.map_or(DEFAULT_KEY_HEADER, |app| app.key_header), app).await?
        }
    };
    let kind = app.map_or(kind, |app| app.stored_kind);
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("insert into chat_tool_servers (owner, name, kind, url, account, bearer_token, tools, settings, token_expires_at, policy) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning {SERVER_COLUMNS}")))
        .bind(user.id)
        .bind(name)
        .bind(kind)
        .bind(&address)
        .bind(account)
        .bind(token)
        .bind(sqlx::types::Json(tools))
        .bind(sqlx::types::Json(settings))
        .bind(request.token_expires_at)
        .bind(sqlx::types::Json(request.policy.clone().unwrap_or_default()))
        .fetch_one(&state.pool)
        .await?;

    if let Some(groups) = &request.groups {
        replace_groups(&state, user.id, row.id, groups).await?;
    }

    state.hub.tools_changed(user.id);

    Ok((StatusCode::CREATED, Json(reloaded(&state, user.id, row.id).await?)))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchChatToolServerRequest>) -> ApiResult<Json<ChatToolServer>> {
    let name = request.name.as_deref().map(str::trim);

    if let Some(name) = name {
        check_name(&state, user.id, name, Some(id)).await?;
    }

    check_expiry(request.token_expires_at)?;

    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!(
        "update chat_tool_servers set enabled = coalesce($3, enabled), always_allow = coalesce($4, always_allow), name = coalesce($5, name),
           token_expires_at = case when $6 then null else coalesce($7, token_expires_at) end, policy = coalesce($8, policy), bearer_token = coalesce($9, bearer_token) where id = $1 and owner = $2 returning {SERVER_COLUMNS}"
    )))
    .bind(id)
    .bind(user.id)
    .bind(request.enabled)
    .bind(request.always_allow)
    .bind(name)
    .bind(request.clear_token_expiry)
    .bind(request.token_expires_at)
    .bind(request.policy.map(sqlx::types::Json))
    .bind(request.bearer_token.as_deref().map(str::trim).filter(|token| !token.is_empty()))
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That tool server"))?;
    state.hub.tools_changed(user.id);

    Ok(Json(reloaded(&state, user.id, row.id).await?))
}

pub async fn set_groups(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SetIntegrationGroupsRequest>) -> ApiResult<Json<ChatToolServer>> {
    reloaded(&state, user.id, id).await?;
    replace_groups(&state, user.id, id, &request.groups).await?;
    state.hub.tools_changed(user.id);

    Ok(Json(reloaded(&state, user.id, id).await?))
}

async fn reloaded(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<ChatToolServer> {
    let rows = list_owned(state, owner).await?;

    rows.iter().find(|row| row.id == id).map(ServerRow::to_proto).ok_or_else(|| ApiError::not_found("That tool server"))
}

#[derive(serde::Deserialize)]
pub struct CallsQuery {
    server: Option<Uuid>,
    provider: Option<String>,
    decision: Option<String>,
    review: Option<String>,
    surface: Option<String>,
    before: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(sqlx::FromRow)]
struct CallRow {
    id: Uuid,
    server_id: Option<Uuid>,
    server_name: String,
    tool: String,
    kind: String,
    conversation_id: Option<Uuid>,
    surface: String,
    provider: Option<String>,
    decision: String,
    review: Option<String>,
    is_error: bool,
    at: chrono::DateTime<chrono::Utc>,
}

const CALLS_PAGE: i64 = 100;

pub async fn calls(State(state): State<AppState>, user: AuthUser, Query(query): Query<CallsQuery>) -> ApiResult<Json<Vec<IntegrationCall>>> {
    let rows = sqlx::query_as::<_, CallRow>(
        "select id, server_id, server_name, tool, kind, conversation_id, surface, provider, decision, review, is_error, at from integration_calls
         where owner = $1 and ($2::uuid is null or server_id = $2) and ($3::text is null or provider = $3) and ($4::text is null or decision = $4)
           and ($5::text is null or review = $5) and ($6::text is null or surface = $6) and ($7::timestamptz is null or at < $7)
         order by at desc, id limit $8",
    )
    .bind(user.id)
    .bind(query.server)
    .bind(query.provider)
    .bind(query.decision)
    .bind(query.review)
    .bind(query.surface)
    .bind(query.before)
    .bind(CALLS_PAGE)
    .fetch_all(&state.pool)
    .await?;
    let calls = rows
        .into_iter()
        .map(|row| IntegrationCall {
            id: row.id,
            server_id: row.server_id,
            server_name: row.server_name,
            tool: row.tool,
            kind: if row.kind == "read" { ToolKind::Read } else { ToolKind::Write },
            conversation_id: row.conversation_id,
            surface: row.surface,
            provider: row.provider,
            decision: row.decision,
            review: row.review,
            is_error: row.is_error,
            at: row.at,
        })
        .collect();

    Ok(Json(calls))
}

pub async fn refresh(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<ChatToolServer>> {
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2"))).bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That tool server"))?;
    if row.integration().is_some() {
        return Ok(Json(row.to_proto()));
    }

    let tools = fetch_tools(&state, &row.name, &row.url, bearer(&state, &row).await?.as_deref(), row.key_header(), row.known_app()).await?;
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
