use crate::auth;
use crate::config::Config;
use crate::control::{Artifact, ControlClient, Denied, Viewer};
use crate::render::{self, DOCUMENT_POLICY, EDITOR_POLICY, FILE_POLICY, PAGE_POLICY, PDF_VIEWER_POLICY, SHELL_POLICY, hardened, notice};
use crate::session::{SESSION_COOKIE, Sessions, cookie};
use anyhow::Result;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, HOST, ORIGIN};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, put};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const MAX_SAVED_BYTES: usize = 20 * 1024 * 1024;
const PDF_MAGIC: &[u8] = b"%PDF-";
const VIEWER_SCRIPT: &str = include_str!("viewer.js");
const VIEWER_STYLE: &str = include_str!("viewer.css");
const SHELL_SCRIPT: &str = include_str!("shell.js");
const RUNTIME_SCRIPT: &str = include_str!("runtime.js");
const RUNTIME_INCLUDE: &str = "<script src=\"/_runtime.js\"></script>";
const VIEWER_INCLUDES: &str = "<link rel=\"stylesheet\" href=\"../mycelium-viewer.css\"><script src=\"../mycelium-viewer.js\"></script>";

struct Inner {
    config: Config,
    http: reqwest::Client,
    control: ControlClient,
    sessions: Sessions,
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    pub fn config(&self) -> &Config {
        &self.0.config
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.0.http
    }

    pub fn sessions(&self) -> &Sessions {
        &self.0.sessions
    }

    fn signed_in_subject(&self, headers: &HeaderMap) -> Option<String> {
        cookie(headers, SESSION_COOKIE).and_then(|value| self.0.sessions.subject(value, chrono::Utc::now().timestamp()))
    }

    fn viewer(&self, headers: &HeaderMap) -> Viewer {
        let subject = self.signed_in_subject(headers);
        let issuer = subject.as_ref().and_then(|_| self.0.config.oidc.as_ref().map(|oidc| oidc.issuer.clone()));

        Viewer { subject, issuer }
    }
}

pub struct Site {
    host: String,
}

impl Site {
    pub fn is_apex(&self, domain: &str) -> bool {
        self.host == domain
    }

    fn slug(&self, domain: &str) -> Option<&str> {
        let prefix = self.host.strip_suffix(domain)?.strip_suffix('.')?;
        let valid = !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');

        valid.then_some(prefix)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Site {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Site, Infallible> {
        let host = parts.headers.get(HOST).and_then(|value| value.to_str().ok()).map(str::to_string).or_else(|| parts.uri.host().map(str::to_string)).unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default().to_lowercase();

        Ok(Site { host })
    }
}

pub async fn serve() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).init();

    let config = Config::from_env()?;
    let listener = TcpListener::bind(config.listen).await?;
    tracing::info!("artifact host listening on {} for *.{}", config.listen, config.domain);

    if config.oidc.is_none() {
        tracing::warn!("MYCELIUM_ARTIFACT_OIDC_ISSUER is not set, so only public artefacts can be opened.");
    }

    run(config, listener).await
}

pub async fn run(config: Config, listener: TcpListener) -> Result<()> {
    axum::serve(listener, router(config)?).await?;

    Ok(())
}

pub fn router(config: Config) -> Result<Router> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
    let secret = format!("{}:{}", config.host_token, config.oidc.as_ref().and_then(|oidc| oidc.client_secret.as_deref()).unwrap_or_default());
    let state = AppState(Arc::new(Inner {
        control: ControlClient::new(http.clone(), config.control_url.clone(), config.host_token.clone()),
        sessions: Sessions::new(&secret),
        http,
        config,
    }));

    Ok(Router::new()
        .route("/healthz", get(healthz))
        .route("/_auth/login", get(auth::login))
        .route("/_auth/callback", get(auth::callback))
        .route("/_artifact.json", get(design_json))
        .route("/_page", get(page_frame))
        .route("/_shell.js", get(shell_script))
        .route("/_runtime.js", get(runtime_script))
        .route("/_pdfjs/{*path}", get(pdf_viewer_asset))
        .route("/_data", get(data_list))
        .route("/_data/{key}", put(data_put))
        .route("/_content", put(content_put).layer(DefaultBodyLimit::max(MAX_SAVED_BYTES)))
        .fallback(content)
        .with_state(state))
}

async fn healthz() -> &'static str {
    "ok"
}

fn not_found() -> Response {
    notice(StatusCode::NOT_FOUND, "Artefact not found", "Nothing lives at this address. Check the link, or ask the person who shared it with you.")
}

