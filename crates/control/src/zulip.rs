use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use serde_json::{Value, json};
use std::time::Duration;

const SEND_MESSAGE: &str = "send_message";
const DEFAULT_LIMIT: u64 = 20;
const MAX_COUNT: u64 = 200;
const MESSAGE_CONTEXT: u64 = 5;
const DATE_PAGE: u64 = 100;
const DATE_PAGES: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(30);
const SEARCH_OPERATORS: [&str; 13] = ["from", "sender", "stream", "channel", "topic", "has", "is", "near", "id", "dm", "pm-with", "dm-including", "search"];

pub fn tools() -> Vec<StoredTool> {
    let channel = json!({"type": "string", "description": "The channel's name."});
    let topic = json!({"type": "string", "description": "A topic in the channel."});
    let people = json!({"type": "array", "items": {"type": "string"}, "description": "Email addresses of the other people in a direct message conversation."});
    let sender = json!({"type": "string", "description": "Who wrote the messages: an email address, a full name or me for the integration's own account."});
    let since = json!({"type": "string", "description": "Only messages sent at or after this ISO 8601 date or date and time, for example 2026-10-29 or 2026-10-29T09:00:00Z. A date or time without an offset is UTC."});
    let until = json!({"type": "string", "description": "Only messages sent before the end of this ISO 8601 date, or before this date and time. A date or time without an offset is UTC."});
    let anchor = json!({"type": "string", "description": "Where to start reading: a message id, newest, oldest or first_unread. Defaults to newest. A message id is not itself returned, so passing a result's next continues without repeating a message."});
    let before = json!({"type": "integer", "description": format!("How many messages to return before the anchor, newest last. Defaults to {DEFAULT_LIMIT} (or {MAX_COUNT} with since or until), at most {MAX_COUNT}.")});
    let after = json!({"type": "integer", "description": format!("How many messages to return after the anchor, at most {MAX_COUNT}. Cannot be combined with since or until.")});
    let coverage = "Every result is {messages, coverage}: coverage says how many messages came back, the oldest_id and newest_id returned, whether found_oldest and found_newest (nothing older or newer remains in the range), and next, the id to pass as anchor to continue (null when there is nothing more).";

    vec![
        StoredTool { name: "list_channels".to_string(), description: "List the channels (streams) the person is subscribed to, with their ids and descriptions.".to_string(), input_schema: json!({"type": "object", "properties": {}}), read_only: None },
        StoredTool {
            name: "get_messages".to_string(),
            description: format!("Read messages in Zulip. Give message (a message id or a Zulip permalink containing /near/<id>) to get that message with the messages before and after it in its topic. Otherwise read a channel, optionally one topic, or a direct message conversation (channel and direct_with are exclusive; give neither to read across everything), optionally narrowed to one sender and to a date range with since and until. Page back through history with anchor, before and after. {coverage}"),
            input_schema: json!({"type": "object", "properties": {"message": {"type": "string", "description": format!("A message id or a Zulip permalink. Returns the message and up to {MESSAGE_CONTEXT} messages either side in its topic, or as many as before and after ask for.")}, "channel": channel, "topic": topic, "direct_with": people, "sender": sender, "anchor": anchor, "before": before, "after": after, "since": since, "until": until}}), read_only: None },
        StoredTool {
            name: "search_messages".to_string(),
            description: format!("Search Zulip messages. query takes keywords and Zulip search operators: from:/sender: (an email, a full name or me), stream:/channel:, topic:, has: (link, image, attachment), is: (starred, mentioned, unread, resolved), near:<id>, id:<id> and dm:/dm-including:; put a leading - in front of an operator to exclude it, and quote a value that has spaces, as in topic:\"lunch plans\". Any other operator is refused with an error. The structured channel, topic, sender, since and until arguments work with or without query. Returns the newest matches; page with anchor, before and after. {coverage}"),
            input_schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "Keywords, with any Zulip search operators."}, "channel": channel, "topic": topic, "sender": sender, "since": since, "until": until, "anchor": anchor, "before": before, "after": after}}), read_only: None },
        StoredTool {
            name: SEND_MESSAGE.to_string(),
            description: "Send a message to a channel topic, or directly to one or more people. Always needs the person's approval in chats, even when Always allow is on. Give either channel and topic, or direct_to.".to_string(),
            input_schema: json!({"type": "object", "properties": {"channel": channel, "topic": {"type": "string", "description": "The topic to post under. Needs channel."}, "direct_to": people, "content": {"type": "string", "description": "The message, in Zulip Markdown."}}, "required": ["content"]}), read_only: None },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == SEND_MESSAGE
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true, binary: None }
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

