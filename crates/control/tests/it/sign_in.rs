use crate::common;
use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use crate::common::{Server, session_cookie};
use mycelium_control::OidcConfig;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Default)]
struct Provider {
    challenge: Mutex<String>,
    nonce: Mutex<String>,
}

async fn start_provider() -> (String, Arc<Provider>) {
    let provider = Arc::new(Provider::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let discovery = json!({"authorization_endpoint": format!("{issuer}/authorize"), "token_endpoint": format!("{issuer}/token")});

    let app = Router::new()
        .route("/.well-known/openid-configuration", get(move || async move { Json(discovery.clone()) }))
        .route(
            "/token",
            post({
                let issuer = issuer.clone();
                move |State(provider): State<Arc<Provider>>, Form(form): Form<HashMap<String, String>>| async move {
                    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
                    let expected = provider.challenge.lock().unwrap().clone();

                    if form.get("code").map(String::as_str) != Some("the-code") || URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != expected {
                        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"})));
                    }

                    let claims = json!({
                        "iss": issuer,
                        "aud": "mycelium",
                        "sub": "user-1",
                        "email": "ada@example.com",
                        "name": "Ada",
                        "nonce": provider.nonce.lock().unwrap().clone(),
                        "exp": chrono::Utc::now().timestamp() + 600,
                    });
                    let id_token = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));

                    (axum::http::StatusCode::OK, Json(json!({"id_token": id_token})))
                }
            }),
        )
        .with_state(provider.clone());

    tokio::spawn(async move { axum::serve(listener, app).await });

    (issuer, provider)
}

fn query_of(location: &str) -> HashMap<String, String> {
    url::Url::parse(location).unwrap().query_pairs().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

fn oidc_cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("myc_oidc="))
        .map(|value| value.split(';').next().unwrap().to_string())
        .unwrap()
}

#[tokio::test]
async fn dev_login_sets_a_session_cookie_that_signs_the_browser_in() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (status, _) = anonymous.json(anonymous.get("/api/v1/me")).await;
    assert_eq!(status, 401);

    let login = anonymous.get("/auth/login?next=/approve%3Fcode%3DWDJB-MJHT").send().await.unwrap();
    assert_eq!(login.status(), 303);
    assert_eq!(login.headers()["location"], "/approve?code=WDJB-MJHT");

    let set_cookie = login.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(!set_cookie.contains("Secure"), "{set_cookie}");

    let signed_in = common::Client { cookie: session_cookie(&login), ..anonymous.clone() };
    let (status, me) = signed_in.json(signed_in.get("/api/v1/me")).await;

    assert_eq!(status, 200);
    assert_eq!(me["email"], "dev@example.com");
}

#[tokio::test]
async fn login_ignores_a_next_address_that_leaves_the_site() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    for next in ["//evil.example", "https://evil.example", "/\\evil.example"] {
        let login = anonymous.get("/auth/login").query(&[("next", next)]).send().await.unwrap();

        assert_eq!(login.headers()["location"], "/", "{next}");
    }
}

