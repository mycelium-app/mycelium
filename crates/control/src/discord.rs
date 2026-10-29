use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use serde_json::{Value, json};
use std::time::Duration;

pub const DEFAULT_ADDRESS: &str = "https://discord.com/api/v10";
const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const MAX_SEARCH_LIMIT: u64 = 25;
const MAX_MESSAGE_CHARACTERS: usize = 2000;
const SNOWFLAKE_LENGTH: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn tools() -> Vec<StoredTool> {
    let server = json!({"type": "string", "description": "The server's id, from list_servers."});
    let channel = json!({"type": "string", "description": "The channel's id, from list_channels."});
    let limit = json!({"type": "integer", "description": format!("How many messages to return, newest first. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT} (at most {MAX_SEARCH_LIMIT} when searching).")});

    vec![
        StoredTool { name: "list_servers".to_string(), description: "List the Discord servers the bot is in, with their ids.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool { name: "list_channels".to_string(), description: "List the channels of a server, with their ids, kinds and categories.".to_string(), input_schema: json!({"type": "object", "properties": {"server": server}, "required": ["server"]}), read_only: None },
        StoredTool { name: "get_messages".to_string(), description: "Get the most recent messages in a channel, newest first.".to_string(), input_schema: json!({"type": "object", "properties": {"channel": channel, "limit": limit}, "required": ["channel"]}), read_only: None },
        StoredTool {
            name: "search_messages".to_string(),
            description: "Search a server's messages by words, optionally within one channel. Returns the newest matches.".to_string(),
            input_schema: json!({"type": "object", "properties": {"server": server, "query": {"type": "string", "description": "Words to search for."}, "channel": channel, "limit": limit}, "required": ["server", "query"]}),
            read_only: None,
        },
        StoredTool {
            name: SEND_MESSAGE.to_string(),
            description: "Send a message to a channel. Always needs the person's approval in chats, even when Always allow is on. Mycelium opens the message with a line saying the Mycelium agent posted it on the person's behalf.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "content": {"type": "string", "description": "The message, in Discord Markdown."}}, "required": ["channel", "content"]}),
            read_only: None,
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == SEND_MESSAGE
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');

    http.request(method, format!("{base}/{path}")).header("authorization", format!("Bot {}", credentials.secret)).timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach Discord. Try again in a moment.".to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Discord did not accept this bot token. Copy it again from the Bot page of your application in the Discord Developer Portal, then try again.".to_string());
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        return Err("The bot is not allowed to do that here. Give it the View Channel, Read Message History and Send Messages permissions for this channel in Discord, then try again.".to_string());
    }

    if status == reqwest::StatusCode::ACCEPTED {
        return Err("Discord is still indexing this server for search. Try again in a moment.".to_string());
    }

    if !status.is_success() {
        let reason = body["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(format!("Discord answered with an error: {reason}"));
    }

    Ok(body)
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(request(http, credentials, reqwest::Method::GET, "users/@me")).await.map(|_| ())
}

fn text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn snowflake<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    text(arguments, key).filter(|value| value.len() <= SNOWFLAKE_LENGTH && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn limit(arguments: &Value, most: u64) -> u64 {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, most)
}

fn message_of(message: &Value) -> Value {
    json!({"id": message["id"], "channel": message["channel_id"], "sent_at": message["timestamp"], "author": message["author"]["global_name"].as_str().or_else(|| message["author"]["username"].as_str()), "content": message["content"]})
}

fn listed(body: &Value) -> Vec<Value> {
    body.as_array().into_iter().flatten().flat_map(|entry| entry.as_array().cloned().unwrap_or_else(|| vec![entry.clone()])).collect()
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    let outcome = match tool {
        "list_servers" => send(request(http, credentials, reqwest::Method::GET, "users/@me/guilds"))
            .await
            .map(|body| Value::Array(body.as_array().into_iter().flatten().map(|server| json!({"id": server["id"], "name": server["name"]})).collect())),
        "list_channels" => {
            let Some(server) = snowflake(arguments, "server") else {
                return failure("Give the server's id in server, digits only, as listed by list_servers.");
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("guilds/{server}/channels")))
                .await
                .map(|body| Value::Array(body.as_array().into_iter().flatten().map(|channel| json!({"id": channel["id"], "name": channel["name"], "kind": channel["type"], "category": channel["parent_id"]})).collect()))
        }
        "get_messages" => {
            let Some(channel) = snowflake(arguments, "channel") else {
                return failure("Give the channel's id in channel, digits only, as listed by list_channels.");
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("channels/{channel}/messages?limit={}", limit(arguments, MAX_LIMIT))))
                .await
                .map(|body| Value::Array(listed(&body).iter().map(message_of).collect()))
        }
        "search_messages" => {
            let (Some(server), Some(query)) = (snowflake(arguments, "server"), text(arguments, "query")) else {
                return failure("Give the server's id in server and the words to search for in query.");
            };
            let mut path = format!("guilds/{server}/messages/search?{}", url::form_urlencoded::Serializer::new(String::new()).append_pair("content", query).append_pair("limit", &limit(arguments, MAX_SEARCH_LIMIT).to_string()).finish());

            if text(arguments, "channel").is_some() {
                let Some(channel) = snowflake(arguments, "channel") else {
                    return failure("Give the channel's id in channel, digits only, as listed by list_channels.");
                };

                path.push_str(&format!("&channel_id={channel}"));
            }

            send(request(http, credentials, reqwest::Method::GET, &path)).await.map(|body| Value::Array(listed(&body["messages"]).iter().map(message_of).collect()))
        }
        SEND_MESSAGE => {
            let (Some(channel), Some(content)) = (snowflake(arguments, "channel"), text(arguments, "content")) else {
                return failure("Give the channel's id in channel and the message in content.");
            };
            let person = credentials.account.trim();
            let person = if person.is_empty() { "the person who connected this bot" } else { person };
            let message = format!("*Posted by the Mycelium agent on behalf of {person}.*\n\n{content}");

            if message.chars().count() > MAX_MESSAGE_CHARACTERS {
                return failure(format!("Discord messages hold at most {MAX_MESSAGE_CHARACTERS} characters, including the line saying the Mycelium agent posted it. Shorten the message and send it again."));
            }

            send(request(http, credentials, reqwest::Method::POST, &format!("channels/{channel}/messages")).json(&json!({"content": message, "allowed_mentions": {"parse": []}})))
                .await
                .map(|body| json!({"id": body["id"], "channel": body["channel_id"]}))
        }
        _ => Err(format!("Discord has no tool called {tool}.")),
    };

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(reason) => failure(reason),
    }
}
