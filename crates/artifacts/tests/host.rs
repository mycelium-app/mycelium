use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mycelium_artifacts::Config;
use mycelium_artifacts::config::OidcConfig;
use reqwest::redirect::Policy;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::net::TcpListener;

const DOMAIN: &str = "artifacts.test";
const HOST_TOKEN: &str = "host-token";

struct Fixture {
    kind: &'static str,
    public: bool,
    media_type: &'static str,
    bytes: Vec<u8>,
}

fn fixture(kind: &'static str, public: bool, media_type: &'static str, bytes: &[u8]) -> Fixture {
    Fixture { kind, public, media_type, bytes: bytes.to_vec() }
}

struct Control {
    artifacts: HashMap<&'static str, Fixture>,
    data: Mutex<HashMap<String, Map<String, Value>>>,
    saved: Mutex<Vec<(String, String, Vec<u8>)>>,
}

fn authorised(headers: &HeaderMap) -> bool {
    headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {HOST_TOKEN}"))
}

async fn control_artifact(State(control): State<Arc<Control>>, headers: HeaderMap, Path(slug): Path<String>) -> Response {
    let Some(found) = control.artifacts.get(slug.as_str()).filter(|_| authorised(&headers)) else { return StatusCode::NOT_FOUND.into_response() };

    Json(json!({"artifact": {"slug": slug, "kind": found.kind, "title": format!("Title of {slug}"), "public": found.public}, "media_type": found.media_type, "sha256": "00"})).into_response()
}

async fn control_content(State(control): State<Arc<Control>>, headers: HeaderMap, Path(slug): Path<String>) -> Response {
    let Some(found) = control.artifacts.get(slug.as_str()).filter(|_| authorised(&headers)) else { return StatusCode::NOT_FOUND.into_response() };

    ([(CONTENT_TYPE, found.media_type)], found.bytes.clone()).into_response()
}

async fn control_data(State(control): State<Arc<Control>>, headers: HeaderMap, Path(slug): Path<String>) -> Response {
    if !authorised(&headers) || !control.artifacts.contains_key(slug.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let data = control.data.lock().unwrap().get(&slug).cloned().unwrap_or_default();

    Json(Value::Object(data)).into_response()
}

async fn control_put_data(State(control): State<Arc<Control>>, headers: HeaderMap, Path((slug, key)): Path<(String, String)>, Json(value): Json<Value>) -> Response {
    if !authorised(&headers) || !control.artifacts.contains_key(slug.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }

    control.data.lock().unwrap().entry(slug).or_default().insert(key.clone(), value.clone());

    Json(json!({"key": key, "value": value})).into_response()
}

async fn control_save_version(State(control): State<Arc<Control>>, headers: HeaderMap, Path(slug): Path<String>, axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>, body: axum::body::Bytes) -> Response {
    if !authorised(&headers) || !control.artifacts.contains_key(slug.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }

    if query["subject"] != "reader" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "edit_forbidden", "message": "You cannot save changes to this artefact."}))).into_response();
    }

    control.saved.lock().unwrap().push((slug, query["subject"].clone(), body.to_vec()));

    (StatusCode::CREATED, Json(json!({"latest_version": 2}))).into_response()
}

#[derive(Clone)]
struct Idp {
    base: String,
    nonce: Arc<Mutex<String>>,
}

async fn idp_discovery(State(idp): State<Idp>) -> Json<Value> {
    Json(json!({"authorization_endpoint": format!("{}/authorize", idp.base), "token_endpoint": format!("{}/token", idp.base)}))
}

async fn idp_token(State(idp): State<Idp>) -> Json<Value> {
    let claims = json!({"iss": idp.base, "aud": "artifacts", "sub": "reader", "nonce": *idp.nonce.lock().unwrap(), "exp": 4_000_000_000_i64});
    let id_token = format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(claims.to_string()));

    Json(json!({"id_token": id_token}))
}

async fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());

    (listener, base)
}

struct Harness {
    control: Arc<Control>,
    base: String,
    http: reqwest::Client,
    nonce: Arc<Mutex<String>>,
    _editor: TempDir,
    _pdfjs: TempDir,
}

