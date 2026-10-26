use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::token_hint;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateModelProviderRequest, ModelProvider, ModelProviderKind, SetFavouriteModelRequest, UpdateModelProviderRequest};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use uuid::Uuid;

const LISTING_TIMEOUT: Duration = Duration::from_secs(15);
const REPLY_TIMEOUT: Duration = Duration::from_secs(600);
const ANTHROPIC_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_MAX_TOKENS: u32 = 8192;
const ANTHROPIC_PREFERRED: [&str; 3] = ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"];
const PROVIDER_COLUMNS: &str = "id, kind, name, base_url, api_key, models, default_model, created_at, favourite_models, model_class";

#[derive(sqlx::FromRow)]
pub struct ProviderRow {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub favourite_models: Vec<String>,
    pub model_class: String,
}

pub enum ProviderError {
    Rejected,
    Unreachable,
    Failed(u16),
    Stopped(String),
    ToolsRejected,
}

pub struct ChatImage {
    pub media_type: String,
    pub data: String,
}

#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Default)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
    pub images: Vec<ChatImage>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
}

impl ChatMessage {
    fn to_json(&self, kind: ModelProviderKind) -> Vec<Value> {
        match (kind, self.tool_calls.is_empty(), self.tool_results.is_empty()) {
            (ModelProviderKind::OpenaiCompatible, false, _) => {
                let calls: Vec<Value> = self.tool_calls.iter().map(|call| json!({"id": call.id, "type": "function", "function": {"name": call.name, "arguments": call.input.to_string()}})).collect();
                let content = if self.content.is_empty() { Value::Null } else { json!(self.content) };

                vec![json!({"role": "assistant", "content": content, "tool_calls": calls})]
            }
            (ModelProviderKind::Anthropic, false, _) => {
                let mut blocks = Vec::new();

                if !self.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": self.content}));
                }

                blocks.extend(self.tool_calls.iter().map(|call| json!({"type": "tool_use", "id": call.id, "name": call.name, "input": call.input})));

                vec![json!({"role": "assistant", "content": blocks})]
            }
            (ModelProviderKind::OpenaiCompatible, _, false) => self.tool_results.iter().map(|result| json!({"role": "tool", "tool_call_id": result.call_id, "content": result.content})).collect(),
            (ModelProviderKind::Anthropic, _, false) => {
                let blocks: Vec<Value> = self.tool_results.iter().map(|result| json!({"type": "tool_result", "tool_use_id": result.call_id, "content": result.content, "is_error": result.is_error})).collect();

                vec![json!({"role": "user", "content": blocks})]
            }
            _ => vec![self.to_plain_json(kind)],
        }
    }

    fn to_plain_json(&self, kind: ModelProviderKind) -> Value {
        if self.images.is_empty() {
            return json!({"role": self.role, "content": self.content});
        }

        let mut parts: Vec<Value> = self
            .images
            .iter()
            .map(|image| match kind {
                ModelProviderKind::OpenaiCompatible => json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", image.media_type, image.data)}}),
                ModelProviderKind::Anthropic => json!({"type": "image", "source": {"type": "base64", "media_type": image.media_type, "data": image.data}}),
            })
            .collect();

        if !self.content.is_empty() {
            parts.push(json!({"type": "text", "text": self.content}));
        }

        json!({"role": self.role, "content": parts})
    }
}

fn cache_control() -> Value {
    json!({"type": "ephemeral"})
}

fn mark_cache_breakpoint(content: &mut Value) {
    if let Value::String(text) = content {
        *content = json!([{"type": "text", "text": text}]);
    }

    if let Some(last) = content.as_array_mut().and_then(|parts| parts.last_mut()) {
        last["cache_control"] = cache_control();
    }
}

fn tool_definitions_json(kind: ModelProviderKind, tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| match kind {
            ModelProviderKind::OpenaiCompatible => json!({"type": "function", "function": {"name": tool.name, "description": tool.description, "parameters": tool.schema}}),
            ModelProviderKind::Anthropic => json!({"name": tool.name, "description": tool.description, "input_schema": tool.schema}),
        })
        .collect()
}

struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub struct Delta {
    pub text: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Clone, Default)]
pub struct ReportedUsage {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
}

