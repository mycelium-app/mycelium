use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::token_hint;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateModelProviderRequest, ModelProvider, ModelProviderKind};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::Duration;
use uuid::Uuid;

const LISTING_TIMEOUT: Duration = Duration::from_secs(15);
const REPLY_TIMEOUT: Duration = Duration::from_secs(600);
const ANTHROPIC_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_MAX_TOKENS: u32 = 8192;
const ANTHROPIC_PREFERRED: [&str; 3] = ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"];
const PROVIDER_COLUMNS: &str = "id, kind, name, base_url, api_key, models, default_model, created_at";

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
}

pub enum ProviderError {
    Rejected,
    Unreachable,
    Failed(u16),
    Stopped(String),
}

pub struct ChatImage {
    pub media_type: String,
    pub data: String,
}

pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
    pub images: Vec<ChatImage>,
}

impl ChatMessage {
    fn to_json(&self, kind: ModelProviderKind) -> Value {
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

#[derive(Default)]
pub struct Delta {
    pub text: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

pub struct ReplyStream {
    response: reqwest::Response,
    kind: ModelProviderKind,
    buffer: Vec<u8>,
    pending: VecDeque<Delta>,
    finished: bool,
}

impl ProviderError {
    pub fn into_api(self, name: &str, address: &str) -> ApiError {
        match self {
            ProviderError::Rejected => ApiError::bad_request("provider_key_rejected", format!("{name} did not accept this API key. Check that it has not expired and is for this service, then try again.")),
            ProviderError::Unreachable => ApiError::bad_request("provider_unreachable", format!("Mycelium could not reach {address}. Check the address and that the server is running, then try again.")),
            ProviderError::Failed(status) => ApiError::bad_request("provider_failed", format!("{name} answered with status {status}. Try again in a moment.")),
            ProviderError::Stopped(message) => ApiError::bad_request("provider_failed", message),
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

    pub async fn stream_reply(&self, http: &reqwest::Client, model: &str, system: &str, messages: &[ChatMessage]) -> ApiResult<Result<ReplyStream, ProviderError>> {
        let kind = self.parsed_kind()?;
        let turns: Vec<Value> = messages.iter().map(|message| message.to_json(kind)).collect();
        let body = match kind {
            ModelProviderKind::OpenaiCompatible => {
                let mut all = vec![json!({"role": "system", "content": system})];
                all.extend(turns);

                json!({"model": model, "messages": all, "stream": true, "stream_options": {"include_usage": true}})
            }
            ModelProviderKind::Anthropic => json!({"model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "system": system, "messages": turns, "stream": true}),
        };
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
            return Ok(Err(ProviderError::Failed(status.as_u16())));
        }

        Ok(Ok(ReplyStream { response, kind, buffer: Vec::new(), pending: VecDeque::new(), finished: false }))
    }
}

impl ReplyStream {
    pub async fn next(&mut self) -> Result<Option<Delta>, ProviderError> {
        loop {
            if let Some(delta) = self.pending.pop_front() {
                return Ok(Some(delta));
            }

            if self.finished {
                return Ok(None);
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
            ModelProviderKind::OpenaiCompatible => Delta {
                text: event["choices"][0]["delta"]["content"].as_str().unwrap_or_default().to_string(),
                input_tokens: event["usage"]["prompt_tokens"].as_i64(),
                output_tokens: event["usage"]["completion_tokens"].as_i64(),
            },
            ModelProviderKind::Anthropic => match event["type"].as_str() {
                Some("content_block_delta") => Delta { text: event["delta"]["text"].as_str().unwrap_or_default().to_string(), ..Delta::default() },
                Some("message_start") => Delta { input_tokens: event["message"]["usage"]["input_tokens"].as_i64(), ..Delta::default() },
                Some("message_delta") => Delta { output_tokens: event["usage"]["output_tokens"].as_i64(), ..Delta::default() },
                Some("error") => return Err(ProviderError::Stopped(format!("The model stopped with an error: {}", event["error"]["message"].as_str().unwrap_or("no reason given")))),
                _ => Delta::default(),
            },
        };

        if !delta.text.is_empty() || delta.input_tokens.is_some() || delta.output_tokens.is_some() {
            self.pending.push_back(delta);
        }

        Ok(())
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
    let draft = ProviderRow { id: Uuid::nil(), kind: enum_text(&request.kind), name: name.to_string(), base_url: base_url.clone(), api_key: key.map(str::to_string), models: Vec::new(), default_model: None, created_at: chrono::Utc::now() };
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

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from model_providers where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That model provider"));
    }

    Ok(StatusCode::NO_CONTENT)
}
