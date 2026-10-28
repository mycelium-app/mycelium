use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{boot, chats, conversations, prompts};
use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use mycelium_proto::api::{
    CreateChatRequest, CreateConversationRequest, PromptRequest, Workflow, WorkflowActor, WorkflowControlRequest, WorkflowDecision, WorkflowEndReason, WorkflowPlan, WorkflowRound, WorkflowRunStep,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

const COLUMNS: &str = "id,owner,parent_id,conversation_id,title,status,plan,steps,round,output,kind,created_at,updated_at,ended_at,started_by,project_id,end_reason,rerun_of,decisions,phases,run_id";

const AGENT_RUN: &str = "agent_run";

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    owner: Uuid,
    parent_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
    title: String,
    status: String,
    plan: Value,
    steps: Value,
    round: i32,
    output: String,
    kind: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    started_by: Option<Value>,
    project_id: Option<Uuid>,
    end_reason: Option<Value>,
    rerun_of: Option<Uuid>,
    decisions: Value,
    phases: Value,
    run_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RoundRow {
    workflow_id: Uuid,
    round: i32,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    reason: Option<String>,
    decided_by: Option<Value>,
    steps: Value,
}

impl Row {
    fn workflow(&self, rounds: Vec<WorkflowRound>) -> ApiResult<Workflow> {
        let plan: WorkflowPlan = serde_json::from_value(self.plan.clone()).map_err(|_| ApiError::not_found("That workflow"))?;
        let steps: Vec<WorkflowRunStep> = serde_json::from_value(self.steps.clone()).map_err(|_| ApiError::not_found("That workflow"))?;
        let gate_open = steps.iter().any(|step| step.status == "gate");
        let status = if self.status == "running" && gate_open { "waiting".to_string() } else { self.status.clone() };
        let stop_rule = plan.stop_when.clone().filter(|stop| !stop.is_empty()).map(|stop| format!("every step\u{2019}s result contains {stop}"));

        Ok(Workflow {
            id: self.id,
            parent_id: self.parent_id,
            conversation_id: self.conversation_id,
            title: self.title.clone(),
            status,
            plan,
            steps,
            round: self.round,
            output: self.output.clone(),
            kind: self.kind.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            ended_at: self.ended_at,
            started_by: self.started_by.clone().and_then(|value| serde_json::from_value(value).ok()),
            project_id: self.project_id,
            end_reason: self.end_reason.clone().and_then(|value| serde_json::from_value(value).ok()),
            stop_rule,
            rerun_of: self.rerun_of,
            rounds,
            decisions: serde_json::from_value(self.decisions.clone()).unwrap_or_default(),
            phases: serde_json::from_value(self.phases.clone()).unwrap_or_default(),
            run_id: self.run_id.clone(),
        })
    }
}

impl RoundRow {
    fn round(&self) -> WorkflowRound {
        WorkflowRound {
            round: self.round,
            started_at: self.started_at,
            ended_at: self.ended_at,
            reason: self.reason.clone(),
            decided_by: self.decided_by.clone().and_then(|value| serde_json::from_value(value).ok()),
            steps: serde_json::from_value(self.steps.clone()).unwrap_or_default(),
        }
    }
}

fn waiting(plan: &WorkflowPlan) -> Vec<WorkflowRunStep> {
    plan.steps
        .iter()
        .map(|step| WorkflowRunStep {
            key: step.key.clone(),
            status: "waiting".into(),
            ..Default::default()
        })
        .collect()
}

fn finished(step: &WorkflowRunStep) -> bool {
    step.status == "done" || step.status == "approved"
}

fn settle(step: &mut WorkflowRunStep, status: &str) {
    step.status = status.into();
    step.ended_at = Some(Utc::now());
}

fn begin(step: &mut WorkflowRunStep, status: &str) {
    step.status = status.into();
    step.started_at.get_or_insert_with(Utc::now);
}

pub async fn actor_of(state: &AppState, owner: Uuid) -> ApiResult<WorkflowActor> {
    let name: Option<String> = sqlx::query_scalar("select name from users where id=$1").bind(owner).fetch_optional(&state.pool).await?;

    Ok(WorkflowActor { kind: "person".into(), name: name.unwrap_or_default() })
}

pub fn validate(plan: &WorkflowPlan, depth: usize) -> ApiResult<()> {
    if depth > 8
        || plan.title.trim().is_empty()
        || plan.steps.is_empty()
        || plan.steps.len() > 64
        || !(1..=20).contains(&plan.max_rounds)
    {
        return Err(ApiError::bad_request(
            "workflow_invalid",
            "Enter a title, 1 to 64 steps, at most 20 rounds and at most 8 nested workflows.",
        ));
    }

    let mut keys = std::collections::HashSet::new();
    for step in &plan.steps {
        if step.key.is_empty()
            || !step
                .key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            || !keys.insert(step.key.clone())
        {
            return Err(ApiError::bad_request(
                "workflow_key_invalid",
                "Give each step a unique key using letters, numbers and underscores.",
            ));
        }

        if let Some(child) = &step.workflow {
            validate(child, depth + 1)?;
        } else if !step.gate
            && (step.prompt.trim().is_empty()
                || (step.provider_id.is_none() && (step.node_id.is_none() || step.agent.is_none())))
        {
            return Err(ApiError::bad_request(
                "workflow_step_invalid",
                "Give each agent step a prompt and a chat provider or a node and agent.",
            ));
        }
    }

    let mut resolved = std::collections::HashSet::new();
    loop {
        let before = resolved.len();
        for step in &plan.steps {
            if step.depends_on.iter().all(|key| resolved.contains(key)) {
                resolved.insert(step.key.clone());
            }
        }

        if resolved.len() == plan.steps.len() {
            break;
        }
        if before == resolved.len() {
            return Err(ApiError::bad_request(
                "workflow_dependencies_invalid",
                "Step dependencies must exist and must not form a cycle.",
            ));
        }
    }

    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(plan): Json<WorkflowPlan>,
) -> ApiResult<Json<Workflow>> {
    user.require_full_access()?;
    let started_by = WorkflowActor { kind: "person".into(), name: user.name.clone() };
    let id = create_owned(&state, user.id, None, None, plan, started_by, None).await?;
    let workflow = load(&state, user.id, id).await?;

    Ok(Json(workflow))
}

pub async fn create_owned(
    state: &AppState,
    owner: Uuid,
    parent: Option<Uuid>,
    conversation: Option<Uuid>,
    plan: WorkflowPlan,
    started_by: WorkflowActor,
    rerun_of: Option<Uuid>,
) -> ApiResult<Uuid> {
    validate(&plan, 0)?;
    let id = Uuid::new_v4();
    let step_project = plan.steps.iter().find_map(|step| step.project_id);
    let mut tx = state.pool.begin().await?;
    sqlx::query("insert into workflows (id,owner,parent_id,conversation_id,title,plan,steps,started_by,rerun_of,project_id) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,coalesce($10,(select project_id from conversations where id=$4)))")
        .bind(id).bind(owner).bind(parent).bind(conversation).bind(&plan.title).bind(json!(plan)).bind(json!(waiting(&plan))).bind(json!(started_by)).bind(rerun_of).bind(step_project).execute(&mut *tx).await?;
    sqlx::query("insert into workflow_rounds (workflow_id,round) values ($1,1)").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    publish(state, owner, id).await?;
    Ok(id)
}

pub async fn rerun(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Workflow>> {
    user.require_full_access()?;
    let original = load(&state, user.id, id).await?;

    if original.kind == AGENT_RUN {
        return Err(ApiError::bad_request("workflow_not_rerunnable", "A Claude Code workflow run cannot be re-run from here. Ask the agent to start it again."));
    }

    if !matches!(original.status.as_str(), "done" | "failed" | "stopped") {
        return Err(ApiError::conflict("workflow_active", "That workflow is still active. Stop it or wait for it to end before re-running it."));
    }

    let started_by = WorkflowActor { kind: "person".into(), name: user.name.clone() };
    let created = create_owned(&state, user.id, original.parent_id, original.conversation_id, original.plan, started_by, Some(id)).await?;
    let workflow = load(&state, user.id, created).await?;

    Ok(Json(workflow))
}

async fn publish(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let workflow = load(state, owner, id).await?;
    state.publish(
        Some(owner),
        mycelium_proto::stream::StreamMessage::Workflow { workflow },
    );
    Ok(())
}

async fn rounds_of(state: &AppState, ids: &[Uuid]) -> ApiResult<HashMap<Uuid, Vec<WorkflowRound>>> {
    let rows = sqlx::query_as::<_, RoundRow>("select workflow_id,round,started_at,ended_at,reason,decided_by,steps from workflow_rounds where workflow_id = any($1) order by workflow_id, round").bind(ids).fetch_all(&state.pool).await?;
    let mut grouped: HashMap<Uuid, Vec<WorkflowRound>> = HashMap::new();

    for row in rows {
        grouped.entry(row.workflow_id).or_default().push(row.round());
    }

    Ok(grouped)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Workflow>>> {
    let rows = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from workflows where owner=$1 order by created_at desc"))).bind(user.id).fetch_all(&state.pool).await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut rounds = rounds_of(&state, &ids).await?;
    let workflows = rows.iter().map(|row| row.workflow(rounds.remove(&row.id).unwrap_or_default())).collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(workflows))
}

pub async fn load(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<Workflow> {
    let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from workflows where id=$1 and owner=$2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That workflow"))?;
    let mut rounds = rounds_of(state, &[id]).await?;

    row.workflow(rounds.remove(&id).unwrap_or_default())
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Workflow>> {
    let workflow = load(&state, user.id, id).await?;

    Ok(Json(workflow))
}

async fn begin_round(connection: &mut sqlx::PgConnection, id: Uuid, round: i32, reason: Option<&str>, decided_by: Option<&WorkflowActor>) -> ApiResult<()> {
    sqlx::query("insert into workflow_rounds (workflow_id,round,reason,decided_by) values ($1,$2,$3,$4) on conflict do nothing")
        .bind(id).bind(round).bind(reason).bind(decided_by.map(|actor| json!(actor))).execute(connection).await?;

    Ok(())
}

async fn keep_round(connection: &mut sqlx::PgConnection, id: Uuid, round: i32, steps: &[WorkflowRunStep]) -> ApiResult<()> {
    sqlx::query("update workflow_rounds set steps=$3,ended_at=now() where workflow_id=$1 and round=$2")
        .bind(id).bind(round).bind(json!(steps)).execute(connection).await?;

    Ok(())
}

fn joined_output(steps: &[WorkflowRunStep]) -> String {
    steps
        .iter()
        .map(|step| format!("{}:\n{}", step.key, step.output))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub async fn control(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<WorkflowControlRequest>,
) -> ApiResult<Json<Workflow>> {
    user.require_full_access()?;
    let actor = WorkflowActor { kind: "person".into(), name: user.name.clone() };
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from workflows where id=$1 and owner=$2 for update"))).bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("That workflow"))?;
    let mut steps: Vec<WorkflowRunStep> = serde_json::from_value(row.steps.clone())
        .map_err(|_| ApiError::not_found("That workflow"))?;
    let plan: WorkflowPlan = serde_json::from_value(row.plan.clone()).map_err(|_| ApiError::not_found("That workflow"))?;
    let active = matches!(row.status.as_str(), "running" | "paused");
    let mut round = row.round;
    let mut output = row.output.clone();
    let mut end_reason: Option<WorkflowEndReason> = None;
    let mut decisions: Vec<WorkflowDecision> = serde_json::from_value(row.decisions.clone()).unwrap_or_default();
    let status = match request.action.as_str() {
        "pause" if row.status == "running" && row.kind != AGENT_RUN => "paused",
        "resume" if row.status == "paused" => "running",
        "stop" if !active => {
            return Err(ApiError::conflict("workflow_ended", "That workflow has already ended, so there is nothing to stop."));
        }
        "stop" => {
            let stopped_step = steps.iter().find(|step| matches!(step.status.as_str(), "running" | "starting" | "gate")).map(|step| step.key.clone());
            end_reason = Some(WorkflowEndReason { reason: "stopped".into(), step: stopped_step, error: None, by: Some(actor.clone()), round: Some(round) });
            "stopped"
        }
        "approve" | "send_back" if active && row.kind != AGENT_RUN => {
            let sending_back = request.action == "send_back";
            let note = request.reason.clone().or_else(|| request.output.clone()).map(|text| text.trim().to_string()).filter(|text| !text.is_empty());

            if sending_back && note.is_none() {
                return Err(ApiError::bad_request("reason_required", "Enter what needs to change, so the next round can fix it"));
            }

            let step = steps
                .iter_mut()
                .find(|step| Some(&step.key) == request.step.as_ref() && step.status == "gate")
                .ok_or_else(|| {
                    ApiError::bad_request(
                        "gate_missing",
                        "Choose a step waiting for your decision.",
                    )
                })?;
            settle(step, if sending_back { "sent_back" } else { "approved" });

            if let (false, Some(text)) = (sending_back, &note) {
                step.output = text.clone();
            }

            let key = step.key.clone();
            decisions.push(WorkflowDecision { round, step: key, decision: request.action.clone(), by: actor.clone(), at: Utc::now(), reason: note.clone() });

            if !sending_back {
                row.status.as_str()
            } else {
                keep_round(&mut tx, id, round, &steps).await?;

                if round >= plan.max_rounds as i32 {
                    output = joined_output(&steps);
                    end_reason = Some(WorkflowEndReason { reason: "round_limit".into(), step: None, error: None, by: None, round: Some(round) });
                    "done"
                } else {
                    round += 1;
                    steps = waiting(&plan);
                    begin_round(&mut tx, id, round, note.as_deref(), Some(&actor)).await?;
                    "running"
                }
            }
        }
        _ => {
            return Err(ApiError::bad_request(
                "workflow_action_invalid",
                "Choose pause, resume, stop, approve or send back a waiting gate.",
            ));
        }
    };
    let ended = matches!(status, "done" | "stopped");
    sqlx::query("update workflows set status=$2,steps=$3,round=$4,output=$5,decisions=$6,end_reason=coalesce($7,end_reason),ended_at=case when $8 then now() else ended_at end,updated_at=now() where id=$1")
        .bind(id)
        .bind(status)
        .bind(json!(steps))
        .bind(round)
        .bind(&output)
        .bind(json!(decisions))
        .bind(end_reason.as_ref().map(|reason| json!(reason)))
        .bind(ended)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    if status == "stopped" {
        if row.kind == AGENT_RUN {
            stop_agent_run(&state, user.id, &row, &mut steps).await;
            sqlx::query("update workflows set steps=$2 where id=$1").bind(id).bind(json!(steps)).execute(&state.pool).await?;
        } else {
            stop_children(&state, user.id, id, Some(&actor)).await?;
        }
    }

    publish(&state, user.id, id).await?;

    if let Err(error) = notify_parent(&state, user.id, id).await {
        tracing::warn!(%id, error = error.message(), "workflow result notification failed");
    }

    let workflow = load(&state, user.id, id).await?;

    Ok(Json(workflow))
}

async fn stop_agent_run(state: &AppState, owner: Uuid, row: &Row, steps: &mut [WorkflowRunStep]) {
    if let Some(conversation) = row.conversation_id {
        let stopped = prompts::stop(state, owner, conversation).await;

        if let Err(error) = stopped {
            tracing::warn!(workflow = %row.id, %conversation, error = error.message(), "stopping a workflow run failed");
        }
    }

    for step in steps.iter_mut().filter(|step| step.status == "running" || step.status == "paused") {
        settle(step, "stopped");
    }
}

async fn notify_parent(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let workflow = load(state, owner, id).await?;
    let Some(parent) = workflow.conversation_id.filter(|_| workflow.parent_id.is_none() && workflow.kind != AGENT_RUN) else {
        return Ok(());
    };

    if !matches!(workflow.status.as_str(), "done" | "failed" | "stopped") {
        return Ok(());
    }

    let conversation = crate::store::find_owned(&state.pool, parent, owner).await?;

    if conversation.archived_at.is_some() {
        return Ok(());
    }

    let claimed = sqlx::query("insert into workflow_dispatches (workflow_id, round, step_key, operation) values ($1,$2,'result','notify') on conflict do nothing").bind(id).bind(workflow.round).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    let text = format!(
        "Workflow {} ({id}) {}.\n\n{}",
        workflow.title, workflow.status, workflow.output
    );

    if conversation.agent == crate::chats::CHAT_AGENT {
        chats::send_or_queue(state, owner, parent, &text).await?;
    } else {
        prompts::send(
            state,
            owner,
            parent,
            PromptRequest {
                text,
                ..Default::default()
            },
        )
        .await?;
    }

    dispatched(state, id, workflow.round, "result", "notify", parent).await
}

async fn stop_children(state: &AppState, owner: Uuid, id: Uuid, by: Option<&WorkflowActor>) -> ApiResult<()> {
    let reason = json!(WorkflowEndReason { reason: "stopped".into(), step: None, error: None, by: by.cloned(), round: None });
    let rows: Vec<(Uuid, Value)> = sqlx::query_as("with recursive children as (select id from workflows where id=$1 and owner=$2 union all select w.id from workflows w join children c on w.parent_id=c.id) update workflows set status='stopped',ended_at=coalesce(ended_at,now()),end_reason=coalesce(end_reason,$3),updated_at=now() where id in (select id from children) returning id,steps").bind(id).bind(owner).bind(reason).fetch_all(&state.pool).await?;
    for (workflow, value) in rows {
        let mut steps: Vec<WorkflowRunStep> = serde_json::from_value(value).unwrap_or_default();
        for step in steps
            .iter()
            .filter(|step| step.status == "running" || step.status == "starting")
        {
            if let Some(conversation) = step.conversation_id {
                let stopped = prompts::stop(state, owner, conversation).await;

                if let Err(error) = stopped {
                    tracing::warn!(%workflow, %conversation, error = error.message(), "stopping a workflow step failed");
                }
            }
        }

        for step in &mut steps {
            if matches!(step.status.as_str(), "running" | "starting") {
                settle(step, "stopped");
            } else if matches!(step.status.as_str(), "waiting" | "gate") {
                step.status = "not_run".into();
            }
        }

        sqlx::query("update workflows set steps=$2 where id=$1").bind(workflow).bind(json!(steps)).execute(&state.pool).await?;
        publish(state, owner, workflow).await?;
    }

    Ok(())
}

fn render(prompt: &str, steps: &[WorkflowRunStep], previous: &str, feedback: &str) -> String {
    let mut rendered = prompt.replace("{{previous}}", previous);
    for step in steps {
        rendered = rendered.replace(&format!("{{{{{}.output}}}}", step.key), &step.output);
    }

    if !feedback.is_empty() {
        rendered.push_str(&format!("\n\nThe last round was sent back with this feedback:\n{feedback}"));
    }

    rendered
}

async fn link_native(
    state: &AppState,
    owner: Uuid,
    parent: Option<Uuid>,
    conversation: Uuid,
    workflow: Uuid,
) -> ApiResult<()> {
    if let Some(parent) = parent {
        sqlx::query("update conversations set parent_id=$2,side_kind='side' where id=$1")
            .bind(conversation)
            .bind(parent)
            .execute(&state.pool)
            .await?;
        let linked = crate::store::find_owned(&state.pool, conversation, owner).await?;
        state.publish(
            Some(owner),
            mycelium_proto::stream::StreamMessage::Conversation(linked.to_proto()?),
        );

        let connect = sqlx::query_scalar::<_, bool>("select exists (select 1 from start_proposals where workflow_id = $1 and connect)").bind(workflow).fetch_one(&state.pool).await?;

        if connect {
            let connected = crate::conversation_connections::connect(state, owner, parent, conversation).await;

            if let Err(error) = connected {
                tracing::warn!(%workflow, error = error.message(), "connecting the step conversation failed");
            }
        }
    }
    Ok(())
}

async fn claim(
    state: &AppState,
    workflow: Uuid,
    round: i32,
    key: &str,
    operation: &str,
) -> ApiResult<Option<Uuid>> {
    let inserted = sqlx::query("insert into workflow_dispatches(workflow_id,round,step_key,operation) values($1,$2,$3,$4) on conflict do nothing").bind(workflow).bind(round).bind(key).bind(operation).execute(&state.pool).await?;
    if inserted.rows_affected() == 1 {
        return Ok(None);
    }
    let result: Option<Uuid> = sqlx::query_scalar("select result from workflow_dispatches where workflow_id=$1 and round=$2 and step_key=$3 and operation=$4").bind(workflow).bind(round).bind(key).bind(operation).fetch_one(&state.pool).await?;
    result.map(Some).ok_or_else(|| ApiError::conflict("workflow_dispatch_interrupted", "A workflow dispatch was interrupted. It has stopped to avoid running the same work twice. Inspect its conversations before starting another run."))
}

async fn dispatched(
    state: &AppState,
    workflow: Uuid,
    round: i32,
    key: &str,
    operation: &str,
    result: Uuid,
) -> ApiResult<()> {
    sqlx::query("update workflow_dispatches set result=$5 where workflow_id=$1 and round=$2 and step_key=$3 and operation=$4").bind(workflow).bind(round).bind(key).bind(operation).bind(result).execute(&state.pool).await?;
    Ok(())
}

pub async fn run_regularly(state: AppState) {
    boot::run_while_ready(&state, "advance the running workflows", std::time::Duration::from_secs(1), || tick(&state)).await;
}

async fn tick(state: &AppState) -> ApiResult<()> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "select id from workflows where status='running' and kind='mycelium' order by created_at limit 100",
    )
    .fetch_all(&state.pool)
    .await?;
    for id in ids {
        if let Err(error) = advance(state, id).await {
            tracing::warn!(%id, error = error.message(), "workflow step failed");
            let owner: Uuid = sqlx::query_scalar("select owner from workflows where id=$1")
                .bind(id)
                .fetch_one(&state.pool)
                .await?;
            stop_children(state, owner, id, None).await?;
            let reason = WorkflowEndReason { reason: "failed".into(), step: None, error: Some(error.message().to_string()), by: None, round: None };
            sqlx::query(
                "update workflows set status='failed',output=$2,end_reason=$3,ended_at=now(),updated_at=now() where id=$1",
            )
            .bind(id)
            .bind(error.message())
            .bind(json!(reason))
            .execute(&state.pool)
            .await?;
            publish(state, owner, id).await?;

            if let Err(error) = notify_parent(state, owner, id).await {
                tracing::warn!(%id, error = error.message(), "workflow result notification failed");
            }
        }
    }

    Ok(())
}

async fn advance(state: &AppState, id: Uuid) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from workflows where id=$1 and status='running' for no key update skip locked"))).bind(id).fetch_optional(&mut *tx).await?;
    let Some(mut row) = row else {
        return Ok(());
    };
    let previous_status = row.status.clone();
    let previous_steps = row.steps.clone();
    let previous_round = row.round;
    let previous_output = row.output.clone();
    let feedback: String = sqlx::query_scalar::<_, Option<String>>("select reason from workflow_rounds where workflow_id=$1 and round=$2 and decided_by->>'kind' in ('person','assistant')").bind(id).bind(row.round).fetch_optional(&mut *tx).await?.flatten().unwrap_or_default();
    let mut end_reason: Option<WorkflowEndReason> = None;
    let plan: WorkflowPlan = serde_json::from_value(row.plan.clone())
        .map_err(|_| ApiError::not_found("That workflow plan"))?;
    let mut steps: Vec<WorkflowRunStep> = serde_json::from_value(row.steps.clone())
        .map_err(|_| ApiError::not_found("Those workflow steps"))?;
    for index in 0..steps.len() {
        if row.status != "running" {
            break;
        }
        let spec = &plan.steps[index];
        if steps[index].status == "starting" {
            let Some(conversation) = steps[index].conversation_id else {
                return Err(ApiError::internal(format!("workflow {id} step {} is starting without a conversation", spec.key)));
            };
            let (ready, status): (bool, String) = sqlx::query_as(
                "select acp_session_id is not null,status from conversations where id=$1",
            )
            .bind(conversation)
            .fetch_one(&state.pool)
            .await?;
            if status == "failed" {
                settle(&mut steps[index], "failed");
                steps[index].error = Some(step_error(state, conversation).await?);
                row.status = "failed".into();
                break;
            }
            if !ready {
                continue;
            }
            prompts::apply_model(state, row.owner, conversation, spec.model.as_deref(), spec.difficulty.as_deref()).await?;

            let text = render(&spec.prompt, &steps, &row.output, &feedback);
            if claim(state, id, row.round, &spec.key, "prompt")
                .await?
                .is_none()
            {
                prompts::send(
                    state,
                    row.owner,
                    conversation,
                    PromptRequest {
                        text,
                        ..Default::default()
                    },
                )
                .await?;
                dispatched(state, id, row.round, &spec.key, "prompt", conversation).await?;
            }
            begin(&mut steps[index], "running");
        } else if steps[index].status == "running" {
            if let Some(child) = steps[index].workflow_id {
                let result: (String, String) =
                    sqlx::query_as("select status,output from workflows where id=$1")
                        .bind(child)
                        .fetch_one(&state.pool)
                        .await?;
                if result.0 == "done" {
                    settle(&mut steps[index], "done");
                    steps[index].summary = summary_of(&result.1);
                    steps[index].output = result.1;
                } else if result.0 == "failed" || result.0 == "stopped" {
                    settle(&mut steps[index], "failed");
                    steps[index].error = Some(format!("The nested workflow {}.", result.0));
                    row.status = "failed".into();
                }
            } else if let Some(conversation) = steps[index].conversation_id {
                let status: String =
                    sqlx::query_scalar("select status from conversations where id=$1")
                        .bind(conversation)
                        .fetch_one(&state.pool)
                        .await?;
                if status == "failed" {
                    settle(&mut steps[index], "failed");
                    steps[index].error = Some(step_error(state, conversation).await?);
                    row.status = "failed".into();
                }
                if status == "review" || status == "done" {
                    steps[index].output = sqlx::query_scalar::<_, Option<String>>("select string_agg(payload->'update'->'content'->>'text','' order by seq) from events where conversation_id=$1 and kind='acp_update' and payload->'update'->>'sessionUpdate'='agent_message_chunk'").bind(conversation).fetch_one(&state.pool).await?.unwrap_or_default();
                    settle(&mut steps[index], "done");
                    steps[index].summary = summary_of(&steps[index].output);
                    steps[index].tokens = conversation_tokens(state, conversation).await?;
                }
            }
        }

        if steps[index].status != "waiting"
            || !spec.depends_on.iter().all(|key| {
                steps
                    .iter()
                    .any(|step| &step.key == key && finished(step))
            })
        {
            continue;
        }
        if spec.gate {
            begin(&mut steps[index], "gate");
            continue;
        }
        if let Some(child) = &spec.workflow {
            let mut child = *child.clone();
            for child_step in &mut child.steps {
                child_step.prompt = render(&child_step.prompt, &steps, &row.output, &feedback);
            }
            let child = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(child) => child,
                None => {
                    let child =
                        create_owned(state, row.owner, Some(id), row.conversation_id, child, WorkflowActor { kind: "assistant".into(), name: row.title.clone() }, None)
                            .await?;
                    dispatched(state, id, row.round, &spec.key, "start", child).await?;
                    child
                }
            };
            steps[index].workflow_id = Some(child);
            begin(&mut steps[index], "running");
        } else if let Some(provider) = spec.provider_id {
            let text = render(&spec.prompt, &steps, &row.output, &feedback);
            let conversation = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(conversation) => conversation,
                None => {
                    let conversation = chats::start(
                        state,
                        row.owner,
                        CreateChatRequest {
                            provider_id: Some(provider),
                            agent: None,
                            account: None,
                            node_id: None,
                            model: spec.model.clone(),
                            prompt: Some(text),
                            parent_id: None,
                        },
                        None,
                        None,
                    )
                    .await?;
                    dispatched(state, id, row.round, &spec.key, "start", conversation.id).await?;
                    conversation.id
                }
            };
            link_native(state, row.owner, row.conversation_id, conversation, id).await?;
            steps[index].conversation_id = Some(conversation);
            begin(&mut steps[index], "running");
        } else {
            let (Some(node_id), Some(agent)) = (spec.node_id, spec.agent) else {
                return Err(ApiError::internal(format!("workflow {id} step {} has no provider, node or agent", spec.key)));
            };
            if !state.hub.is_online(node_id) {
                continue;
            }
            let conversation = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(conversation) => conversation,
                None => {
                    let conversation = conversations::start(
                        state,
                        row.owner,
                        CreateConversationRequest {
                            node_id,
                            agent,
                            account: spec.account.clone(),
                            cwd: spec.cwd.clone(),
                            project_id: spec.project_id,
                            branch: spec.branch.clone(),
                            title: Some(spec.title.clone()),
                            prompt: None,
                            parent_id: None,
                            side_kind: None,
                        model: None,
                        routing: None,
                        },
                    )
                    .await?;
                    dispatched(state, id, row.round, &spec.key, "start", conversation.id).await?;
                    conversation.id
                }
            };
            link_native(state, row.owner, row.conversation_id, conversation, id).await?;
            steps[index].conversation_id = Some(conversation);
            begin(&mut steps[index], "starting");
        }
    }

    if steps.iter().all(finished) {
        row.output = joined_output(&steps);
        let rule_met = plan.stop_when.as_ref().is_some_and(|stop| {
            !stop.is_empty() && steps.iter().all(|step| step.output.contains(stop))
        });
        keep_round(&mut tx, id, row.round, &steps).await?;

        if row.round < plan.max_rounds as i32 && !rule_met {
            row.round += 1;
            steps = waiting(&plan);
            let reason = match plan.stop_when.as_deref().filter(|stop| !stop.is_empty()) {
                Some(stop) => format!("Not every result contained {stop}."),
                None => format!("Every round up to {} runs.", plan.max_rounds),
            };
            let by = WorkflowActor { kind: "rule".into(), name: "Stop rule".into() };
            begin_round(&mut tx, id, row.round, Some(&reason), Some(&by)).await?;
        } else {
            row.status = "done".into();
            let reason = if rule_met { "stop_rule" } else if plan.max_rounds > 1 { "round_limit" } else { "finished" };
            end_reason = Some(WorkflowEndReason { reason: reason.into(), step: None, error: None, by: None, round: Some(row.round) });
        }
    }

    if row.status == "failed" {
        let failed = steps.iter().find(|step| step.status == "failed");
        end_reason = Some(WorkflowEndReason {
            reason: "failed".into(),
            step: failed.map(|step| step.key.clone()),
            error: failed.and_then(|step| step.error.clone()),
            by: None,
            round: Some(row.round),
        });
    }

    if previous_status == row.status
        && previous_steps == json!(steps)
        && previous_round == row.round
        && previous_output == row.output
    {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query(
        "update workflows set status=$2,steps=$3,round=$4,output=$5,end_reason=coalesce($6,end_reason),ended_at=case when $7 then now() else ended_at end,updated_at=now() where id=$1",
    )
    .bind(id)
    .bind(&row.status)
    .bind(json!(steps))
    .bind(row.round)
    .bind(&row.output)
    .bind(end_reason.as_ref().map(|reason| json!(reason)))
    .bind(matches!(row.status.as_str(), "done" | "failed"))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if row.status == "failed" {
        stop_children(state, row.owner, id, None).await?;
        let message = end_reason.as_ref().and_then(|reason| reason.error.clone()).unwrap_or_else(|| "An agent step failed. Open its conversation to resolve the error.".into());
        sqlx::query("update workflows set status='failed',output=$2,end_reason=$3,ended_at=now(),updated_at=now() where id=$1").bind(id).bind(message).bind(end_reason.as_ref().map(|reason| json!(reason))).execute(&state.pool).await?;
    }
    publish(state, row.owner, id).await?;

    if let Err(error) = notify_parent(state, row.owner, id).await {
        tracing::warn!(%id, error = error.message(), "workflow result notification failed");
    }

    Ok(())
}

