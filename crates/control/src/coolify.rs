use crate::auth::AuthUser;
use crate::caldav::host_of;
use crate::chat_tool_servers::{SERVER_COLUMNS, ServerRow, StoredTool};
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

const CREATE_APP: &str = "create_app";
const DEPLOY: &str = "deploy";
const STOP: &str = "stop";
const START: &str = "start";
const DELETE_APP: &str = "delete_app";
const SET_ENV: &str = "set_env";
const DELETE_ENV: &str = "delete_env";
const DEFAULT_ENVIRONMENT: &str = "production";
const DEFAULT_PROJECT: &str = "default";
const SCOPE_KEYS: [&str; 6] = ["project_uuid", "server_uuid", "project_name", "server_name", "wildcard_domain", "environment_name"];
const DEFAULT_LOG_LINES: u64 = 100;
const MAX_LOG_LINES: u64 = 1000;
const TAIL_LINES: usize = 12;
const TIMEOUT: Duration = Duration::from_secs(30);
const SECRET_HIDDEN: &str = "Secret, hidden";

enum Failure {
    Unreachable(String),
    Refused(String),
    Missing(String),
    Other(String),
}

impl Failure {
    fn message(&self) -> &str {
        match self {
            Failure::Unreachable(message) | Failure::Refused(message) | Failure::Missing(message) | Failure::Other(message) => message,
        }
    }
}

type Outcome<T> = Result<T, Failure>;

#[derive(serde::Deserialize)]
pub struct TestRequest {
    url: String,
    #[serde(default)]
    token: String,
}

#[derive(serde::Deserialize)]
pub struct TargetRequest {
    #[serde(default)]
    project_uuid: Option<String>,
    #[serde(default)]
    server_uuid: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct EnvRequest {
    value: String,
    #[serde(default)]
    secret: bool,
    #[serde(default)]
    build_time: bool,
}

#[derive(serde::Deserialize)]
pub struct LogsQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    deployment: Option<String>,
    #[serde(default)]
    lines: Option<u64>,
}

pub fn tools() -> Vec<StoredTool> {
    let app = json!({"type": "string", "description": "The app's uuid, from list_apps."});
    let by_app = |description: &str| json!({"type": "object", "properties": {"app": app.clone()}, "required": ["app"], "description": description});

    vec![
        StoredTool { name: "list_apps".to_string(), description: "List the apps in Coolify, with their status and address.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool { name: "list_projects".to_string(), description: "List the projects in Coolify, with their uuid and name. Use one to say where create_app puts an app.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool { name: "list_servers".to_string(), description: "List the servers in Coolify, with their uuid and name. Use one to say where create_app runs an app.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool { name: "get_app".to_string(), description: "Read an app's status, address, repository, branch, build pack and latest deployment.".to_string(), input_schema: by_app("Read an app."), read_only: None },
        StoredTool { name: "list_deployments".to_string(), description: "List an app's recent deployments with their commit, who triggered them and the result.".to_string(), input_schema: by_app("List deployments."), read_only: None },
        StoredTool {
            name: "get_logs".to_string(),
            description: "Read an app's build log for a deployment, or its runtime log. Runtime logs need a Coolify token with the read:sensitive permission.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "app": app.clone(),
                "kind": {"type": "string", "enum": ["build", "runtime"], "description": "Defaults to runtime."},
                "deployment": {"type": "string", "description": "A deployment uuid for the build log. Defaults to the latest."},
                "lines": {"type": "integer", "description": format!("How many lines from the end. Defaults to {DEFAULT_LOG_LINES}, at most {MAX_LOG_LINES}.")}
            }, "required": ["app"]}),
            read_only: None,
        },
        StoredTool {
            name: CREATE_APP.to_string(),
            description: "Create an app from a git repository, or from a docker image, in a project and on a server, and deploy it. Name them, or give their uuids from list_projects and list_servers. Leave the server out when Coolify has one, and the project out when Coolify has one or one named default. It gets an address under the server's wildcard domain. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "name": {"type": "string"},
                "project": {"type": "string", "description": "The project's name or uuid."},
                "server": {"type": "string", "description": "The server's name or uuid."},
                "environment": {"type": "string", "description": "The project's environment. Defaults to production, or the project's first one."},
                "git_repository": {"type": "string", "description": "The repository's address. Leave out for a docker image."},
                "git_branch": {"type": "string", "description": "Defaults to main."},
                "base_directory": {"type": "string", "description": "Where the app lives in the repository. Defaults to /."},
                "build_pack": {"type": "string", "enum": ["nixpacks", "railpack", "static", "dockerfile"], "description": "Defaults to nixpacks."},
                "ports": {"type": "string", "description": "The port the app listens on. Defaults to 3000."},
                "private_key_uuid": {"type": "string", "description": "A deploy key's uuid in Coolify, for a private repository."},
                "docker_image": {"type": "string", "description": "An image such as nginx:latest, instead of a repository."}
            }, "required": ["name"]}),
            read_only: None,
        },
        StoredTool { name: DEPLOY.to_string(), description: "Deploy an app again from its repository. Always needs the person's approval in chats, even when Always allow is on.".to_string(), input_schema: by_app("Deploy an app."), read_only: None },
        StoredTool { name: STOP.to_string(), description: "Stop a running app. Always needs the person's approval in chats, even when Always allow is on.".to_string(), input_schema: by_app("Stop an app."), read_only: None },
        StoredTool { name: START.to_string(), description: "Start a stopped app. Always needs the person's approval in chats, even when Always allow is on.".to_string(), input_schema: by_app("Start an app."), read_only: None },
        StoredTool { name: DELETE_APP.to_string(), description: "Delete an app from Coolify. Its code stays in git. Always needs the person's approval in chats, even when Always allow is on.".to_string(), input_schema: by_app("Delete an app."), read_only: None },
        StoredTool {
            name: SET_ENV.to_string(),
            description: "Set an environment variable on an app, replacing any value it had. The change applies at the next deploy. Secret values are never shown again. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "app": app.clone(),
                "key": {"type": "string"},
                "value": {"type": "string"},
                "secret": {"type": "boolean", "description": "Hide the value from everyone after saving. Defaults to false."},
                "build_time": {"type": "boolean", "description": "Also available while building. Defaults to false."}
            }, "required": ["app", "key", "value"]}),
            read_only: None,
        },
        StoredTool {
            name: DELETE_ENV.to_string(),
            description: "Delete an environment variable from an app. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"app": app, "key": {"type": "string"}}, "required": ["app", "key"]}),
            read_only: None,
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    matches!(tool, CREATE_APP | DEPLOY | STOP | START | DELETE_APP | SET_ENV | DELETE_ENV)
}