fn operator(name: &str, operand: &str) -> Value {
    json!({"operator": name, "operand": operand})
}

fn instant(arguments: &Value, key: &str) -> Result<Option<i64>, String> {
    let Some(value) = text(arguments, key) else {
        return Ok(None);
    };
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map(|moment| moment.timestamp())
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").map(|moment| moment.and_utc().timestamp()))
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map(|day| {
                let start = day.and_time(chrono::NaiveTime::MIN).and_utc().timestamp();

                if key == "until" { start + 86400 } else { start }
            })
        });

    parsed.map(Some).map_err(|_| format!("Enter {key} as an ISO 8601 date or date and time, for example 2026-10-29 or 2026-10-29T09:00:00Z."))
}

async fn sender(http: &reqwest::Client, credentials: &Credentials, who: &str) -> Result<String, String> {
    if who.eq_ignore_ascii_case("me") {
        return Ok(credentials.account.clone());
    }

    if who.contains('@') || who.chars().all(|character| character.is_ascii_digit()) {
        return Ok(who.to_string());
    }

    let body = send(request(http, credentials, reqwest::Method::GET, "users")).await?;
    let members: Vec<&Value> = body["members"].as_array().into_iter().flatten().collect();
    let wanted = who.to_lowercase();
    let exact: Vec<&&Value> = members.iter().filter(|member| member["full_name"].as_str().is_some_and(|name| name.to_lowercase() == wanted)).collect();
    let partial: Vec<&&Value> = members.iter().filter(|member| member["full_name"].as_str().is_some_and(|name| name.to_lowercase().contains(&wanted))).collect();
    let matches = if exact.is_empty() { partial } else { exact };

    match matches.as_slice() {
        [only] => only["email"].as_str().map(str::to_string).ok_or_else(|| format!("Zulip did not give an email address for {who}. Give their email address in sender.")),
        [] => Err(format!("No one in Zulip is called {who}. Give their email address in sender.")),
        several => Err(format!("More than one person in Zulip matches {who}: {}. Give an email address in sender.", several.iter().filter_map(|member| member["full_name"].as_str()).collect::<Vec<_>>().join(", "))),
    }
}

async fn filters(http: &reqwest::Client, credentials: &Credentials, arguments: &Value) -> Result<Vec<Value>, String> {
    let mut narrow = vec![];

    if let Some(channel) = text(arguments, "channel") {
        narrow.push(operator("stream", channel));
    }

    if let Some(topic) = text(arguments, "topic") {
        narrow.push(operator("topic", topic));
    }

    if let Some(who) = text(arguments, "sender") {
        narrow.push(operator("sender", &sender(http, credentials, who).await?));
    }

    Ok(narrow)
}