impl ReportedUsage {
    pub fn absorb(&mut self, delta: &Delta, previous: &ReportedUsage) {
        self.input = delta.input_tokens.map(|tokens| previous.input.unwrap_or(0) + tokens).or(self.input);
        self.output = delta.output_tokens.map(|tokens| previous.output.unwrap_or(0) + tokens).or(self.output);
        self.cache_read = delta.cache_read_tokens.map(|tokens| previous.cache_read.unwrap_or(0) + tokens).or(self.cache_read);
        self.cache_write = delta.cache_write_tokens.map(|tokens| previous.cache_write.unwrap_or(0) + tokens).or(self.cache_write);
    }

    pub fn is_reported(&self) -> bool {
        self.input.is_some() || self.output.is_some() || self.cache_read.is_some() || self.cache_write.is_some()
    }
}

impl Delta {
    fn without_cached_from_input(mut self, cache_read: Option<i64>, cache_write: Option<i64>) -> Delta {
        let cached = cache_read.unwrap_or(0) + cache_write.unwrap_or(0);

        self.input_tokens = self.input_tokens.map(|input| (input - cached).max(0));
        self.cache_read_tokens = cache_read;
        self.cache_write_tokens = cache_write;

        self
    }
}

pub struct ReplyStream {
    response: reqwest::Response,
    kind: ModelProviderKind,
    buffer: Vec<u8>,
    pending: VecDeque<Delta>,
    calls: BTreeMap<usize, PartialCall>,
    finished: bool,
}

impl ProviderError {
    pub fn into_api(self, name: &str, address: &str) -> ApiError {
        match self {
            ProviderError::Rejected => ApiError::bad_request("provider_key_rejected", format!("{name} did not accept this API key. Check that it has not expired and is for this service, then try again.")),
            ProviderError::Unreachable => ApiError::bad_request("provider_unreachable", format!("Mycelium could not reach {address}. Check the address and that the server is running, then try again.")),
            ProviderError::Failed(status) => ApiError::bad_request("provider_failed", format!("{name} answered with status {status}. Try again in a moment.")),
            ProviderError::Stopped(message) => ApiError::bad_request("provider_failed", message),
            ProviderError::ToolsRejected => ApiError::bad_request("provider_failed", format!("{name} does not take tools. Try again.")),
        }
    }
}

impl ProviderRow {
    pub fn parsed_kind(&self) -> ApiResult<ModelProviderKind> {
        parse_enum(&self.kind)
    }

    pub fn address(&self) -> ApiResult<String> {
        let base = self.base_url.clone().unwrap_or_else(|| ANTHROPIC_URL.to_string());

        Ok(match self.parsed_kind()? {
            ModelProviderKind::OpenaiCompatible => base,
            ModelProviderKind::Anthropic => format!("{base}/v1"),
        })
    }

    pub fn to_proto(&self) -> ApiResult<ModelProvider> {
        Ok(ModelProvider {
            id: self.id,
            kind: self.parsed_kind()?,
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            key_hint: self.api_key.as_deref().map(token_hint),
            models: self.models.clone(),
            default_model: self.default_model.clone(),
            created_at: self.created_at,
            favourites: self.favourite_models.clone(),
            model_class: parse_enum(&self.model_class)?,
        })
    }

    fn authorised(&self, request: reqwest::RequestBuilder) -> ApiResult<reqwest::RequestBuilder> {
        let request = request.header("user-agent", "mycelium");

        Ok(match (self.parsed_kind()?, &self.api_key) {
            (ModelProviderKind::OpenaiCompatible, Some(key)) => request.bearer_auth(key),
            (ModelProviderKind::OpenaiCompatible, None) => request,
            (ModelProviderKind::Anthropic, key) => request.header("anthropic-version", ANTHROPIC_VERSION).header("x-api-key", key.clone().unwrap_or_default()),
        })
    }

    pub async fn list_models(&self, http: &reqwest::Client) -> ApiResult<Result<Vec<String>, ProviderError>> {
        let request = self.authorised(http.get(format!("{}/models?limit=1000", self.address()?)).timeout(LISTING_TIMEOUT))?;
        let Ok(response) = request.send().await else {
            return Ok(Err(ProviderError::Unreachable));
        };
        let status = response.status();

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(Err(ProviderError::Rejected));
        }

        if !status.is_success() {
            return Ok(Err(ProviderError::Failed(status.as_u16())));
        }