pub fn approval_input(tool: &str, input: &Value) -> Value {
    let mut shown = input.clone();

    if tool.ends_with(&format!("__{SET_ENV}")) && input["secret"] == json!(true) {
        shown["value"] = json!(SECRET_HIDDEN);
    }

    shown
}

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    let project = settings["project_uuid"].as_str().map(str::trim).filter(|uuid| !uuid.is_empty());
    let server = settings["server_uuid"].as_str().map(str::trim).filter(|uuid| !uuid.is_empty());

    match (project, server) {
        (Some(project), Some(server)) => Ok(json!({"project_uuid": project, "server_uuid": server})),
        (None, None) => Ok(json!({})),
        _ => Err("Choose both a project and a server, or neither for the whole instance.".to_string()),
    }
}

fn base(address: &str) -> String {
    format!("{}/api/v1", address.trim_end_matches('/'))
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    http.request(method, format!("{}/{path}", base(&credentials.address))).header("authorization", format!("Bearer {}", credentials.secret)).header("accept", "application/json").timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder, address: &str) -> Outcome<Value> {
    let host = host_of(address);
    let response = builder.send().await.map_err(|_| Failure::Unreachable(format!("Could not reach {host}. Check the address and that Coolify is running.")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(Failure::Refused(format!("{host} did not accept the API token. Check it has the read, write and deploy permissions, or create a new one.")));
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(Failure::Missing("Coolify has no app or deployment with that id.".to_string()));
    }

    if !status.is_success() {
        let reason = body["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(Failure::Other(format!("Coolify answered with an error: {reason}")));
    }

    Ok(body)
}

async fn get(http: &reqwest::Client, credentials: &Credentials, path: &str) -> Outcome<Value> {
    send(request(http, credentials, reqwest::Method::GET, path), &credentials.address).await
}

fn list_of(body: &Value, field: &str) -> Vec<Value> {
    match body {
        Value::Array(entries) => entries.clone(),
        other => other[field].as_array().cloned().unwrap_or_default(),
    }
}

fn naming(entries: &[Value]) -> Value {
    Value::Array(entries.iter().map(|entry| json!({"uuid": entry["uuid"], "name": entry["name"], "wildcard_domain": entry["wildcard_domain"]})).filter(|entry| entry["uuid"].is_string()).collect())
}

async fn options(http: &reqwest::Client, credentials: &Credentials) -> Outcome<Value> {
    let version = get(http, credentials, "version").await?;
    let projects = list_of(&get(http, credentials, "projects").await?, "projects");
    let mut servers = list_of(&get(http, credentials, "servers").await?, "servers");

    for server in servers.iter_mut() {
        let Some(uuid) = server["uuid"].as_str().map(str::to_string) else {
            continue;
        };
        let detail = get(http, credentials, &format!("servers/{uuid}")).await.unwrap_or(Value::Null);

        server["wildcard_domain"] = detail["settings"]["wildcard_domain"].clone();
    }

    Ok(json!({"host": host_of(&credentials.address), "version": version.as_str().unwrap_or_default().trim(), "projects": naming(&projects), "servers": naming(&servers)}))
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<Value, String> {
    let discovered = options(http, credentials).await.map_err(|failure| failure.message().to_string())?;
    if credentials.settings["project_uuid"].as_str().is_none_or(str::is_empty) {
        return Ok(json!({}));
    }

    let find = |list: &str, uuid: &str| discovered[list].as_array().and_then(|entries| entries.iter().find(|entry| entry["uuid"] == uuid)).cloned();
    let Some(project) = find("projects", credentials.settings["project_uuid"].as_str().unwrap_or_default()) else {
        return Err(format!("{} has no project with that uuid. Choose a project from the list.", host_of(&credentials.address)));
    };
    let Some(server) = find("servers", credentials.settings["server_uuid"].as_str().unwrap_or_default()) else {
        return Err(format!("{} has no server with that uuid. Choose a server from the list.", host_of(&credentials.address)));
    };

    Ok(json!({"project_name": project["name"], "server_name": server["name"], "wildcard_domain": server["wildcard_domain"], "environment_name": DEFAULT_ENVIRONMENT}))
}

pub async fn test(State(state): State<AppState>, _user: AuthUser, Json(request): Json<TestRequest>) -> ApiResult<Json<Value>> {
    let parsed = url::Url::parse(request.url.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("coolify_address", "Enter the address you open Coolify at, like https://coolify.example.com."));
    };
    let credentials = Credentials { address: parsed.as_str().to_string(), account: String::new(), secret: request.token.trim().to_string(), settings: Value::Null, session: None };

    match options(&state.http, &credentials).await {
        Ok(found) => Ok(Json(found)),
        Err(Failure::Refused(message)) => Err(ApiError::bad_request("coolify_token", message)),
        Err(failure) => Err(ApiError::bad_request("coolify_address", failure.message())),
    }
}

fn stays_in_api(path: &str) -> bool {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let plain = |part: &str| !part.is_empty() && part != ".." && part != "." && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    let allowed: &[&str] = match route {
        "deploy" => &["uuid", "force"],
        route if route.ends_with("/logs") => &["lines"],
        route if route.starts_with("applications/") && route.matches('/').count() == 1 => &["delete_configurations", "delete_volumes"],
        _ => &[],
    };
    let mut keys = Vec::new();
    let query_ok = query.split('&').filter(|pair| !pair.is_empty()).all(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let fresh = !keys.contains(&key);

        keys.push(key);

        fresh && allowed.contains(&key) && (value.is_empty() || plain(value))
    });

    route.split('/').all(plain) && query_ok
}

struct Account {
    http: reqwest::Client,
    credentials: Credentials,
    server_id: Uuid,
}

async fn account(state: &AppState, owner: Uuid, id: Option<Uuid>) -> ApiResult<(ServerRow, Account)> {
    let row = match id {
        Some(id) => sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2 and kind = 'coolify'"))).bind(id).bind(owner).fetch_optional(&state.pool).await?,
        None => sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where owner = $1 and kind = 'coolify' and enabled order by created_at limit 1"))).bind(owner).fetch_optional(&state.pool).await?,
    };
    let row = row.ok_or_else(|| ApiError::not_found("That Coolify account"))?;
    let handle = Account { http: state.http.clone(), credentials: row.credentials(), server_id: row.id };

    Ok((row, handle))
}

fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;

    DateTime::parse_from_rfc3339(text).map(|time| time.with_timezone(&Utc)).ok().or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f").ok().map(|time| time.and_utc()))
}

