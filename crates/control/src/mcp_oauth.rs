use crate::auth::AuthUser;
use crate::chat_tool_servers::{DEFAULT_KEY_HEADER, SERVER_COLUMNS, ServerRow, check_name, fetch_tools, known_app};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, PendingOAuth, new_token};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use mycelium_proto::api::{OAuthStart, StartChatToolServerOAuthRequest};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration as StdDuration;
use uuid::Uuid;

const DISCOVERY_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const NOT_ASKED: &str = "This server does not ask for sign-in.";
const NO_AUTHORISATION_DETAILS: &str = "The server asks for sign-in but publishes no authorisation details.";
const UNSUPPORTED: &str = "This app does not let Mycelium sign in. Use an API key instead.";
const GITHUB_USES_TOKEN: &str = "GitHub's MCP server uses the token you connected the forge with. Reconnect the forge with a fine-grained token that has the MCP permission.";

pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub(crate) struct Authorisation {
    pub issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    metadata_document_supported: bool,
}

pub(crate) struct Discovered {
    pub authorisation: Authorisation,
    scopes: Vec<String>,
}

#[derive(Default)]
pub struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

impl Authorisation {
    pub(crate) fn sign_in(&self) -> &'static str {
        match (self.metadata_document_supported, &self.registration_endpoint) {
            (true, _) => "metadata_document",
            (false, Some(_)) => "registration",
            (false, None) => "client_id",
        }
    }
}

fn tokens_from(body: &Value) -> Option<Tokens> {
    let access_token = body["access_token"].as_str().filter(|token| !token.is_empty())?.to_string();
    let expires_at = body["expires_in"].as_i64().map(|seconds| Utc::now() + Duration::seconds(seconds));

    Some(Tokens { access_token, refresh_token: body["refresh_token"].as_str().map(str::to_string), expires_at })
}

async fn token_request(state: &AppState, endpoint: &str, form: &[(&str, &str)]) -> Result<Tokens, String> {
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
    let response = state
        .http
        .post(endpoint)
        .timeout(DISCOVERY_TIMEOUT)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        return Err(format!("The app answered {}.", response.status()));
    }

    let answer: Value = response.json().await.map_err(|error| error.to_string())?;

    tokens_from(&answer).ok_or_else(|| "The app sent no access token.".to_string())
}

pub async fn refresh(state: &AppState, oauth: &Value, refresh_token: &str) -> Result<Tokens, String> {
    let endpoint = oauth["token_endpoint"].as_str().ok_or("No token endpoint is stored.")?;
    let form = [("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", oauth["client_id"].as_str().unwrap_or_default()), ("resource", oauth["resource"].as_str().unwrap_or_default())];

    token_request(state, endpoint, &form).await
}

async fn get_json(state: &AppState, url: &str) -> Option<Value> {
    let response = state.http.get(url).timeout(DISCOVERY_TIMEOUT).header("accept", "application/json").send().await.ok()?;

    if !response.status().is_success() {
        return None;
    }

    response.json().await.ok()
}

fn well_known(address: &url::Url, name: &str) -> String {
    let path = address.path().trim_end_matches('/');
    let mut origin = address.clone();
    origin.set_path(&format!("/.well-known/{name}{path}"));
    origin.set_query(None);
    origin.set_fragment(None);

    origin.to_string()
}

pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let lowered = header.to_ascii_lowercase();
    let start = lowered.match_indices("bearer").find_map(|(index, _)| {
        let before = lowered[..index].chars().next_back();
        let after = lowered[index + 6..].chars().next();

        (before.is_none_or(|character| character.is_whitespace() || character == ',') && after.is_none_or(char::is_whitespace)).then_some(index + 6)
    })?;
    let mut rest = &header[start..];
    let mut challenge = Challenge::default();

    loop {
        rest = rest.trim_start_matches(|character: char| character.is_whitespace() || character == ',');

        let Some(equals) = rest.find('=') else {
            break;
        };
        let name = rest[..equals].trim();

        if name.is_empty() || name.contains(|character: char| character.is_whitespace() || character == ',') {
            break;
        }

        rest = rest[equals + 1..].trim_start();

        let value = match rest.strip_prefix('"') {
            Some(quoted) => {
                let mut text = String::new();
                let mut characters = quoted.char_indices();
                let mut end = quoted.len();

                while let Some((index, character)) = characters.next() {
                    match character {
                        '\\' => text.extend(characters.next().map(|(_, escaped)| escaped)),
                        '"' => {
                            end = index + 1;
                            break;
                        }
                        other => text.push(other),
                    }
                }

                rest = &quoted[end..];
                text
            }
            None => {
                let end = rest.find(',').unwrap_or(rest.len());
                let text = rest[..end].trim().to_string();
                rest = &rest[end..];
                text
            }
        };

        match name.to_ascii_lowercase().as_str() {
            "resource_metadata" => challenge.resource_metadata = Some(value),
            "scope" => challenge.scope = Some(value),
            "error" => challenge.error = Some(value),
            _ => {}
        }
    }

    Some(challenge)
}