async fn messages(http: &reqwest::Client, credentials: &Credentials, narrow: Vec<Value>, arguments: &Value, around: Option<u64>) -> CallOutput {
    let (since, until) = match (instant(arguments, "since"), instant(arguments, "until")) {
        (Ok(since), Ok(until)) => (since, until),
        (Err(reason), _) | (_, Err(reason)) => return failure(reason),
    };
    let given = match &arguments["anchor"] {
        Value::Number(number) => Some(number.to_string()),
        Value::String(anchor) => Some(anchor.trim().to_string()).filter(|anchor| !anchor.is_empty()),
        _ => None,
    };
    let anchor = around.map(|id| id.to_string()).or(given).unwrap_or_else(|| "newest".to_string());

    if !anchor.chars().all(|character| character.is_ascii_digit()) && !["newest", "oldest", "first_unread"].contains(&anchor.as_str()) {
        return failure("Give anchor as a message id, newest, oldest or first_unread.");
    }

    let dated = since.is_some() || until.is_some();
    let asked_before = arguments["before"].as_u64().or_else(|| arguments["limit"].as_u64()).map(|count| count.min(MAX_COUNT));
    let asked_after = arguments["after"].as_u64().map(|count| count.min(MAX_COUNT));
    let side = if around.is_some() { MESSAGE_CONTEXT } else { 0 };
    let (before, after) = match (asked_before, asked_after) {
        (None, None) if around.is_some() => (MESSAGE_CONTEXT, MESSAGE_CONTEXT),
        (None, None) if dated => (MAX_COUNT, 0),
        (None, None) if anchor == "oldest" || anchor == "first_unread" => (0, DEFAULT_LIMIT),
        (None, None) => (DEFAULT_LIMIT, 0),
        (before, after) => (before.unwrap_or(side), after.unwrap_or(side)),
    };

    if dated && after > 0 {
        return failure("since and until read backwards from the anchor, so leave after out.");
    }

    let mut cursor = anchor;
    let mut include_anchor = around.is_some();
    let mut gathered: Vec<Value> = vec![];
    let mut scanned_oldest: Option<u64> = None;
    let mut found_oldest = false;
    let mut found_newest = false;
    let mut reached_since = false;
    let mut dropped_newer = false;

    for page in 0..(if dated { DATE_PAGES } else { 1 }) {
        let (size_before, size_after) = if dated { (DATE_PAGE, 0) } else { (before, after) };
        let mut query = vec![("anchor", cursor.clone()), ("num_before", size_before.to_string()), ("num_after", size_after.to_string()), ("apply_markdown", "false".to_string()), ("narrow", Value::Array(narrow.clone()).to_string())];

        if cursor.chars().all(|character| character.is_ascii_digit()) {
            query.push(("include_anchor", include_anchor.to_string()));
        }

        let body = match send(request(http, credentials, reqwest::Method::GET, &format!("messages?{}", encoded(&query)))).await {
            Ok(body) => body,
            Err(reason) => return failure(reason),
        };
        let fetched: Vec<Value> = body["messages"].as_array().cloned().unwrap_or_default();

        if page == 0 {
            found_newest = body["found_newest"].as_bool().unwrap_or(false);
        }

        found_oldest = body["found_oldest"].as_bool().unwrap_or(false);
        scanned_oldest = fetched.first().and_then(|message| message["id"].as_u64()).or(scanned_oldest);

        if !dated {
            gathered = fetched;
            break;
        }

        for message in fetched.iter().rev() {
            let sent = message["timestamp"].as_i64().unwrap_or(0);

            if until.is_some_and(|limit| sent >= limit) {
                dropped_newer = true;

                continue;
            }

            if since.is_some_and(|limit| sent < limit) {
                reached_since = true;

                break;
            }

            gathered.push(message.clone());
        }

        if reached_since || found_oldest || fetched.is_empty() || gathered.len() as u64 >= before {
            break;
        }

        if let Some(id) = scanned_oldest {
            cursor = id.to_string();
        }

        include_anchor = false;
    }

    let truncated = dated && gathered.len() as u64 > before;

    if dated {
        gathered.truncate(before as usize);
        gathered.reverse();
    }

    let found_oldest = !truncated && (found_oldest || reached_since);
    let found_newest = found_newest || dropped_newer;
    let oldest_id = gathered.first().and_then(|message| message["id"].as_u64());
    let newest_id = gathered.last().and_then(|message| message["id"].as_u64());
    let next = if after > 0 && before == 0 {
        newest_id.filter(|_| !found_newest)
    } else if found_oldest {
        None
    } else if truncated {
        oldest_id
    } else {
        scanned_oldest
    };
    let listed: Vec<Value> = gathered
        .iter()
        .map(|message| {
            let place = match message["type"].as_str() {
                Some("stream") => json!({"channel": message["display_recipient"], "topic": message["subject"]}),
                _ => json!({"direct_with": message["display_recipient"].as_array().into_iter().flatten().filter_map(|person| person["email"].as_str()).collect::<Vec<_>>()}),
            };

            json!({"id": message["id"], "sent_at": message["timestamp"], "sender": message["sender_full_name"], "sender_email": message["sender_email"], "place": place, "content": message["content"]})
        })
        .collect();
    let coverage = json!({"returned": listed.len(), "found_oldest": found_oldest, "found_newest": found_newest, "oldest_id": oldest_id, "newest_id": newest_id, "next": next});

    CallOutput { text: json!({"messages": listed, "coverage": coverage}).to_string(), is_error: false, binary: None }
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
    match tool {
        "list_channels" => match send(request(http, credentials, reqwest::Method::GET, "users/me/subscriptions")).await {
            Ok(body) => {
                let channels: Vec<Value> = body["subscriptions"].as_array().into_iter().flatten().map(|channel| json!({"id": channel["stream_id"], "name": channel["name"], "description": channel["description"]})).collect();

                CallOutput { text: Value::Array(channels).to_string(), is_error: false, binary: None }
            }
            Err(reason) => failure(reason),
        },
        "get_messages" => {
            if let Some(reference) = text(arguments, "message").map(str::to_string).or_else(|| arguments["message"].as_u64().map(|id| id.to_string())) {
                let id = if reference.chars().all(|character| character.is_ascii_digit()) {
                    reference.parse::<u64>().ok()
                } else {
                    let link = url::Url::parse(&reference).ok();
                    let mut parts = link.as_ref().and_then(|link| link.fragment()).unwrap_or_default().split('/');

                    parts.by_ref().find(|part| *part == "near" || *part == "id");
                    parts.next().and_then(|part| part.parse::<u64>().ok())
                };
                let Some(id) = id else {
                    return failure("Give message as a message id, or a Zulip link that contains /near/ and the message id.");
                };
                let found = match send(request(http, credentials, reqwest::Method::GET, &format!("messages/{id}?apply_markdown=false"))).await {
                    Ok(body) => body["message"].clone(),
                    Err(reason) => return failure(reason),
                };
                let narrow = match found["type"].as_str() {
                    Some("stream") => vec![operator("stream", found["display_recipient"].as_str().unwrap_or_default()), operator("topic", found["subject"].as_str().unwrap_or_default())],
                    _ => {
                        let everyone: Vec<&str> = found["display_recipient"].as_array().into_iter().flatten().filter_map(|person| person["email"].as_str()).collect();
                        let others: Vec<&str> = everyone.iter().copied().filter(|email| *email != credentials.account).collect();

                        vec![operator("pm-with", &if others.is_empty() { everyone } else { others }.join(","))]
                    }
                };

                return messages(http, credentials, narrow, arguments, Some(id)).await;
            }

            let channel = text(arguments, "channel");
            let direct = people(arguments, "direct_with");

            if channel.is_some() && !direct.is_empty() {
                return failure("Give either channel or direct_with, not both.");
            }

            if channel.is_none() && text(arguments, "topic").is_some() {
                return failure("Give the channel that topic is in.");
            }

            let mut narrow = match filters(http, credentials, arguments).await {
                Ok(narrow) => narrow,
                Err(reason) => return failure(reason),
            };

            if !direct.is_empty() {
                narrow.push(operator("pm-with", &direct.join(",")));
            }

            messages(http, credentials, narrow, arguments, None).await
        }
        "search_messages" => {
            let mut narrow = match filters(http, credentials, arguments).await {
                Ok(narrow) => narrow,
                Err(reason) => return failure(reason),
            };
            let mut keywords: Vec<String> = vec![];
            let mut around: Option<u64> = None;
            let mut tokens: Vec<String> = vec![];
            let mut current = String::new();
            let mut quoted = false;

            for character in text(arguments, "query").unwrap_or_default().chars() {
                if character == '"' {
                    quoted = !quoted;
                }

                if character.is_whitespace() && !quoted {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                } else {
                    current.push(character);
                }
            }

            if !current.is_empty() {
                tokens.push(current);
            }

            for token in tokens {
                let (negated, body) = match token.strip_prefix('-') {
                    Some(rest) => (true, rest),
                    None => (false, token.as_str()),
                };
                let parsed = body.split_once(':').filter(|(name, operand)| !name.is_empty() && name.chars().all(|character| character.is_ascii_lowercase() || character == '-') && !operand.is_empty() && !operand.starts_with("//"));
                let Some((name, operand)) = parsed else {
                    keywords.push(token.clone());

                    continue;
                };
                let operand = operand.trim_matches('"');

                if !SEARCH_OPERATORS.contains(&name) {
                    return failure(format!("Zulip search operator {name}: is not supported here. Use one of: {}.", SEARCH_OPERATORS.join(", ")));
                }

                if name == "near" {
                    let Ok(id) = operand.parse::<u64>() else {
                        return failure("Give near: a message id, for example near:565204.");
                    };

                    around = Some(id);

                    continue;
                }

                let (name, operand) = match name {
                    "from" | "sender" => {
                        let email = match sender(http, credentials, operand).await {
                            Ok(email) => email,
                            Err(reason) => return failure(reason),
                        };

                        ("sender", email)
                    }
                    "channel" => ("stream", operand.to_string()),
                    _ => (name, operand.to_string()),
                };
                let mut entry = operator(name, &operand);

                if negated {
                    entry["negated"] = Value::Bool(true);
                }

                narrow.push(entry);
            }

            if !keywords.is_empty() {
                narrow.push(operator("search", &keywords.join(" ")));
            }

            if narrow.is_empty() && around.is_none() && text(arguments, "since").is_none() && text(arguments, "until").is_none() {
                return failure("Give the keywords to search for in query, or filter by channel, topic, sender, since or until.");
            }

            messages(http, credentials, narrow, arguments, around).await
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
                Ok(body) => CallOutput { text: json!({"sent": true, "id": body["id"]}).to_string(), is_error: false, binary: None },
                Err(reason) => failure(reason),
            }
        }
        _ => failure(format!("Zulip has no tool called {tool}.")),
    }
}
