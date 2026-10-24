use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{boot, chats, conversations, prompts};
use axum::{
    Json,
    extract::{Path, State},
};
use mycelium_proto::api::{
    ConfigRequest, CreateChatRequest, CreateConversationRequest, PromptRequest,
};
use mycelium_proto::conversation::AgentId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub steps: Vec<Step>,
    #[serde(default = "one")]
    pub max_rounds: u32,
    #[serde(default)]
    pub stop_when: Option<String>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub gate: bool,
    #[serde(default)]
    pub workflow: Option<Box<Plan>>,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub agent: Option<AgentId>,
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunStep {
    pub key: String,
    pub status: String,
    pub conversation_id: Option<Uuid>,
    pub workflow_id: Option<Uuid>,
    pub output: String,
}

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
}

impl Row {
    fn json(&self) -> Value {
        json!({"id":self.id,"parent_id":self.parent_id,"conversation_id":self.conversation_id,"title":self.title,"status":self.status,"plan":self.plan,"steps":self.steps,"round":self.round,"output":self.output})
    }
}

fn waiting(plan: &Plan) -> Vec<RunStep> {
    plan.steps
        .iter()
        .map(|step| RunStep {
            key: step.key.clone(),
            status: "waiting".into(),
            conversation_id: None,
            workflow_id: None,
            output: String::new(),
        })
        .collect()
}

fn validate(plan: &Plan, depth: usize) -> ApiResult<()> {
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
    Json(plan): Json<Plan>,
) -> ApiResult<Json<Value>> {
    user.require_full_access()?;
    let id = create_owned(&state, user.id, None, None, plan).await?;
    get(State(state), user, Path(id)).await
}