fn first_address(fqdn: &Value) -> Option<String> {
    fqdn.as_str().and_then(|text| text.split(',').map(str::trim).find(|address| !address.is_empty())).map(str::to_string)
}

fn app_status(raw: &str) -> &'static str {
    let state = raw.split(':').next().unwrap_or_default();

    match state {
        "running" => "running",
        "starting" | "restarting" | "in_progress" | "queued" => "deploying",
        "exited" | "stopped" | "paused" => "stopped",
        _ => "failed",
    }
}

fn deployment_result(raw: &str) -> &'static str {
    match raw {
        "finished" => "deployed",
        "failed" => "failed",
        "cancelled-by-user" | "cancelled" => "cancelled",
        _ => "deploying",
    }
}

fn slug(name: &str) -> String {
    let mapped: String = name.trim().to_lowercase().chars().map(|character| if character.is_ascii_alphanumeric() { character } else { '-' }).collect();

    mapped.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
}

fn address_for(name: &str, wildcard: &str) -> Option<String> {
    let scheme = if wildcard.starts_with("http://") { "http" } else { "https" };
    let host = wildcard.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("*.").trim_end_matches('/');
    let label = slug(name);

    match host.is_empty() || label.is_empty() {
        true => None,
        false => Some(format!("{scheme}://{label}.{host}")),
    }
}

