use crate::assistants;
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
use chrono::TimeZone;
use chrono_tz::Tz;
use croner::Cron;
use croner::parser::{CronParser, Seconds};
use mycelium_proto::api::{ConfirmScheduleRequest, CreateConversationRequest, CreateScheduleRequest, EachRun, PatchScheduleRequest, PromptRequest, RunOutcome, Schedule, ScheduleRun, SetTimeZoneRequest};
use mycelium_proto::stream::StreamMessage;
use sqlx::PgConnection;
use uuid::Uuid;

const SCHEDULE_COLUMNS: &str = "id, owner, node_id, agent, cwd, project_id, prompt, cron, each_run, paused, time_zone, proposed, conversation_id, next_run_at, last_run_at, last_conversation_id, created_at, assistant_id";
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
    time_zone: String,
    proposed: bool,
    conversation_id: Option<Uuid>,
    next_run_at: chrono::DateTime<chrono::Utc>,
    last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    last_conversation_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    assistant_id: Option<Uuid>,
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
            time_zone: self.time_zone,
            proposed: self.proposed,
            conversation_id: self.conversation_id,
            next_run_at: self.next_run_at,
            last_run_at: self.last_run_at,
            last_conversation_id: self.last_conversation_id,
            assistant_id: self.assistant_id,
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

fn parse_zone(name: &str) -> ApiResult<Tz> {
    name.trim().parse::<Tz>().map_err(|_| ApiError::bad_request("time_zone_invalid", "Enter a time zone from the IANA list, for example Europe/London."))
}

fn require_prompt(prompt: &str) -> ApiResult<&str> {
    let prompt = prompt.trim();

    if prompt.is_empty() {
        return Err(ApiError::bad_request("prompt_required", "Enter the prompt to run on this schedule."));
    }

    Ok(prompt)
}

pub fn preset_cron(preset: &str) -> Option<String> {
    let preset = preset.trim().to_lowercase();

    if let Some(time) = preset.strip_prefix("weekdays at ") {
        let (hour, minute) = parse_time(time)?;

        return Some(format!("{minute} {hour} * * 1-5"));
    }

    if let Some(time) = preset.strip_prefix("fridays at ") {
        let (hour, minute) = parse_time(time)?;

        return Some(format!("{minute} {hour} * * 5"));
    }

    if let Some(time) = preset.strip_prefix("daily at ") {
        let (hour, minute) = parse_time(time)?;

        return Some(format!("{minute} {hour} * * *"));
    }

    let interval = preset.strip_prefix("every ")?;
    let (count, unit) = interval.split_once(' ')?;
    let count: u32 = count.parse().ok()?;

    match unit {
        "minutes" if (1..60).contains(&count) => Some(format!("*/{count} * * * *")),
        "hours" if (1..24).contains(&count) => Some(format!("0 */{count} * * *")),
        _ => None,
    }
}

pub fn describe_cron(cron: &str) -> String {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    let clock = |hour: &str, minute: &str| Some(format!("{:02}:{:02}", hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?));

    match fields.as_slice() {
        [minutes, "*", "*", "*", "*"] => minutes.strip_prefix("*/").map(|count| format!("Every {count} minutes")),
        ["0", hours, "*", "*", "*"] if hours.starts_with("*/") => hours.strip_prefix("*/").map(|count| format!("Every {count} hours")),
        [minute, hour, "*", "*", "*"] => clock(hour, minute).map(|time| format!("Daily at {time}")),
        [minute, hour, "*", "*", "1-5"] => clock(hour, minute).map(|time| format!("Weekdays at {time}")),
        [minute, hour, "*", "*", "5"] => clock(hour, minute).map(|time| format!("Fridays at {time}")),
        _ => None,
    }
    .unwrap_or_else(|| cron.to_string())
}

fn parse_time(text: &str) -> Option<(u32, u32)> {
    let (hour, minute) = text.trim().split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;

    (hour < 24 && minute < 60).then_some((hour, minute))
}

fn next_run_after(expression: &str, zone: Tz, after: chrono::DateTime<chrono::Utc>) -> ApiResult<chrono::DateTime<chrono::Utc>> {
    let cron: Cron = CronParser::builder()
        .seconds(Seconds::Disallowed)
        .build()
        .parse(expression)
        .map_err(|_| ApiError::bad_request("cron_invalid", "Enter the schedule as five cron fields, for example 0 6 * * 1 for Mondays at 06:00."))?;

    let local = zone.from_utc_datetime(&after.naive_utc());

    cron.find_next_occurrence(&local, false).map(|next| next.with_timezone(&chrono::Utc)).map_err(|_| ApiError::bad_request("cron_never_runs", "This schedule never runs. Check the day and month fields."))
}

