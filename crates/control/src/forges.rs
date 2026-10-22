use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::future::join_all;
use mycelium_proto::api::{ConnectForgeRequest, Forge, ForgeFailure, ForgeKind, ForgeOrganisation, Repositories, Repository};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

const FORGE_TIMEOUT: Duration = Duration::from_secs(15);
const PAGE_SIZE: usize = 100;
const MOST_PAGES: usize = 5;
const FORGE_COLUMNS: &str = "id, kind, base_url, account, token, organisations, created_at";

#[derive(Deserialize)]
pub struct RepositoryQuery {
    forge: Option<Uuid>,
    organisation: Option<String>,
    #[serde(default)]
    q: String,
}

#[derive(sqlx::FromRow)]
pub struct ForgeRow {
    pub id: Uuid,
    pub kind: String,
    pub base_url: String,
    pub account: String,
    pub token: String,
    pub organisations: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub enum ForgeError {
    Rejected,
    Unreachable,
    Failed(u16),
}

impl ForgeError {
    pub fn into_api(self, kind: ForgeKind, base_url: &str) -> ApiError {
        let name = display_name(kind);

        match self {
            ForgeError::Rejected => ApiError::bad_request("forge_token_rejected", format!("{name} did not accept this token. Check that it has not expired and can read your account, then try again.")),
            ForgeError::Unreachable => ApiError::bad_request("forge_unreachable", format!("Mycelium could not reach {base_url}. Check the address and try again.")),
            ForgeError::Failed(status) => ApiError::bad_request("forge_failed", format!("{name} answered with status {status}. Try again in a moment.")),
        }
    }
}

impl ForgeRow {
    pub fn to_proto(&self) -> ApiResult<Forge> {
        Ok(Forge {
            id: self.id,
            kind: parse_kind(&self.kind)?,
            base_url: self.base_url.clone(),
            account: self.account.clone(),
            token_hint: token_hint(&self.token),
            organisations: self.organisations.clone(),
            created_at: self.created_at,
        })
    }
}

pub struct ForgeApi {
    pub id: Uuid,
    pub kind: ForgeKind,
    pub base_url: String,
    token: String,
}

impl ForgeApi {
    pub fn new(id: Uuid, kind: ForgeKind, base_url: String, token: String) -> ForgeApi {
        ForgeApi { id, kind, base_url, token }
    }

    pub fn from_row(row: &ForgeRow) -> ApiResult<ForgeApi> {
        Ok(ForgeApi::new(row.id, parse_kind(&row.kind)?, row.base_url.clone(), row.token.clone()))
    }

    fn api_url(&self) -> String {
        match self.kind {
            ForgeKind::Github if self.base_url == "https://github.com" => "https://api.github.com".to_string(),
            ForgeKind::Github => format!("{}/api/v3", self.base_url),
            ForgeKind::Gitlab => format!("{}/api/v4", self.base_url),
        }
    }

    pub async fn get(&self, http: &reqwest::Client, path: &str) -> Result<Value, ForgeError> {
        let request = http.get(format!("{}{path}", self.api_url())).timeout(FORGE_TIMEOUT).header("user-agent", "mycelium");
        let request = match self.kind {
            ForgeKind::Github => request.bearer_auth(&self.token).header("accept", "application/vnd.github+json").header("x-github-api-version", "2022-11-28"),
            ForgeKind::Gitlab => request.header("private-token", &self.token),
        };
        let response = request.send().await.map_err(|_| ForgeError::Unreachable)?;
        let status = response.status();

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ForgeError::Rejected);
        }

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        response.json().await.map_err(|_| ForgeError::Failed(status.as_u16()))
    }

    pub async fn pages(&self, http: &reqwest::Client, path: &str) -> Result<Vec<Value>, ForgeError> {
        let separator = if path.contains('?') { '&' } else { '?' };
        let mut found = Vec::new();

        for page in 1..=MOST_PAGES {
            let body = self.get(http, &format!("{path}{separator}per_page={PAGE_SIZE}&page={page}")).await?;
            let items = body.as_array().cloned().unwrap_or_default();
            let last = items.len() < PAGE_SIZE;
            found.extend(items);

            if last {
                break;
            }
        }

        Ok(found)
    }

    pub async fn account(&self, http: &reqwest::Client) -> Result<String, ForgeError> {
        let user = self.get(http, "/user").await?;
        let field = if self.kind == ForgeKind::Github { "login" } else { "username" };

        user[field].as_str().map(str::to_string).ok_or(ForgeError::Failed(200))
    }

    pub async fn organisations(&self, http: &reqwest::Client, account: &str) -> Result<Vec<String>, ForgeError> {
        let (path, field) = match self.kind {
            ForgeKind::Github => ("/user/orgs", "login"),
            ForgeKind::Gitlab => ("/groups?min_access_level=10&top_level_only=true", "full_path"),
        };
        let mut names = vec![account.to_string()];

        for group in self.pages(http, path).await? {
            if let Some(name) = group[field].as_str().filter(|name| !names.iter().any(|known| known == name)) {
                names.push(name.to_string());
            }
        }

        Ok(names)
    }

    pub async fn repositories(&self, http: &reqwest::Client) -> Result<Vec<Repository>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Github => "/user/repos?sort=updated",
            ForgeKind::Gitlab => "/projects?membership=true&simple=true&order_by=last_activity_at",
        };
        let name_field = if self.kind == ForgeKind::Github { "full_name" } else { "path_with_namespace" };
        let mut repositories = Vec::new();

        for item in self.pages(http, path).await? {
            let Some(full_name) = item[name_field].as_str() else {
                continue;
            };
            let (owner, name) = full_name.rsplit_once('/').unwrap_or(("", full_name));

            repositories.push(Repository {
                forge_id: self.id,
                owner: owner.to_string(),
                name: name.to_string(),
                full_name: full_name.to_string(),
                description: item["description"].as_str().map(str::to_string),
                private: item["private"].as_bool().unwrap_or_else(|| item["visibility"].as_str().is_some_and(|visibility| visibility != "public")),
                default_branch: item["default_branch"].as_str().map(str::to_string),
            });
        }

        Ok(repositories)
    }
}