impl Account {
    fn target(&self, field: &str) -> String {
        self.credentials.settings[field].as_str().unwrap_or_default().to_string()
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Outcome<Value> {
        if !stays_in_api(path) {
            return Err(Failure::Other("That id is not a Coolify id. Use the uuid from list_apps.".to_string()));
        }

        let builder = request(&self.http, &self.credentials, method, path);
        let builder = match body {
            Some(body) => builder.json(&body),
            None => builder,
        };

        send(builder, &self.credentials.address).await
    }

    fn scoped(&self) -> bool {
        !self.target("project_uuid").is_empty()
    }

    async fn project_environment_ids(&self) -> Outcome<Vec<i64>> {
        let environments = self.call(reqwest::Method::GET, &format!("projects/{}/environments", self.target("project_uuid")), None).await?;

        Ok(list_of(&environments, "environments").iter().filter_map(|environment| environment["id"].as_i64()).collect())
    }

    async fn applications(&self) -> Outcome<Vec<Value>> {
        let all = self.call(reqwest::Method::GET, "applications", None).await?;
        let apps = list_of(&all, "applications");

        if !self.scoped() {
            return Ok(apps);
        }

        let ids = self.project_environment_ids().await?;

        Ok(apps.into_iter().filter(|app| app["environment_id"].as_i64().is_some_and(|id| ids.contains(&id))).collect())
    }

    async fn project_names(&self) -> HashMap<i64, String> {
        let mut names = HashMap::new();
        let Ok(listed) = self.call(reqwest::Method::GET, "projects", None).await else {
            return names;
        };

        for project in list_of(&listed, "projects") {
            let (Some(uuid), Some(name)) = (project["uuid"].as_str(), project["name"].as_str()) else {
                continue;
            };
            let Ok(environments) = self.call(reqwest::Method::GET, &format!("projects/{uuid}/environments"), None).await else {
                continue;
            };

            for environment in list_of(&environments, "environments") {
                if let Some(id) = environment["id"].as_i64() {
                    names.insert(id, name.to_string());
                }
            }
        }

        names
    }

    async fn choose(&self, path: &str, noun: &str, wanted: Option<&str>, fallback_name: Option<&str>) -> Outcome<Value> {
        let body = self.call(reqwest::Method::GET, path, None).await?;
        let entries: Vec<Value> = list_of(&body, path).into_iter().filter(|entry| entry["uuid"].is_string()).collect();
        let names = entries.iter().filter_map(|entry| entry["name"].as_str()).collect::<Vec<_>>().join(", ");
        let matches_name = |entry: &Value, name: &str| entry["name"].as_str().is_some_and(|candidate| candidate.eq_ignore_ascii_case(name));

        let found = match (wanted, entries.len()) {
            (_, 0) => return Err(Failure::Other(format!("Coolify has no {noun}s yet. Create one in Coolify first."))),
            (Some(wanted), _) => entries.iter().find(|entry| entry["uuid"] == wanted || matches_name(entry, wanted)),
            (None, 1) => entries.first(),
            (None, _) => fallback_name.and_then(|name| entries.iter().find(|entry| matches_name(entry, name))),
        };

        match (found, wanted) {
            (Some(entry), _) => Ok(entry.clone()),
            (None, Some(wanted)) => Err(Failure::Other(format!("Coolify has no {noun} called {wanted}. Choose one of: {names}."))),
            (None, None) => Err(Failure::Other(format!("Say which {noun} to use in {noun}. The choices are: {names}."))),
        }
    }

    async fn application(&self, uuid: &str) -> Outcome<Value> {
        self.call(reqwest::Method::GET, &format!("applications/{uuid}"), None).await
    }

    async fn raw_deployments(&self, uuid: &str) -> Outcome<Vec<Value>> {
        let body = self.call(reqwest::Method::GET, &format!("deployments/applications/{uuid}"), None).await?;

        Ok(list_of(&body, "deployments"))
    }

    async fn recorded(&self, state: &AppState, uuid: &str) -> (Option<Uuid>, Vec<(String, String, Option<Uuid>, DateTime<Utc>)>) {
        let built_in = sqlx::query_scalar::<_, Option<Uuid>>("select conversation_id from coolify_apps where server_id = $1 and app_uuid = $2").bind(self.server_id).bind(uuid).fetch_optional(&state.pool).await.ok().flatten().flatten();
        let triggers = sqlx::query_as::<_, (String, String, Option<Uuid>, DateTime<Utc>)>("select deployment_uuid, triggered_by, conversation_id, created_at from coolify_deployments where server_id = $1 and app_uuid = $2")
            .bind(self.server_id)
            .bind(uuid)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

        (built_in, triggers)
    }

    async fn deployment_views(&self, state: &AppState, uuid: &str) -> Outcome<Vec<Value>> {
        let raw = self.raw_deployments(uuid).await?;
        let (_, triggers) = self.recorded(state, uuid).await;

        Ok(raw
            .iter()
            .map(|deployment| {
                let id = deployment["deployment_uuid"].as_str().unwrap_or_default();
                let trigger = triggers.iter().find(|(recorded, ..)| recorded == id);
                let by = match trigger {
                    Some((_, by, ..)) => by.as_str(),
                    None if deployment["is_webhook"] == json!(true) => "git",
                    None => "unknown",
                };
                let started = parse_time(&deployment["created_at"]);
                let result = deployment_result(deployment["status"].as_str().unwrap_or_default());
                let duration = match (started, parse_time(&deployment["updated_at"]), result) {
                    (Some(started), Some(finished), "deployed" | "failed" | "cancelled") => Some((finished - started).num_seconds().max(0)),
                    _ => None,
                };

                json!({
                    "uuid": id,
                    "started_at": deployment["created_at"],
                    "commit": deployment["commit"],
                    "commit_message": deployment["commit_message"],
                    "triggered_by": by,
                    "conversation_id": trigger.and_then(|(_, _, conversation, _)| *conversation),
                    "duration_seconds": duration,
                    "result": result,
                })
            })
            .collect())
    }

    async fn app_view(&self, state: &AppState, app: &Value, deployments: Option<&[Value]>, projects: &HashMap<i64, String>) -> Value {
        let uuid = app["uuid"].as_str().unwrap_or_default();
        let (built_in, triggers) = self.recorded(state, uuid).await;
        let mut status = app_status(app["status"].as_str().unwrap_or_default());
        let latest = deployments.and_then(|list| list.first());

        if let Some(latest) = latest {
            match latest["result"].as_str() {
                Some("deploying") => status = "deploying",
                Some("failed") if status != "running" => status = "failed",
                _ => {}
            }
        }

        let last_deploy = triggers.iter().map(|(.., at)| *at).max().map(|at| at.to_rfc3339()).or_else(|| app["updated_at"].as_str().map(str::to_string));

        json!({
            "uuid": uuid,
            "name": app["name"],
            "status": status,
            "address": first_address(&app["fqdn"]),
            "last_deploy_at": last_deploy,
            "conversation_id": built_in,
            "repository": app["git_repository"],
            "branch": app["git_branch"],
            "base_directory": app["base_directory"],
            "build_pack": app["build_pack"],
            "project": app["environment_id"].as_i64().and_then(|id| projects.get(&id)),
            "server": app["destination"]["server"]["name"],
            "created_at": app["created_at"],
            "latest_deployment": latest,
        })
    }

    async fn environment(&self, state: &AppState, uuid: &str) -> Outcome<Vec<Value>> {
        let body = self.call(reqwest::Method::GET, &format!("applications/{uuid}/envs"), None).await?;
        let changes = sqlx::query_as::<_, (String, String, DateTime<Utc>)>("select key, changed_by, changed_at from coolify_env_changes where server_id = $1 and app_uuid = $2").bind(self.server_id).bind(uuid).fetch_all(&state.pool).await.unwrap_or_default();
        let last_deploy = sqlx::query_scalar::<_, DateTime<Utc>>("select max(created_at) from coolify_deployments where server_id = $1 and app_uuid = $2").bind(self.server_id).bind(uuid).fetch_one(&state.pool).await.ok();

        Ok(list_of(&body, "envs")
            .iter()
            .filter(|env| env["is_preview"] != json!(true))
            .map(|env| {
                let secret = env["is_shown_once"] == json!(true);
                let key = env["key"].as_str().unwrap_or_default();
                let change = changes.iter().find(|(recorded, ..)| recorded == key);
                let changed_at = change.map(|(.., at)| *at).or_else(|| parse_time(&env["updated_at"]));
                let stale = match (change, last_deploy) {
                    (Some((.., at)), Some(deployed)) => *at > deployed,
                    _ => false,
                };

                json!({
                    "key": key,
                    "secret": secret,
                    "build_time": env["is_buildtime"] == json!(true),
                    "value": if secret { Value::Null } else { env["value"].clone() },
                    "changed_at": changed_at.map(|at| at.to_rfc3339()),
                    "changed_by": change.map(|(_, by, _)| by.clone()),
                    "stale": stale,
                })
            })
            .collect())
    }

    async fn set_env(&self, state: &AppState, uuid: &str, key: &str, value: &str, secret: bool, build_time: bool, by: &str) -> Outcome<()> {
        let existing = self.call(reqwest::Method::GET, &format!("applications/{uuid}/envs"), None).await?;
        let known = list_of(&existing, "envs").iter().any(|env| env["key"] == key && env["is_preview"] != json!(true));
        let method = if known { reqwest::Method::PATCH } else { reqwest::Method::POST };
        let mut body = json!({"key": key, "value": value, "is_shown_once": secret, "is_preview": false});

        if build_time {
            body["is_buildtime"] = json!(true);
        }

        let saved = self.call(method.clone(), &format!("applications/{uuid}/envs"), Some(body.clone())).await;

        if saved.is_err() && build_time {
            if let Some(fields) = body.as_object_mut() {
                fields.remove("is_buildtime");
            }

            self.call(method, &format!("applications/{uuid}/envs"), Some(body)).await?;
        } else {
            saved?;
        }

        sqlx::query("insert into coolify_env_changes (server_id, app_uuid, key, changed_by) values ($1, $2, $3, $4) on conflict (server_id, app_uuid, key) do update set changed_by = $4, changed_at = now()")
            .bind(self.server_id)
            .bind(uuid)
            .bind(key)
            .bind(by)
            .execute(&state.pool)
            .await
            .map_err(|error| Failure::Other(error.to_string()))?;

        Ok(())
    }

    async fn delete_env(&self, state: &AppState, uuid: &str, key: &str) -> Outcome<()> {
        let existing = self.call(reqwest::Method::GET, &format!("applications/{uuid}/envs"), None).await?;
        let found = list_of(&existing, "envs").into_iter().find(|env| env["key"] == key && env["is_preview"] != json!(true));
        let Some(env_uuid) = found.and_then(|env| env["uuid"].as_str().map(str::to_string)) else {
            return Err(Failure::Missing(format!("{uuid} has no variable called {key}.")));
        };

        self.call(reqwest::Method::DELETE, &format!("applications/{uuid}/envs/{env_uuid}"), None).await?;
        sqlx::query("delete from coolify_env_changes where server_id = $1 and app_uuid = $2 and key = $3").bind(self.server_id).bind(uuid).bind(key).execute(&state.pool).await.map_err(|error| Failure::Other(error.to_string()))?;

        Ok(())
    }

    async fn deploy(&self, state: &AppState, uuid: &str, by: &str, conversation: Option<Uuid>) -> Outcome<String> {
        let started = self.call(reqwest::Method::POST, &format!("deploy?uuid={uuid}&force=false"), None).await?;
        let deployment = started["deployments"].as_array().and_then(|list| list.first()).and_then(|entry| entry["deployment_uuid"].as_str()).map(str::to_string).or_else(|| started["deployment_uuid"].as_str().map(str::to_string));
        let Some(deployment) = deployment else {
            return Err(Failure::Other("Coolify did not say which deployment it queued.".to_string()));
        };

        sqlx::query("insert into coolify_deployments (server_id, deployment_uuid, app_uuid, triggered_by, conversation_id) values ($1, $2, $3, $4, $5) on conflict do nothing")
            .bind(self.server_id)
            .bind(&deployment)
            .bind(uuid)
            .bind(by)
            .bind(conversation)
            .execute(&state.pool)
            .await
            .map_err(|error| Failure::Other(error.to_string()))?;

        Ok(deployment)
    }

    async fn create_app(&self, state: &AppState, arguments: &Value, conversation: Option<Uuid>) -> Outcome<Value> {
        let Some(name) = arguments["name"].as_str().map(str::trim).filter(|name| !name.is_empty()) else {
            return Err(Failure::Other("Give the app a name.".to_string()));
        };
        let (project_uuid, server_uuid, wildcard, environment_name) = match self.scoped() {
            true => (self.target("project_uuid"), self.target("server_uuid"), self.target("wildcard_domain"), self.credentials.settings["environment_name"].as_str().unwrap_or(DEFAULT_ENVIRONMENT).to_string()),
            false => {
                let project = self.choose("projects", "project", arguments["project"].as_str().map(str::trim).filter(|wanted| !wanted.is_empty()), Some(DEFAULT_PROJECT)).await?;
                let server = self.choose("servers", "server", arguments["server"].as_str().map(str::trim).filter(|wanted| !wanted.is_empty()), None).await?;
                let server_uuid = server["uuid"].as_str().unwrap_or_default().to_string();
                let detail = self.call(reqwest::Method::GET, &format!("servers/{server_uuid}"), None).await.unwrap_or(Value::Null);

                let project_uuid = project["uuid"].as_str().unwrap_or_default().to_string();
                let environments = self.call(reqwest::Method::GET, &format!("projects/{project_uuid}/environments"), None).await?;
                let environments = list_of(&environments, "environments");
                let names = environments.iter().filter_map(|environment| environment["name"].as_str()).collect::<Vec<_>>();
                let wanted = arguments["environment"].as_str().map(str::trim).filter(|wanted| !wanted.is_empty());
                let same = |name: &&str, other: &str| name.eq_ignore_ascii_case(other);
                let environment = match wanted {
                    Some(wanted) => names.iter().find(|name| same(name, wanted)).ok_or_else(|| Failure::Other(format!("The project {} has no environment called {wanted}. Choose one of: {}.", project["name"].as_str().unwrap_or_default(), names.join(", "))))?,
                    None => names.iter().find(|name| same(name, DEFAULT_ENVIRONMENT)).or(names.first()).ok_or_else(|| Failure::Other(format!("The project {} has no environments. Create one in Coolify first.", project["name"].as_str().unwrap_or_default())))?,
                };

                (project_uuid, server_uuid, detail["settings"]["wildcard_domain"].as_str().unwrap_or_default().to_string(), environment.to_string())
            }
        };
        let address = address_for(name, &wildcard);
        let mut body = json!({
            "project_uuid": project_uuid,
            "server_uuid": server_uuid,
            "environment_name": environment_name,
            "name": name,
            "instant_deploy": false,
        });

        if let Some(address) = &address {
            body["domains"] = json!(address);
        }

        let created = match (arguments["docker_image"].as_str(), arguments["git_repository"].as_str()) {
            (Some(image), _) => {
                let (repository, tag) = image.rsplit_once(':').filter(|(_, tag)| !tag.contains('/')).unwrap_or((image, "latest"));

                body["docker_registry_image_name"] = json!(repository);
                body["docker_registry_image_tag"] = json!(tag);
                body["ports_exposes"] = json!(arguments["ports"].as_str().unwrap_or("80"));
                self.call(reqwest::Method::POST, "applications/dockerimage", Some(body)).await?
            }
            (None, Some(repository)) => {
                body["git_repository"] = json!(repository);
                body["git_branch"] = json!(arguments["git_branch"].as_str().unwrap_or("main"));
                body["build_pack"] = json!(arguments["build_pack"].as_str().unwrap_or("nixpacks"));
                body["ports_exposes"] = json!(arguments["ports"].as_str().unwrap_or("3000"));

                if let Some(directory) = arguments["base_directory"].as_str() {
                    body["base_directory"] = json!(directory);
                }

                match arguments["private_key_uuid"].as_str() {
                    Some(key) => {
                        body["private_key_uuid"] = json!(key);
                        self.call(reqwest::Method::POST, "applications/private-deploy-key", Some(body)).await?
                    }
                    None => self.call(reqwest::Method::POST, "applications/public", Some(body)).await?,
                }
            }
            (None, None) => return Err(Failure::Other("Give git_repository, or docker_image.".to_string())),
        };
        let Some(app_uuid) = created["uuid"].as_str().map(str::to_string) else {
            return Err(Failure::Other("Coolify did not say which app it created.".to_string()));
        };

        sqlx::query("insert into coolify_apps (server_id, app_uuid, conversation_id) values ($1, $2, $3) on conflict do nothing").bind(self.server_id).bind(&app_uuid).bind(conversation).execute(&state.pool).await.map_err(|error| Failure::Other(error.to_string()))?;

        let deployment = self.deploy(state, &app_uuid, "agent", conversation).await?;

        Ok(json!({"app_uuid": app_uuid, "app_name": name, "deployment_uuid": deployment, "address": address}))
    }

    async fn lines(&self, uuid: &str, kind: &str, deployment: Option<&str>, count: usize) -> Outcome<Vec<String>> {
        if kind == "runtime" {
            let body = self.call(reqwest::Method::GET, &format!("applications/{uuid}/logs?lines={count}"), None).await?;
            let text = body["logs"].as_str().unwrap_or_default();
            let all: Vec<String> = text.lines().map(str::to_string).collect();

            return Ok(all[all.len().saturating_sub(count)..].to_vec());
        }

        let id = match deployment {
            Some(id) => id.to_string(),
            None => match self.raw_deployments(uuid).await?.first().and_then(|latest| latest["deployment_uuid"].as_str()) {
                Some(id) => id.to_string(),
                None => return Ok(Vec::new()),
            },
        };
        let detail = self.call(reqwest::Method::GET, &format!("deployments/{id}"), None).await?;
        let all: Vec<String> = log_entries(&detail).iter().map(|(_, line)| line.clone()).collect();

        Ok(all[all.len().saturating_sub(count)..].to_vec())
    }
}

fn log_entries(deployment: &Value) -> Vec<(Option<DateTime<Utc>>, String)> {
    let parsed: Value = match &deployment["logs"] {
        Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| Value::Array(text.lines().map(|line| json!({"output": line})).collect())),
        other => other.clone(),
    };

