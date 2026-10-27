use crate::auth::AuthUser;
use crate::chat_tool_servers::{ApiSurface, ServerRow, bearer, known_app, list_owned};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::IntegrationApiGrant;
use std::time::Duration;
use uuid::Uuid;

pub const SCOPES: [&str; 2] = ["read", "write"];

const PROXY_TIMEOUT: Duration = Duration::from_secs(120);

pub fn token_scope(server_id: Uuid, scope: &str) -> String {
    format!("integration:{server_id}:{scope}")
}

pub fn server_of(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/api/v1/integrations/")?;
    let (id, after) = rest.split_once('/')?;

    after.starts_with("proxy").then(|| Uuid::parse_str(id).ok()).flatten()
}

fn surface(server: &ServerRow) -> Option<&'static ApiSurface> {
    if server.kind != "mcp" {
        return None;
    }

    known_app(server.settings["app"].as_str()?)?.api.as_ref()
}

pub fn api_base(server: &ServerRow) -> Option<String> {
    let mut base = url::Url::parse(&server.url).ok()?;
    base.set_path(surface(server)?.path);
    base.set_query(None);
    base.set_fragment(None);

    Some(base.to_string())
}

pub async fn find_with_api(state: &AppState, owner: Uuid, wanted: &str) -> ApiResult<Result<Uuid, String>> {
    let wanted = wanted.trim();
    let servers = list_owned(state, owner).await?;
    let found = servers.iter().find(|server| server.id.to_string() == wanted || server.name.eq_ignore_ascii_case(wanted));
    let Some(server) = found else {
        return Ok(Err(format!("Nothing was requested. There is no integration {wanted:?}. Use an account's name or id.")));
    };

    if api_base(server).is_none() {
        return Ok(Err(format!("Nothing was requested. {} does not offer API access, so use its tools.", server.name)));
    }

    Ok(Ok(server.id))
}

pub fn granted_text(base: &str, server_id: Uuid, token_path: &str, scopes: &[String], expires_at: chrono::DateTime<chrono::Utc>) -> String {
    let kinds: Vec<&str> = scopes.iter().filter_map(|scope| scope.rsplit(':').next()).collect();

    format!(
        "I approved API access to that integration until {} UTC, for {}. The token is in the file {token_path}. Read it from there when you need it and never copy it into a message, a file or a commit. Send requests with an Authorization: Bearer header to {base}/api/v1/integrations/{server_id}/proxy/ followed by the integration's own API path; Mycelium adds the integration's credentials. It stops working at that time, or sooner if I revoke it.",
        expires_at.format("%Y-%m-%d %H:%M"),
        kinds.join(", ")
    )
}

fn is_read(surface: &ApiSurface, method: &Method, path: &str) -> bool {
    if method == Method::GET || method == Method::HEAD {
        return true;
    }

    method == Method::POST && path.rsplit('.').next().is_some_and(|action| surface.post_reads.contains(&action.trim_end_matches('/')))
}

fn climbs_out(path: &str) -> bool {
    let decoded = path.replace("%2e", ".").replace("%2E", ".").replace("%2f", "/").replace("%2F", "/").replace("%5c", "\\").replace("%5C", "\\");

    decoded.split(['/', '\\']).any(|segment| segment == "..")
}

fn permitted(user: &AuthUser, server_id: Uuid, reading: bool) -> bool {
    let Some(scopes) = &user.scopes else {
        return true;
    };

    scopes.iter().any(|scope| scope == &token_scope(server_id, "write") || reading && scope == &token_scope(server_id, "read"))
}

async fn record(state: &AppState, owner: Uuid, server: &ServerRow, tool: &str, reading: bool, is_error: bool) {
    let recorded = sqlx::query("insert into integration_calls (owner, server_id, server_name, tool, kind, surface, decision, is_error) values ($1, $2, $3, $4, $5, 'workspace', 'auto', $6)")
        .bind(owner)
        .bind(server.id)
        .bind(&server.name)
        .bind(tool)
        .bind(if reading { "read" } else { "write" })
        .bind(is_error)
        .execute(&state.pool)
        .await;

    if let Err(error) = recorded {
        tracing::warn!("could not record an integration API call: {error}");
    }
}

