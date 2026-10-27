use crate::auth::AuthUser;
use crate::chat_tool_servers::{SERVER_COLUMNS, StoredTool, ServerRow};
use crate::error::{ApiError, ApiResult};
use crate::integrations::{CallOutput, Credentials};
use crate::mcp_oauth::page;
use crate::state::{AppState, PendingLastFm, new_token};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use md5::{Digest, Md5};
use mycelium_proto::api::LastFmSignIn;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

pub const DEFAULT_ADDRESS: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTHORISE_ADDRESS: &str = "https://www.last.fm/api/auth/";
const DEFAULT_LIMIT: u64 = 10;
const MAX_LIMIT: u64 = 50;
const TIMEOUT: Duration = Duration::from_secs(30);
const REVOKED: i64 = 9;
const SIGN_IN_FIRST: &str = "Sign in to Last.fm in Settings, Integrations to love tracks and scrobble.";
const WRITES: [&str; 4] = ["love_track", "unlove_track", "scrobble", "update_now_playing"];

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    match settings["api_key"].as_str().map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => Ok(json!({"api_key": key})),
        None => Err("Enter your Last.fm API key.".to_string()),
    }
}

pub fn tools() -> Vec<StoredTool> {
    let period = json!({"type": "string", "enum": ["overall", "7day", "1month", "3month", "6month", "12month"], "description": "How far back to look. Defaults to overall."});
    let limit = json!({"type": "integer", "description": format!("How many to return. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});
    let track = json!({"type": "object", "properties": {"artist": {"type": "string"}, "track": {"type": "string"}}, "required": ["artist", "track"]});
    let song = track["properties"].clone();
    let tool = |name: &str, description: &str, input_schema: Value| StoredTool { name: name.to_string(), description: description.to_string(), input_schema, read_only: None };

    vec![
        tool("get_recent_tracks", "Get the tracks the account listened to most recently, newest first.", json!({"type": "object", "properties": {"limit": limit}})),
        tool("get_top_artists", "Get the artists the account listened to most.", json!({"type": "object", "properties": {"period": period, "limit": limit}})),
        tool("get_top_tracks", "Get the tracks the account listened to most.", json!({"type": "object", "properties": {"period": period, "limit": limit}})),
        tool("get_now_playing", "Get the track the account is playing now, if any.", json!({"type": "object", "properties": {}})),
        tool("get_track_info", "Get details of a track, including how often the account played it.", track.clone()),
        tool("love_track", "Love a track. Always needs the person's approval, and the person must have signed in to Last.fm.", json!({"type": "object", "properties": song.clone(), "required": ["artist", "track"]})),
        tool("unlove_track", "Remove a track from the loved tracks. Always needs the person's approval, and the person must have signed in to Last.fm.", json!({"type": "object", "properties": song.clone(), "required": ["artist", "track"]})),
        tool(
            "scrobble",
            "Record that a track was played. Always needs the person's approval, and the person must have signed in to Last.fm.",
            json!({"type": "object", "properties": {"artist": {"type": "string"}, "track": {"type": "string"}, "timestamp": {"type": "integer", "description": "When playback started, in Unix seconds. Defaults to now."}}, "required": ["artist", "track"]}),
        ),
        tool("update_now_playing", "Set the track shown as playing now. Always needs the person's approval, and the person must have signed in to Last.fm.", json!({"type": "object", "properties": song, "required": ["artist", "track"]})),
    ]
}

pub fn always_asks(tool: &str) -> bool {
    WRITES.contains(&tool)
}

fn signature(parameters: &[(&str, String)], secret: &str) -> String {
    let mut sorted: Vec<&(&str, String)> = parameters.iter().filter(|(name, _)| *name != "format").collect();

    sorted.sort_by(|left, right| left.0.cmp(right.0));

    let joined: String = sorted.iter().map(|(name, value)| format!("{name}{value}")).collect();
    let digest = Md5::digest(format!("{joined}{secret}").as_bytes());

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn api_key(credentials: &Credentials) -> String {
    credentials.settings["api_key"].as_str().unwrap_or_default().to_string()
}

async fn send(http: &reqwest::Client, credentials: &Credentials, method: &str, extra: &[(&str, String)], signed: bool, post: bool) -> Result<Value, (String, i64)> {
    let mut parameters: Vec<(&str, String)> = vec![("method", method.to_string()), ("api_key", api_key(credentials))];

    parameters.extend(extra.iter().map(|(name, value)| (*name, value.clone())));

    if signed {
        let signed = signature(&parameters, &credentials.secret);

        parameters.push(("api_sig", signed));
    }

    parameters.push(("format", "json".to_string()));

    let builder = match post {
        true => http.post(&credentials.address).header("content-type", "application/x-www-form-urlencoded").body(url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&parameters).finish()),
        false => http.get(&credentials.address).query(&parameters),
    };
    let unreachable = || ("Mycelium could not reach Last.fm. Try again in a moment.".to_string(), 0);
    let response = builder.timeout(TIMEOUT).send().await.map_err(|_| unreachable())?;
    let body: Value = response.json().await.map_err(|_| unreachable())?;

    match body["error"].as_i64() {
        Some(code) => Err((body["message"].as_str().unwrap_or("Last.fm refused the request.").to_string(), code)),
        None => Ok(body),
    }
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(http, credentials, "user.getInfo", &[("user", credentials.account.clone())], false, false).await.map(|_| ()).map_err(|(reason, _)| format!("Last.fm did not accept this API key and username: {reason}"))
}

fn limit(arguments: &Value) -> String {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT).to_string()
}

fn text(arguments: &Value, key: &str) -> Result<String, (String, i64)> {
    match arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value.to_string()),
        None => Err((format!("Give the {key}."), 0)),
    }
}

