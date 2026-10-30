use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forge_cache::{Cached, ForgeCache};
use crate::state::AppState;
use axum::Json;
use base64::Engine;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use futures_util::future::join_all;
use mycelium_proto::api::{ConnectForgeRequest, Forge, ForgeFailure, ForgeKind, ForgeOrganisation, ForgePerson, Issue, PullRequest, PullRequestComment, PullRequestItem, PullRequestState, Repositories, Repository};
use mycelium_proto::link::GitCredentials;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const FORGE_TIMEOUT: Duration = Duration::from_secs(15);
const PAGE_SIZE: usize = 100;
const MOST_PAGES: usize = 5;
const FORGE_COLUMNS: &str = "id, kind, base_url, account, token, organisations, created_at, (select s.id from chat_tool_servers s where s.forge_id = forges.id limit 1) as mcp_server_id";

#[derive(Deserialize)]
pub struct RepositoryQuery {
    forge: Option<Uuid>,
    organisation: Option<String>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    pub refresh: bool,
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
    pub mcp_server_id: Option<Uuid>,
}

pub enum ForgeError {
    Rejected,
    MissingScope,
    Unreachable,
    Conflict,
    CreateRefused,
    Failed(u16),
}

pub struct TreeEntry {
    pub path: String,
    pub blob: String,
}

pub struct ForgeFile {
    pub text: String,
    pub commit: String,
    blob: String,
}

fn missing_scope_message(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Gitlab => "This token cannot read your account. Create a token with the api scope (read_api is enough to browse; api is needed to clone private repositories, save instructions and create repositories), then try again.",
        ForgeKind::Forgejo => "This token cannot read your account. Create an access token with read and write access to user, repository and issue, then try again.",
        ForgeKind::Github => "This token cannot read your account. Create a fine-grained token with the Contents, Issues and Pull requests permissions, or a classic token with the repo scope, then try again.",
    }
}

impl ForgeError {
    pub fn into_api(self, kind: ForgeKind, base_url: &str) -> ApiError {
        let name = display_name(kind);

        match self {
            ForgeError::Rejected => ApiError::bad_request("forge_token_rejected", format!("{name} did not accept this token. Check that it has not expired and can read your account, then try again.")),
            ForgeError::MissingScope => ApiError::bad_request("forge_token_scope", missing_scope_message(kind)),
            ForgeError::Unreachable => ApiError::bad_request("forge_unreachable", format!("Mycelium could not reach {base_url}. Check the address and try again.")),
            ForgeError::Conflict => ApiError::conflict("forge_file_changed", format!("The file changed on {name} since Mycelium last read it. Pull the latest version, then make your edit again.")),
            ForgeError::CreateRefused => ApiError::bad_request("forge_repository_refused", format!("{name} did not create that repository. It may already exist, or the name may not be allowed. Choose a different name and try again.")),
            ForgeError::Failed(status) => ApiError::bad_request("forge_failed", format!("{name} answered with status {status}. Try again in a moment.")),
        }
    }
}

impl ForgeRow {
    pub fn git_credentials(&self) -> ApiResult<GitCredentials> {
        let username = match parse_kind(&self.kind)? {
            ForgeKind::Github => "x-access-token",
            ForgeKind::Gitlab => "oauth2",
            ForgeKind::Forgejo => self.account.as_str(),
        };

        Ok(GitCredentials { username: username.to_string(), password: self.token.clone() })
    }

    pub async fn approved_clone_urls(&self, http: &reqwest::Client, repository: &str) -> ApiResult<Vec<String>> {
        let mut urls = ForgeApi::from_row(self)?.clone_urls(http, repository).await;
        urls.push(format!("{}/{repository}.git", self.base_url));

        Ok(urls)
    }

