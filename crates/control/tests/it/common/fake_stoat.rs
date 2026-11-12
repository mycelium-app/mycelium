use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

pub const TOKEN: &str = "stoat-token-0123456789abcdef";
pub const CHANNEL: &str = "01HZZZZZZZZZZZZZZZZZZZZZZA";
pub const HISTORY_CHANNEL: &str = "01HHISTORY0000000000000000";
pub const FIRST_HISTORY: u64 = 1000;
const HISTORY_LENGTH: u64 = 800;
const MESSAGE: &str = "01K8ZZZZZZZZZZZZZZZZZZZZZB";

pub fn history_id(number: u64) -> String {
    format!("01HMSG{number:020}")
}

fn history_number(id: &str) -> u64 {
    id[6..].parse().unwrap()
}

fn history_page(query: &std::collections::HashMap<String, String>) -> Vec<Value> {
    let first = FIRST_HISTORY + 1;
    let last = FIRST_HISTORY + HISTORY_LENGTH;
    let limit: usize = query["limit"].parse().unwrap();
    let numbers: Vec<u64> = match (query.get("before"), query.get("after")) {
        (Some(before), _) => (first..history_number(before)).rev().take(limit).collect(),
        (_, Some(after)) => (history_number(after) + 1..=last).take(limit).collect(),
        _ => (first..=last).rev().take(limit).collect(),
    };

    numbers.into_iter().map(|number| json!({"_id": history_id(number), "channel": HISTORY_CHANNEL, "author": "01USERGRACE00000000000000A", "content": format!("Note {number}")})).collect()
}

#[derive(Default)]
pub struct FakeStoat {
    pub seen: Mutex<Vec<(Method, String, String, String)>>,
}

impl FakeStoat {
    pub fn requests(&self, method: Method, path_prefix: &str) -> Vec<(String, String)> {
        self.seen.lock().unwrap().iter().filter(|(seen, path, _, _)| *seen == method && path.starts_with(path_prefix)).map(|(_, path, body, header)| (format!("{path}|{header}"), body.clone())).collect()
    }
}

#[derive(Clone)]
struct Shared {
    stoat: Arc<FakeStoat>,
    events: String,
}

async fn handle(State(shared): State<Shared>, method: Method, uri: Uri, headers: HeaderMap, body: String) -> Response {
    if uri.path() == "/" {
        return Json(json!({"stoat": "0.16.2", "ws": shared.events})).into_response();
    }

    let header = ["x-session-token", "x-bot-token"].into_iter().find(|name| headers.get(*name).and_then(|value| value.to_str().ok()) == Some(TOKEN));
    let Some(header) = header else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"type": "InvalidSession"}))).into_response();
    };

    let path = uri.path_and_query().map(|value| value.to_string()).unwrap_or_default();

    let seen_body = body.clone();

    shared.stoat.seen.lock().unwrap().push((method.clone(), path, body, header.to_string()));

    let users = json!([{"_id": "01USERGRACE00000000000000A", "username": "grace", "display_name": "Grace"}]);
    let query: std::collections::HashMap<String, String> = uri.query().map(|query| url::form_urlencoded::parse(query.as_bytes()).into_owned().collect()).unwrap_or_default();
    let message = json!({"_id": MESSAGE, "channel": CHANNEL, "author": "01USERGRACE00000000000000A", "content": "Noon?", "attachments": [{"_id": "f", "filename": "menu.pdf", "size": 12}]});

    match (method, uri.path()) {
        (Method::GET, "/users/@me") => Json(json!({"_id": "01USERADA00000000000000000A", "username": "ada"})).into_response(),
        (Method::GET, path) if path.ends_with("/messages") && path.contains(HISTORY_CHANNEL) => Json(json!({"messages": history_page(&query), "users": users})).into_response(),
        (Method::GET, path) if path.ends_with("/messages") => Json(json!({"messages": [message], "users": users})).into_response(),
        (Method::POST, path) if path.ends_with("/search") && path.contains(HISTORY_CHANNEL) => {
            let asked: Value = serde_json::from_str(&seen_body).unwrap();
            let fields = ["limit", "before", "after"].into_iter().filter_map(|key| Some((key.to_string(), asked[key].as_str().map(str::to_string).or_else(|| asked[key].as_u64().map(|number| number.to_string()))?))).collect();

            Json(json!({"messages": history_page(&fields), "users": users})).into_response()
        }
        (Method::POST, path) if path.ends_with("/search") => Json(json!({"messages": [message], "users": users})).into_response(),
        (Method::POST, path) if path.ends_with("/messages") => Json(json!({"_id": "01K9SENT0000000000000000AA", "channel": CHANNEL})).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn events(listener: TcpListener) {
    loop {
        let (stream, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let Some(Ok(Message::Text(authenticate))) = socket.next().await else {
                return;
            };
            let authenticate: Value = serde_json::from_str(authenticate.as_str()).unwrap();

            if authenticate["token"] != TOKEN {
                socket.send(Message::Text(json!({"type": "Error", "error": "InvalidSession"}).to_string().into())).await.unwrap();

                return;
            }

            socket.send(Message::Text(json!({"type": "Authenticated"}).to_string().into())).await.unwrap();

            let ready = json!({
                "type": "Ready",
                "users": [{"_id": "01USERADA00000000000000000A", "username": "ada", "relationship": "User"}, {"_id": "01USERGRACE00000000000000A", "username": "grace", "display_name": "Grace"}],
                "servers": [{"_id": "01SERVER00000000000000000A", "name": "Lab", "channels": [CHANNEL]}],
                "channels": [
                    {"_id": CHANNEL, "channel_type": "TextChannel", "server": "01SERVER00000000000000000A", "name": "general"},
                    {"_id": "01DM00000000000000000000AA", "channel_type": "DirectMessage", "recipients": ["01USERADA00000000000000000A", "01USERGRACE00000000000000A"]}
                ]
            });

            socket.send(Message::Text(ready.to_string().into())).await.unwrap();
        });
    }
}

pub async fn fake_stoat() -> (String, Arc<FakeStoat>) {
    let stoat = Arc::new(FakeStoat::default());
    let socket_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let events_address = format!("ws://127.0.0.1:{}", socket_listener.local_addr().unwrap().port());

    tokio::spawn(events(socket_listener));

    let router = Router::new().fallback(any(handle)).with_state(Shared { stoat: stoat.clone(), events: events_address });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    tokio::spawn(async move { axum::serve(listener, router).await });

    (base, stoat)
}
