use crate::config::{OidcConfig, is_localhost};
use crate::error::{ApiError, ApiResult};
use crate::device::client_address;
use crate::state::{AppState, hash_token, new_token};
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mycelium_proto::api::Me;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddr;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const SESSION_COOKIE: &str = "myc_session";
const OIDC_COOKIE: &str = "myc_oidc";
const SESSION_DAYS: i64 = 30;

pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub session: Option<Uuid>,
}

pub struct AuthNode {
    pub id: Uuid,
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let header = headers.get(COOKIE)?.to_str().ok()?;

    header.split(';').filter_map(|pair| pair.trim().split_once('=')).find(|(key, _)| *key == name).map(|(_, value)| value)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<AuthUser, ApiError> {
        let address = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(peer)| client_address(state, &parts.headers, *peer).to_string()).unwrap_or_default();

        if let Some(token) = bearer(&parts.headers).and_then(|value| value.strip_prefix("myc_client_")) {
            let row = sqlx::query_as::<_, (Uuid, String, String, Uuid)>(
                "update client_tokens set last_used_at = now(), address = $2 from users
                 where client_tokens.token_hash = $1 and client_tokens.revoked_at is null and users.id = client_tokens.user_id
                 returning users.id, users.email, users.name, client_tokens.id",
            )
            .bind(hash_token(token))
            .bind(&address)
            .fetch_optional(&state.pool)
            .await?;

            return row.map(|(id, email, name, session)| AuthUser { id, email, name, session: Some(session) }).ok_or_else(ApiError::unauthorised);
        }

        let token = cookie(&parts.headers, SESSION_COOKIE).ok_or_else(ApiError::unauthorised)?;
        let row = sqlx::query_as::<_, (Uuid, String, String, Uuid)>(
            "select users.id, users.email, users.name, web_sessions.id from web_sessions join users on users.id = web_sessions.user_id
             where web_sessions.token_hash = $1 and web_sessions.expires_at > now()",
        )
        .bind(hash_token(token))
        .fetch_optional(&state.pool)
        .await?;
        let user_agent = parts.headers.get(USER_AGENT).and_then(|value| value.to_str().ok()).unwrap_or_default();

        if let Some((_, _, _, session)) = &row {
            sqlx::query(
                "update web_sessions set last_used_at = now(), address = $2, user_agent = $3
                 where id = $1 and (last_used_at is null or last_used_at < now() - interval '1 minute')",
            )
            .bind(session)
            .bind(&address)
            .bind(user_agent)
            .execute(&state.pool)
            .await?;
        }

        row.map(|(id, email, name, session)| AuthUser { id, email, name, session: Some(session) }).ok_or_else(ApiError::unauthorised)
    }
}

impl FromRequestParts<AppState> for AuthNode {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<AuthNode, ApiError> {
        let token = bearer(&parts.headers).and_then(|value| value.strip_prefix("myc_node_")).ok_or_else(ApiError::unauthorised)?;
        let id = sqlx::query_scalar::<_, Uuid>("select id from nodes where token_hash = $1 and revoked_at is null")
            .bind(hash_token(token))
            .fetch_optional(&state.pool)
            .await?;

        id.map(|id| AuthNode { id }).ok_or_else(ApiError::unauthorised)
    }
}

fn session_cookie_attributes(state: &AppState) -> &'static str {
    if is_localhost(&state.config.public_url) {
        ""
    } else {
        "; Secure"
    }
}

async fn start_session(state: &AppState, subject: &str, email: &str, name: &str, next: &str) -> ApiResult<Response> {
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "insert into users (subject, email, name) values ($1, $2, $3)
         on conflict (subject) do update set email = excluded.email, name = excluded.name returning id",
    )
    .bind(subject)
    .bind(email)
    .bind(name)
    .fetch_one(&state.pool)
    .await?;

    let token = new_token();
    sqlx::query("insert into web_sessions (user_id, token_hash, expires_at) values ($1, $2, now() + make_interval(days => $3::int))")
        .bind(user_id)
        .bind(hash_token(&token))
        .bind(SESSION_DAYS as i32)
        .execute(&state.pool)
        .await?;

    let cookie = format!(
        "{SESSION_COOKIE}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}{}",
        SESSION_DAYS * 86400,
        session_cookie_attributes(state)
    );
    let mut response = Redirect::to(next).into_response();
    response.headers_mut().append(SET_COOKIE, header_value(&cookie)?);

    Ok(response)
}

fn header_value(value: &str) -> ApiResult<HeaderValue> {
    HeaderValue::from_str(value).map_err(ApiError::internal)
}

fn safe_next(next: Option<String>) -> String {
    match next {
        Some(next) if next.starts_with('/') && !next.starts_with("//") && !next.contains('\\') => next,
        _ => "/".into(),
    }
}

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

