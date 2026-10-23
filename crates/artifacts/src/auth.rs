use crate::render::notice;
use crate::session::{SESSION_COOKIE, SESSION_DAYS, cookie, random_token};
use crate::{AppState, Site};
use axum::extract::{Query, State};
use axum::http::header::{CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PENDING_COOKIE: &str = "myc_artifacts_oidc";

fn not_found() -> Response {
    notice(StatusCode::NOT_FOUND, "Page not found", "Check the address, or ask the person who shared it with you.")
}

fn sign_in_failed(status: StatusCode, message: &str) -> Response {
    notice(status, "You could not be signed in", message)
}

fn header_value(value: &str) -> Result<HeaderValue, Response> {
    HeaderValue::from_str(value).map_err(|_| sign_in_failed(StatusCode::INTERNAL_SERVER_ERROR, "The sign-in could not be completed. Start again."))
}

fn safe_next(domain: &str, next: Option<String>) -> String {
    let fallback = format!("https://{domain}/");
    let Some(next) = next else { return fallback };
    let Ok(url) = url::Url::parse(&next) else { return fallback };
    let host = url.host_str().unwrap_or_default();
    let ours = host == domain || host.ends_with(&format!(".{domain}"));

    if url.scheme() == "https" && ours { next } else { fallback }
}

pub fn login_url(domain: &str, host: &str, path_and_query: &str) -> String {
    let next: String = url::form_urlencoded::byte_serialize(format!("https://{host}{path_and_query}").as_bytes()).collect();

    format!("https://{domain}/_auth/login?next={next}")
}

async fn discover(state: &AppState, issuer: &str) -> Result<Value, Response> {
    let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
    let unreachable = || sign_in_failed(StatusCode::BAD_GATEWAY, "The identity provider could not be reached. Try again in a minute.");
    let response = state.http().get(url).send().await.map_err(|_| unreachable())?;

    response.json().await.map_err(|_| unreachable())
}

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

async fn start(state: &AppState, site: &Site, query: LoginQuery) -> Result<Response, Response> {
    let config = state.config();

    if !site.is_apex(&config.domain) {
        return Err(not_found());
    }

    let oidc = config.oidc.as_ref().ok_or_else(|| sign_in_failed(StatusCode::SERVICE_UNAVAILABLE, "Signing in is not set up on this artifact host. Ask the person who runs it to set the identity provider."))?;
    let discovery = discover(state, &oidc.issuer).await?;
    let endpoint = discovery["authorization_endpoint"].as_str().ok_or_else(|| sign_in_failed(StatusCode::BAD_GATEWAY, "The identity provider has no sign-in address."))?;
    let next = safe_next(&config.domain, query.next);

    let verifier = random_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let oidc_state = random_token();
    let nonce = random_token();

    let mut url = url::Url::parse(endpoint).map_err(|_| sign_in_failed(StatusCode::BAD_GATEWAY, "The identity provider has no sign-in address."))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oidc.client_id)
        .append_pair("redirect_uri", &format!("https://{}/_auth/callback", config.domain))
        .append_pair("scope", "openid email profile")
        .append_pair("state", &oidc_state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    let pending = URL_SAFE_NO_PAD.encode(json!({"state": oidc_state, "verifier": verifier, "nonce": nonce, "next": next}).to_string());
    let mut response = Redirect::to(url.as_str()).into_response();
    response.headers_mut().append(SET_COOKIE, header_value(&format!("{PENDING_COOKIE}={pending}; HttpOnly; Secure; Path=/_auth; SameSite=Lax; Max-Age=600"))?);

    Ok(response)
}

pub async fn login(State(state): State<AppState>, site: Site, Query(query): Query<LoginQuery>) -> Response {
    start(&state, &site, query).await.unwrap_or_else(|response| response)
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn finish(state: &AppState, site: &Site, headers: &HeaderMap, query: CallbackQuery) -> Result<Response, Response> {
    let config = state.config();

    if !site.is_apex(&config.domain) {
        return Err(not_found());
    }

    let oidc = config.oidc.as_ref().ok_or_else(not_found)?;
    let expired = || sign_in_failed(StatusCode::BAD_REQUEST, "The sign-in took too long or was opened in another browser. Open the artifact link again to start again.");

    if query.error.is_some() {
        return Err(sign_in_failed(StatusCode::BAD_REQUEST, "The identity provider did not sign you in. Open the artifact link again, or ask the person who shared it for access."));
    }

    let (code, query_state) = query.code.zip(query.state).ok_or_else(expired)?;
    let pending = cookie(headers, PENDING_COOKIE).ok_or_else(expired)?;
    let pending: Value = URL_SAFE_NO_PAD.decode(pending).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(expired)?;

    if pending["state"].as_str() != Some(query_state.as_str()) {
        return Err(expired());
    }

    let discovery = discover(state, &oidc.issuer).await?;
    let token_endpoint = discovery["token_endpoint"].as_str().ok_or_else(|| sign_in_failed(StatusCode::BAD_GATEWAY, "The identity provider has no token address."))?;
    let redirect_uri = format!("https://{}/_auth/callback", config.domain);
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", oidc.client_id.as_str()),
        ("code_verifier", pending["verifier"].as_str().unwrap_or_default()),
    ];
    if let Some(secret) = &oidc.client_secret {
        form.push(("client_secret", secret.as_str()));
    }

    let rejected = || sign_in_failed(StatusCode::BAD_GATEWAY, "The identity provider's answer could not be read. Open the artifact link again.");
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&form).finish();
    let reply = state.http().post(token_endpoint).header(CONTENT_TYPE, "application/x-www-form-urlencoded").body(body).send().await.map_err(|_| rejected())?;
    let tokens: Value = reply.json().await.map_err(|_| rejected())?;
    let id_token = tokens["id_token"].as_str().ok_or_else(rejected)?;
    let payload = id_token.split('.').nth(1).ok_or_else(rejected)?;
    let claims: Value = URL_SAFE_NO_PAD.decode(payload).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(rejected)?;

    let issuer_matches = claims["iss"].as_str().is_some_and(|iss| iss.trim_end_matches('/') == oidc.issuer.trim_end_matches('/'));
    let audience_matches = match &claims["aud"] {
        Value::String(audience) => audience == &oidc.client_id,
        Value::Array(audiences) => audiences.iter().any(|audience| audience.as_str() == Some(oidc.client_id.as_str())),
        _ => false,
    };
    let nonce_matches = claims["nonce"] == pending["nonce"];
    let unexpired = claims["exp"].as_i64().is_some_and(|exp| exp > chrono::Utc::now().timestamp());

    if !(issuer_matches && audience_matches && nonce_matches && unexpired) {
        return Err(sign_in_failed(StatusCode::UNAUTHORIZED, "The identity provider's answer could not be checked. Open the artifact link again."));
    }

    let next = safe_next(&config.domain, pending["next"].as_str().map(str::to_string));
    let session = state.sessions().issue(chrono::Utc::now().timestamp());
    let session_cookie = format!("{SESSION_COOKIE}={session}; HttpOnly; Secure; SameSite=Lax; Path=/; Domain=.{}; Max-Age={}", config.domain, SESSION_DAYS * 86400);
    let mut response = Redirect::to(&next).into_response();
    response.headers_mut().append(SET_COOKIE, header_value(&session_cookie)?);
    response.headers_mut().append(SET_COOKIE, header_value(&format!("{PENDING_COOKIE}=; Path=/_auth; Max-Age=0"))?);

    Ok(response)
}

pub async fn callback(State(state): State<AppState>, site: Site, headers: HeaderMap, Query(query): Query<CallbackQuery>) -> Response {
    finish(&state, &site, &headers, query).await.unwrap_or_else(|response| response)
}
