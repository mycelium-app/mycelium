use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::state::AppState;
use crate::store::find_owned;
use crate::walkthrough_diff::{self, Covered, FileChange};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::link::GitCommand;
use mycelium_proto::api::{Walkthrough, WalkthroughFile, WalkthroughStep};
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

const SCOPES: [&str; 3] = ["branch", "uncommitted", "last_commit"];
const ICONS: [&str; 7] = ["logic", "tests", "docs", "config", "ui", "data", "other"];

#[derive(sqlx::FromRow)]
struct WalkthroughRow {
    id: Uuid,
    conversation_id: Uuid,
    steps: Value,
    created_at: chrono::DateTime<chrono::Utc>,
    dismissed_at: Option<chrono::DateTime<chrono::Utc>>,
    scope: String,
    note: Option<String>,
}

struct GivenFile {
    path: String,
    text: String,
}

impl WalkthroughRow {
    fn to_proto(self) -> ApiResult<Walkthrough> {
        let steps = serde_json::from_value(self.steps).map_err(ApiError::internal)?;

        Ok(Walkthrough { id: self.id, conversation_id: self.conversation_id, steps, created_at: self.created_at, dismissed: self.dismissed_at.is_some(), scope: self.scope, note: self.note })
    }
}

fn text_of(value: &Value, field: &str) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or_default().trim_end().to_string()
}

fn file_of(file: &Value, step_number: usize) -> Result<GivenFile, String> {
    let path = text_of(file, "path").trim().to_string();

    if path.is_empty() {
        return Err(format!("Step {step_number} has a file without a path. Give each file its path."));
    }

    let hunks: Vec<String> = file.get("hunks").and_then(Value::as_array).map(|hunks| hunks.iter().filter_map(Value::as_str).map(|hunk| hunk.trim_end().to_string()).collect()).unwrap_or_default();
    let text = match text_of(file, "diff") {
        diff if !diff.is_empty() => diff,
        _ => hunks.join("\n"),
    };

    Ok(GivenFile { path, text })
}