    parsed.as_array().into_iter().flatten().filter(|entry| entry["hidden"] != json!(true)).filter_map(|entry| entry["output"].as_str().map(|line| (parse_time(&entry["timestamp"]), line.to_string()))).collect()
}

fn deploy_steps(deployment: &Value) -> Vec<Value> {
    let entries = log_entries(deployment);
    let result = deployment_result(deployment["status"].as_str().unwrap_or_default());
    let commit: String = deployment["commit"].as_str().unwrap_or_default().chars().take(7).collect();
    let markers: [(&str, &[&str]); 3] = [("Cloned", &["importing", "cloning"]), ("Building", &["building", "nixpacks", "docker build"]), ("Start and health check", &["starting new application", "rolling update", "new container"])];
    let created = parse_time(&deployment["created_at"]);
    let mut starts: Vec<Option<DateTime<Utc>>> = vec![created];
    let mut reached = 1;

    for (_, needles) in markers {
        let hit = entries.iter().find(|(_, line)| needles.iter().any(|needle| line.to_lowercase().contains(needle)));

        match hit {
            Some((at, _)) => {
                starts.push(*at);
                reached += 1;
            }
            None => break,
        }
    }

    let finished = parse_time(&deployment["updated_at"]);
    let labels = ["Created".to_string(), if commit.is_empty() { "Cloned".to_string() } else { format!("Cloned at {commit}") }, "Building".to_string(), "Start and health check".to_string()];

    labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let state = match (result, index < reached) {
                ("deployed", _) => "done",
                (_, false) => "pending",
                ("deploying", true) if index + 1 == reached => "running",
                ("failed" | "cancelled", true) if index + 1 == reached => "failed",
                _ => "done",
            };
            let end = starts.get(index + 1).copied().flatten().or(if index + 1 == reached && result != "deploying" { finished } else { None });
            let seconds = match (starts.get(index).copied().flatten(), end, state) {
                (Some(start), Some(end), "done" | "failed") => Some((end - start).num_seconds().max(0)),
                _ => None,
            };

            json!({"label": label, "state": state, "duration_seconds": seconds})
        })
        .collect()
}