async fn zone_of(state: &AppState, owner: Uuid) -> ApiResult<Tz> {
    let stored = sqlx::query_scalar::<_, Option<String>>("select time_zone from users where id = $1").bind(owner).fetch_one(&state.pool).await?;

    Ok(stored.and_then(|name| name.parse::<Tz>().ok()).unwrap_or(Tz::UTC))
}

fn publish(state: &AppState, row: ScheduleRow) -> ApiResult<Schedule> {
    let owner = row.owner;
    let assistant_id = row.assistant_id;
    let schedule = row.to_proto()?;

    state.publish(Some(owner), StreamMessage::Schedule(schedule.clone()));

    if let Some(assistant_id) = assistant_id {
        let state = state.clone();

        tokio::spawn(async move {
            let _ = assistants::publish(&state, owner, assistant_id).await;
        });
    }

    Ok(schedule)
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
    let prompt = require_prompt(&request.prompt)?;
    let zone = match request.time_zone.as_deref() {
        Some(name) => {
            let zone = parse_zone(name)?;

            sqlx::query("update users set time_zone = $2 where id = $1 and time_zone is null").bind(user.id).bind(zone.name()).execute(&state.pool).await?;

            zone
        }
        None => zone_of(&state, user.id).await?,
    };
    let next_run_at = next_run_after(request.cron.trim(), zone, chrono::Utc::now())?;
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
        "insert into schedules (owner, node_id, agent, cwd, project_id, prompt, cron, each_run, next_run_at, time_zone) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning {SCHEDULE_COLUMNS}",
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
    .bind(zone.name())
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.to_proto()?)))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchScheduleRequest>) -> ApiResult<Json<Schedule>> {
    let found = find_owned(&state, id, user.id).await?;

    reject_proposed(&found)?;

    let prompt = match request.prompt.as_deref() {
        Some(prompt) => require_prompt(prompt)?,
        None => &found.prompt,
    };
    let cron = request.cron.as_deref().map(str::trim).unwrap_or(&found.cron);
    let zone = parse_zone(request.time_zone.as_deref().unwrap_or(&found.time_zone))?;
    let each_run = request.each_run.map(|each_run| enum_text(&each_run)).unwrap_or_else(|| found.each_run.clone());
    let paused = request.paused.unwrap_or(found.paused);
    let schedule_moved = cron != found.cron || zone.name() != found.time_zone;
    let next_run_at = next_run_after(cron, zone, chrono::Utc::now())?;
    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "update schedules set paused = $2, prompt = $3, cron = $4, each_run = $5, time_zone = $6, next_run_at = case when $2 and not $7 then next_run_at else $8 end where id = $1 returning {SCHEDULE_COLUMNS}",
    )))
    .bind(id)
    .bind(paused)
    .bind(prompt)
    .bind(cron)
    .bind(each_run)
    .bind(zone.name())
    .bind(schedule_moved)
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row.to_proto()?))
}

