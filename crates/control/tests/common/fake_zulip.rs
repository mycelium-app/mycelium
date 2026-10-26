use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine;
use serde_json::json;
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

async fn handle(State(zulip): State<Arc<FakeZulip>>, method: Method, uri: Uri, headers: HeaderMap, body: String) -> Response {
    let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{ACCOUNT}:{API_KEY}")));

    if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some(&expected) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"result": "error", "msg": "Invalid API key", "code": "UNAUTHORIZED"}))).into_response();
    }

    zulip.seen.lock().unwrap().push((method.clone(), uri.path().to_string(), format!("{}{}", uri.query().unwrap_or_default(), body)));

    match (method, uri.path()) {
        (Method::GET, "/api/v1/users/me") => Json(json!({"result": "success", "email": ACCOUNT})).into_response(),
        (Method::GET, "/api/v1/users/me/subscriptions") => Json(json!({"result": "success", "subscriptions": [{"stream_id": 7, "name": "general", "description": "Everything"}]})).into_response(),
        (Method::GET, "/api/v1/messages") => Json(json!({"result": "success", "messages": [{"id": 41, "type": "stream", "timestamp": 1700000000, "sender_full_name": "Grace", "sender_email": "grace@example.com", "display_recipient": "general", "subject": "lunch", "content": "Noon?"}, {"id": 42, "type": "stream", "timestamp": 1700000060, "sender_full_name": "Linus", "sender_email": "linus@example.com", "display_recipient": "general", "subject": "lunch", "content": "Pizza secret-recipe?"}]})).into_response(),
        (Method::POST, "/api/v1/messages") => Json(json!({"result": "success", "id": 99})).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn fake_zulip() -> (Router, Arc<FakeZulip>) {
    let zulip = Arc::new(FakeZulip::default());

    (Router::new().fallback(any(handle)).with_state(zulip.clone()), zulip)
}
