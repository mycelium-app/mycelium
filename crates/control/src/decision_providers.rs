use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::token_hint;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateDecisionProviderRequest, DECISION_USES, DecisionProvider, DecisionProviderModels, PatchDecisionProviderRequest, TestDecisionProviderRequest};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PROVIDER_COLUMNS: &str = "id, name, base_url, api_key, model, uses, created_at";
const QUESTION_TYPES: [&str; 3] = ["noul", "choice", "score"];

#[derive(sqlx::FromRow, Clone)]
pub struct DecisionRow {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub uses: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DecisionRow {
    fn to_proto(&self) -> DecisionProvider {
        DecisionProvider { id: self.id, name: self.name.clone(), base_url: self.base_url.clone(), key_hint: self.api_key.as_deref().map(token_hint), model: self.model.clone(), uses: self.uses.clone(), created_at: self.created_at }
    }
}

fn normalise_address(entered: &str) -> ApiResult<String> {
    let parsed = url::Url::parse(entered.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("base_url_invalid", "Enter the server's address, starting with http:// or https://, for example http://localhost:8000."));
    };
    let address = parsed.as_str().trim_end_matches('/');

    Ok(address.strip_suffix("/v1").unwrap_or(address).to_string())
}

fn check_uses(uses: &[String]) -> ApiResult<Vec<String>> {
    let mut checked: Vec<String> = Vec::new();

    for requested in uses {
        if !DECISION_USES.contains(&requested.as_str()) {
            return Err(ApiError::bad_request("use_invalid", format!("{requested} is not something decisions can be used for. Choose from {}.", DECISION_USES.join(", "))));
        }

        if !checked.contains(requested) {
            checked.push(requested.clone());
        }
    }

    Ok(checked)
}

async fn take_uses(state: &AppState, owner: Uuid, from_others_than: Uuid, uses: &[String]) -> ApiResult<()> {
    sqlx::query("update decision_providers set uses = array(select u from unnest(uses) u where u <> all($3)) where owner = $1 and id <> $2").bind(owner).bind(from_others_than).bind(uses).execute(&state.pool).await?;

    Ok(())
}

pub async fn list_models(state: &AppState, base_url: &str, key: Option<&str>) -> ApiResult<Vec<String>> {
    let mut request = state.http.get(format!("{base_url}/v1/models")).timeout(REQUEST_TIMEOUT);

    if let Some(key) = key {
        request = request.bearer_auth(key);
    }

    let response = request.send().await.map_err(|_| ApiError::bad_request("decision_provider_failed", "Mycelium could not reach that address. Check it and that the server is running."))?;
    let status = response.status();

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ApiError::bad_request("decision_provider_failed", "The server refused that key. Check it was copied whole and has not been revoked."));
    }

    if !status.is_success() {
        return Err(ApiError::bad_request("decision_provider_failed", format!("The server answered {} when asked for its models. Check the address.", status.as_u16())));
    }

    let body: Value = response.json().await.map_err(|_| ApiError::bad_request("decision_provider_failed", "The server did not answer with a list of models. Check the address is an OpenJev or Jev-compatible server."))?;
    let models = body["data"].as_array().into_iter().flatten().filter_map(|model| model["id"].as_str()).map(str::to_string).collect();

    Ok(models)
}