impl Harness {
    async fn start() -> Harness {
        let artifacts = HashMap::from([
            ("private-page", fixture("page", false, "text/html", b"<h1>Secret</h1>")),
            ("open-page", fixture("page", true, "text/html", b"<h1>Hello</h1><script>1</script>")),
            ("notes", fixture("markdown", true, "text/markdown", b"# Plan\n\n<script>alert(1)</script>\n\nA <b>bold</b> claim and [a link](javascript:alert(1)).")),
            ("report", fixture("file", true, "application/pdf", b"%PDF-1.4")),
            ("archive", fixture("file", true, "application/zip", b"PK")),
            ("board", fixture("page", false, "text/html", b"<h1>Board</h1>")),
            ("open-board", fixture("page", true, "text/html", b"<h1>Board</h1>")),
            ("layout", fixture("design", true, "application/vnd.mycelium.design+json", br#"{"pages":[]}"#)),
        ]);
        let control = Arc::new(Control { artifacts, data: Mutex::new(HashMap::new()), saved: Mutex::new(Vec::new()) });
        let (control_listener, control_base) = listen().await;
        let control_app = Router::new()
            .route("/internal/artifacts/by-slug/{slug}", get(control_artifact))
            .route("/internal/artifacts/by-slug/{slug}/content", get(control_content))
            .route("/internal/artifacts/by-slug/{slug}/versions", axum::routing::post(control_save_version))
            .route("/internal/artifacts/by-slug/{slug}/data", get(control_data))
            .route("/internal/artifacts/by-slug/{slug}/data/{key}", axum::routing::put(control_put_data))
            .with_state(control.clone());
        tokio::spawn(async move { axum::serve(control_listener, control_app).await });

        let (idp_listener, idp_base) = listen().await;
        let nonce = Arc::new(Mutex::new(String::new()));
        let idp = Idp { base: idp_base.clone(), nonce: nonce.clone() };
        let idp_app = Router::new().route("/.well-known/openid-configuration", get(idp_discovery)).route("/token", axum::routing::post(idp_token)).with_state(idp);
        tokio::spawn(async move { axum::serve(idp_listener, idp_app).await });

        let editor = tempfile::tempdir().unwrap();
        std::fs::write(editor.path().join("index.html"), "<!doctype html><title>Design viewer</title>").unwrap();
        std::fs::create_dir(editor.path().join("assets")).unwrap();
        std::fs::write(editor.path().join("assets/index.js"), "export {};").unwrap();

        let pdfjs = tempfile::tempdir().unwrap();
        std::fs::create_dir(pdfjs.path().join("web")).unwrap();
        std::fs::create_dir(pdfjs.path().join("build")).unwrap();
        std::fs::write(pdfjs.path().join("web/viewer.html"), "<!doctype html><title>PDF.js</title><script src=\"../build/pdf.mjs\" type=\"module\"></script><script src=\"viewer.mjs\" type=\"module\"></script>").unwrap();
        std::fs::write(pdfjs.path().join("build/pdf.mjs"), "export {};").unwrap();

        let config = Config {
            domain: DOMAIN.into(),
            control_url: control_base,
            host_token: HOST_TOKEN.into(),
            oidc: Some(OidcConfig { issuer: idp_base, client_id: "artifacts".into(), client_secret: Some("secret".into()) }),
            listen: "127.0.0.1:0".parse().unwrap(),
            editor_dir: editor.path().to_path_buf(),
            pdfjs_dir: pdfjs.path().to_path_buf(),
        };
        let (listener, base) = listen().await;
        tokio::spawn(mycelium_artifacts::run(config, listener));

        Harness { control, base, http: reqwest::Client::builder().redirect(Policy::none()).build().unwrap(), nonce, _editor: editor, _pdfjs: pdfjs }
    }

    fn request(&self, method: reqwest::Method, host: &str, path: &str, cookie: Option<&str>) -> reqwest::RequestBuilder {
        let request = self.http.request(method, format!("{}{path}", self.base)).header("host", host);

        match cookie {
            Some(cookie) => request.header("cookie", cookie),
            None => request,
        }
    }

    async fn get(&self, slug: &str, path: &str, cookie: Option<&str>) -> reqwest::Response {
        self.request(reqwest::Method::GET, &format!("{slug}.{DOMAIN}"), path, cookie).send().await.unwrap()
    }

    async fn sign_in(&self, next: &str) -> (String, reqwest::Response) {
        let encoded: String = url::form_urlencoded::byte_serialize(next.as_bytes()).collect();
        let login = self.request(reqwest::Method::GET, DOMAIN, &format!("/_auth/login?next={encoded}"), None).send().await.unwrap();
        assert_eq!(login.status(), StatusCode::SEE_OTHER);

        let authorize = url::Url::parse(login.headers()["location"].to_str().unwrap()).unwrap();
        let query: HashMap<String, String> = authorize.query_pairs().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        *self.nonce.lock().unwrap() = query["nonce"].clone();

        let pending = login.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
        let callback = self.request(reqwest::Method::GET, DOMAIN, &format!("/_auth/callback?code=granted&state={}", query["state"]), Some(&pending)).send().await.unwrap();
        let session = callback.headers().get_all("set-cookie").iter().map(|value| value.to_str().unwrap().to_string()).find(|value| value.starts_with("myc_artifacts=")).unwrap();

        (session, callback)
    }

    async fn session(&self) -> String {
        let (session, _) = self.sign_in(&format!("https://private-page.{DOMAIN}/")).await;

        session.split(';').next().unwrap().to_string()
    }
}

#[tokio::test]
async fn a_private_artifact_sends_a_visitor_without_a_session_to_sign_in_and_back() {
    let harness = Harness::start().await;

    let response = harness.get("private-page", "/some/path?x=1", None).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap();
    assert!(location.starts_with(&format!("https://{DOMAIN}/_auth/login?next=")));
    assert!(location.contains(&format!("private-page.{DOMAIN}")));
}

#[tokio::test]
async fn sign_in_goes_to_the_identity_provider_with_a_pkce_challenge() {
    let harness = Harness::start().await;

    let response = harness.request(reqwest::Method::GET, DOMAIN, "/_auth/login", None).send().await.unwrap();

    let location = response.headers()["location"].to_str().unwrap();
    assert!(location.contains("/authorize?"));
    assert!(location.contains("code_challenge_method=S256"));
    assert!(location.contains(&format!("redirect_uri=https%3A%2F%2F{DOMAIN}%2F_auth%2Fcallback")));
}

#[tokio::test]
async fn signing_in_sets_a_session_cookie_for_every_subdomain_and_returns_to_the_artifact() {
    let harness = Harness::start().await;

    let (session, callback) = harness.sign_in(&format!("https://private-page.{DOMAIN}/")).await;

    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(callback.headers()["location"], format!("https://private-page.{DOMAIN}/"));
    assert!(session.contains(&format!("Domain=.{DOMAIN}")));
    assert!(session.contains("HttpOnly"));
    assert!(session.contains("Secure"));
    assert!(session.contains("SameSite=Lax"));
}

#[tokio::test]
async fn a_signed_in_visitor_reads_a_private_artifact() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;

    let response = harness.get("private-page", "/", Some(&cookie)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "<h1>Secret</h1>");
}

#[tokio::test]
async fn a_forged_session_cookie_is_sent_to_sign_in() {
    let harness = Harness::start().await;

    let response = harness.get("private-page", "/", Some("myc_artifacts=4000000000.AAAA")).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_sign_in_cannot_send_the_visitor_to_another_site() {
    let harness = Harness::start().await;

    let (_, callback) = harness.sign_in("https://evil.example/").await;

    assert_eq!(callback.headers()["location"], format!("https://{DOMAIN}/"));
}

#[tokio::test]
async fn a_public_page_is_served_without_a_login_under_a_content_security_policy() {
    let harness = Harness::start().await;

    let response = harness.get("open-page", "/", None).await;

    assert_eq!(response.status(), StatusCode::OK);
    let policy = response.headers()["content-security-policy"].to_str().unwrap().to_string();
    assert!(policy.contains("script-src 'unsafe-inline'"));
    assert!(policy.contains("style-src 'unsafe-inline'"));
    assert!(policy.contains("img-src https:"));
    assert!(policy.contains("font-src https:"));
    assert!(policy.contains("frame-ancestors 'none'"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(response.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert!(response.text().await.unwrap().contains("<h1>Hello</h1>"));
}

#[tokio::test]
async fn markdown_is_rendered_with_raw_html_and_script_links_made_inert() {
    let harness = Harness::start().await;

    let response = harness.get("notes", "/", None).await;

    let body = response.text().await.unwrap();
    assert!(body.contains("<h1>Plan</h1>"));
    assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(body.contains("&lt;b&gt;bold&lt;/b&gt;"));
    assert!(!body.contains("<script>"));
    assert!(!body.contains("javascript:"));
}

#[tokio::test]
async fn a_pdf_opens_in_the_viewer_and_its_bytes_stay_reachable_for_it() {
    let harness = Harness::start().await;

    let opened = harness.get("report", "/", None).await;
    let raw = harness.get("report", "/?raw=1", None).await;

    assert_eq!(opened.status(), 303);
    assert_eq!(opened.headers()["location"], "/_pdfjs/web/viewer.html?file=%2F%3Fraw%3D1");
    assert_eq!(raw.headers()["content-type"], "application/pdf");
    assert!(raw.headers()["content-disposition"].to_str().unwrap().starts_with("inline"));
    assert_eq!(raw.bytes().await.unwrap().as_ref(), b"%PDF-1.4");
}

#[tokio::test]
async fn the_pdf_viewer_assets_are_served_with_their_types_and_a_viewer_policy() {
    let harness = Harness::start().await;

    let page = harness.get("report", "/_pdfjs/web/viewer.html", None).await;
    let script = harness.get("report", "/_pdfjs/build/pdf.mjs", None).await;
    let missing = harness.get("report", "/_pdfjs/web/nothing.js", None).await;

    assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
    let policy = page.headers()["content-security-policy"].to_str().unwrap().to_string();
    assert!(policy.contains("script-src 'self' 'wasm-unsafe-eval'"));
    assert!(policy.contains("style-src 'self';"));
    assert!(!policy.contains("unsafe-inline"));
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
    assert_eq!(script.headers()["content-type"], "text/javascript; charset=utf-8");
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn the_pdf_viewer_loads_the_host_script_that_turns_pdf_scripting_off_before_the_viewer_starts() {
    let harness = Harness::start().await;

    let page = harness.get("report", "/_pdfjs/web/viewer.html", None).await.text().await.unwrap();
    let script = harness.get("report", "/_pdfjs/mycelium-viewer.js", None).await;
    let style = harness.get("report", "/_pdfjs/mycelium-viewer.css", None).await;

    let ours = page.find("mycelium-viewer.js").unwrap();
    assert!(ours < page.find("pdf.mjs").unwrap());
    assert!(page.contains("mycelium-viewer.css"));
    assert_eq!(script.headers()["content-type"], "text/javascript; charset=utf-8");
    let source = script.text().await.unwrap();
    assert!(source.contains("\"enableScripting\", false"));
    assert!(source.contains("\"disablePreferences\", true"));
    assert_eq!(style.headers()["content-type"], "text/css; charset=utf-8");
}

async fn save(harness: &Harness, slug: &str, cookie: Option<&str>, origin: Option<&str>, body: Vec<u8>) -> reqwest::Response {
    let request = harness.request(reqwest::Method::PUT, &format!("{slug}.{DOMAIN}"), "/_content", cookie).body(body);

    match origin {
        Some(origin) => request.header("origin", origin).send().await.unwrap(),
        None => request.send().await.unwrap(),
    }
}

#[tokio::test]
async fn a_signed_in_visitor_saves_an_edited_pdf_and_it_reaches_the_control_plane_under_their_identity() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;

    let response = save(&harness, "report", Some(&cookie), Some(&format!("https://report.{DOMAIN}")), b"%PDF-1.7 edited".to_vec()).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({"version": 2}));
    assert_eq!(*harness.control.saved.lock().unwrap(), vec![("report".to_string(), "reader".to_string(), b"%PDF-1.7 edited".to_vec())]);
}

#[tokio::test]
async fn saving_a_pdf_without_a_session_or_from_another_site_is_refused_and_nothing_is_forwarded() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;

    let signed_out = save(&harness, "report", None, None, b"%PDF-1.7".to_vec()).await;
    let cross_site = save(&harness, "report", Some(&cookie), Some("https://evil.example"), b"%PDF-1.7".to_vec()).await;

    assert_eq!((signed_out.status(), cross_site.status()), (StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN));
    assert!(harness.control.saved.lock().unwrap().is_empty());
}

#[tokio::test]
async fn saving_something_too_large_or_not_a_pdf_is_refused_and_nothing_is_forwarded() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;
    let mut huge = b"%PDF-".to_vec();
    huge.resize(21 * 1024 * 1024, 0);

    let too_large = save(&harness, "report", Some(&cookie), None, huge).await;
    let not_a_pdf = save(&harness, "report", Some(&cookie), None, b"<html>".to_vec()).await;
    let not_a_file = save(&harness, "board", Some(&cookie), None, b"%PDF-1.7".to_vec()).await;

    assert_eq!((too_large.status(), not_a_pdf.status(), not_a_file.status()), (StatusCode::PAYLOAD_TOO_LARGE, StatusCode::UNSUPPORTED_MEDIA_TYPE, StatusCode::UNSUPPORTED_MEDIA_TYPE));
    assert!(harness.control.saved.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_session_cookie_for_another_identity_cannot_save() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;
    let (claims, mac) = cookie.rsplit_once('.').unwrap();
    let (expiry, _) = claims.trim_start_matches("myc_artifacts=").split_once('.').unwrap();
    let forged = format!("myc_artifacts={expiry}.{}.{mac}", URL_SAFE_NO_PAD.encode("someone-else"));

    let refused = save(&harness, "report", Some(&forged), None, b"%PDF-1.7".to_vec()).await;

    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert!(harness.control.saved.lock().unwrap().is_empty());
}

#[tokio::test]
async fn other_files_are_downloaded() {
    let harness = Harness::start().await;

    let archive = harness.get("archive", "/", None).await;

    assert_eq!(archive.headers()["content-type"], "application/zip");
    assert!(archive.headers()["content-disposition"].to_str().unwrap().starts_with("attachment"));
    assert_eq!(archive.headers()["content-security-policy"], "sandbox");
}

#[tokio::test]
async fn an_unknown_slug_and_the_bare_domain_are_not_found_and_health_answers_on_any_host() {
    let harness = Harness::start().await;

    let unknown = harness.get("nothing-here", "/", None).await;
    let bare = harness.request(reqwest::Method::GET, DOMAIN, "/", None).send().await.unwrap();
    let health = harness.request(reqwest::Method::GET, "elsewhere.example", "/healthz", None).send().await.unwrap();

    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(unknown.text().await.unwrap().contains("Artefact not found"));
    assert_eq!(bare.status(), StatusCode::NOT_FOUND);
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test]
async fn page_data_round_trips_through_the_control_plane_for_a_signed_in_visitor() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;

    let saved = harness.request(reqwest::Method::PUT, &format!("board.{DOMAIN}"), "/_data/row-1", Some(&cookie)).json(&json!({"call": "yes"})).send().await.unwrap();
    let listed = harness.get("board", "/_data", Some(&cookie)).await;

    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(listed.json::<Value>().await.unwrap(), json!({"row-1": {"call": "yes"}}));
}

#[tokio::test]
async fn page_data_needs_a_session_to_write_and_to_read_a_private_artifact() {
    let harness = Harness::start().await;

    let private_read = harness.get("board", "/_data", None).await;
    let public_read = harness.get("open-board", "/_data", None).await;
    let public_write = harness.request(reqwest::Method::PUT, &format!("open-board.{DOMAIN}"), "/_data/row-1", None).json(&json!(1)).send().await.unwrap();

    assert_eq!(private_read.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(public_read.status(), StatusCode::OK);
    assert_eq!(public_write.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn page_data_written_from_another_origin_is_refused() {
    let harness = Harness::start().await;
    let cookie = harness.session().await;

    let response = harness.request(reqwest::Method::PUT, &format!("board.{DOMAIN}"), "/_data/row-1", Some(&cookie)).header("origin", "https://evil.example").json(&json!(1)).send().await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_design_opens_the_editor_in_view_mode_and_serves_its_document() {
    let harness = Harness::start().await;

    let root = harness.get("layout", "/", None).await;
    let viewer = harness.get("layout", "/?view=1", None).await;
    let document = harness.get("layout", "/_artifact.json", None).await;

    assert_eq!(root.status(), StatusCode::SEE_OTHER);
    assert_eq!(root.headers()["location"], "/?view=1");
    assert!(viewer.text().await.unwrap().contains("Design viewer"));
    assert_eq!(document.json::<Value>().await.unwrap(), json!({"pages": []}));
}

#[tokio::test]
async fn a_design_loads_the_viewer_assets_from_the_path_the_editor_was_built_for() {
    let harness = Harness::start().await;

    let script = harness.get("layout", "/design/assets/index.js", None).await;

    assert_eq!(script.headers()["content-type"], "text/javascript; charset=utf-8");
    assert_eq!(script.text().await.unwrap(), "export {};");
}

#[tokio::test]
async fn the_design_document_route_is_not_found_for_other_kinds() {
    let harness = Harness::start().await;

    let response = harness.get("open-page", "/_artifact.json", None).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