pub fn challenge_of(headers: &axum::http::HeaderMap) -> Option<Challenge> {
    headers.get_all("www-authenticate").iter().filter_map(|value| value.to_str().ok()).find_map(parse_challenge)
}

async fn authorisation_server_metadata(state: &AppState, issuer: &str) -> Option<Authorisation> {
    let address = url::Url::parse(issuer).ok()?;
    let path = address.path().trim_end_matches('/');
    let mut candidates = vec![well_known(&address, "oauth-authorization-server"), well_known(&address, "openid-configuration")];

    if !path.is_empty() {
        let mut appended = address.clone();
        appended.set_path(&format!("{path}/.well-known/openid-configuration"));
        appended.set_query(None);
        appended.set_fragment(None);
        candidates.push(appended.to_string());
    }

    for candidate in candidates {
        let Some(metadata) = get_json(state, &candidate).await else {
            continue;
        };
        let text = |key: &str| metadata[key].as_str().filter(|value| !value.is_empty()).map(str::to_string);
        let pkce_refused = metadata["code_challenge_methods_supported"].as_array().is_some_and(|methods| !methods.iter().any(|method| method == "S256"));

        if metadata["issuer"].as_str() != Some(issuer) || pkce_refused {
            continue;
        }

        let (Some(authorization_endpoint), Some(token_endpoint)) = (text("authorization_endpoint"), text("token_endpoint")) else {
            continue;
        };

        return Some(Authorisation { issuer: issuer.to_string(), authorization_endpoint, token_endpoint, registration_endpoint: text("registration_endpoint"), metadata_document_supported: metadata["client_id_metadata_document_supported"].as_bool() == Some(true) });
    }

    None
}

pub(crate) async fn discover(state: &AppState, mcp_url: &url::Url) -> Result<Discovered, &'static str> {
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "mycelium", "version": "1"}}});
    let response = state.http.post(mcp_url.as_str()).timeout(DISCOVERY_TIMEOUT).header("accept", "application/json, text/event-stream").json(&initialize).send().await.ok();
    let rejected = response.as_ref().is_some_and(|response| response.status() == StatusCode::UNAUTHORIZED);
    let challenge = response.as_ref().and_then(|response| challenge_of(response.headers())).unwrap_or_default();
    let mut root = mcp_url.clone();
    root.set_path("");

    let mut candidates: Vec<String> = challenge.resource_metadata.iter().cloned().collect();
    candidates.push(well_known(mcp_url, "oauth-protected-resource"));
    candidates.push(well_known(&root, "oauth-protected-resource"));
    candidates.dedup();

    let mut protected = None;

    for candidate in candidates {
        protected = get_json(state, &candidate).await;

        if protected.is_some() {
            break;
        }
    }

    let reason = if rejected || protected.is_some() { NO_AUTHORISATION_DETAILS } else { NOT_ASKED };
    let issuer = match protected.as_ref().and_then(|body| body["authorization_servers"][0].as_str()) {
        Some(issuer) => issuer.to_string(),
        None if rejected => root.origin().ascii_serialization(),
        None => return Err(reason),
    };
    let authorisation = authorisation_server_metadata(state, &issuer).await.ok_or(reason)?;
    let scopes = match challenge.scope {
        Some(scope) => scope.split_whitespace().map(str::to_string).collect(),
        None => protected.as_ref().and_then(|body| body["scopes_supported"].as_array()).map(|scopes| scopes.iter().filter_map(|scope| scope.as_str().map(str::to_string)).collect()).unwrap_or_default(),
    };

    Ok(Discovered { authorisation, scopes })
}

fn redirect_uri(state: &AppState) -> String {
    format!("{}/integrations/oauth/callback", state.config.public_url)
}

fn client_metadata_url(state: &AppState) -> String {
    format!("{}/integrations/oauth/client.json", state.config.public_url)
}