async fn deployment_card(state: &AppState, account: &Account, uuid: &str) -> Outcome<Value> {
    let detail = account.call(reqwest::Method::GET, &format!("deployments/{uuid}"), None).await?;
    let app_uuid = detail["application_id"].as_str().map(str::to_string);
    let recorded = sqlx::query_scalar::<_, String>("select app_uuid from coolify_deployments where server_id = $1 and deployment_uuid = $2").bind(account.server_id).bind(uuid).fetch_optional(&state.pool).await.ok().flatten();
    let app_uuid = recorded.or(app_uuid).unwrap_or_default();
    let app = account.application(&app_uuid).await.unwrap_or(Value::Null);
    let status = deployment_result(detail["status"].as_str().unwrap_or_default());
    let tail: Vec<String> = log_entries(&detail).into_iter().map(|(_, line)| line).collect();

    Ok(json!({
        "uuid": uuid,
        "app_uuid": app_uuid,
        "app_name": detail["application_name"].as_str().map(str::to_string).or_else(|| app["name"].as_str().map(str::to_string)),
        "status": status,
        "steps": deploy_steps(&detail),
        "commit": detail["commit"],
        "address": first_address(&app["fqdn"]),
        "log_tail": &tail[tail.len().saturating_sub(TAIL_LINES)..],
    }))
}