pub async fn list_owned(state: &AppState, owner: Uuid) -> ApiResult<Vec<DecisionRow>> {
    let rows = sqlx::query_as::<_, DecisionRow>(sqlx::AssertSqlSafe(format!("select {PROVIDER_COLUMNS} from decision_providers where owner = $1 order by created_at, id"))).bind(owner).fetch_all(&state.pool).await?;

    Ok(rows)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<DecisionProvider>>> {
    let rows = list_owned(&state, user.id).await?;

    Ok(Json(rows.iter().map(DecisionRow::to_proto).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateDecisionProviderRequest>) -> ApiResult<(StatusCode, Json<DecisionProvider>)> {
    let name = request.name.trim();
    let model = request.model.trim();
    let key = request.api_key.as_deref().map(str::trim).filter(|key| !key.is_empty());

    if name.is_empty() {
        return Err(ApiError::bad_request("name_required", "Enter a name for this provider."));
    }

    if model.is_empty() {
        return Err(ApiError::bad_request("model_required", "Choose a model. Test the connection to list the server's models."));
    }

    let base_url = normalise_address(&request.base_url)?;
    let uses = check_uses(&request.uses)?;
    let row = sqlx::query_as::<_, DecisionRow>(sqlx::AssertSqlSafe(format!("insert into decision_providers (owner, name, base_url, api_key, model, uses) values ($1, $2, $3, $4, $5, $6) returning {PROVIDER_COLUMNS}")))
        .bind(user.id)
        .bind(name)
        .bind(&base_url)
        .bind(key)
        .bind(model)
        .bind(&uses)
        .fetch_one(&state.pool)
        .await?;

    take_uses(&state, user.id, row.id, &uses).await?;

    Ok((StatusCode::CREATED, Json(row.to_proto())))
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchDecisionProviderRequest>) -> ApiResult<Json<DecisionProvider>> {
    let name = request.name.as_deref().map(str::trim);
    let model = request.model.as_deref().map(str::trim);
    let key = request.api_key.as_deref().map(str::trim).filter(|key| !key.is_empty());
    let base_url = request.base_url.as_deref().map(normalise_address).transpose()?;
    let uses = request.uses.as_deref().map(check_uses).transpose()?;

    if name.is_some_and(str::is_empty) {
        return Err(ApiError::bad_request("name_required", "Enter a name for this provider."));
    }

    if model.is_some_and(str::is_empty) {
        return Err(ApiError::bad_request("model_required", "Choose a model. Test the connection to list the server's models."));
    }

    let row = sqlx::query_as::<_, DecisionRow>(sqlx::AssertSqlSafe(format!(
        "update decision_providers set name = coalesce($3, name), base_url = coalesce($4, base_url), api_key = coalesce($5, api_key), model = coalesce($6, model), uses = coalesce($7, uses) where id = $1 and owner = $2 returning {PROVIDER_COLUMNS}"
    )))
    .bind(id)
    .bind(user.id)
    .bind(name)
    .bind(&base_url)
    .bind(key)
    .bind(model)
    .bind(&uses)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That decisions provider"))?;

    if let Some(uses) = &uses {
        take_uses(&state, user.id, id, uses).await?;
    }

    Ok(Json(row.to_proto()))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from decision_providers where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That decisions provider"));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn test(State(state): State<AppState>, _user: AuthUser, Json(request): Json<TestDecisionProviderRequest>) -> ApiResult<Json<DecisionProviderModels>> {
    let base_url = normalise_address(&request.base_url)?;
    let key = request.key.as_deref().map(str::trim).filter(|key| !key.is_empty());
    let models = list_models(&state, &base_url, key).await?;

    Ok(Json(DecisionProviderModels { models }))
}

pub async fn provider_for(state: &AppState, owner: Uuid, usage: &str) -> ApiResult<Option<DecisionRow>> {
    let row = sqlx::query_as::<_, DecisionRow>(sqlx::AssertSqlSafe(format!("select {PROVIDER_COLUMNS} from decision_providers where owner = $1 and $2 = any(uses) limit 1"))).bind(owner).bind(usage).fetch_optional(&state.pool).await?;

    Ok(row)
}

fn check_questions(questions: &Value) -> Result<(), String> {
    let Some(questions) = questions.as_object().filter(|questions| !questions.is_empty()) else {
        return Err("Nothing was decided. Give questions as an object mapping a name to {type, instructions, criteria}.".to_string());
    };

    for (name, question) in questions {
        let kind = question["type"].as_str().unwrap_or_default();

        if !QUESTION_TYPES.contains(&kind) {
            return Err(format!("Nothing was decided. Question {name} has type {kind:?}. Use noul for yes or no, choice or score."));
        }

        if question["instructions"].as_str().is_none_or(|instructions| instructions.trim().is_empty()) {
            return Err(format!("Nothing was decided. Question {name} needs instructions."));
        }
    }

    Ok(())
}

fn describe_failure(status: StatusCode, retry_after: Option<&str>, body: &str) -> String {
    match status.as_u16() {
        422 => "The decisions server could not read the request: a question or the state is the wrong shape. Check each question has a type and instructions.".to_string(),
        400 => format!("The decisions server cannot ask that: {}", body.trim()),
        401 | 403 => "The decisions server refused Mycelium's key. Ask the person to check the key in Settings, Providers.".to_string(),
        429 => "The decisions server is rate limiting requests. Wait a little and ask again.".to_string(),
        503 => match retry_after {
            Some(seconds) => format!("The decisions server is busy. Ask again in {seconds} seconds."),
            None => "The decisions server is busy. Ask again shortly.".to_string(),
        },
        529 => "The decisions server is overloaded. Ask again shortly.".to_string(),
        code => format!("The decisions server answered {code}. Ask the person to check it."),
    }
}

pub async fn decide(state: &AppState, owner: Uuid, usage: &str, subject: &Value, questions: &Value) -> ApiResult<Result<Value, String>> {
    if let Err(message) = check_questions(questions) {
        return Ok(Err(message));
    }

    let Some(provider) = provider_for(state, owner, usage).await? else {
        return Ok(Err("No decisions provider is set for this. Ask the person to add one in Settings, Providers.".to_string()));
    };
    let mut request = state.http.post(format!("{}/v1/systemone", provider.base_url)).timeout(REQUEST_TIMEOUT).json(&json!({"model": provider.model, "state": subject, "questions": questions}));

    if let Some(key) = &provider.api_key {
        request = request.bearer_auth(key);
    }

    let Ok(response) = request.send().await else {
        return Ok(Err("The decisions server could not be reached. Ask the person to check it is running.".to_string()));
    };
    let status = response.status();
    let retry_after = response.headers().get("retry-after").and_then(|value| value.to_str().ok()).map(str::to_string);
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Ok(Err(describe_failure(status, retry_after.as_deref(), &body)));
    }

    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let Some(answers) = parsed["answers"].as_object() else {
        return Ok(Err("The decisions server answered in a shape Mycelium does not know. Ask the person to check it is OpenJev or Jev-compatible.".to_string()));
    };

    Ok(Ok(Value::Object(answers.clone())))
}