        let Ok(body) = response.json::<Value>().await else {
            return Ok(Err(ProviderError::Failed(status.as_u16())));
        };
        let mut models: Vec<String> = body["data"].as_array().map(|items| items.iter().filter_map(|item| item["id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
        models.sort();

        Ok(Ok(models))
    }

    fn caches_through_content_parts(&self, model: &str) -> bool {
        let host = self.base_url.as_deref().and_then(|base| reqwest::Url::parse(base).ok()).and_then(|url| url.host_str().map(str::to_string));

        host.as_deref() == Some("openrouter.ai") && (model.starts_with("anthropic/") || model.starts_with("google/"))
    }

    fn request_body(&self, kind: ModelProviderKind, model: &str, system: &str, messages: &[ChatMessage], tools: &[ToolDefinition]) -> Value {
        let turns: Vec<Value> = messages.iter().flat_map(|message| message.to_json(kind)).collect();
        let mut body = match kind {
            ModelProviderKind::OpenaiCompatible => {
                let mut all = vec![json!({"role": "system", "content": system})];
                all.extend(turns);

                if self.caches_through_content_parts(model) {
                    let last_user = all.iter().rposition(|message| message["role"] == "user");

                    for position in [Some(0), last_user].into_iter().flatten() {
                        mark_cache_breakpoint(&mut all[position]["content"]);
                    }
                }

                json!({"model": model, "messages": all, "stream": true, "stream_options": {"include_usage": true}})
            }
            ModelProviderKind::Anthropic => {
                let mut turns = turns;
                let mut system = json!(system);

                if let Some(last) = turns.last_mut() {
                    mark_cache_breakpoint(&mut last["content"]);
                }

                if tools.is_empty() {
                    mark_cache_breakpoint(&mut system);
                }

                json!({"model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "system": system, "messages": turns, "stream": true})
            }
        };

        if !tools.is_empty() {
            let mut definitions = tool_definitions_json(kind, tools);

            if kind == ModelProviderKind::Anthropic
                && let Some(last) = definitions.last_mut()
            {
                last["cache_control"] = cache_control();
            }

            body["tools"] = json!(definitions);
        }

        body
    }

    pub async fn stream_reply(&self, http: &reqwest::Client, model: &str, system: &str, messages: &[ChatMessage], tools: &[ToolDefinition]) -> ApiResult<Result<ReplyStream, ProviderError>> {
        let kind = self.parsed_kind()?;
        let body = self.request_body(kind, model, system, messages, tools);

        let path = match kind {
            ModelProviderKind::OpenaiCompatible => "chat/completions",
            ModelProviderKind::Anthropic => "messages",
        };
        let request = self.authorised(http.post(format!("{}/{path}", self.address()?)).timeout(REPLY_TIMEOUT).json(&body))?;
        let Ok(response) = request.send().await else {
            return Ok(Err(ProviderError::Unreachable));
        };
        let status = response.status();

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(Err(ProviderError::Rejected));
        }

        if !status.is_success() {
            let mentions_tools = !tools.is_empty() && response.text().await.is_ok_and(|text| text.to_lowercase().contains("tool"));

            return Ok(Err(if mentions_tools { ProviderError::ToolsRejected } else { ProviderError::Failed(status.as_u16()) }));
        }

        Ok(Ok(ReplyStream { response, kind, buffer: Vec::new(), pending: VecDeque::new(), calls: BTreeMap::new(), finished: false }))
    }
}

impl ReplyStream {
    pub async fn next(&mut self) -> Result<Option<Delta>, ProviderError> {
        loop {
            if let Some(delta) = self.pending.pop_front() {
                return Ok(Some(delta));
            }

            if self.finished {
                return Ok(self.completed_calls().map(|tool_calls| Delta { tool_calls, ..Delta::default() }));
            }

            let chunk = self.response.chunk().await.map_err(|_| ProviderError::Stopped("The model stopped replying part way through. Send your message again.".to_string()))?;
            let Some(bytes) = chunk else {
                self.finished = true;

                continue;
            };

            self.buffer.extend_from_slice(&bytes);

            while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                self.read_line(&String::from_utf8_lossy(&line))?;
            }
        }
    }

    fn read_line(&mut self, line: &str) -> Result<(), ProviderError> {
        let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
            return Ok(());
        };

        if data == "[DONE]" {
            self.finished = true;

            return Ok(());
        }

        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        let delta = match self.kind {
            ModelProviderKind::OpenaiCompatible => {
                for (position, call) in event["choices"][0]["delta"]["tool_calls"].as_array().into_iter().flatten().enumerate() {
                    let index = call["index"].as_u64().map(|index| index as usize).unwrap_or(position);
                    let partial = self.calls.entry(index).or_insert_with(|| PartialCall { id: String::new(), name: String::new(), arguments: String::new() });

                    partial.id.push_str(call["id"].as_str().unwrap_or_default());
                    partial.name.push_str(call["function"]["name"].as_str().unwrap_or_default());
                    partial.arguments.push_str(call["function"]["arguments"].as_str().unwrap_or_default());
                }

                Delta {
                    text: event["choices"][0]["delta"]["content"].as_str().unwrap_or_default().to_string(),
                    input_tokens: event["usage"]["prompt_tokens"].as_i64(),
                    output_tokens: event["usage"]["completion_tokens"].as_i64(),
                    ..Delta::default()
                }
                .without_cached_from_input(event["usage"]["prompt_tokens_details"]["cached_tokens"].as_i64(), event["usage"]["prompt_tokens_details"]["cache_write_tokens"].as_i64())
            }
            ModelProviderKind::Anthropic => match event["type"].as_str() {
                Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                    let index = event["index"].as_u64().unwrap_or_default() as usize;

                    self.calls.insert(index, PartialCall { id: event["content_block"]["id"].as_str().unwrap_or_default().to_string(), name: event["content_block"]["name"].as_str().unwrap_or_default().to_string(), arguments: String::new() });

                    Delta::default()
                }
                Some("content_block_delta") if event["delta"]["type"] == "input_json_delta" => {
                    let index = event["index"].as_u64().unwrap_or_default() as usize;

                    if let Some(partial) = self.calls.get_mut(&index) {
                        partial.arguments.push_str(event["delta"]["partial_json"].as_str().unwrap_or_default());
                    }

                    Delta::default()
                }
                Some("content_block_delta") => Delta { text: event["delta"]["text"].as_str().unwrap_or_default().to_string(), ..Delta::default() },
                Some("message_start") => Delta {
                    input_tokens: event["message"]["usage"]["input_tokens"].as_i64(),
                    cache_read_tokens: event["message"]["usage"]["cache_read_input_tokens"].as_i64(),
                    cache_write_tokens: event["message"]["usage"]["cache_creation_input_tokens"].as_i64(),
                    ..Delta::default()
                },
                Some("message_delta") => Delta { output_tokens: event["usage"]["output_tokens"].as_i64(), ..Delta::default() },
                Some("error") => return Err(ProviderError::Stopped(format!("The model stopped with an error: {}", event["error"]["message"].as_str().unwrap_or("no reason given")))),
                _ => Delta::default(),
            },
        };

        if !delta.text.is_empty() || delta.input_tokens.is_some() || delta.output_tokens.is_some() || delta.cache_read_tokens.is_some() || delta.cache_write_tokens.is_some() {
            self.pending.push_back(delta);
        }

        Ok(())
    }