pub async fn login(State(state): State<AppState>, Query(query): Query<LoginQuery>) -> ApiResult<Response> {
    let next = safe_next(query.next);

    if let Some(email) = &state.config.dev_login {
        let name = email.split('@').next().unwrap_or(email);

        return start_session(&state, &format!("dev:{email}"), email, name, &next).await;
    }

    let oidc = state.config.oidc.as_ref().ok_or_else(|| ApiError::internal("no sign-in method configured"))?;
    let discovery = discover(&state, oidc).await?;
    let authorization_endpoint = discovery["authorization_endpoint"].as_str().ok_or_else(|| ApiError::internal("issuer has no authorization endpoint"))?;

    let verifier = new_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let oidc_state = new_token();
    let nonce = new_token();

    let mut url = url::Url::parse(authorization_endpoint).map_err(ApiError::internal)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &oidc.client_id)
        .append_pair("redirect_uri", &format!("{}/auth/callback", state.config.public_url))
        .append_pair("scope", "openid email profile")
        .append_pair("state", &oidc_state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    let pending = URL_SAFE_NO_PAD.encode(json!({"state": oidc_state, "verifier": verifier, "nonce": nonce, "next": next}).to_string());
    let cookie = format!("{OIDC_COOKIE}={pending}; HttpOnly; Path=/auth; SameSite=Lax; Max-Age=600{}", session_cookie_attributes(&state));
    let mut response = Redirect::to(url.as_str()).into_response();
    response.headers_mut().append(SET_COOKIE, header_value(&cookie)?);

    Ok(response)
}

async fn discover(state: &AppState, oidc: &OidcConfig) -> ApiResult<Value> {
    let url = format!("{}/.well-known/openid-configuration", oidc.issuer.trim_end_matches('/'));
    let response = state.http.get(url).send().await.map_err(ApiError::internal)?;

    response.json().await.map_err(ApiError::internal)
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn callback(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<CallbackQuery>) -> ApiResult<Response> {
    let oidc = state.config.oidc.as_ref().ok_or_else(|| ApiError::bad_request("oidc_disabled", "This server signs people in without an identity provider."))?;
    let expired = || ApiError::bad_request("sign_in_expired", "The sign-in took too long or was opened in another browser. Start again.");

    if query.error.is_some() {
        return Err(ApiError::bad_request("sign_in_refused", "The identity provider did not sign you in. Start the sign-in again, or ask the person who runs this control plane for access."));
    }

    let (code, query_state) = query.code.zip(query.state).ok_or_else(expired)?;

    let pending = cookie(&headers, OIDC_COOKIE).ok_or_else(expired)?;
    let pending: Value = URL_SAFE_NO_PAD.decode(pending).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(expired)?;

    if pending["state"].as_str() != Some(query_state.as_str()) {
        return Err(expired());
    }

    let discovery = discover(&state, oidc).await?;
    let token_endpoint = discovery["token_endpoint"].as_str().ok_or_else(|| ApiError::internal("issuer has no token endpoint"))?;

    let redirect_uri = format!("{}/auth/callback", state.config.public_url);
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

    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&form).finish();
    let tokens: Value = state.http.post(token_endpoint).header(CONTENT_TYPE, "application/x-www-form-urlencoded").body(body).send().await.map_err(ApiError::internal)?.json().await.map_err(ApiError::internal)?;
    let id_token = tokens["id_token"].as_str().ok_or_else(|| ApiError::internal("token response has no id_token"))?;
    let payload = id_token.split('.').nth(1).ok_or_else(|| ApiError::internal("malformed id_token"))?;
    let claims: Value = URL_SAFE_NO_PAD.decode(payload).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or_else(|| ApiError::internal("malformed id_token"))?;

    let issuer_matches = claims["iss"].as_str().is_some_and(|iss| iss.trim_end_matches('/') == oidc.issuer.trim_end_matches('/'));
    let audience_matches = match &claims["aud"] {
        Value::String(audience) => audience == &oidc.client_id,
        Value::Array(audiences) => audiences.iter().any(|audience| audience.as_str() == Some(oidc.client_id.as_str())),
        _ => false,
    };
    let nonce_matches = claims["nonce"] == pending["nonce"];
    let unexpired = claims["exp"].as_i64().is_some_and(|exp| exp > chrono::Utc::now().timestamp());

    if !(issuer_matches && audience_matches && nonce_matches && unexpired) {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_id_token", "The identity provider's answer could not be checked. Start the sign-in again."));
    }

    let allowed_emails = &state.config.allowed_emails;
    let verified = claims["email_verified"].as_bool() != Some(false);
    let listed = claims["email"].as_str().is_some_and(|email| allowed_emails.contains(&email.to_lowercase()));

    if !allowed_emails.is_empty() && !(listed && verified) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "not_allowed", "This account is not allowed to use this control plane. Ask the person who runs it to add your email address."));
    }

    let subject = claims["sub"].as_str().ok_or_else(|| ApiError::internal("id_token has no subject"))?;
    let email = claims["email"].as_str().unwrap_or(subject);
    let name = claims["name"].as_str().or(claims["preferred_username"].as_str()).unwrap_or(email);
    let next = safe_next(pending["next"].as_str().map(str::to_string));

    let mut response = start_session(&state, &format!("oidc:{subject}"), email, name, &next).await?;
    response.headers_mut().append(SET_COOKIE, header_value(&format!("{OIDC_COOKIE}=; Path=/auth; Max-Age=0"))?);

    Ok(response)
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = bearer(&headers).and_then(|value| value.strip_prefix("myc_client_")) {
        sqlx::query("update client_tokens set revoked_at = now() where token_hash = $1").bind(hash_token(token)).execute(&state.pool).await?;
    }

    if let Some(token) = cookie(&headers, SESSION_COOKIE) {
        sqlx::query("delete from web_sessions where token_hash = $1").bind(hash_token(token)).execute(&state.pool).await?;
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(SET_COOKIE, header_value(&format!("{SESSION_COOKIE}=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0{}", session_cookie_attributes(&state)))?);

    Ok(response)
}

pub async fn me(user: AuthUser) -> Json<Me> {
    Json(Me { id: user.id, email: user.email, name: user.name })
}