pub fn display_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Github => "GitHub",
        ForgeKind::Gitlab => "GitLab",
    }
}

pub fn parse_kind(text: &str) -> ApiResult<ForgeKind> {
    match text {
        "github" => Ok(ForgeKind::Github),
        "gitlab" => Ok(ForgeKind::Gitlab),
        other => Err(ApiError::internal(format!("unknown forge kind {other}"))),
    }
}

fn token_hint(token: &str) -> String {
    let characters: Vec<char> = token.chars().collect();

    if characters.len() < 12 {
        return "••••".to_string();
    }

    format!("••••{}", characters[characters.len() - 4..].iter().collect::<String>())
}

fn normalised_base_url(kind: ForgeKind, entered: Option<&str>) -> ApiResult<String> {
    let default = match kind {
        ForgeKind::Github => "https://github.com",
        ForgeKind::Gitlab => "https://gitlab.com",
    };
    let entered = entered.map(str::trim).filter(|entered| !entered.is_empty()).unwrap_or(default);
    let parsed = url::Url::parse(entered).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("base_url_invalid", "Enter the forge's address, starting with https://, for example https://git.example.com."));
    };

    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

pub async fn list_owned(state: &AppState, owner: Uuid) -> ApiResult<Vec<ForgeRow>> {
    let rows = sqlx::query_as::<_, ForgeRow>(sqlx::AssertSqlSafe(format!("select {FORGE_COLUMNS} from forges where owner = $1 order by created_at, id"))).bind(owner).fetch_all(&state.pool).await?;

    Ok(rows)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Forge>>> {
    let mut forges = Vec::new();

    for row in list_owned(&state, user.id).await? {
        forges.push(row.to_proto()?);
    }

    Ok(Json(forges))
}

pub async fn connect(State(state): State<AppState>, user: AuthUser, Json(request): Json<ConnectForgeRequest>) -> ApiResult<(StatusCode, Json<Forge>)> {
    let token = request.token.trim().to_string();

    if token.is_empty() {
        return Err(ApiError::bad_request("token_required", "Enter a personal access token."));
    }

    let base_url = normalised_base_url(request.kind, request.base_url.as_deref())?;
    let api = ForgeApi::new(Uuid::nil(), request.kind, base_url.clone(), token.clone());
    let account = api.account(&state.http).await.map_err(|error| error.into_api(request.kind, &base_url))?;
    let organisations = api.organisations(&state.http, &account).await.unwrap_or_else(|_| vec![account.clone()]);

    let row = sqlx::query_as::<_, ForgeRow>(sqlx::AssertSqlSafe(format!(
        "insert into forges (owner, kind, base_url, account, token, organisations) values ($1, $2, $3, $4, $5, $6)
         on conflict (owner, base_url, account) do update set token = excluded.token, kind = excluded.kind, organisations = excluded.organisations
         returning {FORGE_COLUMNS}",
    )))
    .bind(user.id)
    .bind(crate::store::enum_text(&request.kind))
    .bind(&base_url)
    .bind(&account)
    .bind(&token)
    .bind(&organisations)
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.to_proto()?)))
}

pub async fn disconnect(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from forges where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That forge"));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn repositories(State(state): State<AppState>, user: AuthUser, Query(query): Query<RepositoryQuery>) -> ApiResult<Json<Repositories>> {
    let mut rows = list_owned(&state, user.id).await?;

    if let Some(forge) = query.forge {
        rows.retain(|row| row.id == forge);
    }

    let fetched = join_all(rows.iter().map(|row| async {
        let api = ForgeApi::from_row(row)?;
        let repositories = api.repositories(&state.http).await;
        let organisations = api.organisations(&state.http, &row.account).await.unwrap_or_else(|_| row.organisations.clone());

        Ok::<_, ApiError>((api.kind, api.base_url.clone(), row.id, repositories, organisations))
    }))
    .await;

    let needle = query.q.trim().to_lowercase();
    let mut result = Repositories { repositories: Vec::new(), organisations: Vec::new(), failures: Vec::new() };

    for outcome in fetched {
        let (kind, base_url, forge_id, repositories, organisations) = outcome?;

        result.organisations.extend(organisations.into_iter().map(|name| ForgeOrganisation { forge_id, name }));

        match repositories {
            Ok(found) => result.repositories.extend(found.into_iter().filter(|repository| {
                let in_organisation = query.organisation.as_deref().is_none_or(|organisation| repository.owner == organisation || repository.owner.starts_with(&format!("{organisation}/")));

                in_organisation && repository.full_name.to_lowercase().contains(&needle)
            })),
            Err(error) => {
                let message = error.into_api(kind, &base_url).message().to_string();

                result.failures.push(ForgeFailure { forge_id, message });
            }
        }
    }

    Ok(Json(result))
}