pub async fn confirm(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<ConfirmScheduleRequest>>) -> ApiResult<Json<Schedule>> {
    let Json(request) = request.unwrap_or_default();
    let found = find_owned(&state, id, user.id).await?;

    if !found.proposed {
        return Err(ApiError::conflict("schedule_not_proposed", "This schedule is already confirmed."));
    }

    let cron = request.cron.as_deref().map(str::trim).unwrap_or(&found.cron);
    let zone = parse_zone(request.time_zone.as_deref().unwrap_or(&found.time_zone))?;
    let each_run = request.each_run.map(|each_run| enum_text(&each_run)).unwrap_or(found.each_run);
    let next_run_at = next_run_after(cron, zone, chrono::Utc::now())?;
    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "update schedules set proposed = false, cron = $2, time_zone = $3, each_run = $4, next_run_at = $5,
         last_conversation_id = case when $4 = 'same' then coalesce(last_conversation_id, conversation_id) else last_conversation_id end
         where id = $1 returning {SCHEDULE_COLUMNS}",
    )))
    .bind(id)
    .bind(cron)
    .bind(zone.name())
    .bind(each_run)
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(publish(&state, row)?))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let found = find_owned(&state, id, user.id).await?;

    if !found.proposed {
        return Err(ApiError::conflict("schedule_not_proposed", "This schedule is already confirmed. Delete it instead."));
    }

    sqlx::query("delete from schedules where id = $1").bind(id).execute(&state.pool).await?;
    state.publish(Some(user.id), StreamMessage::ScheduleRemoved { schedule_id: id });

    if let Some(conversation_id) = found.conversation_id {
        let text = format!("I declined the schedule you proposed to run \"{}\". Do not set it up another way.", found.prompt);
        let _ = prompts::prompt(State(state.clone()), user, Path(conversation_id), Json(PromptRequest { text, ..Default::default() })).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_time_zone(State(state): State<AppState>, user: AuthUser, Json(request): Json<SetTimeZoneRequest>) -> ApiResult<StatusCode> {
    let zone = parse_zone(&request.time_zone)?;

    sqlx::query("update users set time_zone = $2 where id = $1").bind(user.id).bind(zone.name()).execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub struct ProposedSchedule<'a> {
    pub cron: String,
    pub prompt: &'a str,
    pub each_run: EachRun,
    pub time_zone: Option<&'a str>,
}

pub async fn propose(state: &AppState, conversation_id: Uuid, proposal: ProposedSchedule<'_>) -> ApiResult<Result<Schedule, String>> {
    let (owner, node_id, agent, cwd, project_id, assistant_id) =
        sqlx::query_as::<_, (Uuid, Uuid, String, String, Option<Uuid>, Option<Uuid>)>("select owner, node_id, agent, cwd, project_id, assistant_id from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let agent = if assistant_id.is_some() { "claude".to_string() } else { agent };
    let zone = match proposal.time_zone {
        Some(name) => match name.trim().parse::<Tz>() {
            Ok(zone) => zone,
            Err(_) => return Ok(Err(format!("Nothing was proposed. {name} is not a time zone. Use an IANA name such as Europe/London, or leave it out to use the person's own."))),
        },
        None => zone_of(state, owner).await?,
    };
    let next_run_at = match next_run_after(&proposal.cron, zone, chrono::Utc::now()) {
        Ok(next_run_at) => next_run_at,
        Err(cause) => return Ok(Err(format!("Nothing was proposed. {}", cause.message()))),
    };
    let same = proposal.each_run == EachRun::Same;
    let row = sqlx::query_as::<_, ScheduleRow>(sqlx::AssertSqlSafe(format!(
        "insert into schedules (owner, node_id, agent, cwd, project_id, prompt, cron, each_run, next_run_at, time_zone, proposed, conversation_id, last_conversation_id, assistant_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, true, $11, case when $12 then $11 end, $13) returning {SCHEDULE_COLUMNS}",
    )))
    .bind(owner)
    .bind(node_id)
    .bind(agent)
    .bind(cwd)
    .bind(project_id)
    .bind(proposal.prompt)
    .bind(&proposal.cron)
    .bind(enum_text(&proposal.each_run))
    .bind(next_run_at)
    .bind(zone.name())
    .bind(conversation_id)
    .bind(same)
    .bind(assistant_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Ok(publish(state, row)?))
}

fn reject_proposed(row: &ScheduleRow) -> ApiResult<()> {
    if row.proposed {
        return Err(ApiError::conflict("schedule_proposed", "Confirm this schedule first. It has not been accepted yet."));
    }

    Ok(())
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

    reject_proposed(&row)?;

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
        "select {SCHEDULE_COLUMNS} from schedules where not paused and not proposed and next_run_at <= now() order by next_run_at limit {DUE_BATCH} for update skip locked",
    )))
    .fetch_all(&mut *tx)
    .await?;

    for row in due {
        let held = crate::wellness::quiet_settings(state, row.owner).await?.is_some_and(|settings| settings.quiet_hold_schedules);

        if held {
            record_held(&mut tx, row.id).await?;

            continue;
        }

        let next_run_at = next_run_after(&row.cron, parse_zone(&row.time_zone)?, chrono::Utc::now())?;

        sqlx::query("update schedules set next_run_at = $2 where id = $1").bind(row.id).bind(next_run_at).execute(&mut *tx).await?;
        fire(state, &mut tx, &row).await?;
    }

    tx.commit().await?;

    Ok(())
}

async fn record_held(connection: &mut PgConnection, schedule_id: Uuid) -> ApiResult<()> {
    sqlx::query(
        "insert into schedule_runs (schedule_id, outcome, reason) select $1, 'held', 'Held until quiet hours end.'
         where not exists (select 1 from (select outcome from schedule_runs where schedule_id = $1 order by ran_at desc limit 1) latest where latest.outcome = 'held')",
    )
    .bind(schedule_id)
    .execute(&mut *connection)
    .await?;

    Ok(())
}

