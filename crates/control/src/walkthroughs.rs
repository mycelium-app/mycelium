use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{Walkthrough, WalkthroughFile, WalkthroughStep};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

const ICONS: [&str; 7] = ["logic", "tests", "docs", "config", "ui", "data", "other"];

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

        steps.push(WalkthroughStep { title, key: step.get("key").and_then(Value::as_bool).unwrap_or(false), section: Some(text_of(step, "section").trim().to_string()).filter(|section| !section.is_empty()), icon: Some(text_of(step, "icon").trim().to_string()).filter(|icon| ICONS.contains(&icon.as_str())), explanation: text_of(step, "explanation"), files });
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

async fn find_owned_walkthrough(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<usize> {
    let steps = sqlx::query_scalar::<_, Value>("select w.steps from walkthroughs w join conversations c on c.id = w.conversation_id where w.id = $1 and c.owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?;

    steps.map(|steps| steps.as_array().map_or(0, Vec::len)).ok_or_else(|| ApiError::not_found("That walkthrough"))
}

pub async fn reads(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<i32>>> {
    find_owned_walkthrough(&state, id, user.id).await?;

    let steps = sqlx::query_scalar::<_, i32>("select step from walkthrough_reads where walkthrough_id = $1 and user_id = $2 order by step").bind(id).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(steps))
}

pub async fn mark_read(State(state): State<AppState>, user: AuthUser, Path((id, step)): Path<(Uuid, i32)>) -> ApiResult<StatusCode> {
    let count = find_owned_walkthrough(&state, id, user.id).await?;

    if step < 0 || step as usize >= count {
        return Err(ApiError::bad_request("no_such_step", format!("This walkthrough has {count} steps. Choose a step from 0 to {}.", count.saturating_sub(1))));
    }

    sqlx::query("insert into walkthrough_reads (walkthrough_id, user_id, step) values ($1, $2, $3) on conflict do nothing").bind(id).bind(user.id).bind(step).execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}
