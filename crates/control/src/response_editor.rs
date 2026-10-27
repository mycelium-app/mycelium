use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::response_filter::{self, Selection};
use crate::state::AppState;
use crate::store::{append_event, find_owned, set_status};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{ResponseFilterRequest, ResponseFilterResult, ResponseToEdit, SendResponseRequest};
use mycelium_proto::conversation::{ConversationStatus, EventKind};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const FILTER_PROPERTY: &str = "response_filter";
pub const NOTHING_SENT_TEXT: &str = "The person sent nothing from this response.";
pub const NO_CONVERSATION_TEXT: &str = "This response needs a person to edit it, and this call did not come from a conversation. Nothing was shared.";
const EDITED_NOTE: &str = "[The person edited this response before you saw it.]";
const NO_LONGER_WAITING: &str = "This response is no longer waiting for you.";

pub const SENT_AS_IT_WAS: &str = "shared";
pub const SENT_EDITED: &str = "edited";
pub const SENT_NOTHING: &str = "not_sent";

#[derive(Debug, Clone)]
pub struct Held {
    pub tool: String,
    pub server: String,
    pub account: Option<String>,
    pub filter: Option<String>,
    pub value: Value,
}

pub struct Sent {
    pub text: String,
    pub outcome: &'static str,
}

pub fn with_filter_property(mut schema: Value) -> Value {
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return schema;
    };

    properties.insert(FILTER_PROPERTY.to_string(), json!({"type": "string", "description": "Optional jq filter. If the person edits this tool's response before you see it, the entries your filter selects are preselected for them to keep."}));

    schema
}

pub fn take_filter(arguments: &mut Value) -> Option<String> {
    let removed = arguments.as_object_mut()?.remove(FILTER_PROPERTY)?;

    removed.as_str().map(str::trim).filter(|filter| !filter.is_empty()).map(String::from)
}

fn value_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

pub fn held_from(tool: &str, server: &str, account: Option<&str>, filter: Option<String>, text: &str) -> Held {
    Held { tool: tool.to_string(), server: server.to_string(), account: account.map(String::from), filter, value: value_of(text) }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub async fn hold(state: &AppState, owner: Uuid, conversation_id: Uuid, request_id: &str, held: Held, text: String) -> ApiResult<Sent> {
    let original = held.value.clone();
    let answer = state.hub.await_response(conversation_id, request_id, held.clone());
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::ReviewRequest, &json!({"request_id": request_id, "server": held.server, "account": held.account, "tool": held.tool}), None).await?;
    let waiting = set_status(&mut tx, conversation_id, ConversationStatus::Input).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(waiting.to_proto()?));

    let send = answer.await.ok().flatten();
    let (sent, outcome) = match send {
        None => (Sent { text: NOTHING_SENT_TEXT.to_string(), outcome: SENT_NOTHING }, "nothing"),
        Some(value) if value == original => (Sent { text, outcome: SENT_AS_IT_WAS }, "sent"),
        Some(value) => (Sent { text: format!("{}\n{EDITED_NOTE}", text_of(&value)), outcome: SENT_EDITED }, "edited"),
    };
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::ReviewResolved, &json!({"request_id": request_id, "outcome": outcome}), None).await?;
    let working = set_status(&mut tx, conversation_id, ConversationStatus::Working).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(working.to_proto()?));

    Ok(sent)
}

fn not_waiting() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", NO_LONGER_WAITING)
}

fn already_answered() -> ApiError {
    ApiError::conflict("already_resolved", "This response has already been answered.")
}

pub async fn show(State(state): State<AppState>, user: AuthUser, Path((id, request_id)): Path<(Uuid, String)>) -> ApiResult<Json<ResponseToEdit>> {
    find_owned(&state.pool, id, user.id).await?;

    let held = state.hub.held_response(id, &request_id).ok_or_else(not_waiting)?;
    let mut shown = ResponseToEdit { tool: held.tool, server: held.server, account: held.account, filter: held.filter.clone(), filter_error: None, value: held.value.clone(), selected_paths: None };

    if let Some(filter) = held.filter {
        match response_filter::select_limited(filter, held.value).await {
            Ok(Selection::Paths(paths)) => shown.selected_paths = Some(paths),
            Ok(Selection::Value(value)) => shown.value = value,
            Err(message) => shown.filter_error = Some(response_filter::unreadable(&message)),
        }
    }

    Ok(Json(shown))
}

pub async fn filter(State(state): State<AppState>, user: AuthUser, Path((id, request_id)): Path<(Uuid, String)>, Json(request): Json<ResponseFilterRequest>) -> ApiResult<Json<ResponseFilterResult>> {
    find_owned(&state.pool, id, user.id).await?;

    let held = state.hub.held_response(id, &request_id).ok_or_else(not_waiting)?;
    let filter = request.filter.trim().to_string();

    if filter.is_empty() {
        return Ok(Json(ResponseFilterResult { selected_paths: None, value: Some(held.value) }));
    }

    match response_filter::select_limited(filter, held.value).await {
        Ok(Selection::Paths(paths)) => Ok(Json(ResponseFilterResult { selected_paths: Some(paths), value: None })),
        Ok(Selection::Value(value)) => Ok(Json(ResponseFilterResult { selected_paths: None, value: Some(value) })),
        Err(message) => Err(ApiError::bad_request("filter_unreadable", &response_filter::unreadable(&message))),
    }
}

pub async fn send(State(state): State<AppState>, user: AuthUser, Path((id, request_id)): Path<(Uuid, String)>, Json(request): Json<SendResponseRequest>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    if !state.hub.answer_response(id, &request_id, request.send) {
        return Err(already_answered());
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_property_is_added_to_a_schema_with_properties() {
        let schema = with_filter_property(json!({"type": "object", "properties": {"city": {"type": "string"}}}));

        assert_eq!(schema["properties"][FILTER_PROPERTY]["type"], "string");
        assert_eq!(schema["properties"]["city"]["type"], "string");
    }

    #[test]
    fn a_schema_without_properties_is_left_alone() {
        assert_eq!(with_filter_property(json!({"type": "object"})), json!({"type": "object"}));
    }

    #[test]
    fn taking_the_filter_strips_it_from_the_arguments() {
        let mut arguments = json!({"city": "Paris", "response_filter": " .[0] "});

        assert_eq!(take_filter(&mut arguments), Some(".[0]".to_string()));
        assert_eq!(arguments, json!({"city": "Paris"}));
    }

    #[test]
    fn a_blank_filter_is_stripped_and_ignored() {
        let mut arguments = json!({"response_filter": "  "});

        assert_eq!(take_filter(&mut arguments), None);
        assert_eq!(arguments, json!({}));
    }

    #[test]
    fn plain_text_is_held_as_a_string_and_json_as_json() {
        assert_eq!(value_of("Sent."), json!("Sent."));
        assert_eq!(value_of(r#"{"a":1}"#), json!({"a": 1}));
    }
}