pub async fn call(state: &AppState, _owner: Uuid, server: &ServerRow, credentials: &Credentials, conversation: Option<Uuid>, tool: &str, arguments: &Value) -> CallOutput {
    let handle = Account { http: state.http.clone(), credentials: Credentials { address: credentials.address.clone(), account: String::new(), secret: credentials.secret.clone(), settings: credentials.settings.clone(), session: None }, server_id: server.id };
    let app = arguments["app"].as_str().map(str::trim).filter(|uuid| !uuid.is_empty()).map(str::to_string);
    let outcome: Outcome<Value> = async {
        match tool {
            "list_apps" => {
                let apps = handle.applications().await?;
                let projects = handle.project_names().await;
                let mut views = Vec::new();

                for entry in &apps {
                    views.push(handle.app_view(state, entry, None, &projects).await);
                }

                Ok(Value::Array(views))
            }
            "list_projects" => Ok(naming(&list_of(&handle.call(reqwest::Method::GET, "projects", None).await?, "projects"))),
            "list_servers" => Ok(naming(&list_of(&handle.call(reqwest::Method::GET, "servers", None).await?, "servers"))),
            CREATE_APP => handle.create_app(state, arguments, conversation).await,
            _ => {
                let Some(uuid) = app else {
                    return Err(Failure::Other("Give the app's uuid in app.".to_string()));
                };

                match tool {
                    "get_app" => {
                        let entry = handle.application(&uuid).await?;
                        let deployments = handle.deployment_views(state, &uuid).await.unwrap_or_default();

                        Ok(handle.app_view(state, &entry, Some(deployments.as_slice()), &handle.project_names().await).await)
                    }
                    "list_deployments" => Ok(Value::Array(handle.deployment_views(state, &uuid).await?)),
                    "get_logs" => {
                        let kind = if arguments["kind"].as_str() == Some("build") { "build" } else { "runtime" };
                        let count = arguments["lines"].as_u64().unwrap_or(DEFAULT_LOG_LINES).clamp(1, MAX_LOG_LINES) as usize;
                        let lines = handle.lines(&uuid, kind, arguments["deployment"].as_str(), count).await.map_err(|failure| match failure {
                            Failure::Refused(_) if kind == "runtime" => Failure::Other("Runtime logs need a Coolify token with the read:sensitive permission. Create one in Coolify under Keys & Tokens, then replace this account's token.".to_string()),
                            other => other,
                        })?;

                        Ok(json!({"kind": kind, "lines": lines}))
                    }
                    DEPLOY => {
                        let deployment = handle.deploy(state, &uuid, "agent", conversation).await?;
                        let entry = handle.application(&uuid).await.unwrap_or(Value::Null);

                        Ok(json!({"app_uuid": uuid, "app_name": entry["name"], "deployment_uuid": deployment, "address": first_address(&entry["fqdn"])}))
                    }
                    STOP | START => {
                        handle.call(reqwest::Method::POST, &format!("applications/{uuid}/{tool}"), None).await?;

                        Ok(json!({"app_uuid": uuid, "requested": tool}))
                    }
                    DELETE_APP => {
                        handle.call(reqwest::Method::DELETE, &format!("applications/{uuid}?delete_configurations=true&delete_volumes=true"), None).await?;
                        forget(state, &handle, &uuid).await;

                        Ok(json!({"app_uuid": uuid, "deleted": true}))
                    }
                    SET_ENV => {
                        let (Some(key), Some(value)) = (arguments["key"].as_str().map(str::trim).filter(|key| !key.is_empty()), arguments["value"].as_str()) else {
                            return Err(Failure::Other("Give key and value.".to_string()));
                        };

                        handle.set_env(state, &uuid, key, value, arguments["secret"].as_bool().unwrap_or(false), arguments["build_time"].as_bool().unwrap_or(false), "agent").await?;

                        Ok(json!({"app_uuid": uuid, "key": key, "saved": true, "applies": "at the next deploy"}))
                    }
                    DELETE_ENV => {
                        let Some(key) = arguments["key"].as_str().map(str::trim).filter(|key| !key.is_empty()) else {
                            return Err(Failure::Other("Give the variable's name in key.".to_string()));
                        };

                        handle.delete_env(state, &uuid, key).await?;

                        Ok(json!({"app_uuid": uuid, "key": key, "deleted": true}))
                    }
                    _ => Err(Failure::Other(format!("Coolify has no tool called {tool}."))),
                }
            }
        }
    }
    .await;

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(failure) => CallOutput { text: failure.message().to_string(), is_error: true },
    }
}

