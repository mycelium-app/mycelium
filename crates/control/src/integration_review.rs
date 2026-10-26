use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{append_event, find_owned, set_status};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::conversation::{ConversationStatus, EventKind, ReviewAnswer, ReviewDecision, ReviewItems};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const WITHHELD_TEXT: &str = "The person withheld this result. Do not ask for it again unless they say so.";
pub const NO_CONVERSATION_TEXT: &str = "This result needs a person's review, and this call did not come from a conversation. Nothing was shared.";
const NO_LONGER_WAITING: &str = "This result is no longer waiting for review.";

pub struct Reviewed {
    pub text: String,
    pub review: &'static str,
}

pub fn split(text: &str) -> Vec<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => items,
        _ => vec![Value::String(text.to_string())],
    }
}

fn withheld() -> Reviewed {
    Reviewed { text: WITHHELD_TEXT.to_string(), review: "withheld" }
}

pub async fn hold(state: &AppState, owner: Uuid, conversation_id: Uuid, request_id: &str, server: &str, tool: &str, text: String) -> ApiResult<Reviewed> {
    let items = split(&text);

    if items.is_empty() {
        return Ok(Reviewed { text, review: "shared" });
    }

    let count = items.len();
    let answer = state.hub.await_review(conversation_id, request_id, items.clone());
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::ReviewRequest, &json!({"request_id": request_id, "server": server, "tool": tool, "count": count}), None).await?;
    let waiting = set_status(&mut tx, conversation_id, ConversationStatus::Input).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(waiting.to_proto()?));

    let answered = answer.await.ok();
    let (decision, removed) = match &answered {
        Some(answer) => (answer.decision, answer.removed.clone()),
        None => (ReviewDecision::Withhold, Vec::new()),
    };
    let mut tx = state.pool.begin().await?;
    let event = append_event(&mut tx, conversation_id, EventKind::ReviewResolved, &json!({"request_id": request_id, "decision": decision, "removed_count": removed.len()}), None).await?;
    let working = set_status(&mut tx, conversation_id, ConversationStatus::Working).await?;
    tx.commit().await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(working.to_proto()?));

    Ok(match decision {
        ReviewDecision::Share => Reviewed { text, review: "shared" },
        ReviewDecision::Withhold => withheld(),
        ReviewDecision::Remove => {
            let kept: Vec<Value> = items.iter().enumerate().filter(|(index, _)| !removed.contains(index)).map(|(_, item)| item.clone()).collect();
            let note = format!("[The person removed {} of {count} items before you saw them.]", count - kept.len());

            Reviewed { text: format!("{}\n{note}", Value::Array(kept)), review: "removed" }
        }
    })
}

pub async fn items(State(state): State<AppState>, user: AuthUser, Path((id, request_id)): Path<(Uuid, String)>) -> ApiResult<Json<ReviewItems>> {
    find_owned(&state.pool, id, user.id).await?;

    match state.hub.review_items(id, &request_id) {
        Some(items) => Ok(Json(ReviewItems { items })),
        None => Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", NO_LONGER_WAITING)),
    }
}

pub async fn answer(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(answer): Json<ReviewAnswer>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    let Some(items) = state.hub.review_items(id, &answer.request_id) else {
        return Err(ApiError::conflict("already_resolved", "This result has already been answered."));
    };

    if answer.decision == ReviewDecision::Remove {
        if answer.removed.is_empty() || answer.removed.iter().any(|index| *index >= items.len()) {
            return Err(ApiError::bad_request("nothing_to_remove", "Untick the items to remove."));
        }
    }

    let request_id = answer.request_id.clone();

    if !state.hub.answer_review(id, &request_id, answer) {
        return Err(ApiError::conflict("already_resolved", "This result has already been answered."));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_array_is_one_item_per_element() {
        assert_eq!(split(r#"[{"a":1},{"a":2}]"#).len(), 2);
    }

    #[test]
    fn a_json_object_is_one_item() {
        assert_eq!(split(r#"{"a":1,"b":2}"#).len(), 1);
    }

    #[test]
    fn plain_text_is_one_item_holding_the_text() {
        assert_eq!(split("Sent."), vec![json!("Sent.")]);
    }
}