    pub fn to_proto(&self) -> ApiResult<Forge> {
        Ok(Forge {
            id: self.id,
            kind: parse_kind(&self.kind)?,
            base_url: self.base_url.clone(),
            account: self.account.clone(),
            token_hint: token_hint(&self.token),
            organisations: self.organisations.clone(),
            created_at: self.created_at,
            mcp_server_id: self.mcp_server_id,
            mcp_error: None,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchPullRequest {
    pub number: i64,
    pub label: String,
    pub title: String,
    pub url: String,
    pub state: &'static str,
}

#[derive(Clone)]
pub struct ForgeApi {
    pub id: Uuid,
    pub kind: ForgeKind,
    pub base_url: String,
    token: String,
    cache: Option<Arc<ForgeCache>>,
    refresh: bool,
}

impl ForgeApi {
    pub fn new(id: Uuid, kind: ForgeKind, base_url: String, token: String) -> ForgeApi {
        ForgeApi { id, kind, base_url, token, cache: None, refresh: false }
    }

    pub fn cached(mut self, cache: &Arc<ForgeCache>) -> ForgeApi {
        self.cache = Some(cache.clone());

        self
    }

    pub fn refreshing(mut self, refresh: bool) -> ForgeApi {
        self.refresh = refresh;

        self
    }

    fn invalidate(&self) {
        if let Some(cache) = &self.cache {
            cache.invalidate(self.id);
        }
    }

    pub fn from_row(row: &ForgeRow) -> ApiResult<ForgeApi> {
        Ok(ForgeApi::new(row.id, parse_kind(&row.kind)?, row.base_url.clone(), row.token.clone()))
    }

    fn api_url(&self) -> String {
        match self.kind {
            ForgeKind::Github if self.base_url == "https://github.com" => "https://api.github.com".to_string(),
            ForgeKind::Github => format!("{}/api/v3", self.base_url),
            ForgeKind::Forgejo => format!("{}/api/v1", self.base_url),
            ForgeKind::Gitlab => format!("{}/api/v4", self.base_url),
        }
    }

    fn authorised(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.timeout(FORGE_TIMEOUT).header("user-agent", "mycelium");

        match self.kind {
            ForgeKind::Github => request.bearer_auth(&self.token).header("accept", "application/vnd.github+json").header("x-github-api-version", "2022-11-28"),
            ForgeKind::Forgejo => request.header("authorization", format!("token {}", self.token)),
            ForgeKind::Gitlab => request.header("private-token", &self.token),
        }
    }

    async fn refused(&self, response: reqwest::Response) -> Result<reqwest::Response, ForgeError> {
        match response.status() {
            StatusCode::UNAUTHORIZED => Err(ForgeError::Rejected),
            StatusCode::FORBIDDEN => {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                let other_reason = self.kind == ForgeKind::Gitlab && body["error"].as_str().is_some_and(|error| error != "insufficient_scope");

                Err(if other_reason { ForgeError::Failed(StatusCode::FORBIDDEN.as_u16()) } else { ForgeError::MissingScope })
            }
            _ => Ok(response),
        }
    }

    async fn fetch(&self, http: &reqwest::Client, path: &str) -> Result<Value, ForgeError> {
        let request = self.authorised(http.get(format!("{}{path}", self.api_url())));
        let response = request.send().await.map_err(|_| ForgeError::Unreachable)?;
        let response = self.refused(response).await?;
        let status = response.status();

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        response.json().await.map_err(|_| ForgeError::Failed(status.as_u16()))
    }

    async fn fetch_and_store(&self, cache: &ForgeCache, http: &reqwest::Client, path: &str, key: String) -> Result<Value, ForgeError> {
        let epoch = cache.epoch(self.id);
        let value = self.fetch(http, path).await?;

        cache.store(self.id, key, value.clone(), epoch);

        Ok(value)
    }

    pub async fn get(&self, http: &reqwest::Client, path: &str) -> Result<Value, ForgeError> {
        let Some(cache) = &self.cache else {
            return self.fetch(http, path).await;
        };
        let key = ForgeCache::key(self.id, &format!("{}{path}", self.api_url()));

        if !self.refresh {
            match cache.lookup(&key) {
                Cached::Fresh(value) => return Ok(value),
                Cached::Stale(value) => {
                    if cache.claim_refresh(&key) {
                        let api = self.clone();
                        let http = http.clone();
                        let path = path.to_string();
                        let cache = cache.clone();

                        tokio::spawn(async move {
                            api.fetch_and_store(&cache, &http, &path, key.clone()).await.ok();
                            cache.finish_refresh(&key);
                        });
                    }

                    return Ok(value);
                }
                Cached::Miss => {}
            }
        }

        self.fetch_and_store(cache, http, path, key).await
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
        let field = if self.kind == ForgeKind::Gitlab { "username" } else { "login" };

        user[field].as_str().map(str::to_string).ok_or(ForgeError::Failed(200))
    }

    pub async fn organisations(&self, http: &reqwest::Client, account: &str) -> Result<Vec<String>, ForgeError> {
        let (path, field) = match self.kind {
            ForgeKind::Github => ("/user/orgs", "login"),
            ForgeKind::Forgejo => ("/user/orgs", "username"),
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
            ForgeKind::Github | ForgeKind::Forgejo => "/user/repos?sort=updated",
            ForgeKind::Gitlab => "/projects?membership=true&simple=true&order_by=last_activity_at",
        };
        let name_field = if self.kind == ForgeKind::Gitlab { "path_with_namespace" } else { "full_name" };
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
                updated_at: ["pushed_at", "last_activity_at", "updated_at"].iter().find_map(|field| item[field].as_str()).and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok()).map(|time| time.to_utc()),
            });
        }

        Ok(repositories)
    }
}

impl ForgeApi {
    pub async fn clone_urls(&self, http: &reqwest::Client, repository: &str) -> Vec<String> {
        let project = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => self.fetch(http, &format!("/repos/{repository}")).await,
            ForgeKind::Gitlab => self.fetch(http, &format!("/projects/{}", encode_path(repository))).await,
        };

        let Ok(project) = project else {
            return Vec::new();
        };