    fn completed_calls(&mut self) -> Option<Vec<ToolCall>> {
        let calls = std::mem::take(&mut self.calls);

        if calls.is_empty() {
            return None;
        }

        Some(
            calls
                .into_values()
                .map(|partial| ToolCall {
                    id: if partial.id.is_empty() { format!("call_{}", Uuid::new_v4().simple()) } else { partial.id },
                    name: partial.name,
                    input: serde_json::from_str(&partial.arguments).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
                })
                .collect(),
        )
    }
}

fn default_model(kind: ModelProviderKind, models: &[String]) -> Option<String> {
    match kind {
        ModelProviderKind::Anthropic => ANTHROPIC_PREFERRED.iter().find(|preferred| models.iter().any(|model| model == *preferred)).map(|model| model.to_string()).or_else(|| models.first().cloned()),
        ModelProviderKind::OpenaiCompatible => models.first().cloned(),
    }
}

pub async fn list_owned(state: &AppState, owner: Uuid) -> ApiResult<Vec<ProviderRow>> {
    let rows = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!("select {PROVIDER_COLUMNS} from model_providers where owner = $1 order by created_at, id"))).bind(owner).fetch_all(&state.pool).await?;

    Ok(rows)
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProviderRow> {
    let row = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!("select {PROVIDER_COLUMNS} from model_providers where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That model provider"))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ModelProvider>>> {
    let mut providers = Vec::new();

    for row in list_owned(&state, user.id).await? {
        providers.push(row.to_proto()?);
    }

    Ok(Json(providers))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateModelProviderRequest>) -> ApiResult<(StatusCode, Json<ModelProvider>)> {
    let name = request.name.trim();
    let key = request.api_key.as_deref().map(str::trim).filter(|key| !key.is_empty());
    let entered = request.base_url.as_deref().map(str::trim).filter(|entered| !entered.is_empty());

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for this provider."));
    }

    if request.kind == ModelProviderKind::Anthropic && key.is_none() {
        return Err(ApiError::bad_request("api_key_required", "Enter an Anthropic API key."));
    }

    if request.kind == ModelProviderKind::OpenaiCompatible && entered.is_none() {
        return Err(ApiError::bad_request("base_url_required", "Enter the server's address, for example http://localhost:11434/v1 for Ollama."));
    }

    let base_url = match entered {
        Some(entered) => {
            let parsed = url::Url::parse(entered).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
            let Some(parsed) = parsed else {
                return Err(ApiError::bad_request("base_url_invalid", "Enter the server's address, starting with http:// or https://, for example http://localhost:11434/v1."));
            };

            Some(parsed.as_str().trim_end_matches('/').to_string())
        }
        None => None,
    };
    let draft = ProviderRow { id: Uuid::nil(), kind: enum_text(&request.kind), name: name.to_string(), base_url: base_url.clone(), api_key: key.map(str::to_string), models: Vec::new(), default_model: None, created_at: chrono::Utc::now(), favourite_models: vec![], model_class: "cloud".into() };
    let mut models = draft.list_models(&state.http).await?.map_err(|error| error.into_api(name, &draft.address().unwrap_or_default()))?;

    if models.is_empty() && request.kind == ModelProviderKind::Anthropic {
        models = ANTHROPIC_PREFERRED.iter().map(|model| model.to_string()).collect();
    }

    let chosen = default_model(request.kind, &models);
    let row = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!(
        "insert into model_providers (owner, kind, name, base_url, api_key, models, default_model) values ($1, $2, $3, $4, $5, $6, $7) returning {PROVIDER_COLUMNS}"
    )))
    .bind(user.id)
    .bind(&draft.kind)
    .bind(name)
    .bind(&base_url)
    .bind(&draft.api_key)
    .bind(&models)
    .bind(&chosen)
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.to_proto()?)))
}

