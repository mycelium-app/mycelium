use axum::Router;
use axum::extract::State;
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Json;
use crate::common::{Server, serve};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const API_KEY: &str = "key-1234";
const SECRET: &str = "secret-5678";
const SESSION: &str = "session-abcd";

#[derive(Default)]
struct FakeLastFm {
    calls: Mutex<Vec<(Method, Vec<(String, String)>)>>,
    revoked: Mutex<bool>,
}

impl FakeLastFm {
    fn methods(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().filter_map(|(_, parameters)| value(parameters, "method")).collect()
    }

    fn last(&self, method: &str) -> Option<(Method, Vec<(String, String)>)> {
        self.calls.lock().unwrap().iter().rev().find(|(_, parameters)| value(parameters, "method").as_deref() == Some(method)).cloned()
    }
}

fn value(parameters: &[(String, String)], name: &str) -> Option<String> {
    parameters.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
}

fn signed_correctly(parameters: &[(String, String)]) -> bool {
    let mut sorted: Vec<&(String, String)> = parameters.iter().filter(|(name, _)| name != "format" && name != "api_sig").collect();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let joined: String = sorted.iter().map(|(name, value)| format!("{name}{value}")).collect();
    let expected: String = Md5::digest(format!("{joined}{SECRET}").as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();

    value(parameters, "api_sig").as_deref() == Some(expected.as_str())
}

async fn handle(State(fake): State<Arc<FakeLastFm>>, method: Method, uri: axum::http::Uri, body: String) -> Response {
    let source = if method == Method::POST { body } else { uri.query().unwrap_or_default().to_string() };
    let parameters: Vec<(String, String)> = url::form_urlencoded::parse(source.as_bytes()).into_owned().collect();
    fake.calls.lock().unwrap().push((method, parameters.clone()));

    if value(&parameters, "api_key").as_deref() != Some(API_KEY) {
        return Json(json!({"error": 10, "message": "Invalid API key"})).into_response();
    }

    let name = value(&parameters, "method").unwrap_or_default();

    if name.starts_with("track.") && name != "track.getInfo" || name == "auth.getSession" {
        if !signed_correctly(&parameters) {
            return Json(json!({"error": 13, "message": "Invalid method signature supplied"})).into_response();
        }

        if *fake.revoked.lock().unwrap() && name != "auth.getSession" {
            return Json(json!({"error": 9, "message": "Invalid session key - Please re-authenticate"})).into_response();
        }
    }

    match name.as_str() {
        "user.getInfo" => Json(json!({"user": {"name": "ada"}})).into_response(),
        "user.getRecentTracks" => Json(json!({"recenttracks": {"track": [
            {"name": "Teardrop", "artist": {"#text": "Massive Attack"}, "album": {"#text": "Mezzanine"}, "@attr": {"nowplaying": "true"}},
            {"name": "Angel", "artist": {"#text": "Massive Attack"}, "album": {"#text": "Mezzanine"}, "date": {"uts": "1700000000"}}
        ]}})).into_response(),
        "user.getTopArtists" => Json(json!({"topartists": {"artist": [{"name": "Portishead", "playcount": "42"}]}})).into_response(),
        "user.getTopTracks" => Json(json!({"toptracks": {"track": [{"name": "Roads", "artist": {"name": "Portishead"}, "playcount": "7"}]}})).into_response(),
        "track.getInfo" => Json(json!({"track": {"name": "Roads", "artist": {"name": "Portishead"}, "listeners": "100", "playcount": "1000", "userplaycount": "7", "userloved": "1"}})).into_response(),
        "auth.getSession" if value(&parameters, "token").as_deref() == Some("token-1") => Json(json!({"session": {"name": "ada", "key": SESSION}})).into_response(),
        "auth.getSession" => Json(json!({"error": 4, "message": "Invalid authentication token"})).into_response(),
        "track.love" | "track.unlove" | "track.scrobble" | "track.updateNowPlaying" => Json(json!({})).into_response(),
        _ => Json(json!({"error": 3, "message": "Invalid Method"})).into_response(),
    }
}

async fn fake_lastfm() -> (String, Arc<FakeLastFm>) {
    let fake = Arc::new(FakeLastFm::default());
    let base = serve(Router::new().route("/", any(handle)).with_state(fake.clone())).await;

    (base, fake)
}

async fn add_lastfm(server: &Server, base: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Last.fm", "kind": "lastfm", "url": base, "account": "ada", "bearer_token": SECRET, "settings": {"api_key": API_KEY}}))).await
}

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn call(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("last_fm__{tool}"), "arguments": arguments}});
    let reply = if matches!(tool, "love_track" | "unlove_track" | "scrobble" | "update_now_playing") { server.approved_agent_mcp(token, body).await } else { mcp(server, token, body).await };

    reply["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn sign_in(server: &Server, id: &str, token: &str) -> (u16, String) {
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/chat-tool-servers/{id}/lastfm-sign-in"))).await;
    assert_eq!(status, 200, "{started}");
    let authorise = url::Url::parse(started["authorize_url"].as_str().unwrap()).unwrap();
    assert_eq!(authorise.host_str(), Some("www.last.fm"));
    let query: Vec<(String, String)> = authorise.query_pairs().into_owned().collect();
    assert_eq!(value(&query, "api_key").as_deref(), Some(API_KEY));
    let callback = value(&query, "cb").unwrap();
    let response = server.anonymous().http.get(format!("{callback}&token={token}")).send().await.unwrap();

    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn reads_work_without_signing_in() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_lastfm().await;
    let (status, added) = add_lastfm(&server, &base).await;

    let recent = text(&call(&server, &node.token, "get_recent_tracks", json!({})).await);
    let playing = text(&call(&server, &node.token, "get_now_playing", json!({})).await);
    let artists = text(&call(&server, &node.token, "get_top_artists", json!({"period": "7day"})).await);
    let tracks = text(&call(&server, &node.token, "get_top_tracks", json!({})).await);
    let info = text(&call(&server, &node.token, "get_track_info", json!({"artist": "Portishead", "track": "Roads"})).await);
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 201, "{added}");
    assert!(recent.contains("Angel") && recent.contains("Teardrop"), "{recent}");
    assert!(playing.contains("Teardrop") && !playing.contains("Angel"), "{playing}");
    assert!(artists.contains("Portishead"), "{artists}");
    assert!(tracks.contains("Roads"), "{tracks}");
    assert!(info.contains("\"loved\":true"), "{info}");
    assert_eq!(value(&fake.last("user.getTopArtists").unwrap().1, "period").as_deref(), Some("7day"));
    assert!(!stored.contains(SECRET));
    assert_eq!((added["signed_in"].clone(), added["settings"].clone()), (json!(null), json!({"api_key": API_KEY})));
}