pub async fn proxy(State(state): State<AppState>, user: AuthUser, Path((server_id, path)): Path<(Uuid, String)>, method: Method, RawQuery(query): RawQuery, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let servers = list_owned(&state, user.id).await?;
    let server = servers.iter().find(|server| server.id == server_id).ok_or_else(|| ApiError::not_found("That integration"))?;
    let Some(base) = api_base(server) else {
        return Err(ApiError::bad_request("api_unavailable", "This integration does not offer API access."));
    };
    let Some(surface) = surface(server) else {
        return Err(ApiError::bad_request("api_unavailable", "This integration does not offer API access."));
    };

    if climbs_out(&path) {
        return Err(ApiError::bad_request("path_not_allowed", "That path leaves the integration's API. Use a path inside it."));
    }

    let reading = is_read(surface, &method, &path);

    if !permitted(&user, server_id, reading) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "scope_not_granted", "This token allows reads only. Ask the person for write access."));
    }

    let credential = bearer(&state, server).await?.unwrap_or_default();
    let target = match &query {
        Some(query) => format!("{base}{path}?{query}"),
        None => format!("{base}{path}"),
    };
    let mut request = state.http.request(method.clone(), target).timeout(PROXY_TIMEOUT).bearer_auth(credential).body(body.to_vec());

    for name in [header::CONTENT_TYPE, header::ACCEPT] {
        if let Some(value) = headers.get(&name) {
            request = request.header(name, value);
        }
    }

    let tool = format!("api:{} {}", method, path);
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            record(&state, user.id, server, &tool, reading, true).await;

            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "integration_unreachable", format!("Mycelium could not reach {}.", server.name)));
        }
    };
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = response.bytes().await.map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "integration_unreachable", format!("{} stopped answering part way.", server.name)))?;

    record(&state, user.id, server, &tool, reading, !status.is_success()).await;

    let mut answer = (status, bytes).into_response();

    if let Some(content_type) = content_type {
        answer.headers_mut().insert(header::CONTENT_TYPE, content_type);
    }

    Ok(answer)
}

#[derive(sqlx::FromRow)]
struct GrantRow {
    token_id: Uuid,
    conversation_id: Option<Uuid>,
    conversation_title: Option<String>,
    scopes: Vec<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
    calls: i64,
}

pub async fn grants(State(state): State<AppState>, user: AuthUser, Path(server_id): Path<Uuid>) -> ApiResult<Json<Vec<IntegrationApiGrant>>> {
    let rows = sqlx::query_as::<_, GrantRow>(
        "select t.id as token_id, t.conversation_id, c.title as conversation_title, r.scopes, t.expires_at, (select count(*) from api_token_calls k where k.token_id = t.id) as calls
         from api_access_requests r join client_tokens t on t.id = r.token_id left join conversations c on c.id = t.conversation_id
         where r.owner = $1 and r.server_id = $2 and r.state = 'approved' and t.revoked_at is null and t.expires_at > now()
         order by t.expires_at",
    )
    .bind(user.id)
    .bind(server_id)
    .fetch_all(&state.pool)
    .await?;
    let grants = rows.into_iter().map(|row| IntegrationApiGrant { token_id: row.token_id, conversation_id: row.conversation_id, conversation_title: row.conversation_title, scopes: row.scopes, expires_at: row.expires_at, calls: row.calls }).collect();

    Ok(Json(grants))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTLINE: ApiSurface = ApiSurface { path: "/api/", post_reads: &["list", "info", "search", "search_titles", "drafts", "viewed", "archived", "deleted"] };

    #[test]
    fn outline_post_endpoints_that_only_read_count_as_reads() {
        assert!(is_read(&OUTLINE, &Method::POST, "documents.list"));
        assert!(is_read(&OUTLINE, &Method::POST, "documents.search"));
        assert!(!is_read(&OUTLINE, &Method::POST, "documents.create"));
        assert!(!is_read(&OUTLINE, &Method::DELETE, "documents.info"));
        assert!(is_read(&OUTLINE, &Method::GET, "anything"));
    }

    #[test]
    fn only_proxy_paths_name_a_server() {
        let id = Uuid::new_v4();

        assert_eq!(server_of(&format!("/api/v1/integrations/{id}/proxy/documents.list")), Some(id));
        assert_eq!(server_of(&format!("/api/v1/chat-tool-servers/{id}")), None);
    }

    #[test]
    fn dot_segments_are_refused_however_they_are_written() {
        assert!(climbs_out("../auth.info"));
        assert!(climbs_out("documents.list/../../auth.info"));
        assert!(climbs_out("%2e%2e/auth.info"));
        assert!(climbs_out("a%2F..%2Fb"));
        assert!(!climbs_out("documents.list"));
        assert!(!climbs_out("files/a..b"));
    }
}
