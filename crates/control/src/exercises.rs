use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::prompts;
use crate::state::AppState;
use crate::store::{find_owned, parse_enum};
use crate::wellness;
use crate::walkthroughs;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use mycelium_proto::api::{CreateConversationRequest, Exercise, ExerciseReview, ProposeExercisesRequest, ReviewComment, SolutionFile};
use mycelium_proto::link::{ControlToNode, GitCommand};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const KINDS: [&str; 3] = ["language", "area", "other"];
const TEST_TIMEOUT_SECS: u32 = 600;
const MOST_SUGGESTED: i64 = 3;
const EXERCISE_COLUMNS: &str = "id, kind, name, title, brief, minutes, state, branch, conversation_id, reviewing, not_now_until, last_tests, created_at, started_at";

#[derive(sqlx::FromRow)]
struct ExerciseRow {
    id: Uuid,
    kind: String,
    name: String,
    title: String,
    brief: String,
    minutes: Option<i32>,
    state: String,
    branch: Option<String>,
    conversation_id: Option<Uuid>,
    reviewing: bool,
    not_now_until: Option<DateTime<Utc>>,
    last_tests: Option<Value>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct PlaceRow {
    node_id: Uuid,
    project_id: Option<Uuid>,
    agent: String,
    title: String,
    state: String,
    conversation_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct ExerciseQuery {
    skill: Option<String>,
}

async fn latest_review(state: &AppState, exercise_id: Uuid) -> ApiResult<Option<ExerciseReview>> {
    let row = sqlx::query_as::<_, (Uuid, String, Value, Option<Value>, DateTime<Utc>)>("select id, summary, comments, tests, created_at from exercise_reviews where exercise_id = $1 order by created_at desc limit 1").bind(exercise_id).fetch_optional(&state.pool).await?;

    Ok(row.map(|(id, summary, comments, tests, created_at)| ExerciseReview { id, summary, comments: serde_json::from_value(comments).unwrap_or_default(), tests, created_at }))
}

async fn to_proto(state: &AppState, row: ExerciseRow) -> ApiResult<Exercise> {
    let review = latest_review(state, row.id).await?;

    Ok(Exercise {
        id: row.id,
        kind: row.kind,
        name: row.name,
        title: row.title,
        brief: row.brief,
        minutes: row.minutes,
        state: row.state,
        branch: row.branch,
        conversation_id: row.conversation_id,
        reviewing: row.reviewing,
        not_now_until: row.not_now_until,
        tests: row.last_tests,
        review,
        created_at: row.created_at,
        started_at: row.started_at,
    })
}

async fn one(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<Exercise> {
    let row = sqlx::query_as::<_, ExerciseRow>(sqlx::AssertSqlSafe(format!("select {EXERCISE_COLUMNS} from exercises where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That exercise"))?;

    to_proto(state, row).await
}

async fn many(state: &AppState, rows: Vec<ExerciseRow>) -> ApiResult<Vec<Exercise>> {
    let mut exercises = Vec::with_capacity(rows.len());

    for row in rows {
        exercises.push(to_proto(state, row).await?);
    }

    Ok(exercises)
}

async fn place_of(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<PlaceRow> {
    sqlx::query_as::<_, PlaceRow>("select node_id, project_id, agent, title, state, conversation_id from exercises where id = $1 and owner = $2").bind(id).bind(owner).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That exercise"))
}

fn slug_of(title: &str, id: Uuid) -> String {
    let words: Vec<String> = title.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect();
    let joined: String = words.join("-").chars().take(40).collect();

    format!("{}-{}", joined.trim_end_matches('-'), &id.simple().to_string()[..6])
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ExerciseQuery>) -> ApiResult<Json<Vec<Exercise>>> {
    let rows = sqlx::query_as::<_, ExerciseRow>(sqlx::AssertSqlSafe(format!("select {EXERCISE_COLUMNS} from exercises where owner = $1 and ($2::text is null or name = $2) order by created_at desc"))).bind(user.id).bind(query.skill).fetch_all(&state.pool).await?;

    Ok(Json(many(&state, rows).await?))
}

pub async fn suggested(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Exercise>>> {
    let rows = sqlx::query_as::<_, ExerciseRow>(sqlx::AssertSqlSafe(format!(
        "select {EXERCISE_COLUMNS} from exercises e where owner = $1 and state = 'suggested' and (not_now_until is null or not_now_until <= now())
           and not exists (select 1 from hidden_skills h where h.owner = e.owner and h.kind = e.kind and h.name = e.name)
         order by created_at desc limit {MOST_SUGGESTED}",
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(many(&state, rows).await?))
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<Exercise>)> {
    user.require_full_access()?;

    let place = place_of(&state, id, user.id).await?;

    if place.conversation_id.is_some() && place.state != "suggested" {
        return Ok((StatusCode::OK, Json(one(&state, id, user.id).await?)));
    }

    let Some(project_id) = place.project_id else {
        return Err(ApiError::conflict("exercise_needs_project", "This exercise has no project to branch from. Register the project on its node first."));
    };
    let branch = format!("learn/{}", slug_of(&place.title, id));
    let request = CreateConversationRequest { account: None, node_id: place.node_id, agent: parse_enum(&place.agent)?, cwd: String::new(), project_id: Some(project_id), branch: Some(branch.clone()), title: Some(place.title.clone()), prompt: None, parent_id: None, side_kind: None };
    let conversation = conversations::start(&state, &user, request).await?;

    sqlx::query("update exercises set state = 'started', branch = $2, cwd = $3, conversation_id = $4, started_at = now() where id = $1").bind(id).bind(&branch).bind(&conversation.cwd).bind(conversation.id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok((StatusCode::CREATED, Json(one(&state, id, user.id).await?)))
}

pub async fn not_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Exercise>> {
    user.require_full_access()?;
    place_of(&state, id, user.id).await?;

    let zone = wellness::zone_of(&state, user.id, None).await?;
    let tomorrow = Utc::now().with_timezone(&zone).date_naive() + Duration::days(1);
    let until = tomorrow.and_hms_opt(0, 0, 0).and_then(|midnight| midnight.and_local_timezone(zone).earliest()).map(|midnight| midnight.with_timezone(&Utc)).unwrap_or_else(|| Utc::now() + Duration::days(1));

    sqlx::query("update exercises set not_now_until = $2 where id = $1").bind(id).bind(until).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(one(&state, id, user.id).await?))
}

async fn started_place(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<(Uuid, Uuid)> {
    let place = place_of(state, id, owner).await?;

    match place.conversation_id {
        Some(conversation_id) => Ok((conversation_id, place.node_id)),
        None => Err(ApiError::conflict("exercise_not_started", "Start the exercise first.")),
    }
}

pub async fn run_tests(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    user.require_full_access()?;

    let (conversation_id, _) = started_place(&state, id, user.id).await?;
    let row = find_owned(&state.pool, conversation_id, user.id).await?;
    let command = match row.project_id {
        Some(project_id) => sqlx::query_scalar::<_, Option<String>>("select test_command from projects where id = $1").bind(project_id).fetch_optional(&state.pool).await?.flatten(),
        None => None,
    };
    let result = relay(&state, row.node_id, row.cwd, GitCommand::RunTests { command, timeout_secs: TEST_TIMEOUT_SECS }).await?;

    sqlx::query("update exercises set last_tests = $2 where id = $1").bind(id).bind(&result).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(result))
}

fn review_prompt(tests: Option<&Value>) -> String {
    let results = match tests {
        Some(tests) => format!(" The last test run: {tests}."),
        None => String::new(),
    };

    format!("Review my solution to this exercise on this branch. Read the changes with git, comment on specific lines, and call save_exercise_review once with a short summary and your comments. Say what is good as well as what to improve, and do not give the full answer.{results} Do not edit, create or delete any file.")
}

pub async fn review(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<Exercise>)> {
    user.require_full_access()?;

    let (conversation_id, _) = started_place(&state, id, user.id).await?;
    let tests = sqlx::query_scalar::<_, Option<Value>>("update exercises set reviewing = true where id = $1 returning last_tests").bind(id).fetch_one(&state.pool).await?;

    prompts::continue_unless_queued(&state, user.id, conversation_id, &review_prompt(tests.as_ref())).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok((StatusCode::ACCEPTED, Json(one(&state, id, user.id).await?)))
}

pub async fn done(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Exercise>> {
    user.require_full_access()?;
    started_place(&state, id, user.id).await?;

    sqlx::query("update exercises set state = 'done', reviewing = false where id = $1").bind(id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::WellnessChanged);

    Ok(Json(one(&state, id, user.id).await?))
}

pub async fn solution(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<SolutionFile>>> {
    let source = sqlx::query_scalar::<_, Option<Uuid>>("select source_conversation_id from exercises where id = $1 and owner = $2").bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That exercise"))?;
    let Some(source) = source else {
        return Ok(Json(Vec::new()));
    };
    let changes = walkthroughs::scope_changes(&state, source, "branch").await.unwrap_or_default();

    Ok(Json(changes.into_iter().map(|change| SolutionFile { path: change.path, diff: change.hunks.iter().map(|hunk| hunk.text.as_str()).collect::<Vec<_>>().join("\n") }).collect()))
}

pub async fn propose(State(state): State<AppState>, user: AuthUser, Json(request): Json<ProposeExercisesRequest>) -> ApiResult<(StatusCode, Json<Value>)> {
    user.require_full_access()?;

    let prompt = "Propose up to three practice exercises for me, based on code agents wrote recently in my conversations. Use find_conversations and recall to look. For each, call propose_exercise with a short title, a brief that says what to build or fix without giving the answer, the skill it practises (a language, or an area of the codebase) and the minutes it should take, naming the conversation the code came from. Do not change any files.".to_string();
    let conversation = conversations::start(&state, &user, CreateConversationRequest { account: None, node_id: request.node_id, agent: request.agent, cwd: request.cwd, project_id: request.project_id, branch: None, title: Some("Propose exercises".to_string()), prompt: Some(prompt), parent_id: None, side_kind: None }).await?;

    Ok((StatusCode::ACCEPTED, Json(json!({"conversation_id": conversation.id}))))
}

pub async fn propose_from_agent(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Exercise, String>> {
    let text = |field: &str| arguments.get(field).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    let (Some(title), Some(brief), Some(name)) = (text("title"), text("brief"), text("skill_name")) else {
        return Ok(Err("Nothing was saved. Give the exercise a title, a brief and the name of the skill it practises.".to_string()));
    };
    let kind = text("skill_kind").unwrap_or("other");

    if !KINDS.contains(&kind) {
        return Ok(Err("Nothing was saved. skill_kind is language, area or other.".to_string()));
    }

    let own = sqlx::query_as::<_, (Uuid,)>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?.0;
    let source = match text("source_conversation_id") {
        Some(given) => match given.parse::<Uuid>() {
            Ok(source) => source,
            Err(_) => return Ok(Err("Nothing was saved. source_conversation_id is not a conversation id.".to_string())),
        },
        None => conversation_id,
    };
    let place = sqlx::query_as::<_, (Uuid, Option<Uuid>, String)>("select node_id, project_id, agent from conversations where id = $1 and owner = $2").bind(source).bind(own).fetch_optional(&state.pool).await?;
    let Some((node_id, project_id, agent)) = place else {
        return Ok(Err("Nothing was saved. The source conversation is not one of the person's.".to_string()));
    };
    let minutes = arguments.get("minutes").and_then(Value::as_i64).filter(|minutes| (1..=480).contains(minutes)).map(|minutes| minutes as i32);
    let row = sqlx::query_as::<_, ExerciseRow>(sqlx::AssertSqlSafe(format!(
        "insert into exercises (owner, kind, name, title, brief, source_conversation_id, node_id, project_id, agent, minutes) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning {EXERCISE_COLUMNS}",
    )))
    .bind(own)
    .bind(kind)
    .bind(name)
    .bind(title)
    .bind(brief)
    .bind(source)
    .bind(node_id)
    .bind(project_id)
    .bind(agent)
    .bind(minutes)
    .fetch_one(&state.pool)
    .await?;

    state.publish(Some(own), StreamMessage::WellnessChanged);

    Ok(Ok(to_proto(state, row).await?))
}

pub async fn save_review(state: &AppState, conversation_id: Uuid, arguments: &Value) -> ApiResult<Result<Exercise, String>> {
    let found = sqlx::query_as::<_, (Uuid, Uuid, Option<Value>)>("select id, owner, last_tests from exercises where conversation_id = $1 and reviewing").bind(conversation_id).fetch_optional(&state.pool).await?;
    let Some((exercise_id, owner, tests)) = found else {
        return Ok(Err("Nothing was saved. Only a review the person asked for on an exercise can be saved.".to_string()));
    };
    let Some(summary) = arguments.get("summary").and_then(Value::as_str).map(str::trim).filter(|summary| !summary.is_empty()) else {
        return Ok(Err("Nothing was saved. Give a short summary of the review.".to_string()));
    };
    let comments: Vec<ReviewComment> = match arguments.get("comments") {
        Some(given) => match serde_json::from_value(given.clone()) {
            Ok(comments) => comments,
            Err(_) => return Ok(Err("Nothing was saved. Each comment needs a path, a line number, a body and optionally praise.".to_string())),
        },
        None => Vec::new(),
    };

    sqlx::query("insert into exercise_reviews (exercise_id, summary, comments, tests) values ($1, $2, $3, $4)").bind(exercise_id).bind(summary).bind(json!(comments)).bind(tests).execute(&state.pool).await?;
    sqlx::query("update exercises set state = 'reviewed', reviewing = false where id = $1").bind(exercise_id).execute(&state.pool).await?;
    state.publish(Some(owner), StreamMessage::WellnessChanged);

    Ok(Ok(one(state, exercise_id, owner).await?))
}

pub async fn refuse_edit(state: &AppState, node_id: Uuid, conversation_id: Uuid, request_id: &str, request: &Value) -> ApiResult<bool> {
    let changes_files = matches!(request["toolCall"]["kind"].as_str(), Some("edit" | "delete" | "move"));

    if !changes_files {
        return Ok(false);
    }

    let reviewing = sqlx::query_scalar::<_, bool>("select exists (select 1 from exercises where conversation_id = $1 and reviewing)").bind(conversation_id).fetch_one(&state.pool).await?;

    if !reviewing {
        return Ok(false);
    }

    let reject = request["options"].as_array().into_iter().flatten().find(|option| option["kind"].as_str().is_some_and(|kind| kind.starts_with("reject")));
    let outcome = match reject.and_then(|option| option["optionId"].as_str()) {
        Some(option_id) => json!({"outcome": "selected", "optionId": option_id}),
        None => json!({"outcome": "cancelled"}),
    };

    state.hub.send(node_id, ControlToNode::PermissionResponse { conversation_id, request_id: request_id.to_string(), outcome });

    Ok(true)
}
