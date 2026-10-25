use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use serde_json::{Value, json};
use std::time::Duration;

const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn tools() -> Vec<StoredTool> {
    let channel = json!({"type": "string", "description": "The channel's name."});
    let people = json!({"type": "array", "items": {"type": "string"}, "description": "Email addresses of the other people in a direct message conversation."});
    let limit = json!({"type": "integer", "description": format!("How many messages to return, newest last. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});

    vec![
        StoredTool { name: "list_channels".to_string(), description: "List the channels (streams) the person is subscribed to, with their ids and descriptions.".to_string(), input_schema: json!({"type": "object", "properties": {}}) },
        StoredTool {
            name: "get_messages".to_string(),
            description: "Get the most recent messages in a channel, optionally narrowed to one topic, or in a direct message conversation. Give either channel or direct_with.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "topic": {"type": "string", "description": "A topic in the channel. Needs channel."}, "direct_with": people, "limit": limit}}),
        },
        StoredTool {
            name: "search_messages".to_string(),
            description: "Search messages by keywords, optionally within one channel. Returns the newest matches.".to_string(),
            input_schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "Keywords to search for."}, "channel": channel, "limit": limit}, "required": ["query"]}),
        },
        StoredTool {
            name: SEND_MESSAGE.to_string(),
            description: "Send a message to a channel topic, or directly to one or more people. Always needs the person's approval in chats, even when Always allow is on. Give either channel and topic, or direct_to.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "topic": {"type": "string", "description": "The topic to post under. Needs channel."}, "direct_to": people, "content": {"type": "string", "description": "The message, in Zulip Markdown."}}, "required": ["content"]}),
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == SEND_MESSAGE
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn encoded(pairs: &[(&str, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish()
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');

    http.request(method, format!("{base}/api/v1/{path}")).basic_auth(&credentials.account, Some(&credentials.secret)).timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach Zulip. Check the Zulip address and that the server is running, then try again.".to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Zulip did not accept this email and API key. Check them in Zulip under Personal settings, Account and privacy, then try again.".to_string());
    }

    if !status.is_success() || body["result"] == "error" {
        let reason = body["msg"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(format!("Zulip answered with an error: {reason}"));
    }

    Ok(body)
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(request(http, credentials, reqwest::Method::GET, "users/me")).await.map(|_| ())
}

fn text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments[key].as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn people(arguments: &Value, key: &str) -> Vec<String> {
    arguments[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|person| !person.is_empty()).map(str::to_string).collect()
}

fn limit(arguments: &Value) -> u64 {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn operator(name: &str, operand: &str) -> Value {
    json!({"operator": name, "operand": operand})
}

async fn messages(http: &reqwest::Client, credentials: &Credentials, narrow: Vec<Value>, limit: u64) -> CallOutput {
    let query = [("anchor", "newest".to_string()), ("num_before", limit.to_string()), ("num_after", "0".to_string()), ("apply_markdown", "false".to_string()), ("narrow", Value::Array(narrow).to_string())];

    match send(request(http, credentials, reqwest::Method::GET, &format!("messages?{}", encoded(&query)))).await {
        Ok(body) => {
            let listed: Vec<Value> = body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|message| {
                    let place = match message["type"].as_str() {
                        Some("stream") => json!({"channel": message["display_recipient"], "topic": message["subject"]}),
                        _ => json!({"direct_with": message["display_recipient"].as_array().into_iter().flatten().filter_map(|person| person["email"].as_str()).collect::<Vec<_>>()}),
                    };

                    json!({"id": message["id"], "sent_at": message["timestamp"], "sender": message["sender_full_name"], "sender_email": message["sender_email"], "place": place, "content": message["content"]})
                })
                .collect();

            CallOutput { text: Value::Array(listed).to_string(), is_error: false }
        }
        Err(reason) => failure(reason),
    }
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    match tool {
        "list_channels" => match send(request(http, credentials, reqwest::Method::GET, "users/me/subscriptions")).await {
            Ok(body) => {
                let channels: Vec<Value> = body["subscriptions"].as_array().into_iter().flatten().map(|channel| json!({"id": channel["stream_id"], "name": channel["name"], "description": channel["description"]})).collect();

                CallOutput { text: Value::Array(channels).to_string(), is_error: false }
            }
            Err(reason) => failure(reason),
        },
        "get_messages" => {
            let channel = text(arguments, "channel");
            let direct = people(arguments, "direct_with");

            match (channel, direct.is_empty()) {
                (Some(channel), true) => {
                    let mut narrow = vec![operator("stream", channel)];

                    if let Some(topic) = text(arguments, "topic") {
                        narrow.push(operator("topic", topic));
                    }

                    messages(http, credentials, narrow, limit(arguments)).await
                }
                (None, false) => messages(http, credentials, vec![operator("pm-with", &direct.join(","))], limit(arguments)).await,
                _ => failure("Give either channel or direct_with, not both and not neither."),
            }
        }
        "search_messages" => {
            let Some(query) = text(arguments, "query") else {
                return failure("Give the keywords to search for in query.");
            };
            let mut narrow = vec![operator("search", query)];

            if let Some(channel) = text(arguments, "channel") {
                narrow.insert(0, operator("stream", channel));
            }

            messages(http, credentials, narrow, limit(arguments)).await
        }
        SEND_MESSAGE => {
            let Some(content) = text(arguments, "content") else {
                return failure("Give the message in content.");
            };
            let channel = text(arguments, "channel");
            let direct = people(arguments, "direct_to");
            let form = match (channel, direct.is_empty()) {
                (Some(channel), true) => {
                    let Some(topic) = text(arguments, "topic") else {
                        return failure("Give the topic to post under in topic.");
                    };

                    vec![("type", "stream".to_string()), ("to", channel.to_string()), ("topic", topic.to_string()), ("content", content.to_string())]
                }
                (None, false) => vec![("type", "private".to_string()), ("to", json!(direct).to_string()), ("content", content.to_string())],
                _ => return failure("Give either channel and topic, or direct_to, not both and not neither."),
            };

            match send(request(http, credentials, reqwest::Method::POST, "messages").header("content-type", "application/x-www-form-urlencoded").body(encoded(&form))).await {
                Ok(body) => CallOutput { text: json!({"sent": true, "id": body["id"]}).to_string(), is_error: false },
                Err(reason) => failure(reason),
            }
        }
        _ => failure(format!("Zulip has no tool called {tool}.")),
    }
}