fn bad_gateway() -> Response {
    notice(StatusCode::BAD_GATEWAY, "This artefact could not be loaded", "The artefact host could not reach the control plane. Try again in a minute.")
}

fn unauthorised() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorised", "message": "Sign in to save changes."}))).into_response()
}

fn forbidden() -> Response {
    notice(StatusCode::FORBIDDEN, "You cannot open this artefact", "It belongs to someone else. Ask its owner to make it public.")
}

fn denied_json(denied: Denied) -> Response {
    match denied {
        Denied::Missing => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found", "message": "Nothing lives at this address."}))).into_response(),
        Denied::Forbidden => (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden", "message": "Only the owner of this artefact can change its data."}))).into_response(),
    }
}

fn refuse_cross_site(headers: &HeaderMap, site: &Site) -> Option<Response> {
    let origin = headers.get(ORIGIN)?.to_str().ok()?;
    let same_host = url::Url::parse(origin).ok().and_then(|url| url.host_str().map(str::to_string)).is_some_and(|host| host == site.host);

    if same_host {
        None
    } else {
        Some((StatusCode::FORBIDDEN, Json(json!({"error": "cross_site", "message": "Save changes from the artefact's own page."}))).into_response())
    }
}

enum Gate {
    Open(Artifact),
    Closed(Response),
}

async fn gate(state: &AppState, site: &Site, headers: &HeaderMap, uri: &Uri, navigation: bool) -> Gate {
    let config = state.config();
    let Some(slug) = site.slug(&config.domain) else { return Gate::Closed(not_found()) };
    let viewer = state.viewer(headers);

    match state.0.control.artifact(slug, &viewer).await {
        Ok(Ok(artifact)) => return Gate::Open(artifact),
        Ok(Err(Denied::Missing)) => return Gate::Closed(not_found()),
        Ok(Err(Denied::Forbidden)) if viewer.subject.is_some() => return Gate::Closed(if navigation { forbidden() } else { denied_json(Denied::Forbidden) }),
        Ok(Err(Denied::Forbidden)) => {}
        Err(error) => {
            tracing::warn!("loading artifact {slug}: {error}");

            return Gate::Closed(bad_gateway());
        }
    }

    if !navigation {
        return Gate::Closed(unauthorised());
    }

    if config.oidc.is_none() {
        return Gate::Closed(notice(StatusCode::SERVICE_UNAVAILABLE, "Sign in is not set up", "This artefact is private and the artefact host has no identity provider. Ask the person who runs it to set one, or ask the owner to make the artifact public."));
    }

    let path_and_query = uri.path_and_query().map(|value| value.as_str()).unwrap_or("/");

    Gate::Closed(Redirect::to(&auth::login_url(&config.domain, &site.host, path_and_query)).into_response())
}

fn no_cache(mut response: Response) -> Response {
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    response
}

fn media_type_header(media_type: &str) -> HeaderValue {
    HeaderValue::from_str(media_type).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

fn download_name(title: &str, fallback: &str) -> String {
    let name: String = title.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') { character } else { '-' }).collect();
    let name = name.trim_matches('-');

    if name.is_empty() { fallback.to_string() } else { name.to_string() }
}

fn editor_media_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" | "ftl" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn existing_asset(directory: &std::path::Path, path: &str) -> Option<std::path::PathBuf> {
    let requested = std::path::Path::new(path.trim_start_matches('/'));
    let plain = requested.components().all(|component| matches!(component, std::path::Component::Normal(_)));
    let asset = directory.join(requested);

    (plain && !requested.as_os_str().is_empty() && asset.is_file()).then_some(asset)
}

fn with_viewer_includes(html: &[u8]) -> Vec<u8> {
    let html = String::from_utf8_lossy(html);
    let included = if html.contains("<script") { html.replacen("<script", &format!("{VIEWER_INCLUDES}<script"), 1) } else { format!("{html}{VIEWER_INCLUDES}") };

    included.into_bytes()
}

fn with_runtime_include(html: &[u8]) -> Vec<u8> {
    let html = String::from_utf8_lossy(html);
    let included = if html.contains("<script") { html.replacen("<script", &format!("{RUNTIME_INCLUDE}<script"), 1) } else { format!("{html}{RUNTIME_INCLUDE}") };

    included.into_bytes()
}

fn viewer_own_asset(path: &str) -> Option<Response> {
    let (body, media_type) = match path {
        "mycelium-viewer.js" => (VIEWER_SCRIPT, "text/javascript; charset=utf-8"),
        "mycelium-viewer.css" => (VIEWER_STYLE, "text/css; charset=utf-8"),
        _ => return None,
    };
    let mut response = body.into_response();
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(media_type));

    Some(no_cache(hardened(response, PDF_VIEWER_POLICY)))
}

