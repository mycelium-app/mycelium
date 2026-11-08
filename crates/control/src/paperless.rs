use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use crate::paging::{Page, all_rows};
use serde_json::{Value, json};
use std::time::Duration;

const UPDATE_DOCUMENT: &str = "update_document";
const DOWNLOAD_DOCUMENT: &str = "download_document";
const API: &str = "api";
const API_METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
const PAGE_SIZE: u64 = 100;
const MAX_TEXT_CHARACTERS: usize = 20_000;
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn tools() -> Vec<StoredTool> {
    let limit = json!({"type": "integer", "description": format!("How many to return. Defaults to {DEFAULT_LIMIT}, at most {MAX_LIMIT}.")});
    let document = json!({"type": "integer", "description": "The document's id, from search_documents."});
    let listing = |name: &str, description: &str| StoredTool { name: name.to_string(), description: description.to_string(), input_schema: json!({"type": "object", "properties": {"limit": limit.clone()}}), read_only: None };

    vec![
        StoredTool {
            name: "search_documents".to_string(),
            description: "Search documents by words in their title and text, optionally narrowed by tag, correspondent or document type ids. Returns the best matches with their metadata, without the text.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "query": {"type": "string", "description": "Words to search for. Leave out to list the newest documents."},
                "tag": {"type": "integer", "description": "Only documents with this tag id."},
                "correspondent": {"type": "integer", "description": "Only documents from this correspondent id."},
                "document_type": {"type": "integer", "description": "Only documents of this document type id."},
                "limit": limit.clone()
            }}),
            read_only: None,
        },
        StoredTool {
            name: "get_document".to_string(),
            description: format!("Read a document's metadata and its text. The text is cut at {MAX_TEXT_CHARACTERS} characters, unless you give save_to: then the whole text is saved to the file."),
            input_schema: json!({"type": "object", "properties": {"document": document.clone()}, "required": ["document"]}),
            read_only: None,
        },
        StoredTool {
            name: DOWNLOAD_DOCUMENT.to_string(),
            description: "Download a document's file, such as its PDF, into the conversation folder. Give save_to, the path to write it to: the file is saved unchanged, so it can be sent on elsewhere.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "document": document.clone(),
                "original": {"type": "boolean", "description": "Download the file as it was uploaded. Defaults to true. Set to false for the archived version Paperless-ngx made."}
            }, "required": ["document"]}),
            read_only: None,
        },
        listing("list_tags", "List the tags, with their ids and how many documents have each."),
        listing("list_correspondents", "List the correspondents, with their ids and how many documents each has."),
        listing("list_document_types", "List the document types, with their ids and how many documents each has."),
        StoredTool {
            name: UPDATE_DOCUMENT.to_string(),
            description: "Set a document's tags, its correspondent, or both. Tags replaces the whole list, so include the tags to keep. Always needs the person's approval in chats, even when Always allow is on.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "document": document,
                "tags": {"type": "array", "items": {"type": "integer"}, "description": "The ids of every tag the document should have."},
                "correspondent": {"type": ["integer", "null"], "description": "The correspondent's id, or null to clear it."}
            }, "required": ["document"]}),
            read_only: None,
        },
        StoredTool {
            name: API.to_string(),
            description: "Call any Paperless-ngx API endpoint, for everything the other tools do not cover: filtering documents by date, tag or correspondent, custom fields, storage paths, notes and the rest. Give a path under /api, such as documents/ or tags/, or a full url of this Paperless-ngx from an earlier answer. GET runs at once; POST, PUT, PATCH and DELETE follow this account's approvals. With all_pages, a GET of a list follows every page and returns all the results. When the answer is a file, give save_to and it is saved unchanged.".to_string(),
            input_schema: json!({"type": "object", "properties": {
                "method": {"type": "string", "enum": API_METHODS},
                "path": {"type": "string", "description": "The endpoint under /api, such as documents/, or a full url of this Paperless-ngx."},
                "query": {"type": "object", "description": "Query parameters, such as {\"created__date__gte\": \"2026-04-06\", \"created__date__lte\": \"2027-04-05\", \"tags__id__all\": [3, 4]}. A list is sent as comma separated ids."},
                "body": {"type": "object", "description": "The JSON body for POST, PUT and PATCH."},
                "all_pages": {"type": "boolean", "description": "For a GET of a list, follow every page and return all the results. Defaults to false."}
            }, "required": ["method", "path"]}),
            read_only: None,
        },
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == UPDATE_DOCUMENT || tool == API
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true, binary: None }
}

enum Answer {
    Json(Value),
    File(Vec<u8>),
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');

    signed(http, credentials, method, &format!("{base}/api/{path}"))
}

fn signed(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
    http.request(method, url).header("authorization", format!("Token {}", credentials.secret)).header("accept", "application/json; version=9").timeout(TIMEOUT)
}

