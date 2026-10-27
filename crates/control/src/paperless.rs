use crate::chat_tool_servers::StoredTool;
use crate::integrations::{CallOutput, Credentials};
use serde_json::{Value, json};
use std::time::Duration;

const UPDATE_DOCUMENT: &str = "update_document";
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;
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
            description: format!("Read a document's metadata and its text. The text is cut at {MAX_TEXT_CHARACTERS} characters."),
            input_schema: json!({"type": "object", "properties": {"document": document.clone()}, "required": ["document"]}),
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
    ]
}

pub fn always_asks(tool: &str) -> bool {
    tool == UPDATE_DOCUMENT
}

fn failure(text: impl Into<String>) -> CallOutput {
    CallOutput { text: text.into(), is_error: true }
}

fn request(http: &reqwest::Client, credentials: &Credentials, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    let base = credentials.address.trim_end_matches('/');

    http.request(method, format!("{base}/api/{path}")).header("authorization", format!("Token {}", credentials.secret)).header("accept", "application/json; version=9").timeout(TIMEOUT)
}

async fn send(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder.send().await.map_err(|_| "Mycelium could not reach Paperless-ngx. Check the Paperless-ngx address and that the server is running, then try again.".to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Paperless-ngx did not accept this API token. Copy it again from My Profile in Paperless-ngx, then try again.".to_string());
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("Paperless-ngx has no document with that id.".to_string());
    }

    if !status.is_success() {
        let reason = body["detail"].as_str().map(str::to_string).unwrap_or_else(|| format!("status {}", status.as_u16()));

        return Err(format!("Paperless-ngx answered with an error: {reason}"));
    }

    Ok(body)
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

pub async fn call(http: &reqwest::Client, credentials: &Credentials, tool: &str, arguments: &Value) -> CallOutput {
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
                .map(|body| json!({"total": body["count"], "documents": body["results"].as_array().into_iter().flatten().map(summary).collect::<Vec<_>>()}))
        }
        "get_document" => {
            let Some(id) = arguments["document"].as_u64() else {
                return failure("Give the document's id in document.");
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("documents/{id}/"))).await.map(|document| {
                let content = document["content"].as_str().unwrap_or_default();
                let cut: String = content.chars().take(MAX_TEXT_CHARACTERS).collect();
                let mut detail = summary(&document);

                detail["notes"] = document["notes"].clone();
                detail["text"] = json!(cut);
                detail["text_truncated"] = json!(cut.len() < content.len());
                detail
            })
        }
        "list_tags" | "list_correspondents" | "list_document_types" => {
            let path = match tool {
                "list_tags" => "tags",
                "list_correspondents" => "correspondents",
                _ => "document_types",
            };

            send(request(http, credentials, reqwest::Method::GET, &format!("{path}/?page_size={}", limit(arguments)))).await.map(|body| named(&body))
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

            send(request(http, credentials, reqwest::Method::PATCH, &format!("documents/{id}/")).json(&Value::Object(changes))).await.map(|document| summary(&document))
        }
        _ => Err(format!("Paperless-ngx has no tool called {tool}.")),
    };

    match outcome {
        Ok(value) => CallOutput { text: value.to_string(), is_error: false },
        Err(reason) => failure(reason),
    }
}