async fn pdf_viewer_asset(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    if let Some(own) = viewer_own_asset(&path) {
        return own;
    }

    let Some(asset) = existing_asset(&state.config().pdfjs_dir, &path) else { return not_found() };
    let Ok(bytes) = tokio::fs::read(&asset).await else { return not_found() };
    let bytes = if path.trim_start_matches('/') == "web/viewer.html" { with_viewer_includes(&bytes) } else { bytes };

    let mut response = bytes.into_response();
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(editor_media_type(&asset)));

    no_cache(hardened(response, PDF_VIEWER_POLICY))
}

async fn editor_asset(state: &AppState, uri: &Uri) -> Response {
    let directory = &state.config().editor_dir;
    let path = uri.path().strip_prefix("/design").unwrap_or(uri.path());
    let wanted = existing_asset(directory, path).unwrap_or_else(|| directory.join("index.html"));

    let Ok(bytes) = tokio::fs::read(&wanted).await else {
        return notice(StatusCode::SERVICE_UNAVAILABLE, "The Design viewer is not installed", "This artefact host has no editor files. Ask the person who runs it to set MYCELIUM_EDITOR_DIR.");
    };

    let mut response = bytes.into_response();
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(editor_media_type(&wanted)));

    no_cache(hardened(response, EDITOR_POLICY))
}

async fn content(State(state): State<AppState>, site: Site, method: Method, uri: Uri, headers: HeaderMap) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let artifact = match gate(&state, &site, &headers, &uri, true).await {
        Gate::Open(artifact) => artifact,
        Gate::Closed(response) => return response,
    };

    if artifact.kind == "design" {
        let viewing = uri.query().is_some_and(|query| query.split('&').any(|pair| pair == "view=1"));

        if uri.path() == "/" && !viewing {
            return Redirect::to("/?view=1").into_response();
        }

        return editor_asset(&state, &uri).await;
    }

    if uri.path() != "/" {
        return not_found();
    }

    if artifact.kind == "page" {
        return page_shell(&artifact.title);
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();
    let stored = match state.0.control.content(slug, &state.viewer(&headers)).await {
        Ok(Ok(stored)) => stored,
        Ok(Err(Denied::Missing)) => return not_found(),
        Ok(Err(Denied::Forbidden)) => return forbidden(),
        Err(error) => {
            tracing::warn!("loading content of {slug}: {error}");

            return bad_gateway();
        }
    };

    match artifact.kind.as_str() {
        "markdown" => no_cache(render::markdown(&artifact.title, &String::from_utf8_lossy(&stored.bytes))),
        _ => {
            let media_type = if stored.media_type.is_empty() { artifact.media_type.clone() } else { stored.media_type.clone() };
            let viewing_pdf = media_type == "application/pdf" && !uri.query().is_some_and(|query| query.split('&').any(|pair| pair == "raw=1"));

            if viewing_pdf {
                return Redirect::to("/_pdfjs/web/viewer.html?file=%2F%3Fraw%3D1").into_response();
            }

            let inline = media_type == "application/pdf" || (media_type.starts_with("image/") && media_type != "image/svg+xml");
            let name = download_name(&artifact.title, slug);
            let disposition = if inline { format!("inline; filename=\"{name}\"") } else { format!("attachment; filename=\"{name}\"") };
            let mut response = stored.bytes.into_response();
            response.headers_mut().insert(CONTENT_TYPE, media_type_header(&media_type));
            response.headers_mut().insert(CONTENT_DISPOSITION, media_type_header(&disposition));

            no_cache(hardened(response, FILE_POLICY))
        }
    }
}

fn page_shell(title: &str) -> Response {
    let title = render::escape(title);
    let body = format!("<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>html,body,iframe{{margin:0;border:0;width:100%;height:100%}}</style></head><body><iframe id=\"artifact\" title=\"{title}\" sandbox=\"allow-scripts allow-forms allow-popups\" src=\"/_page\"></iframe><script src=\"/_shell.js\"></script></body></html>");

    no_cache(render::html(StatusCode::OK, body, SHELL_POLICY))
}