async fn step_error(state: &AppState, conversation: Uuid) -> ApiResult<String> {
    let message: Option<String> = sqlx::query_scalar("select payload->>'message' from events where conversation_id=$1 and kind='error' order by seq desc nulls last limit 1").bind(conversation).fetch_optional(&state.pool).await?.flatten();

    Ok(message.filter(|text| !text.trim().is_empty()).unwrap_or_else(|| "The agent step failed. Open its conversation to resolve the error.".into()))
}

fn summary_of(output: &str) -> String {
    output.trim().chars().take(400).collect()
}

async fn conversation_tokens(state: &AppState, conversation: Uuid) -> ApiResult<Option<i64>> {
    let tokens: Option<i64> = sqlx::query_scalar("select sum(input_tokens + output_tokens)::bigint from usage_turns where conversation_id=$1").bind(conversation).fetch_one(&state.pool).await?;

    Ok(tokens.filter(|count| *count > 0))
}

fn line_number(line: &Value, field: &str) -> Option<i64> {
    line.get(field).and_then(Value::as_i64)
}

pub async fn index_agent_run(state: &AppState, conversation: Uuid, run_id: &str, line: &Value, usage_limited: bool) -> ApiResult<()> {
    let Some((owner, title, agent)) = sqlx::query_as::<_, (Uuid, String, String)>("select owner,title,agent from conversations where id=$1").bind(conversation).fetch_optional(&state.pool).await? else {
        return Ok(());
    };
    let plan = json!({"title": format!("Workflow in {title}"), "steps": [], "max_rounds": 1});
    let started_by = json!(WorkflowActor { kind: "assistant".into(), name: if agent == "codex" { "Codex".into() } else { "Claude Code".into() } });
    let mut tx = state.pool.begin().await?;
    sqlx::query("insert into workflows (id,owner,conversation_id,title,plan,steps,kind,run_id,started_by,project_id) values ($1,$2,$3,$4,$5,'[]','agent_run',$6,$7,(select project_id from conversations where id=$3)) on conflict do nothing")
        .bind(Uuid::new_v4()).bind(owner).bind(conversation).bind(format!("Workflow in {title}")).bind(plan).bind(run_id).bind(started_by).execute(&mut *tx).await?;
    let row = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from workflows where conversation_id=$1 and run_id=$2 for update"))).bind(conversation).bind(run_id).fetch_one(&mut *tx).await?;
    let mut steps: Vec<WorkflowRunStep> = serde_json::from_value(row.steps.clone()).unwrap_or_default();
    let mut phases: Vec<String> = serde_json::from_value(row.phases.clone()).unwrap_or_default();
    let mut end_reason: Option<WorkflowEndReason> = row.end_reason.clone().and_then(|value| serde_json::from_value(value).ok());
    let now = Utc::now();
    let key = line["key"].as_str().map(str::to_string);
    let kind = line["type"].as_str().unwrap_or_default();

    match kind {
        "plan" => {
            phases = line["phases"].as_array().map(|list| list.iter().filter_map(|phase| phase.as_str().map(str::to_string)).collect()).unwrap_or(phases);
        }
        "started" => {
            let Some(key) = key.clone() else {
                return Ok(());
            };
            end_reason = None;

            match steps.iter_mut().find(|step| step.key == key) {
                Some(step) => {
                    step.status = "running".into();
                    step.ended_at = None;
                    step.error = None;
                }
                None => steps.push(WorkflowRunStep {
                    key,
                    status: "running".into(),
                    label: Some(line["label"].as_str().unwrap_or("Agent").to_string()),
                    phase: Some(line["phase"].as_str().unwrap_or("Run").to_string()),
                    agent_id: line["agentId"].as_str().map(str::to_string),
                    started_at: Some(now),
                    ..Default::default()
                }),
            }
        }
        "result" | "failed" => {
            if let Some(step) = steps.iter_mut().find(|step| Some(&step.key) == key.as_ref()) {
                if usage_limited {
                    step.status = "paused".into();
                } else if kind == "result" {
                    settle(step, "done");
                    step.summary = line["summary"].as_str().or_else(|| line["result"].as_str()).map(|text| summary_of(text).lines().next().unwrap_or_default().to_string()).unwrap_or_default();
                } else {
                    settle(step, "failed");
                    step.error = line["reason"].as_str().map(str::to_string);
                }
            }
        }
        "ended" => {
            let state_name = line["state"].as_str().unwrap_or("completed");
            let reason = match state_name {
                "failed" => "failed",
                "stopped" => "stopped",
                _ => "finished",
            };
            end_reason = Some(WorkflowEndReason { reason: reason.into(), step: None, error: line["reason"].as_str().map(str::to_string), by: None, round: None });

            for step in steps.iter_mut().filter(|step| step.status == "running" || step.status == "paused") {
                settle(step, if reason == "finished" { "done" } else { "stopped" });
            }
        }
        _ => {}
    }

    if let Some(step) = steps.iter_mut().find(|step| Some(&step.key) == key.as_ref()) {
        step.tokens = line_number(line, "tokens").or(step.tokens);
        step.turns = line_number(line, "turns").map(|turns| turns as i32).or(step.turns);
        step.elapsed_ms = line_number(line, "elapsed_ms").or(step.elapsed_ms);
    }

    let status: String = match end_reason.as_ref().map(|reason| reason.reason.as_str()) {
        Some("failed") => "failed".into(),
        Some("stopped") => "stopped".into(),
        Some(_) => "done".into(),
        None if !steps.is_empty() && steps.iter().all(|step| step.status == "done" || step.status == "failed") => "done".into(),
        None if steps.iter().any(|step| step.status == "paused") => "paused".into(),
        None => "running".into(),
    };
    let ended = matches!(status.as_str(), "done" | "failed" | "stopped");
    let ended_at = if ended { Some(row.ended_at.unwrap_or(now)) } else { None };
    sqlx::query("update workflows set status=$2,steps=$3,phases=$4,end_reason=$5,ended_at=$6,updated_at=now() where id=$1")
        .bind(row.id).bind(&status).bind(json!(steps)).bind(json!(phases)).bind(end_reason.as_ref().map(|reason| json!(reason))).bind(ended_at).execute(&mut *tx).await?;
    tx.commit().await?;
    publish(state, owner, row.id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(steps: Value) -> WorkflowPlan {
        serde_json::from_value(json!({"title":"Review","steps":steps})).unwrap()
    }
    #[test]
    fn dependencies_allow_fanout_but_reject_cycles_and_unknown_steps() {
        assert!(validate(&plan(json!([{"key":"a","title":"A","gate":true},{"key":"b","title":"B","gate":true,"depends_on":["a"]},{"key":"c","title":"C","gate":true,"depends_on":["a"]}])),0).is_ok());
        assert!(validate(&plan(json!([{"key":"a","title":"A","gate":true,"depends_on":["b"]},{"key":"b","title":"B","gate":true,"depends_on":["a"]}])),0).is_err());
        assert!(
            validate(
                &plan(json!([{"key":"a","title":"A","gate":true,"depends_on":["missing"]}])),
                0
            )
            .is_err()
        );
    }
    #[test]
    fn outputs_and_previous_round_are_passed_to_prompts() {
        let step = WorkflowRunStep {
            key: "build".into(),
            status: "done".into(),
            conversation_id: None,
            output: "result".into(),
            ..Default::default()
        };
        assert_eq!(
            render(
                "Review {{build.output}} after {{previous}}",
                &[step],
                "first draft",
                ""
            ),
            "Review result after first draft"
        );
    }
    #[test]
    fn round_limits_and_duplicate_keys_are_rejected() {
        let mut p = plan(json!([{"key":"a","title":"A","gate":true}]));
        p.max_rounds = 0;
        assert!(validate(&p, 0).is_err());
        p.max_rounds = 21;
        assert!(validate(&p, 0).is_err());
        p.max_rounds = 1;
        p.steps.push(p.steps[0].clone());
        assert!(validate(&p, 0).is_err());
    }
}