pub async fn create_owned(
    state: &AppState,
    owner: Uuid,
    parent: Option<Uuid>,
    conversation: Option<Uuid>,
    plan: Plan,
) -> ApiResult<Uuid> {
    validate(&plan, 0)?;
    let id = Uuid::new_v4();
    sqlx::query("insert into workflows (id,owner,parent_id,conversation_id,title,plan,steps) values ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(owner).bind(parent).bind(conversation).bind(&plan.title).bind(json!(plan)).bind(json!(waiting(&plan))).execute(&state.pool).await?;
    publish(state, owner, id).await?;
    Ok(id)
}

async fn publish(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let row = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where id=$1 and owner=$2").bind(id).bind(owner).fetch_one(&state.pool).await?;
    state.publish(
        Some(owner),
        mycelium_proto::stream::StreamMessage::Workflow {
            workflow: row.json(),
        },
    );
    Ok(())
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let rows = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where owner=$1 order by created_at desc").bind(user.id).fetch_all(&state.pool).await?;
    Ok(Json(json!(rows.iter().map(Row::json).collect::<Vec<_>>())))
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let row = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where id=$1 and owner=$2").bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("That workflow"))?;
    Ok(Json(row.json()))
}

#[derive(Deserialize)]
pub struct Control {
    pub action: String,
    pub step: Option<String>,
    pub output: Option<String>,
}

pub async fn control(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<Control>,
) -> ApiResult<Json<Value>> {
    user.require_full_access()?;
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where id=$1 and owner=$2 for update").bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("That workflow"))?;
    let mut steps: Vec<RunStep> = serde_json::from_value(row.steps.clone())
        .map_err(|_| ApiError::not_found("That workflow"))?;
    let status = match request.action.as_str() {
        "pause" if row.status == "running" => "paused",
        "resume" if row.status == "paused" => "running",
        "stop" => "stopped",
        "approve" if row.status == "running" || row.status == "paused" => {
            let step = steps
                .iter_mut()
                .find(|step| Some(&step.key) == request.step.as_ref() && step.status == "gate")
                .ok_or_else(|| {
                    ApiError::bad_request(
                        "gate_missing",
                        "Choose a step waiting for your decision.",
                    )
                })?;
            step.status = "done".into();
            step.output = request.output.unwrap_or_default();
            row.status.as_str()
        }
        _ => {
            return Err(ApiError::bad_request(
                "workflow_action_invalid",
                "Choose pause, resume, stop, or approve a waiting gate.",
            ));
        }
    };
    sqlx::query("update workflows set status=$2,steps=$3,updated_at=now() where id=$1")
        .bind(id)
        .bind(status)
        .bind(json!(steps))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if status == "stopped" {
        stop_children(&state, user.id, id).await?;
    }

    publish(&state, user.id, id).await?;

    if let Err(error) = notify_parent(&state, user.id, id).await {
        tracing::warn!(%id, error = error.message(), "workflow result notification failed");
    }

    get(State(state), user, Path(id)).await
}

async fn notify_parent(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let row = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where id=$1 and owner=$2").bind(id).bind(owner).fetch_one(&state.pool).await?;
    let Some(parent) = row.conversation_id.filter(|_| row.parent_id.is_none()) else {
        return Ok(());
    };

    if !matches!(row.status.as_str(), "done" | "failed" | "stopped") {
        return Ok(());
    }

    let conversation = crate::store::find_owned(&state.pool, parent, owner).await?;

    if conversation.archived_at.is_some() {
        return Ok(());
    }

    let claimed = sqlx::query("insert into workflow_dispatches (workflow_id, round, step_key, operation) values ($1,$2,'result','notify') on conflict do nothing").bind(id).bind(row.round).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    let text = format!(
        "Workflow {} ({id}) {}.\n\n{}",
        row.title, row.status, row.output
    );

    if conversation.agent == crate::chats::CHAT_AGENT {
        chats::send_or_queue(state, owner, parent, &text).await?;
    } else {
        let user = AuthUser {
            id: owner,
            email: String::new(),
            name: String::new(),
            session: None,
            scopes: None,
            expires_at: None,
        };
        prompts::prompt(
            State(state.clone()),
            user,
            Path(parent),
            Json(PromptRequest {
                text,
                ..Default::default()
            }),
        )
        .await?;
    }

    dispatched(state, id, row.round, "result", "notify", parent).await
}

async fn stop_children(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<()> {
    let rows: Vec<(Uuid, Value)> = sqlx::query_as("with recursive children as (select id from workflows where id=$1 and owner=$2 union all select w.id from workflows w join children c on w.parent_id=c.id) update workflows set status='stopped',updated_at=now() where id in (select id from children) returning id,steps").bind(id).bind(owner).fetch_all(&state.pool).await?;
    for (workflow, value) in rows {
        let steps: Vec<RunStep> = serde_json::from_value(value).unwrap_or_default();
        for step in steps
            .iter()
            .filter(|step| step.status == "running" || step.status == "starting")
        {
            if let Some(conversation) = step.conversation_id {
                let user = AuthUser {
                    id: owner,
                    email: String::new(),
                    name: String::new(),
                    session: None,
                    scopes: None,
                    expires_at: None,
                };
                let _ = prompts::cancel(State(state.clone()), user, Path(conversation)).await;
            }
        }
        publish(state, owner, workflow).await?;
    }

    Ok(())
}

fn render(prompt: &str, steps: &[RunStep], previous: &str) -> String {
    let mut rendered = prompt.replace("{{previous}}", previous);
    for step in steps {
        rendered = rendered.replace(&format!("{{{{{}.output}}}}", step.key), &step.output);
    }

    rendered
}

async fn link_native(
    state: &AppState,
    owner: Uuid,
    parent: Option<Uuid>,
    conversation: Uuid,
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
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if !boot::is_ready(&state) {
            continue;
        }
        if let Err(error) = tick(&state).await {
            tracing::warn!(error = error.message(), "workflow scheduler failed");
        }
    }
}

async fn tick(state: &AppState) -> ApiResult<()> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "select id from workflows where status='running' order by created_at limit 100",
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
            stop_children(state, owner, id).await?;
            sqlx::query(
                "update workflows set status='failed',output=$2,updated_at=now() where id=$1",
            )
            .bind(id)
            .bind(error.message())
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
    let row = sqlx::query_as::<_, Row>("select id,owner,parent_id,conversation_id,title,status,plan,steps,round,output from workflows where id=$1 and status='running' for no key update skip locked").bind(id).fetch_optional(&mut *tx).await?;
    let Some(mut row) = row else {
        return Ok(());
    };
    let previous = row.json();
    let plan: Plan = serde_json::from_value(row.plan.clone())
        .map_err(|_| ApiError::not_found("That workflow plan"))?;
    let mut steps: Vec<RunStep> = serde_json::from_value(row.steps.clone())
        .map_err(|_| ApiError::not_found("Those workflow steps"))?;
    let user = AuthUser {
        id: row.owner,
        email: String::new(),
        name: String::new(),
        session: None,
        scopes: None,
        expires_at: None,
    };
    for index in 0..steps.len() {
        if row.status != "running" {
            break;
        }
        let spec = &plan.steps[index];
        if steps[index].status == "starting" {
            let conversation = steps[index].conversation_id.unwrap();
            let (ready, status): (bool, String) = sqlx::query_as(
                "select acp_session_id is not null,status from conversations where id=$1",
            )
            .bind(conversation)
            .fetch_one(&state.pool)
            .await?;
            if status == "failed" {
                steps[index].status = "failed".into();
                row.status = "failed".into();
                break;
            }
            if !ready {
                continue;
            }
            let options: Option<Value> =
                sqlx::query_scalar("select options from conversations where id=$1")
                    .bind(conversation)
                    .fetch_one(&state.pool)
                    .await?;
            let effort = options
                .as_ref()
                .and_then(|options| options.get("config_options"))
                .and_then(Value::as_array)
                .and_then(|options| {
                    options.iter().find(|option| {
                        option.get("category").and_then(Value::as_str) == Some("thought_level")
                            || option
                                .get("id")
                                .and_then(Value::as_str)
                                .is_some_and(|id| id.contains("effort") || id.contains("reasoning"))
                    })
                })
                .and_then(|option| option.get("id"))
                .and_then(Value::as_str);
            if let Some(model) = &spec.model {
                let available = options
                    .as_ref()
                    .and_then(|options| options.get("models"))
                    .and_then(|models| models.get("availableModels"))
                    .and_then(Value::as_array);
                if !available.is_some_and(|models| {
                    models
                        .iter()
                        .any(|entry| entry.get("modelId").and_then(Value::as_str) == Some(model))
                }) {
                    return Err(ApiError::bad_request(
                        "workflow_model_unavailable",
                        "The requested model is not offered by this agent. Choose one of its available models.",
                    ));
                }
            }
            if let Some(difficulty) = &spec.difficulty {
                let option = options
                    .as_ref()
                    .and_then(|options| options.get("config_options"))
                    .and_then(Value::as_array)
                    .and_then(|options| {
                        options
                            .iter()
                            .find(|option| option.get("id").and_then(Value::as_str) == effort)
                    });
                let values = option
                    .and_then(|option| option.get("options"))
                    .and_then(Value::as_array);
                let offered = values.is_some_and(|values| {
                    values.iter().any(|value| {
                        value.get("value").and_then(Value::as_str) == Some(difficulty)
                            || value
                                .get("options")
                                .and_then(Value::as_array)
                                .is_some_and(|group| {
                                    group.iter().any(|value| {
                                        value.get("value").and_then(Value::as_str)
                                            == Some(difficulty)
                                    })
                                })
                    })
                });
                if effort.is_none() || !offered {
                    return Err(ApiError::bad_request(
                        "workflow_difficulty_unavailable",
                        "The requested difficulty is not offered by this agent. Choose an available difficulty or use the agent default.",
                    ));
                }
            }
            let config = spec
                .difficulty
                .as_ref()
                .zip(effort)
                .map(|(difficulty, key)| {
                    std::collections::BTreeMap::from([(key.to_string(), json!(difficulty))])
                });
            if spec.model.is_some() || config.is_some() {
                prompts::config(
                    State(state.clone()),
                    user.clone(),
                    Path(conversation),
                    Json(ConfigRequest {
                        model: spec.model.clone(),
                        config,
                        mode: None,
                    }),
                )
                .await?;
            }

            let text = render(&spec.prompt, &steps, &row.output);
            if claim(state, id, row.round, &spec.key, "prompt")
                .await?
                .is_none()
            {
                prompts::prompt(
                    State(state.clone()),
                    user.clone(),
                    Path(conversation),
                    Json(PromptRequest {
                        text,
                        ..Default::default()
                    }),
                )
                .await?;
                dispatched(state, id, row.round, &spec.key, "prompt", conversation).await?;
            }
            steps[index].status = "running".into();
        } else if steps[index].status == "running" {
            if let Some(child) = steps[index].workflow_id {
                let result: (String, String) =
                    sqlx::query_as("select status,output from workflows where id=$1")
                        .bind(child)
                        .fetch_one(&state.pool)
                        .await?;
                if result.0 == "done" {
                    steps[index].status = "done".into();
                    steps[index].output = result.1;
                } else if result.0 == "failed" || result.0 == "stopped" {
                    steps[index].status = "failed".into();
                    row.status = "failed".into();
                }
            } else if let Some(conversation) = steps[index].conversation_id {
                let status: String =
                    sqlx::query_scalar("select status from conversations where id=$1")
                        .bind(conversation)
                        .fetch_one(&state.pool)
                        .await?;
                if status == "failed" {
                    steps[index].status = "failed".into();
                    row.status = "failed".into();
                }
                if status == "review" || status == "done" {
                    steps[index].output = sqlx::query_scalar::<_, Option<String>>("select string_agg(payload->'update'->'content'->>'text','' order by seq) from events where conversation_id=$1 and kind='acp_update' and payload->'update'->>'sessionUpdate'='agent_message_chunk'").bind(conversation).fetch_one(&state.pool).await?.unwrap_or_default();
                    steps[index].status = "done".into();
                }
            }
        }

        if steps[index].status != "waiting"
            || !spec.depends_on.iter().all(|key| {
                steps
                    .iter()
                    .any(|step| &step.key == key && step.status == "done")
            })
        {
            continue;
        }
        if spec.gate {
            steps[index].status = "gate".into();
            continue;
        }
        if let Some(child) = &spec.workflow {
            let mut child = *child.clone();
            for child_step in &mut child.steps {
                child_step.prompt = render(&child_step.prompt, &steps, &row.output);
            }
            let child = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(child) => child,
                None => {
                    let child =
                        create_owned(state, row.owner, Some(id), row.conversation_id, child)
                            .await?;
                    dispatched(state, id, row.round, &spec.key, "start", child).await?;
                    child
                }
            };
            steps[index].workflow_id = Some(child);
            steps[index].status = "running".into();
        } else if let Some(provider) = spec.provider_id {
            let text = render(&spec.prompt, &steps, &row.output);
            let conversation = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(conversation) => conversation,
                None => {
                    let conversation = chats::start(
                        state,
                        &user,
                        CreateChatRequest {
                            provider_id: provider,
                            model: spec.model.clone(),
                            prompt: Some(text),
                            parent_id: None,
                        },
                        None,
                    )
                    .await?;
                    dispatched(state, id, row.round, &spec.key, "start", conversation.id).await?;
                    conversation.id
                }
            };
            link_native(state, row.owner, row.conversation_id, conversation).await?;
            steps[index].conversation_id = Some(conversation);
            steps[index].status = "running".into();
        } else {
            if !state.hub.is_online(spec.node_id.unwrap()) {
                continue;
            }
            let conversation = match claim(state, id, row.round, &spec.key, "start").await? {
                Some(conversation) => conversation,
                None => {
                    let conversation = conversations::start(
                        state,
                        &user,
                        CreateConversationRequest {
                            node_id: spec.node_id.unwrap(),
                            agent: spec.agent.unwrap(),
                            account: spec.account.clone(),
                            cwd: spec.cwd.clone(),
                            project_id: spec.project_id,
                            branch: spec.branch.clone(),
                            title: Some(spec.title.clone()),
                            prompt: None,
                            parent_id: None,
                            side_kind: None,
                        },
                    )
                    .await?;
                    dispatched(state, id, row.round, &spec.key, "start", conversation.id).await?;
                    conversation.id
                }
            };
            link_native(state, row.owner, row.conversation_id, conversation).await?;
            steps[index].conversation_id = Some(conversation);
            steps[index].status = "starting".into();
        }
    }

    if steps.iter().all(|step| step.status == "done") {
        row.output = steps
            .iter()
            .map(|step| format!("{}:\n{}", step.key, step.output))
            .collect::<Vec<_>>()
            .join("\n\n");
        if row.round < plan.max_rounds as i32
            && !plan.stop_when.as_ref().is_some_and(|stop| {
                !stop.is_empty() && steps.iter().all(|step| step.output.contains(stop))
            })
        {
            row.round += 1;
            steps = waiting(&plan);
        } else {
            row.status = "done".into();
        }
    }

    if previous["status"] == json!(row.status)
        && previous["steps"] == json!(steps)
        && previous["round"] == json!(row.round)
        && previous["output"] == json!(row.output)
    {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query(
        "update workflows set status=$2,steps=$3,round=$4,output=$5,updated_at=now() where id=$1",
    )
    .bind(id)
    .bind(&row.status)
    .bind(json!(steps))
    .bind(row.round)
    .bind(&row.output)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if row.status == "failed" {
        stop_children(state, row.owner, id).await?;
        sqlx::query("update workflows set status='failed',output='An agent step failed. Open its conversation to resolve the error.',updated_at=now() where id=$1").bind(id).execute(&state.pool).await?;
    }
    publish(state, row.owner, id).await?;

    if let Err(error) = notify_parent(state, row.owner, id).await {
        tracing::warn!(%id, error = error.message(), "workflow result notification failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(steps: Value) -> Plan {
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
        let step = RunStep {
            key: "build".into(),
            status: "done".into(),
            conversation_id: None,
            workflow_id: None,
            output: "result".into(),
        };
        assert_eq!(
            render(
                "Review {{build.output}} after {{previous}}",
                &[step],
                "first draft"
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