#[tokio::test]
async fn adding_last_fm_checks_the_key_and_asks_for_a_username() {
    let server = Server::start().await;
    let (base, _) = fake_lastfm().await;
    let post = |body: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body));

    let (_, wrong_key) = post(json!({"name": "Last.fm", "kind": "lastfm", "url": base, "account": "ada", "bearer_token": SECRET, "settings": {"api_key": "wrong"}})).await;
    let (_, no_key) = post(json!({"name": "Last.fm", "kind": "lastfm", "url": base, "account": "ada", "bearer_token": SECRET})).await;
    let (_, no_user) = post(json!({"name": "Last.fm", "kind": "lastfm", "url": base, "bearer_token": SECRET, "settings": {"api_key": API_KEY}})).await;

    assert_eq!(wrong_key["error"], "tool_server_failed");
    assert_eq!(no_key["error"], "settings_invalid");
    assert_eq!((no_user["error"].clone(), no_user["message"].clone()), (json!("account_required"), json!("Enter your Last.fm username.")));
}

#[tokio::test]
async fn writes_without_a_session_ask_to_sign_in() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_lastfm().await;
    add_lastfm(&server, &base).await;

    let result = call(&server, &node.token, "love_track", json!({"artist": "Portishead", "track": "Roads"})).await;

    assert_eq!(result["isError"], true);
    assert_eq!(text(&result), "Sign in to Last.fm in Settings, Integrations to love tracks and scrobble.");
    assert!(fake.last("track.love").is_none());
}

#[tokio::test]
async fn sign_in_stores_a_session_and_writes_are_signed() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_lastfm().await;
    let (_, added) = add_lastfm(&server, &base).await;
    let id = added["id"].as_str().unwrap();

    let (status, page) = sign_in(&server, id, "token-1").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let loved = call(&server, &node.token, "love_track", json!({"artist": "Portishead", "track": "Roads"})).await;
    let scrobbled = call(&server, &node.token, "scrobble", json!({"artist": "Portishead", "track": "Roads", "timestamp": 1700000000})).await;

    assert_eq!(status, 200, "{page}");
    assert_eq!(listed[0]["signed_in"], true);
    assert!(!listed.to_string().contains(SESSION));
    assert_ne!(loved["isError"], true, "{loved}");
    assert_ne!(scrobbled["isError"], true, "{scrobbled}");
    let (method, parameters) = fake.last("track.love").unwrap();
    assert_eq!((method, value(&parameters, "sk").as_deref()), (Method::POST, Some(SESSION)));
    assert_eq!(value(&fake.last("track.scrobble").unwrap().1, "timestamp").as_deref(), Some("1700000000"));
    assert!(fake.methods().contains(&"auth.getSession".to_string()));

    let out = server.user.delete(&format!("/api/v1/chat-tool-servers/{id}/lastfm-sign-in")).send().await.unwrap().status().as_u16();
    let (_, after) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    assert_eq!(out, 204);
    assert_eq!(after[0].get("signed_in"), None);
}

#[tokio::test]
async fn a_revoked_session_is_cleared_and_reads_keep_working() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (base, fake) = fake_lastfm().await;
    let (_, added) = add_lastfm(&server, &base).await;
    sign_in(&server, added["id"].as_str().unwrap(), "token-1").await;
    *fake.revoked.lock().unwrap() = true;

    let loved = call(&server, &node.token, "love_track", json!({"artist": "Portishead", "track": "Roads"})).await;
    let recent = text(&call(&server, &node.token, "get_recent_tracks", json!({})).await);
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(text(&loved), "Sign in to Last.fm in Settings, Integrations to love tracks and scrobble.");
    assert!(recent.contains("Teardrop"));
    assert_eq!(listed[0].get("signed_in"), None);
}

#[tokio::test]
async fn an_expired_or_rejected_sign_in_stores_nothing() {
    let server = Server::start().await;
    let (base, _) = fake_lastfm().await;
    let (_, added) = add_lastfm(&server, &base).await;
    let anonymous = server.anonymous();

    let unknown = anonymous.http.get(format!("{}/integrations/lastfm/callback?sign_in=unknown&token=token-1", server.base)).send().await.unwrap();
    let (rejected, page) = sign_in(&server, added["id"].as_str().unwrap(), "bad-token").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(unknown.status().as_u16(), 400);
    assert_eq!(rejected, 502, "{page}");
    assert_eq!(listed[0].get("signed_in"), None);
}