pub async fn refresh(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<ModelProvider>> {
    let row = find_owned(&state, id, user.id).await?;
    let kind = row.parsed_kind()?;
    let models = row.list_models(&state.http).await?.map_err(|error| error.into_api(&row.name, &row.address().unwrap_or_default()))?;
    let kept = row.default_model.clone().filter(|chosen| models.contains(chosen));
    let chosen = kept.or_else(|| default_model(kind, &models));
    let updated = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!("update model_providers set models = $2, default_model = $3 where id = $1 returning {PROVIDER_COLUMNS}")))
        .bind(id)
        .bind(&models)
        .bind(&chosen)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(updated.to_proto()?))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<UpdateModelProviderRequest>) -> ApiResult<Json<ModelProvider>> {
    let name = request.name.as_deref().map(str::trim);

    if name.is_some_and(str::is_empty) {
        return Err(ApiError::bad_request("name_required", "Enter a name for this provider."));
    }

    let row = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!("update model_providers set name = coalesce($3, name), model_class = coalesce($4, model_class) where id = $1 and owner = $2 returning {PROVIDER_COLUMNS}")))
        .bind(id)
        .bind(user.id)
        .bind(name)
        .bind(request.model_class.map(|class| enum_text(&class)))
        .fetch_optional(&state.pool)
        .await?;
    let Some(row) = row else {
        return Err(ApiError::not_found("That model provider"));
    };

    Ok(Json(row.to_proto()?))
}

