use crate::auth::AuthUser;
use crate::boot::is_ready;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::projects;
use crate::prompts;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use croner::Cron;
use croner::parser::{CronParser, Seconds};
use mycelium_proto::api::{CreateConversationRequest, CreateScheduleRequest, EachRun, PatchScheduleRequest, PromptRequest, RunOutcome, Schedule, ScheduleRun};
use sqlx::PgConnection;
use uuid::Uuid;

const SCHEDULE_COLUMNS: &str = "id, owner, node_id, agent, cwd, project_id, prompt, cron, each_run, paused, next_run_at, last_run_at, last_conversation_id, created_at";
const RUN_COLUMNS: &str = "id, schedule_id, ran_at, outcome, reason, conversation_id";
const DUE_BATCH: i64 = 20;
const RECENT_RUNS: i64 = 50;

#[derive(sqlx::FromRow)]
struct ScheduleRow {
    id: Uuid,
    owner: Uuid,
    node_id: Uuid,
    agent: String,
    cwd: String,
    project_id: Option<Uuid>,
    prompt: String,
    cron: String,
    each_run: String,
    paused: bool,
    next_run_at: chrono::DateTime<chrono::Utc>,
    last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    last_conversation_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl ScheduleRow {
    fn to_proto(self) -> ApiResult<Schedule> {
        Ok(Schedule {
            id: self.id,
            node_id: self.node_id,
            agent: parse_enum(&self.agent)?,
            cwd: self.cwd,
            project_id: self.project_id,
            prompt: self.prompt,
            cron: self.cron,
            each_run: parse_enum(&self.each_run)?,
            paused: self.paused,
            next_run_at: self.next_run_at,
            last_run_at: self.last_run_at,
            last_conversation_id: self.last_conversation_id,
            created_at: self.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    schedule_id: Uuid,
    ran_at: chrono::DateTime<chrono::Utc>,
    outcome: String,
    reason: Option<String>,
    conversation_id: Option<Uuid>,
}

impl RunRow {
    fn to_proto(self) -> ApiResult<ScheduleRun> {
        Ok(ScheduleRun { id: self.id, schedule_id: self.schedule_id, ran_at: self.ran_at, outcome: parse_enum(&self.outcome)?, reason: self.reason, conversation_id: self.conversation_id })
    }
}

fn next_run_after(expression: &str, after: chrono::DateTime<chrono::Utc>) -> ApiResult<chrono::DateTime<chrono::Utc>> {
    let cron: Cron = CronParser::builder()
        .seconds(Seconds::Disallowed)
        .build()
        .parse(expression)
        .map_err(|_| ApiError::bad_request("cron_invalid", "Enter the schedule as five cron fields, for example 0 6 * * 1 for Mondays at 06:00."))?;

    cron.find_next_occurrence(&after, false).map_err(|_| ApiError::bad_request("cron_never_runs", "This schedule never runs. Check the day and month fields."))
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ScheduleRow> {
    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!("select {SCHEDULE_COLUMNS} from schedules where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That schedule"))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Schedule>>> {
    let rows = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!("select {SCHEDULE_COLUMNS} from schedules where owner = $1 order by created_at, id"))).bind(user.id).fetch_all(&state.pool).await?;
    let mut schedules = Vec::new();

    for row in rows {
        schedules.push(row.to_proto()?);
    }

    Ok(Json(schedules))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateScheduleRequest>) -> ApiResult<(StatusCode, Json<Schedule>)> {
    let prompt = request.prompt.trim();

    if prompt.is_empty() {
        return Err(ApiError::bad_request("prompt_required", "Enter the prompt to run on this schedule."));
    }

    let next_run_at = next_run_after(request.cron.trim(), chrono::Utc::now())?;
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(request.node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    if let Some(project_id) = request.project_id {
        let project = projects::find_owned(&state, project_id, user.id).await?;

        if project.node_id != request.node_id {
            return Err(ApiError::bad_request("project_not_on_node", "This project is not set up on that node. Choose a node it is set up on."));
        }
    } else if request.cwd.trim().is_empty() {
        return Err(ApiError::bad_request("cwd_required", "Enter the folder the agent should work in."));
    }

    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "insert into schedules (owner, node_id, agent, cwd, project_id, prompt, cron, each_run, next_run_at) values ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning {SCHEDULE_COLUMNS}",
    )))
    .bind(user.id)
    .bind(request.node_id)
    .bind(enum_text(&request.agent))
    .bind(request.cwd.trim())
    .bind(request.project_id)
    .bind(prompt)
    .bind(request.cron.trim())
    .bind(enum_text(&request.each_run))
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.to_proto()?)))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchScheduleRequest>) -> ApiResult<Json<Schedule>> {
    let found = find_owned(&state, id, user.id).await?;
    let next_run_at = next_run_after(&found.cron, chrono::Utc::now())?;
    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "update schedules set paused = $2, next_run_at = case when $2 then next_run_at else $3 end where id = $1 returning {SCHEDULE_COLUMNS}",
    )))
    .bind(id)
    .bind(request.paused)
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row.to_proto()?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from schedules where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That schedule"));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn runs(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ScheduleRun>>> {
    let rows = sqlx::query_as::<_, RunRow>(sqlx::AssertSqlSafe(format!(
        "select {RUN_COLUMNS} from schedule_runs where schedule_id in (select id from schedules where owner = $1) order by ran_at desc, id limit {RECENT_RUNS}",
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let mut runs = Vec::new();

    for row in rows {
        runs.push(row.to_proto()?);
    }

    Ok(Json(runs))
}

pub async fn run_now(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<ScheduleRun>)> {
    let mut tx = state.pool.begin().await?;
    let locked = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!("select {SCHEDULE_COLUMNS} from schedules where id = $1 and owner = $2 for update"))).bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let Some(row) = locked else {
        return Err(ApiError::not_found("That schedule"));
    };

    let run = fire(&state, &mut tx, &row).await?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(run)))
}