fn track_of(track: &Value) -> Value {
    json!({
        "artist": track["artist"]["#text"].as_str().or_else(|| track["artist"]["name"].as_str()),
        "track": track["name"],
        "album": track["album"]["#text"],
        "playing_now": track["@attr"]["nowplaying"] == "true",
        "played_at": track["date"]["uts"],
        "play_count": track["playcount"],
    })
}

fn listed(body: &Value, container: &str, item: &str) -> Value {
    let items = body[container][item].as_array().cloned().unwrap_or_default();

    Value::Array(items.iter().map(track_of).collect())
}

async fn run(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> Result<Value, (String, i64)> {
    let user = ("user", credentials.account.clone());

    match tool {
        "get_recent_tracks" => Ok(listed(&send(http, credentials, "user.getRecentTracks", &[user, ("limit", limit(arguments))], false, false).await?, "recenttracks", "track")),
        "get_now_playing" => {
            let body = send(http, credentials, "user.getRecentTracks", &[user, ("limit", "1".to_string())], false, false).await?;
            let tracks = listed(&body, "recenttracks", "track");
            let playing = tracks.as_array().and_then(|tracks| tracks.iter().find(|track| track["playing_now"] == true)).cloned();

            Ok(json!({"now_playing": playing}))
        }
        "get_top_artists" | "get_top_tracks" => {
            let period = arguments["period"].as_str().unwrap_or("overall").to_string();
            let (method, container, item) = if tool == "get_top_artists" { ("user.getTopArtists", "topartists", "artist") } else { ("user.getTopTracks", "toptracks", "track") };
            let body = send(http, credentials, method, &[user, ("period", period), ("limit", limit(arguments))], false, false).await?;
            let items = body[container][item].as_array().cloned().unwrap_or_default();

            Ok(Value::Array(items.iter().map(|entry| json!({"name": entry["name"], "artist": entry["artist"]["name"], "play_count": entry["playcount"]})).collect()))
        }
        "get_track_info" => {
            let body = send(http, credentials, "track.getInfo", &[("artist", text(arguments, "artist")?), ("track", text(arguments, "track")?), ("username", credentials.account.clone())], false, false).await?;
            let info = &body["track"];

            Ok(json!({"artist": info["artist"]["name"], "track": info["name"], "listeners": info["listeners"], "play_count": info["playcount"], "your_play_count": info["userplaycount"], "loved": info["userloved"] == "1"}))
        }
        _ if always_asks(tool) => {
            let Some(session) = credentials.session.as_deref() else {
                return Err((SIGN_IN_FIRST.to_string(), REVOKED));
            };
            let method = match tool {
                "love_track" => "track.love",
                "unlove_track" => "track.unlove",
                "scrobble" => "track.scrobble",
                _ => "track.updateNowPlaying",
            };
            let mut parameters = vec![("artist", text(arguments, "artist")?), ("track", text(arguments, "track")?), ("sk", session.to_string())];

            if tool == "scrobble" {
                let timestamp = arguments["timestamp"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp());

                parameters.push(("timestamp", timestamp.to_string()));
            }

            send(http, credentials, method, &parameters, true, true).await?;

            Ok(json!({"done": true}))
        }
        _ => Err((format!("Last.fm has no tool called {tool}."), 0)),
    }
}

pub async fn call(state: &AppState, owner: Uuid, server: &ServerRow, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    match run(&state.http, credentials, tool, arguments).await {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err((_, REVOKED)) => {
            if credentials.session.is_some() {
                let _ = sqlx::query("update chat_tool_servers set session_key = null where id = $1").bind(server.id).execute(&state.pool).await;
                state.hub.tools_changed(owner);
                state.publish(Some(owner), StreamMessage::IntegrationsChanged);
            }

            CallOutput { text: SIGN_IN_FIRST.to_string(), is_error: true }
        }
        Err((reason, _)) => CallOutput { text: reason, is_error: true },
    }
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ServerRow> {
    let row = sqlx::query_as::<_, ServerRow>(sqlx::AssertSqlSafe(format!("select {SERVER_COLUMNS} from chat_tool_servers where id = $1 and owner = $2 and kind = 'lastfm'"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That Last.fm integration"))
}

pub async fn start_sign_in(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<LastFmSignIn>> {
    let row = find_owned(&state, id, user.id).await?;
    let sign_in = new_token();
    let callback = format!("{}/integrations/lastfm/callback?sign_in={sign_in}", state.config.public_url);
    let mut authorise = url::Url::parse(AUTHORISE_ADDRESS).map_err(ApiError::internal)?;

    authorise.query_pairs_mut().append_pair("api_key", &row.credentials().settings["api_key"].as_str().unwrap_or_default().to_string()).append_pair("cb", &callback);
    state.hub.track_lastfm(sign_in, PendingLastFm { owner: user.id, server: id, created: std::time::Instant::now() });

    Ok(Json(LastFmSignIn { authorize_url: authorise.to_string() }))
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    sign_in: Option<String>,
    token: Option<String>,
}

pub async fn callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> Response {
    let Some(pending) = query.sign_in.as_deref().and_then(|sign_in| state.hub.take_lastfm(sign_in)) else {
        return page(StatusCode::BAD_REQUEST, "This sign-in has expired. Start it again from Settings, Integrations.");
    };
    let Some(token) = query.token else {
        return page(StatusCode::BAD_REQUEST, "Last.fm did not sign you in. Start it again from Settings, Integrations.");
    };
    let Ok(row) = find_owned(&state, pending.server, pending.owner).await else {
        return page(StatusCode::BAD_REQUEST, "This Last.fm integration no longer exists.");
    };
    let session = send(&state.http, &row.credentials(), "auth.getSession", &[("token", token)], true, false).await;
    let Some(key) = session.ok().and_then(|body| body["session"]["key"].as_str().map(str::to_string)) else {
        return page(StatusCode::BAD_GATEWAY, "Last.fm did not give Mycelium access. Start the sign-in again from Settings, Integrations.");
    };

    let stored = sqlx::query("update chat_tool_servers set session_key = $2 where id = $1").bind(row.id).bind(&key).execute(&state.pool).await;

    if stored.is_err() {
        return page(StatusCode::INTERNAL_SERVER_ERROR, "Mycelium could not save the sign-in. Try again.");
    }

    state.hub.tools_changed(pending.owner);
    state.publish(Some(pending.owner), StreamMessage::IntegrationsChanged);

    page(StatusCode::OK, &format!("{} is signed in to Last.fm. You can close this tab and return to Mycelium.", row.name))
}

pub async fn sign_out(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state, id, user.id).await?;
    sqlx::query("update chat_tool_servers set session_key = null where id = $1").bind(id).execute(&state.pool).await?;
    state.hub.tools_changed(user.id);
    state.publish(Some(user.id), StreamMessage::IntegrationsChanged);

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::signature;

    #[test]
    fn api_sig_sorts_parameters_and_appends_the_secret() {
        let parameters = [("method", "auth.getSession".to_string()), ("token", "abc".to_string()), ("api_key", "key".to_string()), ("format", "json".to_string())];

        assert_eq!(signature(&parameters, "secret"), "6629efc98b97f7c35ff32314185ffaa1");
    }
}