pub async fn set_favourite(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<SetFavouriteModelRequest>) -> ApiResult<Json<ModelProvider>> {
    user.require_full_access()?;
    let row = find_owned(&state, id, user.id).await?;

    if !row.models.contains(&request.model) {
        return Err(ApiError::bad_request("model_unknown", "Choose a model this provider lists."));
    }

    let updated = sqlx::query_as::<_, ProviderRow>(sqlx::AssertSqlSafe(format!("update model_providers set favourite_models = case when $3 then array_append(array_remove(favourite_models, $4), $4) else array_remove(favourite_models, $4) end where id = $1 and owner = $2 returning {PROVIDER_COLUMNS}")))
        .bind(id)
        .bind(user.id)
        .bind(request.favourite)
        .bind(&request.model)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(updated.to_proto()?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from model_providers where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That model provider"));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(kind: ModelProviderKind, base_url: &str) -> ProviderRow {
        ProviderRow { id: Uuid::nil(), kind: enum_text(&kind), name: "p".into(), base_url: Some(base_url.into()), api_key: None, models: vec![], default_model: None, created_at: chrono::Utc::now(), favourite_models: vec![], model_class: "cloud".into() }
    }

    fn user(text: &str) -> ChatMessage {
        ChatMessage { role: "user", content: text.into(), ..ChatMessage::default() }
    }

    fn tool() -> ToolDefinition {
        ToolDefinition { name: "look".into(), description: "Look".into(), schema: json!({"type": "object"}) }
    }

    const MARK: &str = r#"{"type":"ephemeral"}"#;

    fn marked(value: &Value) -> bool {
        value["cache_control"].to_string() == MARK
    }

    #[test]
    fn anthropic_marks_the_last_tool_and_the_last_message_block() {
        let row = provider(ModelProviderKind::Anthropic, "https://api.anthropic.com");
        let body = row.request_body(ModelProviderKind::Anthropic, "claude-sonnet-5-5", "sys", &[user("one"), user("two")], &[tool(), tool()]);

        assert!(!marked(&body["tools"][0]));
        assert!(marked(&body["tools"][1]));
        assert_eq!(body["system"], json!("sys"));
        assert!(!marked(&body["messages"][0]["content"][0]) && body["messages"][0]["content"] == json!("one"));
        assert_eq!(body["messages"][1]["content"], json!([{"type": "text", "text": "two", "cache_control": {"type": "ephemeral"}}]));
    }

    #[test]
    fn anthropic_without_tools_marks_the_system_block() {
        let row = provider(ModelProviderKind::Anthropic, "https://api.anthropic.com");
        let body = row.request_body(ModelProviderKind::Anthropic, "claude-sonnet-5-5", "sys", &[user("one")], &[]);

        assert_eq!(body["system"], json!([{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}]));
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn anthropic_marks_the_last_block_of_a_tool_result_message() {
        let results = ChatMessage { role: "user", tool_results: vec![ToolResult { call_id: "a".into(), content: "x".into(), is_error: false }], ..ChatMessage::default() };
        let row = provider(ModelProviderKind::Anthropic, "https://api.anthropic.com");
        let body = row.request_body(ModelProviderKind::Anthropic, "claude-sonnet-5-5", "sys", &[user("one"), results], &[tool()]);

        assert!(marked(&body["messages"][1]["content"][0]));
    }

    #[test]
    fn openrouter_anthropic_and_google_models_mark_the_system_and_last_user_message() {
        let row = provider(ModelProviderKind::OpenaiCompatible, "https://openrouter.ai/api/v1");

        for model in ["anthropic/claude-sonnet-5-5", "google/gemini-2.5-pro"] {
            let body = row.request_body(ModelProviderKind::OpenaiCompatible, model, "sys", &[user("one"), user("two")], &[tool()]);

            assert_eq!(body["messages"][0]["content"], json!([{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}]));
            assert_eq!(body["messages"][1]["content"], json!("one"));
            assert_eq!(body["messages"][2]["content"], json!([{"type": "text", "text": "two", "cache_control": {"type": "ephemeral"}}]));
            assert!(!marked(&body["tools"][0]));
        }
    }

    #[test]
    fn openrouter_models_that_cache_automatically_are_left_unchanged() {
        let row = provider(ModelProviderKind::OpenaiCompatible, "https://openrouter.ai/api/v1");
        let body = row.request_body(ModelProviderKind::OpenaiCompatible, "openai/gpt-5", "sys", &[user("one")], &[]);

        assert_eq!(body, json!({"model": "openai/gpt-5", "messages": [{"role": "system", "content": "sys"}, {"role": "user", "content": "one"}], "stream": true, "stream_options": {"include_usage": true}}));
    }

    #[test]
    fn other_openai_compatible_providers_are_left_unchanged_even_for_anthropic_model_ids() {
        let row = provider(ModelProviderKind::OpenaiCompatible, "http://localhost:11434/v1");
        let body = row.request_body(ModelProviderKind::OpenaiCompatible, "anthropic/claude-sonnet-5-5", "sys", &[user("one")], &[tool()]);

        assert_eq!(body["messages"], json!([{"role": "system", "content": "sys"}, {"role": "user", "content": "one"}]));
        assert!(!marked(&body["tools"][0]));
    }
}