        ["ssh_url_to_repo", "http_url_to_repo", "ssh_url", "clone_url"].iter().filter_map(|field| project[*field].as_str()).map(str::to_string).collect()
    }

    pub async fn file(&self, http: &reqwest::Client, repository: &str, path: &str) -> Result<Option<ForgeFile>, ForgeError> {
        let body = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => self.fetch(http, &format!("/repos/{repository}/contents/{}", encode_segments(path))).await,
            ForgeKind::Gitlab => {
                let project = self.fetch(http, &format!("/projects/{}", encode_path(repository))).await?;
                let branch = project["default_branch"].as_str().unwrap_or("main");

                self.fetch(http, &format!("/projects/{}/repository/files/{}?ref={}", encode_path(repository), encode_path(path), encode_path(branch))).await
            }
        };
        let body = match body {
            Ok(body) => body,
            Err(ForgeError::Failed(404)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let encoded: String = body["content"].as_str().unwrap_or_default().chars().filter(|character| !character.is_whitespace()).collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ForgeError::Failed(200))?;
        let text = String::from_utf8(bytes).map_err(|_| ForgeError::Failed(200))?;
        let commit = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let commits = self.fetch(http, &format!("/repos/{repository}/commits?per_page=1&path={}", encode_segments(path))).await?;

                commits[0]["sha"].as_str().ok_or(ForgeError::Failed(200))?.to_string()
            }
            ForgeKind::Gitlab => body["last_commit_id"].as_str().ok_or(ForgeError::Failed(200))?.to_string(),
        };

        Ok(Some(ForgeFile { text, commit, blob: body["sha"].as_str().unwrap_or_default().to_string() }))
    }

    pub async fn write_file(&self, http: &reqwest::Client, repository: &str, path: &str, text: &str, message: &str, existing: Option<&ForgeFile>) -> Result<String, ForgeError> {
        let (method, url, body) = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let mut body = serde_json::json!({"message": message, "content": base64::engine::general_purpose::STANDARD.encode(text)});

                if let Some(existing) = existing {
                    body["sha"] = Value::String(existing.blob.clone());
                }

                let method = if self.kind == ForgeKind::Forgejo && existing.is_none() { reqwest::Method::POST } else { reqwest::Method::PUT };

                (method, format!("/repos/{repository}/contents/{}", encode_segments(path)), body)
            }
            ForgeKind::Gitlab => {
                let project = self.get(http, &format!("/projects/{}", encode_path(repository))).await?;
                let branch = project["default_branch"].as_str().unwrap_or("main");
                let mut body = serde_json::json!({"branch": branch, "content": text, "commit_message": message});
                let method = if existing.is_some() { reqwest::Method::PUT } else { reqwest::Method::POST };

                if let Some(existing) = existing {
                    body["last_commit_id"] = Value::String(existing.commit.clone());
                }

                (method, format!("/projects/{}/repository/files/{}", encode_path(repository), encode_path(path)), body)
            }
        };
        let request = self.authorised(http.request(method, format!("{}{url}", self.api_url()))).json(&body);
        let sent = request.send().await;

        self.invalidate();

        let response = sent.map_err(|_| ForgeError::Unreachable)?;
        let response = self.refused(response).await?;
        let status = response.status();

        if matches!(status.as_u16(), 400 | 409 | 422) {
            return Err(ForgeError::Conflict);
        }

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let answer: Value = response.json().await.map_err(|_| ForgeError::Failed(status.as_u16()))?;

                answer["commit"]["sha"].as_str().map(str::to_string).ok_or(ForgeError::Failed(status.as_u16()))
            }
            ForgeKind::Gitlab => self.file(http, repository, path).await?.map(|file| file.commit).ok_or(ForgeError::Failed(status.as_u16())),
        }
    }

    pub async fn default_branch(&self, http: &reqwest::Client, repository: &str) -> Result<String, ForgeError> {
        let project = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => self.fetch(http, &format!("/repos/{repository}")).await?,
            ForgeKind::Gitlab => self.fetch(http, &format!("/projects/{}", encode_path(repository))).await?,
        };

        Ok(project["default_branch"].as_str().unwrap_or("main").to_string())
    }

    pub async fn tree(&self, http: &reqwest::Client, repository: &str, branch: &str) -> Result<Vec<TreeEntry>, ForgeError> {
        let found = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => match self.fetch(http, &format!("/repos/{repository}/git/trees/{}?recursive=1", encode_path(branch))).await {
                Ok(body) => body["tree"].as_array().cloned().unwrap_or_default().into_iter().filter(|entry| entry["type"] == "blob").filter_map(|entry| Some(TreeEntry { path: entry["path"].as_str()?.to_string(), blob: entry["sha"].as_str()?.to_string() })).collect(),
                Err(ForgeError::Failed(404 | 409)) => Vec::new(),
                Err(error) => return Err(error),
            },
            ForgeKind::Gitlab => match self.pages(http, &format!("/projects/{}/repository/tree?recursive=true&ref={}", encode_path(repository), encode_path(branch))).await {
                Ok(entries) => entries.into_iter().filter(|entry| entry["type"] == "blob").filter_map(|entry| Some(TreeEntry { path: entry["path"].as_str()?.to_string(), blob: entry["id"].as_str()?.to_string() })).collect(),
                Err(ForgeError::Failed(404)) => Vec::new(),
                Err(error) => return Err(error),
            },
        };

        Ok(found)
    }

    pub async fn blob(&self, http: &reqwest::Client, repository: &str, branch: &str, path: &str) -> Result<Vec<u8>, ForgeError> {
        let body = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => self.fetch(http, &format!("/repos/{repository}/contents/{}?ref={}", encode_segments(path), encode_path(branch))).await?,
            ForgeKind::Gitlab => self.fetch(http, &format!("/projects/{}/repository/files/{}?ref={}", encode_path(repository), encode_path(path), encode_path(branch))).await?,
        };
        let encoded: String = body["content"].as_str().unwrap_or_default().chars().filter(|character| !character.is_whitespace()).collect();

        base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ForgeError::Failed(200))
    }

    async fn send_change(&self, http: &reqwest::Client, method: reqwest::Method, url: String, body: Value) -> Result<Value, ForgeError> {
        let sent = self.authorised(http.request(method, format!("{}{url}", self.api_url()))).json(&body).send().await;

        self.invalidate();

        let response = sent.map_err(|_| ForgeError::Unreachable)?;
        let response = self.refused(response).await?;
        let status = response.status();

        if matches!(status.as_u16(), 400 | 409 | 422) {
            return Err(ForgeError::Conflict);
        }

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        Ok(response.json().await.unwrap_or(Value::Null))
    }

    pub async fn commit_blob(&self, http: &reqwest::Client, repository: &str, branch: &str, path: &str, content: &[u8], message: &str, existing_blob: Option<&str>) -> Result<String, ForgeError> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);

        match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let mut body = serde_json::json!({"message": message, "content": encoded, "branch": branch});

                if let Some(blob) = existing_blob {
                    body["sha"] = Value::String(blob.to_string());
                }

                let method = if self.kind == ForgeKind::Forgejo && existing_blob.is_none() { reqwest::Method::POST } else { reqwest::Method::PUT };
                let answer = self.send_change(http, method, format!("/repos/{repository}/contents/{}", encode_segments(path)), body).await?;

                answer["commit"]["sha"].as_str().map(str::to_string).ok_or(ForgeError::Failed(200))
            }
            ForgeKind::Gitlab => {
                let method = if existing_blob.is_some() { reqwest::Method::PUT } else { reqwest::Method::POST };
                let body = serde_json::json!({"branch": branch, "content": encoded, "encoding": "base64", "commit_message": message});

                self.send_change(http, method, format!("/projects/{}/repository/files/{}", encode_path(repository), encode_path(path)), body).await?;

                let head = self.fetch(http, &format!("/projects/{}/repository/branches/{}", encode_path(repository), encode_path(branch))).await?;

                head["commit"]["id"].as_str().map(str::to_string).ok_or(ForgeError::Failed(200))
            }
        }
    }

    pub async fn remove_blob(&self, http: &reqwest::Client, repository: &str, branch: &str, path: &str, message: &str, existing_blob: &str) -> Result<(), ForgeError> {
        match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let body = serde_json::json!({"message": message, "sha": existing_blob, "branch": branch});

                self.send_change(http, reqwest::Method::DELETE, format!("/repos/{repository}/contents/{}", encode_segments(path)), body).await?;
            }
            ForgeKind::Gitlab => {
                let body = serde_json::json!({"branch": branch, "commit_message": message});

                self.send_change(http, reqwest::Method::DELETE, format!("/projects/{}/repository/files/{}", encode_path(repository), encode_path(path)), body).await?;
            }
        }

        Ok(())
    }

    pub async fn issues(&self, http: &reqwest::Client, repository: &str, query: &str) -> Result<Vec<Issue>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Forgejo => {
                let encoded: String = url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();

                format!("/repos/{repository}/issues?type=issues&state=open&limit=50&q={encoded}")
            }
            ForgeKind::Github => {
                let search = format!("repo:{repository} is:issue is:open {query}");
                let encoded: String = url::form_urlencoded::byte_serialize(search.trim().as_bytes()).collect();

                format!("/search/issues?per_page=50&sort=updated&order=desc&q={encoded}")
            }
            ForgeKind::Gitlab => {
                let encoded: String = url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();

                format!("/projects/{}/issues?state=opened&per_page=50&order_by=updated_at&sort=desc&search={encoded}", encode_path(repository))
            }
        };
        let body = self.get(http, &path).await?;
        let items = if self.kind == ForgeKind::Github { body["items"].as_array() } else { body.as_array() };

        Ok(items.map(|items| items.iter().filter_map(|item| self.to_issue(repository, item)).collect()).unwrap_or_default())
    }

    pub async fn issue(&self, http: &reqwest::Client, repository: &str, number: i64) -> Result<Option<Issue>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => format!("/repos/{repository}/issues/{number}"),
            ForgeKind::Gitlab => format!("/projects/{}/issues/{number}", encode_path(repository)),
        };

        match self.get(http, &path).await {
            Ok(item) => Ok(self.to_issue(repository, &item)),
            Err(ForgeError::Failed(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn to_issue(&self, repository: &str, item: &Value) -> Option<Issue> {
        let (number, url, body) = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => (&item["number"], &item["html_url"], &item["body"]),
            ForgeKind::Gitlab => (&item["iid"], &item["web_url"], &item["description"]),
        };

        Some(Issue {
            forge_id: self.id,
            repository: repository.to_string(),
            number: number.as_i64()?,
            title: item["title"].as_str()?.to_string(),
            url: url.as_str().unwrap_or_default().to_string(),
            body: body.as_str().unwrap_or_default().to_string(),
            pull_requests: None,
            author: self.person(&item[if self.kind == ForgeKind::Gitlab { "author" } else { "user" }]),
            assignees: self.people(&item["assignees"]),
            updated_at: moment(&item["updated_at"]),
        })
    }

    fn person(&self, value: &Value) -> Option<ForgePerson> {
        let name = value["login"].as_str().or_else(|| value["username"].as_str())?;

        Some(ForgePerson { name: name.to_string(), avatar_url: value["avatar_url"].as_str().filter(|url| !url.is_empty()).map(str::to_string) })
    }

    fn people(&self, value: &Value) -> Vec<ForgePerson> {
        value.as_array().into_iter().flatten().filter_map(|person| self.person(person)).collect()
    }

    pub async fn pull_requests(&self, http: &reqwest::Client, repository: &str) -> Result<Vec<PullRequestItem>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Github => format!("/repos/{repository}/pulls?state=all&sort=updated&direction=desc&per_page=50"),
            ForgeKind::Forgejo => format!("/repos/{repository}/pulls?state=all&sort=recentupdate&limit=50"),
            ForgeKind::Gitlab => format!("/projects/{}/merge_requests?state=all&order_by=updated_at&sort=desc&per_page=50", encode_path(repository)),
        };
        let body = self.get(http, &path).await?;

        Ok(body.as_array().into_iter().flatten().filter_map(|item| self.to_pull_request(repository, item)).collect())
    }

    pub async fn pull_request(&self, http: &reqwest::Client, repository: &str, number: i64) -> Result<Option<PullRequestItem>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => format!("/repos/{repository}/pulls/{number}"),
            ForgeKind::Gitlab => format!("/projects/{}/merge_requests/{number}", encode_path(repository)),
        };

        match self.get(http, &path).await {
            Ok(item) => Ok(self.to_pull_request(repository, &item)),
            Err(ForgeError::Failed(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn to_pull_request(&self, repository: &str, item: &Value) -> Option<PullRequestItem> {
        let draft = item["draft"].as_bool().unwrap_or(false);
        let merged = !item["merged_at"].is_null() || item["merged"].as_bool() == Some(true);
        let (number, label, url, body, state, source, target) = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let number = item["number"].as_i64()?;
                let state = match (merged, item["state"].as_str()) {
                    (true, _) => PullRequestState::Merged,
                    (false, Some("open")) => PullRequestState::Open,
                    _ => PullRequestState::Closed,
                };

                (number, format!("PR #{number}"), &item["html_url"], &item["body"], state, &item["head"]["ref"], &item["base"]["ref"])
            }
            ForgeKind::Gitlab => {
                let number = item["iid"].as_i64()?;
                let state = match item["state"].as_str() {
                    Some("opened") => PullRequestState::Open,
                    Some("merged") => PullRequestState::Merged,
                    _ => PullRequestState::Closed,
                };

                (number, format!("MR !{number}"), &item["web_url"], &item["description"], state, &item["source_branch"], &item["target_branch"])
            }
        };

        Some(PullRequestItem {
            forge_id: self.id,
            repository: repository.to_string(),
            number,
            label,
            title: item["title"].as_str()?.to_string(),
            url: url.as_str().unwrap_or_default().to_string(),
            body: body.as_str().unwrap_or_default().to_string(),
            state,
            draft,
            source_branch: source.as_str().unwrap_or_default().to_string(),
            target_branch: target.as_str().unwrap_or_default().to_string(),
            author: self.person(&item[if self.kind == ForgeKind::Gitlab { "author" } else { "user" }]),
            assignees: self.people(&item["assignees"]),
            updated_at: moment(&item["updated_at"]),
        })
    }

    pub async fn pull_request_timeline(&self, http: &reqwest::Client, repository: &str, number: i64) -> Result<Vec<PullRequestComment>, ForgeError> {
        let mut timeline: Vec<PullRequestComment> = Vec::new();

        match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let conversation = self.get(http, &format!("/repos/{repository}/issues/{number}/comments?per_page={PAGE_SIZE}")).await?;
                let reviews = self.get(http, &format!("/repos/{repository}/pulls/{number}/comments?per_page={PAGE_SIZE}")).await.unwrap_or(Value::Null);

                let threads = if self.kind == ForgeKind::Github { self.github_thread_resolution(http, repository, number).await } else { HashMap::new() };

                for (comments, reviewed) in [(&conversation, false), (&reviews, true)] {
                    for comment in comments.as_array().into_iter().flatten() {
                        let resolved = match (self.kind, reviewed) {
                            (ForgeKind::Forgejo, true) => Some(!comment["resolver"].is_null()),
                            (ForgeKind::Github, true) => comment["id"].as_i64().and_then(|id| threads.get(&id).copied()),
                            _ => None,
                        };

                        timeline.push(PullRequestComment {
                            author: self.person(&comment["user"]),
                            body: comment["body"].as_str().unwrap_or_default().to_string(),
                            created_at: moment(&comment["created_at"]),
                            path: comment["path"].as_str().map(str::to_string),
                            resolved,
                        });
                    }
                }
            }
            ForgeKind::Gitlab => {
                let discussions = self.get(http, &format!("/projects/{}/merge_requests/{number}/discussions?per_page={PAGE_SIZE}", encode_path(repository))).await?;

                for discussion in discussions.as_array().into_iter().flatten() {
                    for note in discussion["notes"].as_array().into_iter().flatten().filter(|note| note["system"].as_bool() != Some(true)) {
                        let resolved = note["resolvable"].as_bool().filter(|resolvable| *resolvable).and_then(|_| note["resolved"].as_bool());

                        timeline.push(PullRequestComment {
                            author: self.person(&note["author"]),
                            body: note["body"].as_str().unwrap_or_default().to_string(),
                            created_at: moment(&note["created_at"]),
                            path: note["position"]["new_path"].as_str().map(str::to_string),
                            resolved,
                        });
                    }
                }
            }
        }

        timeline.sort_by_key(|comment| comment.created_at);

        Ok(timeline)
    }

    async fn github_thread_resolution(&self, http: &reqwest::Client, repository: &str, number: i64) -> HashMap<i64, bool> {
        let (owner, name) = repository.split_once('/').unwrap_or(("", repository));
        let query = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { reviewThreads(first: 100) { nodes { isResolved comments(first: 100) { nodes { databaseId } } } } } } }";
        let body = serde_json::json!({"query": query, "variables": {"owner": owner, "name": name, "number": number}});
        let address = if self.base_url == "https://github.com" { "https://api.github.com/graphql".to_string() } else { format!("{}/api/graphql", self.base_url) };
        let sent = self.authorised(http.post(address)).json(&body).send().await;
        let Ok(response) = sent else {
            return HashMap::new();
        };
        let answer: Value = response.json().await.unwrap_or(Value::Null);
        let mut resolution = HashMap::new();

        for thread in answer["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].as_array().into_iter().flatten() {
            let resolved = thread["isResolved"].as_bool().unwrap_or(false);

            for comment in thread["comments"]["nodes"].as_array().into_iter().flatten() {
                if let Some(id) = comment["databaseId"].as_i64() {
                    resolution.insert(id, resolved);
                }
            }
        }

        resolution
    }

    pub async fn pull_request_diff(&self, http: &reqwest::Client, repository: &str, number: i64) -> Result<String, ForgeError> {
        let (path, accept) = match self.kind {
            ForgeKind::Github => (format!("/repos/{repository}/pulls/{number}"), "application/vnd.github.diff"),
            ForgeKind::Forgejo => (format!("/repos/{repository}/pulls/{number}.diff"), "text/plain"),
            ForgeKind::Gitlab => {
                let changes = self.get(http, &format!("/projects/{}/merge_requests/{number}/changes", encode_path(repository))).await?;
                let files = changes["changes"].as_array().into_iter().flatten();

                return Ok(files
                    .map(|file| {
                        let (old, new) = (file["old_path"].as_str().unwrap_or_default(), file["new_path"].as_str().unwrap_or_default());

                        format!("diff --git a/{old} b/{new}\n--- a/{old}\n+++ b/{new}\n{}", file["diff"].as_str().unwrap_or_default())
                    })
                    .collect());
            }
        };
        let request = self.authorised(http.get(format!("{}{path}", self.api_url()))).header("accept", accept);
        let response = request.send().await.map_err(|_| ForgeError::Unreachable)?;
        let response = self.refused(response).await?;
        let status = response.status();

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        response.text().await.map_err(|_| ForgeError::Failed(status.as_u16()))
    }

    pub async fn linked_pull_requests(&self, http: &reqwest::Client, repository: &str, number: i64) -> Result<Vec<PullRequest>, ForgeError> {
        let mut linked: Vec<PullRequest> = Vec::new();

        match self.kind {
            ForgeKind::Forgejo => return Err(ForgeError::Failed(StatusCode::NOT_IMPLEMENTED.as_u16())),
            ForgeKind::Github => {
                let events = self.get(http, &format!("/repos/{repository}/issues/{number}/timeline?per_page={PAGE_SIZE}")).await?;

                for event in events.as_array().into_iter().flatten().filter(|event| event["event"] == "cross-referenced") {
                    let source = &event["source"]["issue"];
                    let (Some(number), Some(url), false) = (source["number"].as_i64(), source["html_url"].as_str(), source["pull_request"].is_null()) else {
                        continue;
                    };
                    let state = match (source["pull_request"]["merged_at"].is_null(), source["state"].as_str()) {
                        (false, _) => PullRequestState::Merged,
                        (true, Some("open")) => PullRequestState::Open,
                        (true, _) => PullRequestState::Closed,
                    };

                    linked.push(PullRequest { number, label: format!("PR #{number}"), url: url.to_string(), state });
                }
            }
            ForgeKind::Gitlab => {
                let requests = self.get(http, &format!("/projects/{}/issues/{number}/related_merge_requests", encode_path(repository))).await?;

                for request in requests.as_array().into_iter().flatten() {
                    let (Some(number), Some(url)) = (request["iid"].as_i64(), request["web_url"].as_str()) else {
                        continue;
                    };
                    let state = match request["state"].as_str() {
                        Some("opened") => PullRequestState::Open,
                        Some("merged") => PullRequestState::Merged,
                        _ => PullRequestState::Closed,
                    };

                    linked.push(PullRequest { number, label: format!("MR !{number}"), url: url.to_string(), state });
                }
            }
        }

        let mut seen = std::collections::HashSet::new();
        linked.retain(|request| seen.insert(request.number));

        Ok(linked)
    }

    pub async fn branch_pull_requests(&self, http: &reqwest::Client, repository: &str) -> Result<HashMap<String, BranchPullRequest>, ForgeError> {
        let path = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => format!("/repos/{repository}/pulls?state=all&sort=updated&direction=desc&per_page={PAGE_SIZE}"),
            ForgeKind::Gitlab => format!("/projects/{}/merge_requests?state=all&order_by=updated_at&sort=desc&per_page={PAGE_SIZE}", encode_path(repository)),
        };
        let body = self.get(http, &path).await?;
        let mut found: HashMap<String, BranchPullRequest> = HashMap::new();

        for item in body.as_array().into_iter().flatten() {
            let Some((branch, request)) = self.to_branch_pull_request(repository, item) else {
                continue;
            };
            let replaces = found.get(&branch).is_none_or(|kept| kept.state != "open" && kept.state != "draft" && (request.state == "open" || request.state == "draft"));

            if replaces {
                found.insert(branch, request);
            }
        }

        Ok(found)
    }

    fn to_branch_pull_request(&self, repository: &str, item: &Value) -> Option<(String, BranchPullRequest)> {
        let draft = item["draft"].as_bool().unwrap_or(false);

        Some(match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let from_fork = item["head"]["repo"]["full_name"].as_str().is_some_and(|source| !source.eq_ignore_ascii_case(repository));

                if from_fork {
                    return None;
                }

                let number = item["number"].as_i64()?;
                let state = match (item["merged_at"].is_null(), item["state"].as_str(), draft) {
                    (false, _, _) => "merged",
                    (true, Some("open"), true) => "draft",
                    (true, Some("open"), false) => "open",
                    _ => "closed",
                };

                (item["head"]["ref"].as_str()?.to_string(), BranchPullRequest { number, label: format!("PR #{number}"), title: item["title"].as_str()?.to_string(), url: item["html_url"].as_str()?.to_string(), state })
            }
            ForgeKind::Gitlab => {
                let number = item["iid"].as_i64()?;
                let state = match (item["state"].as_str(), draft) {
                    (Some("opened"), true) => "draft",
                    (Some("opened"), false) => "open",
                    (Some("merged"), _) => "merged",
                    _ => "closed",
                };

                (item["source_branch"].as_str()?.to_string(), BranchPullRequest { number, label: format!("MR !{number}"), title: item["title"].as_str()?.to_string(), url: item["web_url"].as_str()?.to_string(), state })
            }
        })
    }

    pub async fn create_repository(&self, http: &reqwest::Client, account: &str, repository: &str) -> Result<(), ForgeError> {
        let (owner, name) = repository.rsplit_once('/').unwrap_or(("", repository));
        let (path, body) = match self.kind {
            ForgeKind::Github | ForgeKind::Forgejo => {
                let path = if owner == account { "/user/repos".to_string() } else { format!("/orgs/{}/repos", encode_path(owner)) };

                (path, serde_json::json!({"name": name, "private": true, "auto_init": true}))
            }
            ForgeKind::Gitlab => {
                let namespace = self.get(http, &format!("/namespaces/{}", encode_path(owner))).await?;
                let body = serde_json::json!({"name": name, "path": name, "namespace_id": namespace["id"], "visibility": "private", "initialize_with_readme": true});

                ("/projects".to_string(), body)
            }
        };
        let request = self.authorised(http.post(format!("{}{path}", self.api_url()))).json(&body);
        let sent = request.send().await;

        self.invalidate();

        let response = sent.map_err(|_| ForgeError::Unreachable)?;
        let response = self.refused(response).await?;
        let status = response.status();

        if matches!(status.as_u16(), 400 | 409 | 422) {
            return Err(ForgeError::CreateRefused);
        }

        if !status.is_success() {
            return Err(ForgeError::Failed(status.as_u16()));
        }

        Ok(())
    }
}

