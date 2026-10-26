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

const DISCOVERY_TIMEOUT: StdDuration = StdDuration::from_secs(10);
const UNSUPPORTED: &str = "This app does not let Mycelium sign in. Use an API key instead.";

pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

struct Authorisation {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    scopes: Vec<String>,
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

async fn resource_metadata_url(state: &AppState, mcp_url: &url::Url) -> Option<String> {
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "mycelium", "version": "1"}}});
    let response = state.http.post(mcp_url.as_str()).timeout(DISCOVERY_TIMEOUT).header("accept", "application/json, text/event-stream").json(&initialize).send().await.ok()?;
    let challenge = response.headers().get("www-authenticate")?.to_str().ok()?;
    let after = challenge.split("resource_metadata=\"").nth(1)?;

    after.split('"').next().map(str::to_string)
}

async fn authorisation_of(state: &AppState, mcp_url: &url::Url) -> Option<Authorisation> {
    let protected = match resource_metadata_url(state, mcp_url).await {
        Some(metadata) => get_json(state, &metadata).await,
        None => get_json(state, &well_known(mcp_url, "oauth-protected-resource")).await,
    };
    let issuer = protected.as_ref().and_then(|body| body["authorization_servers"][0].as_str()).and_then(|issuer| url::Url::parse(issuer).ok()).unwrap_or_else(|| {
        let mut origin = mcp_url.clone();
        origin.set_path("");
        origin
    });
    let metadata = match get_json(state, &well_known(&issuer, "oauth-authorization-server")).await {
        Some(metadata) => metadata,
        None => get_json(state, &well_known(&issuer, "openid-configuration")).await?,
    };
    let text = |key: &str| metadata[key].as_str().map(str::to_string);

    Some(Authorisation {
        authorization_endpoint: text("authorization_endpoint")?,
        token_endpoint: text("token_endpoint")?,
        registration_endpoint: text("registration_endpoint").unwrap_or_default(),
        scopes: metadata["scopes_supported"].as_array().map(|scopes| scopes.iter().filter_map(|scope| scope.as_str().map(str::to_string)).collect()).unwrap_or_default(),
    })
}

fn redirect_uri(state: &AppState) -> String {
    format!("{}/integrations/oauth/callback", state.config.public_url)
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Json(request): Json<StartChatToolServerOAuthRequest>) -> ApiResult<Json<OAuthStart>> {
    let name = request.name.trim();
    let kind = request.kind.as_deref().map(str::trim).filter(|kind| !kind.is_empty()).unwrap_or("mcp");

    check_name(&state, user.id, name, None).await?;

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
    let authorisation = authorisation_of(&state, &mcp_url).await.filter(|authorisation| !authorisation.registration_endpoint.is_empty()).ok_or_else(unsupported)?;

    let redirect = redirect_uri(&state);
    let registration = json!({"client_name": "Mycelium", "redirect_uris": [redirect], "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]});
    let registered = state.http.post(&authorisation.registration_endpoint).timeout(DISCOVERY_TIMEOUT).json(&registration).send().await.map_err(|_| unsupported())?;

    if !registered.status().is_success() {
        return Err(unsupported());
    }

    let registered: Value = registered.json().await.map_err(|_| unsupported())?;
    let client_id = registered["client_id"].as_str().ok_or_else(unsupported)?.to_string();
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

        if !authorisation.scopes.is_empty() {
            query.append_pair("scope", &authorisation.scopes.join(" "));
        }
    }

    let settings = match app {
        Some(app) if app.stored_in_settings => json!({"app": app.id}),
        _ => json!({}),
    };

    state.hub.track_oauth(sign_in, PendingOAuth { owner: user.id, name: name.to_string(), url: resource, settings, client_id, token_endpoint: authorisation.token_endpoint, verifier, forge_id: request.forge_id, created: std::time::Instant::now() });

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
    check_name(state, pending.owner, &pending.name, None).await?;

    let listed = fetch_tools(state, &pending.name, &pending.url, Some(&tokens.access_token), DEFAULT_KEY_HEADER, app).await?;
    let oauth = json!({"client_id": pending.client_id, "token_endpoint": pending.token_endpoint, "resource": pending.url});

    sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!(
        "insert into chat_tool_servers (owner, name, kind, url, bearer_token, oauth_refresh_token, oauth_expires_at, oauth, tools, settings, forge_id) values ($1, $2, 'mcp', $3, $4, $5, $6, $7, $8, $9, $10) returning {SERVER_COLUMNS}"
    )))
    .bind(pending.owner)
    .bind(&pending.name)
    .bind(&pending.url)
    .bind(&tokens.access_token)
    .bind(&tokens.refresh_token)
    .bind(tokens.expires_at)
    .bind(sqlx::types::Json(oauth))
    .bind(sqlx::types::Json(listed))
    .bind(sqlx::types::Json(&pending.settings))
    .bind(pending.forge_id)
    .fetch_one(&state.pool)
    .await?;
    state.hub.tools_changed(pending.owner);
    state.publish(Some(pending.owner), StreamMessage::IntegrationsChanged);

    Ok(())
}