async fn fetch(builder: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, Answer), String> {
    let unreachable = || "Mycelium could not reach Paperless-ngx. Check the Paperless-ngx address and that the server is running, then try again.".to_string();
    let response = builder.send().await.map_err(|_| unreachable())?;
    let status = response.status();
    let json = response.headers().get("content-type").and_then(|value| value.to_str().ok()).is_some_and(|kind| kind.contains("json"));
    let bytes = response.bytes().await.map_err(|_| unreachable())?;

    match json || bytes.is_empty() {
        true => Ok((status, Answer::Json(serde_json::from_slice(&bytes).unwrap_or(Value::Null)))),
        false => Ok((status, Answer::File(bytes.to_vec()))),
    }
}

fn checked(status: reqwest::StatusCode, answer: Answer) -> Result<Answer, String> {
    if status.is_success() {
        return Ok(answer);
    }

    let body = match answer {
        Answer::Json(body) => body,
        Answer::File(_) => Value::Null,
    };

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Paperless-ngx did not accept this API token. Copy it again from My Profile in Paperless-ngx, then try again.".to_string());
    }

    let reason = body["detail"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

    Err(format!("Paperless-ngx answered with an error: {reason}"))
}

async fn send_any(builder: reqwest::RequestBuilder) -> Result<Answer, String> {
    let (status, answer) = fetch(builder).await?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("Paperless-ngx has no document with that id.".to_string());
    }

    checked(status, answer)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    match send_any(builder).await? {
        Answer::Json(body) => Ok(body),
        Answer::File(_) => Ok(Value::Null),
    }
}

pub async fn verify(http: &reqwest::Client, credentials: &Credentials) -> Result<(), String> {
    send(request(http, credentials, reqwest::Method::GET, "documents/?page_size=1")).await.map(|_| ())
}