pub async fn run_regularly(state: AppState) {
    loop {
        tokio::time::sleep(state.config.scheduler_tick).await;

        if !is_ready(&state) {
            continue;
        }

        let ran = run_due(&state).await;

        if let Err(cause) = ran {
            tracing::error!("could not run the due schedules: {}", cause.message());
        }
    }
}

async fn run_due(state: &AppState) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let due = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "select {SCHEDULE_COLUMNS} from schedules where not paused and next_run_at <= now() order by next_run_at limit {DUE_BATCH} for update skip locked",
    )))
    .fetch_all(&mut *tx)
    .await?;

    for row in due {
        let next_run_at = next_run_after(&row.cron, chrono::Utc::now())?;

        sqlx::query("update schedules set next_run_at = $2 where id = $1").bind(row.id).bind(next_run_at).execute(&mut *tx).await?;
        fire(state, &mut tx, &row).await?;
    }

    tx.commit().await?;

    Ok(())
}

async fn fire(state: &AppState, connection: &mut PgConnection, row: &ScheduleRow) -> ApiResult<ScheduleRun> {
    let user = sqlx::query_as::<_, (Uuid, String, String)>("select id, email, name from users where id = $1").bind(row.owner).fetch_one(&mut *connection).await?;
    let user = AuthUser { id: user.0, email: user.1, name: user.2, session: None };
    let started = start_run(state, &user, row).await;
    let (outcome, reason, conversation_id) = match started {
        Ok(conversation_id) => (RunOutcome::Started, None, Some(conversation_id)),
        Err(cause) => (RunOutcome::Skipped, Some(cause.message().to_string()), None),
    };

    let run = sqlx::query_as::<_, RunRow>(sqlx::AssertSqlSafe(format!("insert into schedule_runs (schedule_id, outcome, reason, conversation_id) values ($1, $2, $3, $4) returning {RUN_COLUMNS}")))
        .bind(row.id)
        .bind(enum_text(&outcome))
        .bind(&reason)
        .bind(conversation_id)
        .fetch_one(&mut *connection)
        .await?;

    sqlx::query("update schedules set last_run_at = $2, last_conversation_id = coalesce($3, last_conversation_id) where id = $1").bind(row.id).bind(run.ran_at).bind(conversation_id).execute(&mut *connection).await?;

    run.to_proto()
}

async fn start_run(state: &AppState, user: &AuthUser, row: &ScheduleRow) -> ApiResult<Uuid> {
    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let each_run: EachRun = parse_enum(&row.each_run)?;

    if let (EachRun::Same, Some(conversation_id)) = (each_run, row.last_conversation_id) {
        let queued = prompts::prompt(State(state.clone()), AuthUser { id: user.id, email: user.email.clone(), name: user.name.clone(), session: None }, Path(conversation_id), Json(PromptRequest { text: row.prompt.clone() })).await;

        if queued.is_ok() {
            return Ok(conversation_id);
        }
    }

    let request = CreateConversationRequest { account: None, node_id: row.node_id, agent: parse_enum(&row.agent)?, cwd: row.cwd.clone(), project_id: row.project_id, branch: None, title: None, prompt: Some(row.prompt.clone()) };
    let conversation = conversations::start(state, user, request).await?;

    Ok(conversation.id)
}
