use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::state::AppState;
use crate::store::find_owned;
use crate::walkthrough_diff::{self, Covered, FileChange};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use mycelium_proto::link::GitCommand;
use mycelium_proto::api::{Walkthrough, WalkthroughAnswer, WalkthroughAnswerRequest, WalkthroughCheck, WalkthroughFile, WalkthroughNote, WalkthroughStep};
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
    checks: Option<Value>,
}

struct GivenFile {
    path: String,
    text: String,
}

impl WalkthroughRow {
    fn to_proto(self) -> ApiResult<Walkthrough> {
        let steps = serde_json::from_value(self.steps).map_err(ApiError::internal)?;

        let checks = self.checks.and_then(|checks| checks.as_array().cloned()).unwrap_or_default().iter().filter_map(shown_check).collect();

        Ok(Walkthrough { id: self.id, conversation_id: self.conversation_id, steps, created_at: self.created_at, dismissed: self.dismissed_at.is_some(), scope: self.scope, note: self.note, checks })
    }
}

fn shown_check(check: &Value) -> Option<WalkthroughCheck> {
    let choices = check.get("choices")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect();

    Some(WalkthroughCheck { step: check.get("step")?.as_i64()? as i32, question: text_of(check, "question"), choices })
}

fn checks_of(arguments: &Value, step_count: usize) -> Result<Option<Value>, String> {
    let Some(given) = arguments.get("checks").and_then(Value::as_array).filter(|checks| !checks.is_empty()) else {
        return Ok(None);
    };
    let mut seen = Vec::new();

    for (index, check) in given.iter().enumerate() {
        let number = index + 1;
        let step = check.get("step").and_then(Value::as_i64).unwrap_or(0);
        let choices = check.get("choices").and_then(Value::as_array).map_or(0, Vec::len);
        let answer = check.get("answer").and_then(Value::as_i64).unwrap_or(-1);

        if step < 1 || step as usize > step_count {
            return Err(format!("Nothing was stored. Check {number} is about step {step}, and this walkthrough has {step_count} steps. Number the step from 1 to {step_count}."));
        }

        if seen.contains(&step) {
            return Err(format!("Nothing was stored. Check {number} is a second check for step {step}. Give each step at most one check."));
        }

        seen.push(step);

        if text_of(check, "question").trim().is_empty() {
            return Err(format!("Nothing was stored. Check {number} has no question. Give every check a question."));
        }

        if !(2..=4).contains(&choices) {
            return Err(format!("Nothing was stored. Check {number} has {choices} choices. Give it 2 to 4 choices."));
        }

        if answer < 0 || answer as usize >= choices {
            return Err(format!("Nothing was stored. Check {number} has its answer at {answer}, and its choices are numbered from 0 to {}. Give the number of the correct choice.", choices - 1));
        }
    }

    Ok(Some(Value::Array(given.clone())))
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

    let checks = match checks_of(arguments, steps.len()) {
        Ok(checks) => checks,
        Err(message) => return Ok(Err(message)),
    };
    let changes = scope_changes(state, conversation_id, &scope).await;

    if changes.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Err(format!("Nothing was stored. There are no changes in the {scope} scope, so there is nothing to walk through and the current walkthrough stays. Choose a scope that has changes (uncommitted, last_commit or branch).")));
    }

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

    let row = sqlx::query_as::<_, WalkthroughRow>("insert into walkthroughs (conversation_id, steps, scope, note, checks) values ($1, $2, $3, $4, $5) returning id, conversation_id, steps, created_at, dismissed_at, scope, note, checks")
        .bind(conversation_id)
        .bind(serde_json::to_value(&steps).map_err(ApiError::internal)?)
        .bind(&scope)
        .bind(&note)
        .bind(&checks)
        .fetch_one(&state.pool)
        .await?;
    let walkthrough = row.to_proto()?;
    state.publish(Some(owner), StreamMessage::Walkthrough(walkthrough.clone()));

    Ok(Ok(walkthrough))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<Walkthrough>>> {
    find_owned(&state.pool, id, user.id).await?;

    let row = sqlx::query_as::<_, WalkthroughRow>("select id, conversation_id, steps, created_at, dismissed_at, scope, note, checks from walkthroughs where conversation_id = $1 order by created_at desc, id desc limit 1").bind(id).fetch_optional(&state.pool).await?;

    Ok(Json(row.map(WalkthroughRow::to_proto).transpose()?))
}