pub async fn client_metadata(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"client_id": client_metadata_url(&state), "client_name": "Mycelium", "redirect_uris": [redirect_uri(&state)], "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"], "token_endpoint_auth_method": "none"}))
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Json(request): Json<StartChatToolServerOAuthRequest>) -> ApiResult<Json<OAuthStart>> {
    let name = request.name.trim();
    let kind = request.kind.as_deref().map(str::trim).filter(|kind| !kind.is_empty()).unwrap_or("mcp");

    if let Some(server) = request.server_id {
        sqlx::query_scalar::<_, Uuid>("select id from chat_tool_servers where id = $1 and owner = $2").bind(server).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That tool server"))?;
    }

    check_name(&state, user.id, name, request.server_id).await?;

    if let Some(forge_id) = request.forge_id {
        let forge = crate::forges::find_owned(&state, forge_id, user.id).await?;

        if forge.kind == "github" {
            return Err(ApiError::bad_request("oauth_unsupported", GITHUB_USES_TOKEN));
        }
    }

    let app = known_app(kind).filter(|app| app.stored_kind == "mcp");

    if kind != "mcp" && app.is_none() {
        return Err(ApiError::bad_request("kind_invalid", "Sign in with OAuth is for MCP servers and Outline."));
    }

    let parsed = url::Url::parse(request.url.trim()).ok().filter(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some());
    let Some(parsed) = parsed else {
        return Err(ApiError::bad_request("url_invalid", "Enter the server's address, starting with http:// or https://, for example https://example.com/mcp."));
    };
    let resource = app.map_or_else(|| parsed.as_str().to_string(), |app| app.address(&parsed));
    let mcp_url = url::Url::parse(&resource).map_err(ApiError::internal)?;
    let unsupported = || ApiError::bad_request("oauth_unsupported", UNSUPPORTED);
    let Discovered { authorisation, scopes } = discover(&state, &mcp_url).await.map_err(|_| unsupported())?;

    let redirect = redirect_uri(&state);
    let client_id = match (authorisation.metadata_document_supported, authorisation.registration_endpoint.as_deref()) {
        (true, _) => client_metadata_url(&state),
        (false, None) => return Err(unsupported()),
        (false, Some(registration_endpoint)) => {
            let registration = json!({"client_name": "Mycelium", "redirect_uris": [redirect], "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]});
            let registered = state.http.post(registration_endpoint).timeout(DISCOVERY_TIMEOUT).json(&registration).send().await.map_err(|_| unsupported())?;

            if !registered.status().is_success() {
                return Err(unsupported());
            }

            let registered: Value = registered.json().await.map_err(|_| unsupported())?;

            registered["client_id"].as_str().ok_or_else(unsupported)?.to_string()
        }
    };
    let verifier = new_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let sign_in = new_token();
    let mut authorize = url::Url::parse(&authorisation.authorization_endpoint).map_err(ApiError::internal)?;

    {
        let mut query = authorize.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &sign_in)
            .append_pair("resource", &resource);

        if !scopes.is_empty() {
            query.append_pair("scope", &scopes.join(" "));
        }
    }

    let settings = match app {
        Some(app) if app.stored_in_settings => json!({"app": app.id}),
        _ => json!({}),
    };

    state.hub.track_oauth(sign_in, PendingOAuth { owner: user.id, name: name.to_string(), url: resource, settings, client_id, token_endpoint: authorisation.token_endpoint, verifier, forge_id: request.forge_id, server: request.server_id, created: std::time::Instant::now() });

    Ok(Json(OAuthStart { authorize_url: authorize.to_string() }))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

pub(crate) fn page(status: StatusCode, message: &str) -> Response {
    let escaped = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");

    (status, Html(format!("<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\"><title>Mycelium</title></head><body><main><h1>Mycelium</h1><p>{escaped}</p></main></body></html>"))).into_response()
}

pub async fn callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> Response {
    let expired = || page(StatusCode::BAD_REQUEST, "This sign-in has expired. Start it again from Settings, Integrations.");
    let Some(pending) = query.state.as_deref().and_then(|sign_in| state.hub.take_oauth(sign_in)) else {
        return expired();
    };
    let Some(code) = query.code else {
        return page(StatusCode::BAD_REQUEST, &format!("{} did not sign you in. Start it again from Settings, Integrations.", pending.name));
    };

    let redirect = redirect_uri(&state);
    let form = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", redirect.as_str()), ("client_id", pending.client_id.as_str()), ("code_verifier", pending.verifier.as_str()), ("resource", pending.url.as_str())];
    let Ok(tokens) = token_request(&state, &pending.token_endpoint, &form).await else {
        return page(StatusCode::BAD_GATEWAY, &format!("{} did not give Mycelium access. Start the sign-in again from Settings, Integrations.", pending.name));
    };
    let app = pending.settings["app"].as_str().and_then(known_app);
    let connected = connect(&state, &pending, &tokens, app).await;

    match connected {
        Ok(()) => page(StatusCode::OK, &format!("{} is connected. You can close this tab and return to Mycelium.", pending.name)),
        Err(error) => page(StatusCode::BAD_REQUEST, error.message()),
    }
}

async fn connect(state: &AppState, pending: &PendingOAuth, tokens: &Tokens, app: Option<&crate::chat_tool_servers::KnownApp>) -> ApiResult<()> {
    check_name(state, pending.owner, &pending.name, pending.server).await?;

    let listed = fetch_tools(state, &pending.name, &pending.url, Some(&tokens.access_token), DEFAULT_KEY_HEADER, app).await?;
    let oauth = json!({"client_id": pending.client_id, "token_endpoint": pending.token_endpoint, "resource": pending.url});

    match pending.server {
        Some(server) => {
            let updated = sqlx::query("update chat_tool_servers set bearer_token = $3, oauth_refresh_token = $4, oauth_expires_at = $5, oauth = $6, tools = $7 where id = $1 and owner = $2")
                .bind(server)
                .bind(pending.owner)
                .bind(crate::secrets::Sealed::from(tokens.access_token.as_str()))
                .bind(tokens.refresh_token.as_deref().map(crate::secrets::Sealed::from))
                .bind(tokens.expires_at)
                .bind(sqlx::types::Json(oauth))
                .bind(sqlx::types::Json(listed))
                .execute(&state.pool)
                .await?;

            if updated.rows_affected() == 0 {
                return Err(ApiError::not_found("That tool server"));
            }
        }
        None => {
            let tool_prefix = crate::chat_tool_servers::free_prefix(state, pending.owner, &pending.name).await?;

            sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!(
                "insert into chat_tool_servers (owner, name, kind, url, bearer_token, oauth_refresh_token, oauth_expires_at, oauth, tools, settings, forge_id, tool_prefix) values ($1, $2, 'mcp', $3, $4, $5, $6, $7, $8, $9, $10, $11) returning {SERVER_COLUMNS}"
            )))
            .bind(pending.owner)
            .bind(&pending.name)
            .bind(&pending.url)
            .bind(crate::secrets::Sealed::from(tokens.access_token.as_str()))
            .bind(tokens.refresh_token.as_deref().map(crate::secrets::Sealed::from))
            .bind(tokens.expires_at)
            .bind(sqlx::types::Json(oauth))
            .bind(sqlx::types::Json(listed))
            .bind(crate::secrets::SealedSettings(pending.settings.clone()))
            .bind(pending.forge_id)
            .bind(&tool_prefix)
            .fetch_one(&state.pool)
            .await?;
        }
    }

    state.hub.tools_changed(pending.owner);
    state.publish(Some(pending.owner), StreamMessage::IntegrationsChanged);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(header: &str) -> (Option<String>, Option<String>, Option<String>) {
        let challenge = parse_challenge(header).expect("a bearer challenge");

        (challenge.resource_metadata, challenge.scope, challenge.error)
    }

    #[test]
    fn a_challenge_with_quoted_values_gives_its_metadata_scope_and_error() {
        let header = r#"Bearer realm="mcp", error="insufficient_scope", scope="files:read files:write", resource_metadata="https://mcp.example.test/.well-known/oauth-protected-resource""#;

        assert_eq!(parsed(header), (Some("https://mcp.example.test/.well-known/oauth-protected-resource".to_string()), Some("files:read files:write".to_string()), Some("insufficient_scope".to_string())));
    }

    #[test]
    fn a_challenge_with_unquoted_values_and_any_case_is_read() {
        assert_eq!(parsed("BEARER error=invalid_token, Resource_Metadata=https://mcp.example.test/meta, scope=read"), (Some("https://mcp.example.test/meta".to_string()), Some("read".to_string()), Some("invalid_token".to_string())));
    }

    #[test]
    fn a_quoted_value_keeps_commas_and_escaped_quotes() {
        assert_eq!(parsed(r#"Bearer scope="a, b \"c\"", error="x""#), (None, Some(r#"a, b "c""#.to_string()), Some("x".to_string())));
    }

    #[test]
    fn the_bearer_challenge_is_found_after_another_scheme() {
        assert_eq!(parsed(r#"Basic realm="files", Bearer scope="read""#), (None, Some("read".to_string()), None));
    }

    #[test]
    fn a_bare_bearer_challenge_has_no_parameters() {
        assert_eq!(parsed("Bearer"), (None, None, None));
    }

    #[test]
    fn a_challenge_without_the_bearer_scheme_is_not_a_challenge() {
        assert!(parse_challenge(r#"Basic realm="files""#).is_none());
        assert!(parse_challenge("Bearerish error=x").is_none());
    }
}
