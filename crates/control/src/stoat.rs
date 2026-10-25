use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const MAX_SEARCH_QUERY: usize = 64;
const IDENTIFIER_LENGTH: usize = 26;
const TIMEOUT: Duration = Duration::from_secs(30);
const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn normalise_settings(settings: &Value) -> Result<Value, String> {
    match settings["token_kind"].as_str() {
        None | Some("user") => Ok(json!({"token_kind": "user"})),
        Some("bot") => Ok(json!({"token_kind": "bot"})),
        Some(_) => Err("Choose whether the token belongs to a user session or a bot.".to_string()),
    }
}

pub fn tools() -> Vec<StoredTool> {
    let channel = json!({"type": "string", "description": "The channel's id, from list_servers."});
    let limit = json!({"type": "integer", "description": format!("How many messages to return. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});

    vec![
        StoredTool {
            name: "list_servers".to_string(),
            description: "List the servers the account is in with their channels, and its direct message and group channels, with ids.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        StoredTool {
            name: "get_messages".to_string(),
            description: "Get the most recent messages in a channel, oldest first.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "limit": limit}, "required": ["channel"]}),
        },
        StoredTool {
            name: "search_messages".to_string(),
            description: "Search one channel's messages by words. Returns the newest matches, oldest first.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "query": {"type": "string", "description": format!("Words to search for, at most {MAX_SEARCH_QUERY} characters.")}, "limit": limit}, "required": ["channel", "query"]}),
        },
        StoredTool {
            name: SEND_MESSAGE.to_string(),
            description: "Send a message to a channel. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "content": {"type": "string", "description": "The message, in Markdown."}, "reply_to": {"type": "string", "description": "The id of a message in the channel to reply to."}}, "required": ["channel", "content"]}),
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == SEND_MESSAGE
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn base(credentials: &Credentials) -> &str {
    credentials.address.trim_end_matches('/')
}

fn token_header(credentials: &Credentials) -> &'static str {
    match credentials.settings["token_kind"].as_str() {
        Some("bot") => "x-bot-token",
        _ => "x-session-token",
    }
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    http.request(method, format!("{}/{path}", base(credentials))).header(token_header(credentials), &credentials.secret).timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach Stoat. Check the server address and that the server is running, then try again.".to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Stoat did not accept this token. Check that it is current and that the token type matches it, then try again.".to_string());
    }

    if !status.is_success() {
        let reason = body["type"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(format!("Stoat answered with an error: {reason}"));
    }

    Ok(body)
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(request(http, credentials, reqwest::Method::GET, "users/@me")).await.map(|_| ())
}

fn text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn identifier(arguments: &Value, key: &str) -> Result<String, String> {
    let value = text(arguments, key).unwrap_or_default();

    if value.len() == IDENTIFIER_LENGTH && value.chars().all(|character| character.is_ascii_alphanumeric()) {
        return Ok(value.to_string());
    }

    Err(format!("Give the id in {key}, 26 letters and numbers, as listed by list_servers or get_messages."))
}

fn limit(arguments: &Value) -> u64 {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn sent_at(id: &str) -> Option<String> {
    let milliseconds = id.chars().take(10).try_fold(0i64, |total, character| Some(total * 32 + CROCKFORD.find(character.to_ascii_uppercase())? as i64))?;

    chrono::DateTime::from_timestamp_millis(milliseconds).map(|time| time.to_rfc3339())
}

fn user_name(user: &Value) -> Value {
    if user["display_name"].is_string() {
        return user["display_name"].clone();
    }

    user["username"].clone()
}

fn messages(body: &Value) -> Value {
    let list = if body.is_array() { body } else { &body["messages"] };
    let names: HashMap<&str, Value> = body["users"].as_array().into_iter().flatten().filter_map(|user| Some((user["_id"].as_str()?, user_name(user)))).collect();
    let mut listed: Vec<&Value> = list.as_array().into_iter().flatten().collect();

    listed.sort_by(|left, right| left["_id"].as_str().cmp(&right["_id"].as_str()));

    Value::Array(
        listed
            .into_iter()
            .map(|message| {
                let author = message["author"].as_str().unwrap_or_default();
                let attachments: Vec<Value> = message["attachments"].as_array().into_iter().flatten().map(|file| json!({"name": file["filename"], "size": file["size"]})).collect();

                json!({
                    "id": message["_id"],
                    "sent_at": message["_id"].as_str().and_then(sent_at),
                    "author": names.get(author).cloned().unwrap_or(Value::Null),
                    "author_id": author,
                    "content": message["content"],
                    "attachments": attachments,
                    "edited": message["edited"].is_string(),
                })
            })
            .collect(),
    )
}

async fn servers(http: &reqwest::Client, credentials: &Credentials) -> Result<Value, String> {
    let root = send(http.get(format!("{}/", base(credentials))).timeout(TIMEOUT)).await?;
    let Some(events) = root["ws"].as_str() else {
        return Err("This server did not say where its events are. Check the server address, then try again.".to_string());
    };
    let unreachable = || "Mycelium could not open Stoat's events connection to list servers. Check the server address, then try again.".to_string();
    let mut address = url::Url::parse(events).map_err(|_| unreachable())?;

    address.query_pairs_mut().append_pair("version", "1").append_pair("format", "json");
    let (mut socket, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::connect_async(address.as_str())).await.map_err(|_| unreachable())?.map_err(|_| unreachable())?;

    socket.send(Message::Text(json!({"type": "Authenticate", "token": credentials.secret}).to_string().into())).await.map_err(|_| unreachable())?;

    let ready = tokio::time::timeout(TIMEOUT, async {
        while let Some(Ok(frame)) = socket.next().await {
            let Message::Text(payload) = frame else {
                continue;
            };
            let event: Value = serde_json::from_str(payload.as_str()).unwrap_or(Value::Null);

            match event["type"].as_str() {
                Some("Ready") => return Ok(event),
                Some("Error") => return Err("Stoat did not accept this token. Check that it is current and that the token type matches it, then try again.".to_string()),
                _ => {}
            }
        }

        Err(unreachable())
    })
    .await
    .map_err(|_| unreachable())??;
    let _ = socket.close(None).await;

    let people: HashMap<&str, (Value, bool)> = ready["users"].as_array().into_iter().flatten().filter_map(|user| Some((user["_id"].as_str()?, (user_name(user), user["relationship"] == "User")))).collect();
    let channels = ready["channels"].as_array().cloned().unwrap_or_default();
    let listed: Vec<Value> = ready["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|server| {
            let inside: Vec<Value> = channels.iter().filter(|channel| channel["server"] == server["_id"]).map(|channel| json!({"id": channel["_id"], "name": channel["name"], "type": channel["channel_type"]})).collect();

            json!({"id": server["_id"], "name": server["name"], "channels": inside})
        })
        .collect();
    let direct: Vec<Value> = channels
        .iter()
        .filter(|channel| channel["server"].is_null())
        .map(|channel| {
            let others: Vec<Value> = channel["recipients"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|id| people.get(id)).filter(|(_, itself)| !itself).map(|(name, _)| name.clone()).collect();
            let name = if channel["name"].is_string() { channel["name"].clone() } else { json!(others.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")) };

            json!({"id": channel["_id"], "name": name, "type": channel["channel_type"]})
        })
        .collect();

    Ok(json!({"servers": listed, "direct_channels": direct}))
}

async fn run(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> Result<Value, String> {
    match tool {
        "list_servers" => servers(http, credentials).await,
        "get_messages" => {
            let channel = identifier(arguments, "channel")?;
            let body = send(request(http, credentials, reqwest::Method::GET, &format!("channels/{channel}/messages")).query(&[("limit", limit(arguments).to_string()), ("sort", "Latest".to_string()), ("include_users", "true".to_string())])).await?;

            Ok(messages(&body))
        }
        "search_messages" => {
            let channel = identifier(arguments, "channel")?;
            let Some(query) = text(arguments, "query") else {
                return Err("Give the words to search for in query.".to_string());
            };

            if query.chars().count() > MAX_SEARCH_QUERY {
                return Err(format!("Shorten query to {MAX_SEARCH_QUERY} characters or fewer."));
            }

            let body = send(request(http, credentials, reqwest::Method::POST, &format!("channels/{channel}/search")).json(&json!({"query": query, "limit": limit(arguments), "sort": "Latest", "include_users": true}))).await?;

            Ok(messages(&body))
        }
        SEND_MESSAGE => {
            let channel = identifier(arguments, "channel")?;
            let Some(content) = text(arguments, "content") else {
                return Err("Give the message in content.".to_string());
            };
            let mut payload = json!({"content": content});

            if text(arguments, "reply_to").is_some() {
                payload["replies"] = json!([{"id": identifier(arguments, "reply_to")?, "mention": false}]);
            }

            let body = send(request(http, credentials, reqwest::Method::POST, &format!("channels/{channel}/messages")).header("idempotency-key", uuid::Uuid::new_v4().to_string()).json(&payload)).await?;

            Ok(json!({"sent": true, "id": body["_id"]}))
        }
        _ => Err(format!("Stoat has no tool called {tool}.")),
    }
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    match run(http, credentials, tool, arguments).await {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(reason) => failure(reason),
    }
}