pub async fn uncovered(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<WalkthroughFile>>> {
    let row = sqlx::query_as::<_, WalkthroughRow>("select w.id, w.conversation_id, w.steps, w.created_at, w.dismissed_at, w.scope, w.note, w.checks from walkthroughs w join conversations c on c.id = w.conversation_id where w.id = $1 and c.owner = $2")
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

pub async fn answers(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<WalkthroughAnswer>>> {
    find_owned_walkthrough(&state, id, user.id).await?;

    let rows = sqlx::query_as::<_, (i32, i32, bool)>("select step, choice, correct from walkthrough_answers where walkthrough_id = $1 and owner = $2 order by step").bind(id).bind(user.id).fetch_all(&state.pool).await?;
    let checks = sqlx::query_scalar::<_, Option<Value>>("select checks from walkthroughs where id = $1").bind(id).fetch_one(&state.pool).await?.and_then(|checks| checks.as_array().cloned()).unwrap_or_default();
    let explanation = |step: i32| checks.iter().find(|check| check.get("step").and_then(Value::as_i64) == Some(i64::from(step))).map(|check| text_of(check, "explanation")).unwrap_or_default();

    Ok(Json(rows.into_iter().map(|(step, choice, correct)| WalkthroughAnswer { step, choice, correct, explanation: explanation(step) }).collect()))
}

pub async fn answer(State(state): State<AppState>, user: AuthUser, Path((id, step)): Path<(Uuid, i32)>, Json(request): Json<WalkthroughAnswerRequest>) -> ApiResult<Json<WalkthroughAnswer>> {
    find_owned_walkthrough(&state, id, user.id).await?;

    let checks = sqlx::query_scalar::<_, Option<Value>>("select checks from walkthroughs where id = $1").bind(id).fetch_one(&state.pool).await?;
    let check = checks.and_then(|checks| checks.as_array().cloned()).unwrap_or_default().into_iter().find(|check| check.get("step").and_then(Value::as_i64) == Some(i64::from(step)));
    let Some(check) = check else {
        return Err(ApiError::bad_request("no_such_check", "This walkthrough has no check for that step. Choose a step that has a question."));
    };
    let choices = check.get("choices").and_then(Value::as_array).map_or(0, Vec::len);

    if request.choice < 0 || request.choice as usize >= choices {
        return Err(ApiError::bad_request("no_such_choice", format!("This question has {choices} choices. Choose one from 0 to {}.", choices - 1)));
    }

    let correct = check.get("answer").and_then(Value::as_i64) == Some(i64::from(request.choice));

    sqlx::query(
        "insert into walkthrough_answers (walkthrough_id, owner, step, choice, correct) values ($1, $2, $3, $4, $5)
         on conflict (walkthrough_id, owner, step) do update set choice = excluded.choice, correct = excluded.correct, answered_at = now()",
    )
    .bind(id)
    .bind(user.id)
    .bind(step)
    .bind(request.choice)
    .bind(correct)
    .execute(&state.pool)
    .await?;

    Ok(Json(WalkthroughAnswer { step, choice: request.choice, correct, explanation: text_of(&check, "explanation") }))
}

async fn set_dismissed(state: &AppState, user: &AuthUser, id: Uuid, dismissed: bool) -> ApiResult<StatusCode> {
    let row = sqlx::query_as::<_, WalkthroughRow>(
        "update walkthroughs w set dismissed_at = case when $3 then now() else null end from conversations c where c.id = w.conversation_id and w.id = $1 and c.owner = $2 returning w.id, w.conversation_id, w.steps, w.created_at, w.dismissed_at, w.scope, w.note, w.checks",
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

#[derive(Deserialize)]
pub struct NoteRequest {
    step: i32,
    file: Option<String>,
    line_start: Option<i32>,
    line_end: Option<i32>,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    comment: String,
}

#[derive(sqlx::FromRow)]
struct NoteRow {
    id: Uuid,
    step: i32,
    file: Option<String>,
    line_start: Option<i32>,
    line_end: Option<i32>,
    quote: String,
    comment: String,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl NoteRow {
    fn to_proto(self) -> WalkthroughNote {
        WalkthroughNote { id: self.id, step: self.step, file: self.file, line_start: self.line_start, line_end: self.line_end, quote: self.quote, comment: self.comment, sent: self.sent_at.is_some(), created_at: self.created_at }
    }
}

pub async fn add_note(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<NoteRequest>) -> ApiResult<(StatusCode, Json<WalkthroughNote>)> {
    let count = find_owned_walkthrough(&state, id, user.id).await?;

    if request.step < 0 || request.step as usize >= count {
        return Err(ApiError::bad_request("no_such_step", format!("This walkthrough has {count} steps. Choose a step from 0 to {}.", count.saturating_sub(1))));
    }

    let comment = request.comment.trim().to_string();
    let quote = request.quote.trim_end().to_string();

    if comment.is_empty() && quote.trim().is_empty() {
        return Err(ApiError::bad_request("note_empty", "Write a note, or select lines to annotate."));
    }

    let row = sqlx::query_as::<_, NoteRow>("insert into walkthrough_notes (walkthrough_id, user_id, step, file, line_start, line_end, quote, comment) values ($1, $2, $3, $4, $5, $6, $7, $8) returning id, step, file, line_start, line_end, quote, comment, sent_at, created_at")
        .bind(id)
        .bind(user.id)
        .bind(request.step)
        .bind(request.file)
        .bind(request.line_start)
        .bind(request.line_end)
        .bind(quote)
        .bind(comment)
        .fetch_one(&state.pool)
        .await?;
    publish_notes(&state, user.id, id).await?;

    Ok((StatusCode::CREATED, Json(row.to_proto())))
}

async fn load_notes(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Vec<WalkthroughNote>> {
    let rows = sqlx::query_as::<_, NoteRow>("select id, step, file, line_start, line_end, quote, comment, sent_at, created_at from walkthrough_notes where walkthrough_id = $1 and user_id = $2 order by created_at, id").bind(id).bind(owner).fetch_all(&state.pool).await?;

    Ok(rows.into_iter().map(NoteRow::to_proto).collect())
}

async fn publish_notes(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let conversation_id = sqlx::query_scalar::<_, Uuid>("select conversation_id from walkthroughs where id = $1").bind(id).fetch_one(&state.pool).await?;
    let notes = load_notes(state, owner, id).await?;
    state.publish(Some(owner), StreamMessage::WalkthroughNotes { conversation_id, walkthrough_id: id, notes });

    Ok(())
}

pub async fn notes(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<WalkthroughNote>>> {
    find_owned_walkthrough(&state, id, user.id).await?;

    Ok(Json(load_notes(&state, user.id, id).await?))
}

pub async fn mark_notes_sent(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned_walkthrough(&state, id, user.id).await?;

    sqlx::query("update walkthrough_notes set sent_at = now() where walkthrough_id = $1 and user_id = $2 and sent_at is null").bind(id).bind(user.id).execute(&state.pool).await?;
    publish_notes(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_note(State(state): State<AppState>, user: AuthUser, Path((id, note_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    find_owned_walkthrough(&state, id, user.id).await?;

    sqlx::query("delete from walkthrough_notes where id = $1 and walkthrough_id = $2 and user_id = $3").bind(note_id).bind(id).bind(user.id).execute(&state.pool).await?;
    publish_notes(&state, user.id, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
