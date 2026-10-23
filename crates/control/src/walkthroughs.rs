use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{Walkthrough, WalkthroughFile, WalkthroughStep};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct WalkthroughRow {
    id: Uuid,
    conversation_id: Uuid,
    steps: Value,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl WalkthroughRow {
    fn to_proto(self) -> ApiResult<Walkthrough> {
        let steps = serde_json::from_value(self.steps).map_err(ApiError::internal)?;

        Ok(Walkthrough { id: self.id, conversation_id: self.conversation_id, steps, created_at: self.created_at })
    }
}

fn text_of(value: &Value, field: &str) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or_default().trim_end().to_string()
}

fn file_of(file: &Value, step_number: usize) -> Result<WalkthroughFile, String> {
    let path = text_of(file, "path").trim().to_string();

    if path.is_empty() {
        return Err(format!("Step {step_number} has a file without a path. Give each file its path."));
    }

    let hunks: Vec<String> = file.get("hunks").and_then(Value::as_array).map(|hunks| hunks.iter().filter_map(Value::as_str).map(|hunk| hunk.trim_end().to_string()).collect()).unwrap_or_default();
    let diff = match text_of(file, "diff") {
        diff if !diff.is_empty() => diff,
        _ => hunks.join("\n"),
    };

    if diff.trim().is_empty() {
        return Err(format!("{path} in step {step_number} has no diff. Give its diff as text, or its hunks."));
    }

    Ok(WalkthroughFile { path, diff })
}

pub async fn record(state: &AppState, owner: Uuid, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Walkthrough, String>> {
    let Some(given) = arguments.get("steps").and_then(Value::as_array).filter(|steps| !steps.is_empty()) else {
        return Ok(Err("Nothing was stored. Give the walkthrough's steps as a list, in reading order.".to_string()));
    };

    let mut steps = Vec::new();

    for (index, step) in given.iter().enumerate() {
        let number = index + 1;
        let title = text_of(step, "title").trim().to_string();

        if title.is_empty() {
            return Ok(Err(format!("Nothing was stored. Step {number} has no title. Give every step a title.")));
        }

        let mut files = Vec::new();

        for file in step.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            match file_of(file, number) {
                Ok(file) => files.push(file),
                Err(message) => return Ok(Err(format!("Nothing was stored. {message}"))),
            }
        }

        steps.push(WalkthroughStep { title, key: step.get("key").and_then(Value::as_bool).unwrap_or(false), explanation: text_of(step, "explanation"), files });
    }

    let row = sqlx::query_as::<_, WalkthroughRow>("insert into walkthroughs (conversation_id, steps) values ($1, $2) returning id, conversation_id, steps, created_at")
        .bind(conversation_id)
        .bind(serde_json::to_value(&steps).map_err(ApiError::internal)?)
        .fetch_one(&state.pool)
        .await?;
    let walkthrough = row.to_proto()?;
    state.publish(Some(owner), StreamMessage::Walkthrough(walkthrough.clone()));

    Ok(Ok(walkthrough))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<Walkthrough>>> {
    find_owned(&state.pool, id, user.id).await?;

    let row = sqlx::query_as::<_, WalkthroughRow>("select id, conversation_id, steps, created_at from walkthroughs where conversation_id = $1 order by created_at desc, id desc limit 1").bind(id).fetch_optional(&state.pool).await?;

    Ok(Json(row.map(WalkthroughRow::to_proto).transpose()?))
}