async fn forget(state: &AppState, handle: &Account, uuid: &str) {
    for table in ["coolify_apps", "coolify_deployments", "coolify_env_changes"] {
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!("delete from {table} where server_id = $1 and app_uuid = $2"))).bind(handle.server_id).bind(uuid).execute(&state.pool).await;
    }
}

fn failed(failure: Failure) -> ApiError {
    match failure {
        Failure::Missing(message) => ApiError::new(StatusCode::NOT_FOUND, "not_found", message),
        other => ApiError::bad_request("coolify_unavailable", other.message()),
    }
}

pub async fn target_options(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, Some(id)).await?;

    Ok(Json(options(&handle.http, &handle.credentials).await.map_err(failed)?))
}

pub async fn set_target(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<TargetRequest>) -> ApiResult<Json<Value>> {
    let (row, handle) = account(&state, user.id, Some(id)).await?;
    let chosen = normalise_settings(&json!({"project_uuid": request.project_uuid, "server_uuid": request.server_uuid})).map_err(|reason| ApiError::bad_request("settings_invalid", reason))?;
    let credentials = Credentials { address: handle.credentials.address.clone(), account: String::new(), secret: handle.credentials.secret.clone(), settings: chosen.clone(), session: None };
    let discovered = verify(&handle.http, &credentials).await.map_err(|reason| ApiError::bad_request("tool_server_failed", reason))?;
    let mut settings = row.settings.0.clone();

    if let (Some(fields), Some(chosen), Some(discovered)) = (settings.as_object_mut(), chosen.as_object(), discovered.as_object()) {
        for key in SCOPE_KEYS {
            fields.remove(key);
        }

        fields.extend(chosen.clone());
        fields.extend(discovered.clone());
    }

    sqlx::query("update chat_tool_servers set settings = $3 where id = $1 and owner = $2").bind(id).bind(user.id).bind(sqlx::types::Json(settings)).execute(&state.pool).await?;
    state.hub.tools_changed(user.id);

    Ok(Json(serde_json::to_value(crate::chat_tool_servers::reloaded(&state, user.id, id).await?).map_err(ApiError::internal)?))
}

pub async fn list_apps(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let Ok((_, handle)) = account(&state, user.id, None).await else {
        return Ok(Json(json!({"apps": []})));
    };
    let apps = handle.applications().await.map_err(failed)?;
    let projects = handle.project_names().await;
    let mut views = Vec::new();

    for entry in &apps {
        views.push(handle.app_view(&state, entry, None, &projects).await);
    }

    Ok(Json(json!({"apps": views})))
}

pub async fn get_app(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;
    let entry = handle.application(&uuid).await.map_err(failed)?;
    let deployments = handle.deployment_views(&state, &uuid).await.unwrap_or_default();

    Ok(Json(handle.app_view(&state, &entry, Some(deployments.as_slice()), &handle.project_names().await).await))
}

pub async fn deployments(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;

    Ok(Json(Value::Array(handle.deployment_views(&state, &uuid).await.map_err(failed)?)))
}

pub async fn logs(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>, Query(query): Query<LogsQuery>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;
    let kind = if query.kind.as_deref() == Some("build") { "build" } else { "runtime" };
    let count = query.lines.unwrap_or(DEFAULT_LOG_LINES).clamp(1, MAX_LOG_LINES) as usize;

    match handle.lines(&uuid, kind, query.deployment.as_deref(), count).await {
        Ok(lines) => Ok(Json(json!({"lines": lines, "available": true, "message": null}))),
        Err(Failure::Refused(_)) if kind == "runtime" => Ok(Json(json!({"lines": [], "available": false, "message": "Runtime logs need a Coolify token with the read:sensitive permission. Replace the token with one that has it."}))),
        Err(other) => Err(failed(other)),
    }
}

pub async fn envs(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;

    Ok(Json(Value::Array(handle.environment(&state, &uuid).await.map_err(failed)?)))
}

pub async fn put_env(State(state): State<AppState>, user: AuthUser, Path((uuid, key)): Path<(String, String)>, Json(request): Json<EnvRequest>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;

    if request.value.is_empty() {
        return Err(ApiError::bad_request("value_required", format!("Enter a value for {key}")));
    }

    handle.set_env(&state, &uuid, &key, &request.value, request.secret, request.build_time, "person").await.map_err(failed)?;

    let saved = handle.environment(&state, &uuid).await.map_err(failed)?.into_iter().find(|env| env["key"] == key);

    Ok(Json(saved.unwrap_or(Value::Null)))
}

pub async fn delete_env(State(state): State<AppState>, user: AuthUser, Path((uuid, key)): Path<(String, String)>) -> ApiResult<StatusCode> {
    let (_, handle) = account(&state, user.id, None).await?;

    handle.delete_env(&state, &uuid, &key).await.map_err(failed)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn redeploy(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;
    let deployment = handle.deploy(&state, &uuid, "person", None).await.map_err(failed)?;

    Ok(Json(json!({"deployment_uuid": deployment})))
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<StatusCode> {
    let (_, handle) = account(&state, user.id, None).await?;

    handle.call(reqwest::Method::POST, &format!("applications/{uuid}/start"), None).await.map_err(failed)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<StatusCode> {
    let (_, handle) = account(&state, user.id, None).await?;

    handle.call(reqwest::Method::POST, &format!("applications/{uuid}/stop"), None).await.map_err(failed)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_app(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<StatusCode> {
    let (_, handle) = account(&state, user.id, None).await?;

    handle.call(reqwest::Method::DELETE, &format!("applications/{uuid}?delete_configurations=true&delete_volumes=true"), None).await.map_err(failed)?;
    forget(&state, &handle, &uuid).await;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn deployment(State(state): State<AppState>, user: AuthUser, Path(uuid): Path<String>) -> ApiResult<Json<Value>> {
    let (_, handle) = account(&state, user.id, None).await?;

    Ok(Json(deployment_card(&state, &handle, &uuid).await.map_err(failed)?))
}