async fn page_frame(State(state): State<AppState>, site: Site, uri: Uri, headers: HeaderMap) -> Response {
    let artifact = match gate(&state, &site, &headers, &uri, false).await {
        Gate::Open(artifact) => artifact,
        Gate::Closed(response) => return response,
    };

    if artifact.kind != "page" {
        return not_found();
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();

    match state.0.control.content(slug, &state.viewer(&headers)).await {
        Ok(Ok(stored)) => {
            let mut response = with_runtime_include(&stored.bytes).into_response();
            response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));

            no_cache(hardened(response, PAGE_POLICY))
        }
        Ok(Err(Denied::Missing)) => not_found(),
        Ok(Err(Denied::Forbidden)) => forbidden(),
        Err(error) => {
            tracing::warn!("loading content of {slug}: {error}");

            bad_gateway()
        }
    }
}

fn script_response(body: &'static str, policy: &'static str) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("text/javascript; charset=utf-8"));

    no_cache(hardened(response, policy))
}

async fn shell_script() -> Response {
    script_response(SHELL_SCRIPT, SHELL_POLICY)
}

async fn runtime_script() -> Response {
    script_response(RUNTIME_SCRIPT, PAGE_POLICY)
}

async fn design_json(State(state): State<AppState>, site: Site, uri: Uri, headers: HeaderMap) -> Response {
    let artifact = match gate(&state, &site, &headers, &uri, false).await {
        Gate::Open(artifact) => artifact,
        Gate::Closed(response) => return response,
    };

    if artifact.kind != "design" {
        return not_found();
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();

    match state.0.control.content(slug, &state.viewer(&headers)).await {
        Ok(Ok(stored)) => {
            let mut response = stored.bytes.into_response();
            response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

            no_cache(hardened(response, DOCUMENT_POLICY))
        }
        Ok(Err(Denied::Missing)) => not_found(),
        Ok(Err(Denied::Forbidden)) => forbidden(),
        Err(error) => {
            tracing::warn!("loading design {slug}: {error}");

            bad_gateway()
        }
    }
}

async fn data_list(State(state): State<AppState>, site: Site, uri: Uri, headers: HeaderMap) -> Response {
    if let Gate::Closed(response) = gate(&state, &site, &headers, &uri, false).await {
        return response;
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();

    match state.0.control.data(slug, &state.viewer(&headers)).await {
        Ok(Ok(data)) => no_cache(Json(data).into_response()),
        Ok(Err(denied)) => denied_json(denied),
        Err(error) => {
            tracing::warn!("loading data of {slug}: {error}");

            bad_gateway()
        }
    }
}

async fn data_put(State(state): State<AppState>, site: Site, Path(key): Path<String>, uri: Uri, headers: HeaderMap, Json(value): Json<Value>) -> Response {
    if let Gate::Closed(response) = gate(&state, &site, &headers, &uri, false).await {
        return response;
    }

    if state.signed_in_subject(&headers).is_none() {
        return unauthorised();
    }

    if let Some(refusal) = refuse_cross_site(&headers, &site) {
        return refusal;
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();

    match state.0.control.put_data(slug, &key, &value, &state.viewer(&headers)).await {
        Ok((status, saved)) if status.is_success() => Json(saved).into_response(),
        Ok((status, refused)) => {
            let message = refused["message"].as_str().unwrap_or("Mycelium could not save this entry. Try again in a minute.");

            (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY), Json(json!({"error": "not_saved", "message": message}))).into_response()
        }
        Err(error) => {
            tracing::warn!("saving data of {slug}: {error}");

            bad_gateway()
        }
    }
}

async fn content_put(State(state): State<AppState>, site: Site, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let artifact = match gate(&state, &site, &headers, &uri, false).await {
        Gate::Open(artifact) => artifact,
        Gate::Closed(response) => return response,
    };

    if state.signed_in_subject(&headers).is_none() {
        return unauthorised();
    }

    if let Some(refusal) = refuse_cross_site(&headers, &site) {
        return refusal;
    }

    if artifact.kind != "file" || artifact.media_type != "application/pdf" || !body.starts_with(PDF_MAGIC) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(json!({"error": "not_a_pdf", "message": "Only a PDF can be saved here."}))).into_response();
    }

    let slug = site.slug(&state.config().domain).unwrap_or_default();

    match state.0.control.save_version(slug, &state.viewer(&headers), body).await {
        Ok((status, saved)) if status.is_success() => Json(json!({"version": saved["latest_version"]})).into_response(),
        Ok((status, refused)) => {
            let message = refused["message"].as_str().unwrap_or("Mycelium could not save this PDF. Try again in a minute.");

            (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY), Json(json!({"error": "not_saved", "message": message}))).into_response()
        }
        Err(error) => {
            tracing::warn!("saving a version of {slug}: {error}");

            (StatusCode::BAD_GATEWAY, Json(json!({"error": "unreachable", "message": "The artefact host could not reach Mycelium. Try again in a minute."}))).into_response()
        }
    }
}