async fn scope_changes(state: &AppState, conversation_id: Uuid, scope: &str) -> Option<Vec<FileChange>> {
    let (node_id, cwd) = sqlx::query_as::<_, (Uuid, String)>("select node_id, cwd from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await.ok()??;
    let output = relay(state, node_id, cwd, GitCommand::ScopeDiff { scope: scope.to_string() }).await.ok()?;

    Some(walkthrough_diff::parse(output.get("diff")?.as_str()?))
}

pub async fn record(state: &AppState, owner: Uuid, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Walkthrough, String>> {
    let Some(given) = arguments.get("steps").and_then(Value::as_array).filter(|steps| !steps.is_empty()) else {
        return Ok(Err("Nothing was stored. Give the walkthrough's steps as a list, in reading order.".to_string()));
    };

    let scope = match text_of(arguments, "scope").trim() {
        "" => "branch".to_string(),
        scope if SCOPES.contains(&scope) => scope.to_string(),
        other => return Ok(Err(format!("Nothing was stored. {other} is not a scope. Use branch, uncommitted or last_commit."))),
    };
    let mut steps = Vec::new();
    let mut given_files = Vec::new();

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

        steps.push(WalkthroughStep { title, key: step.get("key").and_then(Value::as_bool).unwrap_or(false), section: Some(text_of(step, "section").trim().to_string()).filter(|section| !section.is_empty()), icon: Some(text_of(step, "icon").trim().to_string()).filter(|icon| ICONS.contains(&icon.as_str())), explanation: text_of(step, "explanation"), files: Vec::new() });
        given_files.push(files);
    }

    let changes = scope_changes(state, conversation_id, &scope).await;
    let mut covered = Covered::new();

    for (index, (step, files)) in steps.iter_mut().zip(given_files).enumerate() {
        for given in files {
            let Some(changes) = &changes else {
                if given.text.trim().is_empty() {
                    return Ok(Err(format!("Nothing was stored. {} in step {} has no diff. Give its diff as text, or its hunks.", given.path, index + 1)));
                }

                step.files.push(WalkthroughFile { path: given.path, diff: given.text, unchanged: false });
                continue;
            };

            let change = changes.iter().find(|change| change.path == given.path);
            let indexes = change.map(|change| walkthrough_diff::matching_or_all(change, &given.text)).unwrap_or_default();
            covered.entry(given.path.clone()).or_default().extend(&indexes);
            step.files.push(walkthrough_diff::file_for(change, &given.path, &indexes));
        }
    }

    let left_out = changes.as_deref().map(|changes| walkthrough_diff::uncovered(changes, &covered)).unwrap_or_default();

    if !left_out.is_empty() {
        let listed: Vec<String> = left_out.iter().flat_map(|file| file.diff.lines().filter(|line| line.starts_with("@@")).map(|header| format!("- {}: {header}", file.path)).collect::<Vec<_>>().into_iter().chain(file.diff.lines().all(|line| !line.starts_with("@@")).then(|| format!("- {}: the whole file", file.path)))).collect();

        return Ok(Err(format!("Nothing was stored. These changes are not in any step:\n{}\nAdd them to a step (a file given with only its path covers all of its changes), then call walkthrough again.", listed.join("\n"))));
    }

    let note = changes.is_none().then(|| "The changes in this scope could not be read from the node, so the diffs are as the agent wrote them.".to_string());

    let row = sqlx::query_as::<_, WalkthroughRow>("insert into walkthroughs (conversation_id, steps, scope, note) values ($1, $2, $3, $4) returning id, conversation_id, steps, created_at, dismissed_at, scope, note")
        .bind(conversation_id)
        .bind(serde_json::to_value(&steps).map_err(ApiError::internal)?)
        .bind(&scope)
        .bind(&note)
        .fetch_one(&state.pool)
        .await?;
    let walkthrough = row.to_proto()?;
    state.publish(Some(owner), StreamMessage::Walkthrough(walkthrough.clone()));

    Ok(Ok(walkthrough))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<Walkthrough>>> {
    find_owned(&state.pool, id, user.id).await?;

    let row = sqlx::query_as::<_, WalkthroughRow>("select id, conversation_id, steps, created_at, dismissed_at, scope, note from walkthroughs where conversation_id = $1 order by created_at desc, id desc limit 1").bind(id).fetch_optional(&state.pool).await?;

    Ok(Json(row.map(WalkthroughRow::to_proto).transpose()?))
}

pub async fn uncovered(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<WalkthroughFile>>> {
    let row = sqlx::query_as::<_, WalkthroughRow>("select w.id, w.conversation_id, w.steps, w.created_at, w.dismissed_at, w.scope, w.note from walkthroughs w join conversations c on c.id = w.conversation_id where w.id = $1 and c.owner = $2")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("That walkthrough"))?;
    let (conversation_id, scope) = (row.conversation_id, row.scope.clone());
    let walkthrough = row.to_proto()?;
    let Some(changes) = scope_changes(&state, conversation_id, &scope).await else {
        return Ok(Json(Vec::new()));
    };

    let shown = walkthrough.steps.iter().flat_map(|step| step.files.iter()).filter(|file| !file.unchanged).map(|file| (file.path.clone(), file.diff.clone()));

    Ok(Json(walkthrough_diff::uncovered(&changes, &walkthrough_diff::covered_by(&changes, shown))))
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

async fn set_dismissed(state: &AppState, user: &AuthUser, id: Uuid, dismissed: bool) -> ApiResult<StatusCode> {
    let row = sqlx::query_as::<_, WalkthroughRow>(
        "update walkthroughs w set dismissed_at = case when $3 then now() else null end from conversations c where c.id = w.conversation_id and w.id = $1 and c.owner = $2 returning w.id, w.conversation_id, w.steps, w.created_at, w.dismissed_at, w.scope, w.note",
    )
    .bind(id)
    .bind(user.id)
    .bind(dismissed)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("That walkthrough"))?;
    state.publish(Some(user.id), StreamMessage::Walkthrough(row.to_proto()?));

    Ok(StatusCode::NO_CONTENT)
}

pub async fn dismiss(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    set_dismissed(&state, &user, id, true).await
}

pub async fn reopen(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    set_dismissed(&state, &user, id, false).await
}