fn limit(arguments: &Value) -> u64 {
    arguments["limit"].as_u64().unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn summary(document: &Value) -> Value {
    json!({"id": document["id"], "title": document["title"], "created": document["created"], "added": document["added"], "correspondent": document["correspondent"], "document_type": document["document_type"], "tags": document["tags"], "original_file_name": document["original_file_name"]})
}

fn named(body: &Value) -> Value {
    Value::Array(body["results"].as_array().into_iter().flatten().map(|entry| json!({"id": entry["id"], "name": entry["name"], "document_count": entry["document_count"]})).collect())
}

fn api_route(credentials: &Credentials, path: &str) -> Option<String> {
    let base = credentials.address.trim_end_matches('/');
    let entered = path.trim();
    let route = entered.strip_prefix(base).unwrap_or(entered).trim_start_matches('/');
    let route = route.strip_prefix("api/").unwrap_or(route);
    let (resource, query) = route.split_once('?').unwrap_or((route, ""));
    let plain = !resource.is_empty() && resource.trim_end_matches('/').split('/').all(|part| !part.is_empty() && part != ".." && part != ".") && !resource.contains(['\\', ':', '%', '@', '#']) && !query.contains('#');

    plain.then(|| route.to_string())
}

fn next_route(credentials: &Credentials, next: &str) -> Option<String> {
    let parsed = url::Url::parse(next).ok()?;
    let (_, route) = parsed.path().split_once("/api/")?;
    let route = match parsed.query() {
        Some(query) => format!("{route}?{query}"),
        None => route.to_string(),
    };

    api_route(credentials, &route)
}

fn page_of(credentials: &Credentials, body: &Value) -> Result<Page, String> {
    let next = match body["next"].as_str() {
        Some(next) => Some(next_route(credentials, next).ok_or("Paperless-ngx gave a next page that is not one of its own addresses, so Mycelium did not follow it.")?),
        None => None,
    };

    Ok(Page { rows: body["results"].as_array().cloned().unwrap_or_default(), next })
}

fn query_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(query_value).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

async fn api(http: &reqwest::Client, credentials: &Credentials, arguments: &Value) -> Result<Answer, String> {
    let method = arguments["method"].as_str().map(str::to_ascii_uppercase).filter(|method| API_METHODS.contains(&method.as_str())).and_then(|method| reqwest::Method::from_bytes(method.as_bytes()).ok());
    let Some(method) = method else {
        return Err(format!("Choose a method: {}.", API_METHODS.join(", ")));
    };
    let Some(route) = arguments["path"].as_str().and_then(|path| api_route(credentials, path)) else {
        return Err("Give a path under /api, such as documents/, or a Paperless-ngx url. Do not use \"..\".".to_string());
    };
    let base = credentials.address.trim_end_matches('/');
    let mut url = url::Url::parse(&format!("{base}/api/{route}")).map_err(|_| "That path is not a Paperless-ngx address.".to_string())?;
    let all_pages = method == reqwest::Method::GET && arguments["all_pages"].as_bool() == Some(true);

    if let Some(query) = arguments["query"].as_object() {
        let mut pairs = url.query_pairs_mut();

        for (key, value) in query {
            pairs.append_pair(key, &query_value(value));
        }
    }

    if all_pages && !url.query_pairs().any(|(key, _)| key == "page_size") {
        url.query_pairs_mut().append_pair("page_size", &PAGE_SIZE.to_string());
    }

    let mut builder = signed(http, credentials, method.clone(), url.as_str());

    if let Some(body) = arguments.get("body").filter(|body| !body.is_null() && method != reqwest::Method::GET) {
        builder = builder.json(body);
    }

    let (status, answer) = fetch(builder).await?;
    let body = match checked(status, answer)? {
        Answer::Json(body) => body,
        file => return Ok(file),
    };

    if !all_pages || !body["results"].is_array() {
        return Ok(Answer::Json(json!({"status": status.as_u16(), "body": body})));
    }

    let first = page_of(credentials, &body)?;
    let fetch_next = |route: String| async move {
        let (status, answer) = fetch(request(http, credentials, reqwest::Method::GET, &route)).await?;
        let Answer::Json(body) = checked(status, answer)? else {
            return Err("Paperless-ngx answered a page with a file instead of a list.".to_string());
        };

        page_of(credentials, &body)
    };
    let (rows, pages) = all_rows(first, url.query().unwrap_or_default(), fetch_next, |reason| reason).await?;

    Ok(Answer::Json(json!({"status": status.as_u16(), "pages": pages, "rows": rows.len(), "body": {"count": body["count"], "results": rows}})))
}

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value, saving: bool) -> CallOutput {
    let outcome = match tool {
        "search_documents" => {
            let path = {
                let mut query = url::form_urlencoded::Serializer::new(String::new());

                query.append_pair("page_size", &limit(arguments).to_string());

                if let Some(words) = arguments["query"].as_str().map(str::trim).filter(|words| !words.is_empty()) {
                    query.append_pair("query", words);
                }

                for (argument, filter) in [("tag", "tags__id__all"), ("correspondent", "correspondent__id"), ("document_type", "document_type__id")] {
                    if let Some(id) = arguments[argument].as_u64() {
                        query.append_pair(filter, &id.to_string());
                    }
                }

                format!("documents/?{}", query.finish())
            };

            send(request(http, credentials, reqwest::Method::GET, &path))
                .await
                .map(|body| Answer::Json(json!({"total": body["count"], "documents": body["results"].as_array().into_iter().flatten().map(summary).collect::<Vec<_>>()})))
        }
        "get_document" => {
            let Some(id) = arguments["document"].as_u64() else {
                return failure("Give the document's id in document.");
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("documents/{id}/"))).await.map(|document| {
                let content = document["content"].as_str().unwrap_or_default();
                let cut: String = if saving { content.to_string() } else { content.chars().take(MAX_TEXT_CHARACTERS).collect() };
                let mut detail = summary(&document);

                detail["notes"] = document["notes"].clone();
                detail["text"] = json!(cut);
                detail["text_truncated"] = json!(cut.len() < content.len());
                Answer::Json(detail)
            })
        }
        DOWNLOAD_DOCUMENT => {
            let Some(id) = arguments["document"].as_u64() else {
                return failure("Give the document's id in document.");
            };
            let original = arguments["original"].as_bool().unwrap_or(true);

            send_any(request(http, credentials, reqwest::Method::GET, &format!("documents/{id}/download/?original={original}"))).await
        }
        API => api(http, credentials, arguments).await,
        "list_tags" | "list_correspondents" | "list_document_types" => {
            let path = match tool {
                "list_tags" => "tags",
                "list_correspondents" => "correspondents",
                _ => "document_types",
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("{path}/?page_size={}", limit(arguments)))).await.map(|body| Answer::Json(named(&body)))
        }
        UPDATE_DOCUMENT => {
            let Some(id) = arguments["document"].as_u64() else {
                return failure("Give the document's id in document.");
            };
            let mut changes = serde_json::Map::new();

            if let Some(tags) = arguments["tags"].as_array() {
                changes.insert("tags".to_string(), Value::Array(tags.iter().filter(|tag| tag.is_u64()).cloned().collect()));
            }

            if arguments.get("correspondent").is_some_and(|correspondent| correspondent.is_null() || correspondent.is_u64()) {
                changes.insert("correspondent".to_string(), arguments["correspondent"].clone());
            }

            if changes.is_empty() {
                return failure("Give tags, correspondent or both to change.");
            }

            send(request(http, credentials, reqwest::Method::PATCH, &format!("documents/{id}/")).json(&Value::Object(changes))).await.map(|document| Answer::Json(summary(&document)))
        }
        _ => Err(format!("Paperless-ngx has no tool called {tool}.")),
    };

    match outcome {
        Ok(Answer::Json(value)) => CallOutput { text: value.to_string(), is_error: false, binary: None },
        Ok(Answer::File(bytes)) => CallOutput { text: format!("A file of {} bytes.", bytes.len()), is_error: false, binary: Some(bytes) },
        Err(reason) => failure(reason),
    }
}