async fn fire(state: &AppState, connection: &mut PgConnection, row: &ScheduleRow) -> ApiResult<ScheduleRun> {
    let user = sqlx::query_as::<_, (Uuid, String, String)>("select id, email, name from users where id = $1").bind(row.owner).fetch_one(&mut *connection).await?;
    let user = AuthUser { id: user.0, email: user.1, name: user.2, session: None, scopes: None, expires_at: None };
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
    if let Some(assistant_id) = row.assistant_id {
        let woken = assistants::prompt(State(state.clone()), user.clone(), Path(assistant_id), Json(PromptRequest { text: row.prompt.clone(), ..Default::default() })).await?;

        return Ok(woken.0.id);
    }

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let each_run: EachRun = parse_enum(&row.each_run)?;

    let reusable = match (each_run, row.last_conversation_id) {
        (EachRun::Same, Some(conversation_id)) => {
            let status = sqlx::query_scalar::<_, String>("select status from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

            status.filter(|status| status != "failed").map(|_| conversation_id)
        }
        _ => None,
    };

    if let Some(conversation_id) = reusable {
        let queued = prompts::prompt(State(state.clone()), AuthUser { id: user.id, email: user.email.clone(), name: user.name.clone(), session: None, scopes: None, expires_at: None }, Path(conversation_id), Json(PromptRequest { text: row.prompt.clone(), ..Default::default() })).await;

        if queued.is_ok() {
            return Ok(conversation_id);
        }
    }

    let request = CreateConversationRequest { account: None, node_id: row.node_id, agent: parse_enum(&row.agent)?, cwd: row.cwd.clone(), project_id: row.project_id, branch: None, title: None, prompt: Some(row.prompt.clone()), parent_id: None, side_kind: None };
    let conversation = conversations::start(state, user, request).await?;

    Ok(conversation.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> chrono::DateTime<chrono::Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn presets_become_cron_expressions_and_anything_else_is_not_a_preset() {
        assert_eq!(preset_cron("Weekdays at 20:00").as_deref(), Some("0 20 * * 1-5"));
        assert_eq!(preset_cron("daily at 9:05").as_deref(), Some("5 9 * * *"));
        assert_eq!(preset_cron("Fridays at 16:00").as_deref(), Some("0 16 * * 5"));
        assert_eq!(preset_cron("every 15 minutes").as_deref(), Some("*/15 * * * *"));
        assert_eq!(preset_cron("every 2 hours").as_deref(), Some("0 */2 * * *"));

        for text in ["weekdays at 25:00", "daily at noon", "every 0 minutes", "every 60 minutes", "every day", "fortnightly"] {
            assert_eq!(preset_cron(text), None, "{text}");
        }
    }

    #[test]
    fn a_cron_expression_is_described_the_way_a_preset_names_it() {
        assert_eq!(describe_cron("0 8 * * 1-5"), "Weekdays at 08:00");
        assert_eq!(describe_cron("5 9 * * *"), "Daily at 09:05");
        assert_eq!(describe_cron("0 16 * * 5"), "Fridays at 16:00");
        assert_eq!(describe_cron("*/15 * * * *"), "Every 15 minutes");
        assert_eq!(describe_cron("0 */2 * * *"), "Every 2 hours");
        assert_eq!(describe_cron("0 8 1 * *"), "0 8 1 * *");
    }

    #[test]
    fn a_schedule_keeps_its_local_time_across_the_clocks_going_forward_and_back() {
        let london = Tz::Europe__London;

        let before_spring = next_run_after("0 20 * * *", london, utc("2026-03-27T12:00:00Z")).unwrap();
        let after_spring = next_run_after("0 20 * * *", london, utc("2026-03-28T21:00:00Z")).unwrap();
        let before_autumn = next_run_after("0 20 * * *", london, utc("2026-10-24T12:00:00Z")).unwrap();
        let after_autumn = next_run_after("0 20 * * *", london, utc("2026-10-25T21:00:00Z")).unwrap();

        assert_eq!(before_spring, utc("2026-03-27T20:00:00Z"));
        assert_eq!(after_spring, utc("2026-03-29T19:00:00Z"));
        assert_eq!(before_autumn, utc("2026-10-24T19:00:00Z"));
        assert_eq!(after_autumn, utc("2026-10-26T20:00:00Z"));
    }

    #[test]
    fn weekday_schedules_skip_the_weekend_in_the_zone_and_not_in_utc() {
        let sydney = Tz::Australia__Sydney;

        let next = next_run_after("0 8 * * 1-5", sydney, utc("2026-10-23T22:00:00Z")).unwrap();

        assert_eq!(next, utc("2026-10-25T21:00:00Z"));
    }
}
