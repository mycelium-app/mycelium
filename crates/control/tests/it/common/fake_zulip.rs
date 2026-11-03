use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const ACCOUNT: &str = "ada@example.com";
pub const API_KEY: &str = "zulip-key-0123456789abcdef";

#[derive(Default)]
pub struct FakeZulip {
    pub seen: Mutex<Vec<(Method, String, String)>>,
}

impl FakeZulip {
    pub fn sent(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().filter(|(method, path, _)| method == Method::POST && path == "/api/v1/messages").map(|(_, _, body)| body.clone()).collect()
    }

    pub fn paths(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|(_, path, _)| path.clone()).collect()
    }
}

pub const DAY: i64 = 86400;
pub const START: i64 = 1_700_000_000;

fn message(id: u64, sent_at: i64, sender: (&str, &str), channel: &str, topic: &str, content: &str) -> Value {
    json!({"id": id, "type": "stream", "timestamp": sent_at, "sender_full_name": sender.0, "sender_email": sender.1, "display_recipient": channel, "subject": topic, "content": content})
}

fn history() -> Vec<Value> {
    let grace = ("Grace", "grace@example.com");
    let linus = ("Linus", "linus@example.com");
    let ada = ("Ada", ACCOUNT);
    let mut all = vec![message(41, START, grace, "general", "lunch", "Noon?"), message(42, START + 60, linus, "general", "lunch", "Pizza secret-recipe?")];

    for (index, id) in (51..=58).enumerate() {
        let who = [grace, linus, ada][index % 3];
        let topic = if id == 56 { "ops" } else { "release" };

        all.push(message(id, START + DAY * (index as i64 + 1), who, "dev", topic, &format!("Release note {id}")));
    }

    all
}

fn applies(message: &Value, operator: &Value) -> bool {
    let operand = operator["operand"].as_str().unwrap_or_default();
    let held = match operator["operator"].as_str() {
        Some("stream") => message["display_recipient"].as_str().unwrap_or_default() == operand,
        Some("topic") => message["subject"].as_str().unwrap_or_default() == operand,
        Some("sender") => message["sender_email"].as_str().unwrap_or_default() == operand,
        Some("search") => message["content"].as_str().unwrap_or_default().to_lowercase().contains(&operand.to_lowercase()),
        _ => true,
    };

    held != operator["negated"].as_bool().unwrap_or(false)
}

fn page(uri: &Uri) -> Value {
    let query: std::collections::HashMap<String, String> = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()).into_owned().collect();
    let narrow: Vec<Value> = serde_json::from_str(query.get("narrow").map(String::as_str).unwrap_or("[]")).unwrap();
    let matching: Vec<Value> = history().into_iter().filter(|message| narrow.iter().all(|operator| applies(message, operator))).collect();
    let before: usize = query["num_before"].parse().unwrap();
    let after: usize = query["num_after"].parse().unwrap();
    let include = query.get("include_anchor").is_none_or(|value| value == "true");
    let id = |message: &Value| message["id"].as_u64().unwrap();
    let mut extra = 0;
    let (older, newer): (Vec<Value>, Vec<Value>) = match query["anchor"].as_str() {
        "newest" => (matching.clone(), vec![]),
        "oldest" | "first_unread" => (vec![], matching.clone()),
        anchor => {
            let anchor: u64 = anchor.parse().unwrap();
            let older: Vec<Value> = matching.iter().filter(|message| id(message) < anchor).cloned().collect();
            let mut newer: Vec<Value> = matching.iter().filter(|message| id(message) > anchor).cloned().collect();

            if include && let Some(held) = matching.iter().find(|message| id(message) == anchor) {
                newer.insert(0, held.clone());
                extra = 1;
            }

            (older, newer)
        }
    };
    let taken_older: Vec<Value> = older[older.len().saturating_sub(before)..].to_vec();
    let taken_newer: Vec<Value> = newer.iter().take(after + extra).cloned().collect();
    let messages: Vec<Value> = taken_older.iter().chain(taken_newer.iter()).cloned().collect();

    json!({"result": "success", "messages": messages, "found_oldest": older.len() <= before, "found_newest": newer.len() <= after + extra, "found_anchor": true})
}

async fn handle(State(zulip): State<Arc<FakeZulip>>, method: Method, uri: Uri, headers: HeaderMap, body: String) -> Response {
    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{ACCOUNT}:{API_KEY}")));

    if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some(&expected) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"result": "error", "msg": "Invalid API key", "code": "UNAUTHORIZED"}))).into_response();
    }

    zulip.seen.lock().unwrap().push((method.clone(), uri.path().to_string(), format!("{}{}", uri.query().unwrap_or_default(), body)));

    match (method, uri.path()) {
        (Method::GET, "/api/v1/users/me") => Json(json!({"result": "success", "email": ACCOUNT})).into_response(),
        (Method::GET, "/api/v1/users/me/subscriptions") => Json(json!({"result": "success", "subscriptions": [{"stream_id": 7, "name": "general", "description": "Everything"}]})).into_response(),
        (Method::GET, "/api/v1/users") => Json(json!({"result": "success", "members": [{"email": "grace@example.com", "full_name": "Grace Hopper"}, {"email": "linus@example.com", "full_name": "Linus"}, {"email": ACCOUNT, "full_name": "Ada"}]})).into_response(),
        (Method::GET, "/api/v1/messages") => Json(page(&uri)).into_response(),
        (Method::GET, path) if path.starts_with("/api/v1/messages/") => match history().into_iter().find(|message| path.ends_with(&format!("/{}", message["id"]))) {
            Some(found) => Json(json!({"result": "success", "message": found})).into_response(),
            None => (StatusCode::BAD_REQUEST, Json(json!({"result": "error", "msg": "Invalid message(s)"}))).into_response(),
        },
        (Method::POST, "/api/v1/messages") => Json(json!({"result": "success", "id": 99})).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn fake_zulip() -> (Router, Arc<FakeZulip>) {
    let zulip = Arc::new(FakeZulip::default());

    (Router::new().fallback(any(handle)).with_state(zulip.clone()), zulip)
}
