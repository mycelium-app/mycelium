use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use crate::message_paging::{self, Direction};
use serde_json::{Value, json};
use std::time::Duration;

pub const DEFAULT_ADDRESS: &str = "https://discord.com/api/v10";
const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const SEARCH_PAGE_SIZE: u64 = 25;
const MAX_MESSAGE_CHARACTERS: usize = 2000;
const SNOWFLAKE_LENGTH: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn tools() -> Vec<StoredTool> {
    let server = json!({"type": "string", "description": "The server's id, from list_servers."});
    let channel = json!({"type": "string", "description": "The channel's id, from list_channels."});
    let limit = json!({"type": "integer", "description": format!("How many messages to return, newest first. Defaults to {DEFAULT_LIMIT}, at most {}. Larger limits are read in several requests.", message_paging::MOST_MESSAGES)});
    let before = json!({"type": "string", "description": "A message id: return the messages sent before it. Pass a result's coverage.next to continue reading further back."});
    let after = json!({"type": "string", "description": "A message id: return the messages sent after it. Pass a result's coverage.next to continue reading forwards."});
    let coverage = "Every result is {messages, coverage}: coverage says how many messages came back, the oldest_id and newest_id returned, whether found_oldest and found_newest (nothing older or newer remains), and next, the value to pass on to continue (null when there is nothing more).";

    vec![
        StoredTool { name: "list_servers".to_string(), description: "List the Discord servers the bot is in, with their ids.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool { name: "list_channels".to_string(), description: "List the channels of a server, with their ids, kinds and categories.".to_string(), input_schema: json!({"type": "object", "properties": {"server": server}, "required": ["server"]}), read_only: None },
        StoredTool { name: "get_messages".to_string(), description: format!("Get the most recent messages in a channel, newest first. Page back with before, or forwards with after. {coverage}"), input_schema: json!({"type": "object", "properties": {"channel": channel, "limit": limit, "before": before, "after": after}, "required": ["channel"]}), read_only: None },
        StoredTool {
            name: "search_messages".to_string(),
            description: format!("Search a server's messages by words, optionally within one channel. Returns the newest matches. Pass a result's coverage.next as offset to continue. {coverage}"),
            input_schema: json!({"type": "object", "properties": {"server": server, "query": {"type": "string", "description": "Words to search for."}, "channel": channel, "limit": limit, "offset": {"type": "integer", "description": "How many matches to skip."}}, "required": ["server", "query"]}),
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
    CallOutput { text: text.into(), is_error: true, binary: None, images: Vec::new() }
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

fn limit(arguments: &Value) -> u64 {
    message_paging::total_limit(arguments["limit"].as_u64(), DEFAULT_LIMIT)
}

fn message_id(message: &Value) -> Option<&str> {
    message["id"].as_str()
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

            let (direction, start) = match (text(arguments, "before"), text(arguments, "after")) {
                (Some(_), Some(_)) => return failure("Give either before or after, not both."),
                (None, Some(_)) => (Direction::Forward, snowflake(arguments, "after")),
                (_, _) => (Direction::Backward, snowflake(arguments, "before")),
            };

            if (text(arguments, "before").is_some() && snowflake(arguments, "before").is_none()) || (text(arguments, "after").is_some() && snowflake(arguments, "after").is_none()) {
                return failure("Give the message id in before or after, digits only, as returned by get_messages.");
            }

            let cursor_name = if direction == Direction::Forward { "after" } else { "before" };
            let collected = message_paging::collect(limit(arguments), direction, start.map(str::to_string), message_id, |size, cursor| {
                let mut path = format!("channels/{channel}/messages?limit={size}");

                if let Some(cursor) = cursor {
                    path.push_str(&format!("&{cursor_name}={cursor}"));
                }

                async move { send(request(http, credentials, reqwest::Method::GET, &path)).await.map(|body| listed(&body)) }
            })
            .await;

            collected.map(|collected| json!({"messages": collected.messages.iter().rev().map(message_of).collect::<Vec<_>>(), "coverage": collected.coverage}))
        }
        "search_messages" => {
            let (Some(server), Some(query)) = (snowflake(arguments, "server"), text(arguments, "query")) else {
                return failure("Give the server's id in server and the words to search for in query.");
            };
            let channel = match text(arguments, "channel") {
                Some(_) => match snowflake(arguments, "channel") {
                    Some(channel) => Some(channel),
                    None => return failure("Give the channel's id in channel, digits only, as listed by list_channels."),
                },
                None => None,
            };
            let first_offset = arguments["offset"].as_u64().unwrap_or(0);
            let gathered = message_paging::gather(
                limit(arguments),
                SEARCH_PAGE_SIZE,
                Some(first_offset.to_string()),
                |size, offset| {
                    let size = size.to_string();
                    let offset = offset.unwrap_or_default();
                    let mut serializer = url::form_urlencoded::Serializer::new(String::new());

                    serializer.append_pair("content", query).append_pair("limit", &size).append_pair("offset", &offset);

                    if let Some(channel) = channel {
                        serializer.append_pair("channel_id", channel);
                    }

                    let path = format!("guilds/{server}/messages/search?{}", serializer.finish());

                    async move { send(request(http, credentials, reqwest::Method::GET, &path)).await.map(|body| listed(&body["messages"])) }
                },
                |_, gathered| Some((first_offset + gathered as u64).to_string()),
            )
            .await;

            gathered.map(|gathered| {
                let next = if gathered.exhausted { None } else { gathered.cursor };
                let oldest = gathered.messages.last().and_then(message_id);
                let newest = gathered.messages.first().and_then(message_id);
                let coverage = message_paging::coverage(gathered.messages.len(), oldest, newest, gathered.exhausted, first_offset == 0, next);

                json!({"messages": gathered.messages.iter().map(message_of).collect::<Vec<_>>(), "coverage": coverage})
            })
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
        Ok(value) => CallOutput { text: value.to_string(), is_error: false, binary: None, images: Vec::new() },
        Err(reason) => failure(reason),
    }
}