fn moment(value: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    value.as_str().and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok()).map(|time| time.to_utc())
}

fn encode_path(repository: &str) -> String {
    url::form_urlencoded::byte_serialize(repository.as_bytes()).collect::<String>().replace('+', "%20")
}

fn encode_segments(path: &str) -> String {
    path.split('/').map(encode_path).collect::<Vec<_>>().join("/")
}

pub fn display_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Github | ForgeKind::Forgejo => "GitHub",
        ForgeKind::Gitlab => "GitLab",
        ForgeKind::Forgejo => "Forgejo",
    }
}

pub fn parse_kind(text: &str) -> ApiResult<ForgeKind> {
    match text {
        "github" => Ok(ForgeKind::Github),
        "gitlab" => Ok(ForgeKind::Gitlab),
        "forgejo" => Ok(ForgeKind::Forgejo),
        other => Err(ApiError::internal(format!("unknown forge kind {other}"))),
    }
}

pub fn token_hint(token: &str) -> String {
    let characters: Vec<char> = token.chars().collect();

    if characters.len() < 12 {
        return "••••".to_string();
    }

    format!("••••{}", characters[characters.len() - 4..].iter().collect::<String>())
}

fn normalised_base_url(kind: ForgeKind, entered: Option<&str>) -> ApiResult<String> {
    let default = match kind {
        ForgeKind::Github | ForgeKind::Forgejo => "https://github.com",
        ForgeKind::Gitlab => "https://gitlab.com",
        ForgeKind::Forgejo => "https://codeberg.org",
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

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ForgeRow> {
    let row = sqlx::query_as::<_, ForgeRow>(sqlx::AssertSqlSafe(format!("select {FORGE_COLUMNS} from forges where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That forge"))
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

    state.forge_cache.invalidate(row.id);

    let mcp_error = link_mcp_server(&state, user.id, &row, request.kind, request.add_mcp != Some(false)).await?;
    let linked = find_owned(&state, row.id, user.id).await?;
    let mut forge = linked.to_proto()?;
    forge.mcp_error = mcp_error;

    Ok((StatusCode::CREATED, Json(forge)))
}

pub async fn link_mcp(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Forge>> {
    let row = find_owned(&state, id, user.id).await?;
    let kind = row.to_proto()?.kind;
    let mcp_error = link_mcp_server(&state, user.id, &row, kind, true).await?;
    let mut forge = find_owned(&state, id, user.id).await?.to_proto()?;
    forge.mcp_error = mcp_error;

    Ok(Json(forge))
}

fn mcp_address(kind: ForgeKind, base_url: &str) -> Option<(&'static str, String)> {
    match kind {
        ForgeKind::Gitlab => Some(("GitLab", format!("{base_url}/api/v4/mcp"))),
        ForgeKind::Github if base_url == "https://github.com" => Some(("GitHub", "https://api.githubcopilot.com/mcp/".to_string())),
        ForgeKind::Github | ForgeKind::Forgejo => None,
    }
}

async fn link_mcp_server(state: &AppState, owner: Uuid, forge: &ForgeRow, kind: ForgeKind, wanted: bool) -> ApiResult<Option<String>> {
    let Some((product, address)) = mcp_address(kind, &forge.base_url) else {
        return Ok(None);
    };
    let linked = sqlx::query_scalar::<_, Uuid>("select id from chat_tool_servers where forge_id = $1").bind(forge.id).fetch_optional(&state.pool).await?;

    if linked.is_none() && !wanted {
        return Ok(None);
    }

    let tools = match crate::chat_tool_servers::fetch_tools(state, product, &address, Some(&forge.token), crate::chat_tool_servers::DEFAULT_KEY_HEADER, None).await {
        Ok(tools) => tools,
        Err(_) if kind == ForgeKind::Github => return Ok(Some(format!("{product}'s MCP server refused this token. It needs a fine-grained token with the MCP permission."))),
        Err(_) => return Ok(Some(format!("{product}'s MCP server refused this token. It needs a fine-grained token with the MCP permission, or sign in instead."))),
    };

    match linked {
        Some(id) => {
            sqlx::query("update chat_tool_servers set bearer_token = $2, tools = $3 where id = $1").bind(id).bind(&forge.token).bind(sqlx::types::Json(tools)).execute(&state.pool).await?;
        }
        None => {
            let host = url::Url::parse(&forge.base_url).ok().and_then(|parsed| parsed.host_str().map(str::to_string)).unwrap_or_default();
            let name = match crate::chat_tool_servers::check_name(state, owner, product, None).await {
                Ok(()) => product.to_string(),
                Err(_) => format!("{product} {host}"),
            };

            sqlx::query("insert into chat_tool_servers (owner, name, kind, url, bearer_token, tools, settings, forge_id) values ($1, $2, 'mcp', $3, $4, $5, '{}', $6)")
                .bind(owner)
                .bind(name)
                .bind(&address)
                .bind(&forge.token)
                .bind(sqlx::types::Json(tools))
                .bind(forge.id)
                .execute(&state.pool)
                .await?;
        }
    }

    state.hub.tools_changed(owner);
    state.publish(Some(owner), mycelium_proto::stream::StreamMessage::IntegrationsChanged);

    Ok(None)
}

pub async fn disconnect(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from forges where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That forge"));
    }

    state.forge_cache.invalidate(id);
    state.hub.tools_changed(user.id);
    state.publish(Some(user.id), mycelium_proto::stream::StreamMessage::IntegrationsChanged);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AvatarQuery {
    url: String,
}

const AVATAR_LIMIT: usize = 1_000_000;
const PUBLIC_AVATAR_HOSTS: [&str; 3] = ["avatars.githubusercontent.com", "secure.gravatar.com", "gravatar.com"];

pub async fn avatar(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<AvatarQuery>) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;

    let forge = find_owned(&state, id, user.id).await?;
    let wanted = url::Url::parse(&query.url).ok().filter(|wanted| wanted.scheme() == "https" || wanted.scheme() == "http");
    let forge_origin = url::Url::parse(&forge.base_url).ok().map(|base| base.origin());
    let Some(wanted) = wanted else {
        return Err(ApiError::bad_request("avatar_url_invalid", "Enter the address of an image."));
    };
    let host = wanted.host_str().unwrap_or_default().to_string();
    let on_forge = forge_origin.as_ref() == Some(&wanted.origin());

    if !wanted.username().is_empty() || wanted.password().is_some() || (!on_forge && (wanted.scheme() != "https" || wanted.port_or_known_default() != Some(443) || !PUBLIC_AVATAR_HOSTS.contains(&host.as_str()))) {
        return Err(ApiError::not_found("That avatar"));
    }

    let api = ForgeApi::from_row(&forge)?;
    let request = state.http.get(wanted).timeout(FORGE_TIMEOUT).header("user-agent", "mycelium");
    let request = if on_forge { api.authorised(request) } else { request };
    let response = request.send().await.map_err(|_| ApiError::not_found("That avatar"))?;
    let media_type = response.headers().get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();

    if !response.status().is_success() || !media_type.starts_with("image/") || media_type.starts_with("image/svg") {
        return Err(ApiError::not_found("That avatar"));
    }

    let bytes = crate::outbound::read_limited(response, AVATAR_LIMIT).await.map_err(|_| ApiError::not_found("That avatar"))?;

    Ok(([("content-type", media_type), ("cache-control", "private, max-age=86400".to_string()), ("x-content-type-options", "nosniff".to_string())], bytes).into_response())
}

pub async fn repositories(State(state): State<AppState>, user: AuthUser, Query(query): Query<RepositoryQuery>) -> ApiResult<Json<Repositories>> {
    let mut rows = list_owned(&state, user.id).await?;

    if let Some(forge) = query.forge {
        rows.retain(|row| row.id == forge);
    }

    let fetched = join_all(rows.iter().map(|row| async {
        let api = ForgeApi::from_row(row)?.cached(&state.forge_cache).refreshing(query.refresh);
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

    result.repositories.sort_by(|first, second| second.updated_at.cmp(&first.updated_at));

    Ok(Json(result))
}