#[tokio::test]
async fn logout_ends_the_session() {
    let server = Server::start().await;

    let response = server.user.post("/auth/logout").send().await.unwrap();
    assert_eq!(response.status(), 204);

    let (status, _) = server.user.json(server.user.get("/api/v1/me")).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn oidc_sign_in_uses_pkce_and_sets_a_secure_session_cookie() {
    let (issuer, provider) = start_provider().await;
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.public_url = "https://mycelium.example.com".into();
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let login = anonymous.get("/auth/login?next=/nodes").send().await.unwrap();
    assert_eq!(login.status(), 303);

    let location = login.headers()["location"].to_str().unwrap().to_string();
    let authorize = query_of(&location);
    assert!(location.starts_with(&format!("{issuer}/authorize")));
    assert_eq!(authorize["code_challenge_method"], "S256");
    assert_eq!(authorize["response_type"], "code");
    assert_eq!(authorize["redirect_uri"], "https://mycelium.example.com/auth/callback");
    assert!(!authorize.contains_key("client_secret"));

    *provider.challenge.lock().unwrap() = authorize["code_challenge"].clone();
    *provider.nonce.lock().unwrap() = authorize["nonce"].clone();

    let pending = oidc_cookie(&login);
    let callback = anonymous
        .get("/auth/callback")
        .query(&[("code", "the-code"), ("state", authorize["state"].as_str())])
        .header("cookie", &pending)
        .send()
        .await
        .unwrap();

    assert_eq!(callback.status(), 303);
    assert_eq!(callback.headers()["location"], "/nodes");

    let session_header = callback.headers().get_all("set-cookie").iter().map(|value| value.to_str().unwrap().to_string()).find(|value| value.starts_with("myc_session=")).unwrap();
    assert!(session_header.contains("Secure"));
    assert!(session_header.contains("HttpOnly"));

    let signed_in = common::Client { cookie: session_cookie(&callback), ..anonymous.clone() };
    let (_, me) = signed_in.json(signed_in.get("/api/v1/me")).await;

    assert_eq!(me["email"], "ada@example.com");
    assert_eq!(me["name"], "Ada");
}

#[tokio::test]
async fn oidc_callback_rejects_a_state_that_does_not_match_the_browser() {
    let (issuer, provider) = start_provider().await;
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let login = anonymous.get("/auth/login").send().await.unwrap();
    let authorize = query_of(login.headers()["location"].to_str().unwrap());
    *provider.challenge.lock().unwrap() = authorize["code_challenge"].clone();

    let (status, body) = anonymous
        .json(anonymous.get("/auth/callback").query(&[("code", "the-code"), ("state", "someone-elses")]).header("cookie", oidc_cookie(&login)))
        .await;

    assert_eq!(status, 400);
    assert_eq!(body["error"], "sign_in_expired");
}

#[tokio::test]
async fn an_identity_provider_refusal_is_answered_with_the_json_error_and_a_fix() {
    let (issuer, _) = start_provider().await;
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let (status, body) = anonymous.json(anonymous.get("/auth/callback").query(&[("error", "access_denied"), ("state", "x")])).await;

    assert_eq!((status, body["error"].as_str()), (400, Some("sign_in_refused")));
    assert_eq!(body["message"], "The identity provider did not sign you in. Start the sign-in again, or ask the person who runs this control plane for access.");
}

#[tokio::test]
async fn a_callback_without_a_code_asks_to_start_again() {
    let (issuer, _) = start_provider().await;
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let (status, body) = anonymous.json(anonymous.get("/auth/callback")).await;

    assert_eq!((status, body["error"].as_str()), (400, Some("sign_in_expired")));
}

#[tokio::test]
async fn oidc_callback_rejects_an_id_token_issued_for_another_login() {
    let (issuer, provider) = start_provider().await;
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let login = anonymous.get("/auth/login").send().await.unwrap();
    let authorize = query_of(login.headers()["location"].to_str().unwrap());
    *provider.challenge.lock().unwrap() = authorize["code_challenge"].clone();
    *provider.nonce.lock().unwrap() = "a-different-nonce".into();

    let (status, body): (u16, Value) = anonymous
        .json(anonymous.get("/auth/callback").query(&[("code", "the-code"), ("state", authorize["state"].as_str())]).header("cookie", oidc_cookie(&login)))
        .await;

    assert_eq!(status, 401);
    assert_eq!(body["error"], "invalid_id_token");
}

async fn sign_in_as_ada(allowed_emails: &[&str]) -> (reqwest::Response, Server) {
    let (issuer, provider) = start_provider().await;
    let allowed: Vec<String> = allowed_emails.iter().map(|email| email.to_string()).collect();
    let server = Server::start_with(|config| {
        config.dev_login = None;
        config.allowed_emails = allowed;
        config.oidc = Some(OidcConfig { issuer: issuer.clone(), client_id: "mycelium".into(), client_secret: None });
    })
    .await;
    let anonymous = server.anonymous();

    let login = anonymous.get("/auth/login").send().await.unwrap();
    let authorize = query_of(login.headers()["location"].to_str().unwrap());
    *provider.challenge.lock().unwrap() = authorize["code_challenge"].clone();
    *provider.nonce.lock().unwrap() = authorize["nonce"].clone();

    let callback = anonymous.get("/auth/callback").query(&[("code", "the-code"), ("state", authorize["state"].as_str())]).header("cookie", oidc_cookie(&login)).send().await.unwrap();

    (callback, server)
}

#[tokio::test]
async fn an_account_missing_from_the_allowed_emails_cannot_sign_in() {
    let (callback, server) = sign_in_as_ada(&["grace@example.com"]).await;

    assert_eq!(callback.status(), 403);
    assert!(callback.headers().get_all("set-cookie").iter().all(|value| !value.to_str().unwrap().starts_with("myc_session=")));
    let body: Value = callback.json().await.unwrap();
    assert_eq!(body["error"], "not_allowed");
    let users = sqlx::query_scalar::<_, i64>("select count(*) from users").fetch_one(&server.pool).await.unwrap();
    assert_eq!(users, 0);
}

#[tokio::test]
async fn an_allowed_email_signs_in() {
    let (callback, _server) = sign_in_as_ada(&["grace@example.com", "ada@example.com"]).await;

    assert_eq!(callback.status(), 303);
}

#[tokio::test]
async fn every_account_signs_in_when_no_emails_are_listed() {
    let (callback, _server) = sign_in_as_ada(&[]).await;

    assert_eq!(callback.status(), 303);
}
