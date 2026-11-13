use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::state::AppState;
use crate::store::CONVERSATION_GROUP_SQL;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{ApproveStartProposalRequest, HarnessModel, StartProposal, StartProposalNode, StartProposalState, StartProposalStep, WorkflowPlan};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const LISTED: i64 = 100;
const COLUMNS: &str = "id, conversation_id, kind, plan, choices, connect, state, workflow_id, target_group_id, created_at, decided_at";

#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: Uuid,
    conversation_id: Option<Uuid>,
    kind: String,
    plan: Value,
    choices: Value,
    connect: bool,
    state: String,
    workflow_id: Option<Uuid>,
    target_group_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn node_of(state: &AppState, names: &[(Uuid, String)], id: Uuid) -> StartProposalNode {
    let name = names.iter().find(|(node, _)| *node == id).map(|(_, name)| name.clone()).unwrap_or_default();

    StartProposalNode { id, name, online: state.hub.is_online(id) }
}

async fn to_proto(state: &AppState, row: ProposalRow) -> ApiResult<StartProposal> {
    let plan: WorkflowPlan = serde_json::from_value(row.plan.clone()).map_err(|_| ApiError::not_found("That proposal"))?;
    let mut node_ids: Vec<Uuid> = plan.steps.iter().filter_map(|step| step.node_id).collect();
    let mut project_ids: Vec<Uuid> = plan.steps.iter().filter_map(|step| step.project_id).collect();
    node_ids.extend(row.choices.as_object().into_iter().flat_map(|choices| choices.values()).flat_map(|choice| choice["node"].as_array().cloned().unwrap_or_default()).filter_map(|node| node["id"].as_str().and_then(|id| id.parse::<Uuid>().ok())));
    project_ids.dedup();
    let names = sqlx::query_as::<_, (Uuid, String)>("select id, name from nodes where id = any($1)").bind(&node_ids).fetch_all(&state.pool).await?;
    let projects = sqlx::query_as::<_, (Uuid, String)>("select id, name from projects where id = any($1)").bind(&project_ids).fetch_all(&state.pool).await?;

    let steps = plan
        .steps
        .iter()
        .map(|step| {
            let choice = &row.choices[&step.key];
            let node_choices = choice["node"].as_array().cloned().unwrap_or_default().iter().filter_map(|node| node["id"].as_str().and_then(|id| id.parse().ok())).map(|id| node_of(state, &names, id)).collect();
            let account_choices = choice["account"].as_array().cloned().unwrap_or_default().iter().filter_map(|account| account.as_str().map(String::from)).collect();
            let model_choices = choice["model"].as_array().cloned().unwrap_or_default().iter().filter_map(|model| Some(HarnessModel { id: model["id"].as_str()?.to_string(), name: model["name"].as_str()?.to_string() })).collect();
            let folder = step.worktree_path.clone().or_else(|| step.project_id.and_then(|id| projects.iter().find(|(project, _)| *project == id).map(|(_, name)| name.clone())).or_else(|| Some(step.cwd.clone()).filter(|cwd| !cwd.is_empty())));

            StartProposalStep {
                key: step.key.clone(),
                title: step.title.clone(),
                depends_on: step.depends_on.clone(),
                gate: step.gate,
                node: step.node_id.map(|id| node_of(state, &names, id)),
                folder,
                project: step.project_id.and_then(|id| projects.iter().find(|(project, _)| *project == id).map(|(_, name)| name.clone())),
                agent: step.agent.map(|agent| agent.as_str().to_string()),
                account: step.account.clone(),
                model: step.model.clone(),
                first_message: step.prompt.clone(),
                node_choices,
                account_choices,
                model_choices,
            }
        })
        .collect();

    let proposal_state = match row.state.as_str() {
        "started" => StartProposalState::Started,
        "declined" => StartProposalState::Declined,
        _ => StartProposalState::Proposed,
    };

    let source_group = match row.conversation_id {
        Some(conversation) => sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!("select name from groups where id = (select {CONVERSATION_GROUP_SQL} from conversations where id = $1)"))).bind(conversation).fetch_optional(&state.pool).await?,
        None => None,
    };
    let target_group = sqlx::query_scalar::<_, String>("select name from groups where id = $1").bind(row.target_group_id).fetch_optional(&state.pool).await?;

    Ok(StartProposal { id: row.id, conversation_id: row.conversation_id, kind: row.kind, title: plan.title, steps, state: proposal_state, connect: row.connect, workflow_id: row.workflow_id, source_group, target_group, created_at: row.created_at, decided_at: row.decided_at })
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProposalRow> {
    let row = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from start_proposals where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That proposal"))
}

async fn publish(state: &AppState, owner: Uuid, row: ProposalRow) -> ApiResult<StartProposal> {
    let proposal = to_proto(state, row).await?;
    state.publish(Some(owner), StreamMessage::StartProposal(proposal.clone()));

    Ok(proposal)
}

async fn signed_in_accounts(state: &AppState, node_id: Uuid, agent: &str) -> ApiResult<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>("select account from logins where node_id = $1 and agent = $2 and state = 'signed_in' and account is not null order by account").bind(node_id).bind(agent).fetch_all(&state.pool).await?)
}

pub async fn propose(state: &AppState, owner: Uuid, conversation_id: Uuid, kind: &str, plan: Value, target_group: Option<Uuid>) -> ApiResult<Result<StartProposal, String>> {
    let mut plan = plan;
    let mut choices = serde_json::Map::new();
    let steps = plan.get_mut("steps").and_then(Value::as_array_mut);

    for step in steps.into_iter().flatten().filter_map(Value::as_object_mut) {
        if step.get("gate") == Some(&json!(true)) || step.contains_key("workflow") || step.get("provider_id").is_some_and(|provider| !provider.is_null()) {
            continue;
        }

        let key = step.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
        let named = step.get("node").or_else(|| step.get("node_id")).and_then(Value::as_str).map(String::from);
        let candidates = sqlx::query_as::<_, (Uuid, String)>("select id, name from nodes where created_by = $1 and revoked_at is null and ($2::text is null or id::text = lower($2) or lower(name) = lower($2)) order by name")
            .bind(owner)
            .bind(&named)
            .fetch_all(&state.pool)
            .await?;

        if candidates.is_empty() {
            return Ok(Err(match named {
                Some(name) => format!("Nothing was proposed. None of the person's nodes is named {name}. Look them up with list_places."),
                None => "Nothing was proposed. The person has no nodes. Look them up with list_places.".to_string(),
            }));
        }

        let mut ordered = candidates.clone();
        ordered.sort_by_key(|(id, _)| !state.hub.is_online(*id));
        let chosen = ordered[0].0;
        step.insert("node_id".into(), json!(chosen));
        step.remove("node");

        if let Some(path) = step.get("worktree_path").and_then(Value::as_str) {
            let project_id = step.get("project_id").and_then(Value::as_str).and_then(|id| id.parse::<Uuid>().ok());
            let Some(project_id) = project_id else {
                return Ok(Err("Nothing was proposed. Give the project_id of the project the worktree belongs to.".to_string()));
            };
            let project = crate::projects::find_owned(state, project_id, owner).await?;

            if let Err(error) = crate::worktrees::find_existing(state, &project, chosen, path).await {
                return Ok(Err(format!("Nothing was proposed. {}", error.message())));
            }
        }

        let mut step_choices = serde_json::Map::new();

        if ordered.len() > 1 {
            step_choices.insert("node".into(), json!(ordered.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>()));
        } else if let Some(agent) = step.get("agent").and_then(Value::as_str).map(String::from) {
            let accounts = signed_in_accounts(state, chosen, &agent).await?;
            let requested = step.get("account").and_then(Value::as_str).map(String::from);

            match requested {
                Some(account) if !accounts.is_empty() && !accounts.contains(&account) => {
                    return Ok(Err(format!("Nothing was proposed. {account} is not signed in to {agent} on {}. Signed in: {}. Look them up with list_places.", ordered[0].1, accounts.join(", "))));
                }
                Some(_) => {}
                None if accounts.len() == 1 => {
                    step.insert("account".into(), json!(accounts[0]));
                }
                None if accounts.len() > 1 => {
                    step_choices.insert("account".into(), json!(accounts));
                }
                None => {}
            }

            let offered = crate::prompts::offered_models(state, owner, chosen, &agent).await?;
            let models: Vec<Value> = offered.iter().filter_map(|entry| Some(json!({"id": entry.get("modelId")?.as_str()?, "name": entry.get("name").and_then(Value::as_str).or_else(|| entry.get("modelId")?.as_str())?}))).collect();

            if models.len() > 1 {
                step_choices.insert("model".into(), json!(models));
            }

            if let Some(model) = step.get("model").and_then(Value::as_str).map(String::from) {
                if !offered.is_empty() {
                    match crate::prompts::resolve_model(&offered, &model) {
                        Ok(id) => {
                            step.insert("model".into(), json!(id));
                        }
                        Err(listed) => return Ok(Err(format!("Nothing was proposed. {agent} on {} does not offer the model {model}. It offers: {listed}. Leave model out to use the agent's default.", ordered[0].1))),
                    }
                }
            }
        }

        if !step_choices.is_empty() {
            choices.insert(key.clone(), Value::Object(step_choices));
        }
    }

    let parsed = match serde_json::from_value::<WorkflowPlan>(plan.clone()) {
        Ok(parsed) => parsed,
        Err(error) => return Ok(Err(format!("Nothing was proposed. Enter a workflow plan: {error}"))),
    };

    if let Err(error) = crate::workflows::validate(&parsed, 0) {
        return Ok(Err(format!("Nothing was proposed. {}", error.message())));
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into start_proposals (owner, conversation_id, kind, plan, choices, target_group_id) values ($1, $2, $3, $4, $5, $6) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(kind)
        .bind(json!(parsed))
        .bind(Value::Object(choices))
        .bind(target_group)
        .fetch_one(&state.pool)
        .await?;

    Ok(Ok(publish(state, owner, find_owned(state, id, owner).await?).await?))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<StartProposal>>> {
    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from start_proposals where owner = $1 order by (state = 'proposed') desc, created_at desc limit {LISTED}"))).bind(user.id).fetch_all(&state.pool).await?;
    let mut proposals = Vec::new();

    for row in rows {
        proposals.push(to_proto(&state, row).await?);
    }

    Ok(Json(proposals))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<ApproveStartProposalRequest>>) -> ApiResult<Json<StartProposal>> {
    let request = request.map(|Json(request)| request).unwrap_or_default();

    Ok(Json(approve_for(&state, user.id, id, request).await?))
}

pub async fn approve_for(state: &AppState, owner: Uuid, id: Uuid, request: ApproveStartProposalRequest) -> ApiResult<StartProposal> {
    let found = find_owned(state, id, owner).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let mut plan: WorkflowPlan = serde_json::from_value(found.plan.clone()).map_err(|_| ApiError::not_found("That proposal"))?;

    for step in &mut plan.steps {
        let choice = request.choices.get(&step.key);
        let open = &found.choices[&step.key];
        let nodes: Vec<Uuid> = open["node"].as_array().cloned().unwrap_or_default().iter().filter_map(|node| node["id"].as_str().and_then(|id| id.parse().ok())).collect();
        let accounts: Vec<String> = open["account"].as_array().cloned().unwrap_or_default().iter().filter_map(|account| account.as_str().map(String::from)).collect();
        let models: Vec<String> = open["model"].as_array().cloned().unwrap_or_default().iter().filter_map(|model| model["id"].as_str().map(String::from)).collect();

        if !nodes.is_empty() {
            match choice.and_then(|choice| choice.node_id).filter(|node| nodes.contains(node)) {
                Some(node) => step.node_id = Some(node),
                None => return Err(ApiError::conflict("choice_needed", format!("Choose a node for {}.", step.title))),
            }
        }

        if !accounts.is_empty() {
            match choice.and_then(|choice| choice.account.clone()).filter(|account| accounts.contains(account)) {
                Some(account) => step.account = Some(account),
                None => return Err(ApiError::conflict("choice_needed", format!("Choose an account for {}.", step.title))),
            }
        }

        if let Some(model) = choice.and_then(|choice| choice.model.clone()) {
            if !models.contains(&model) {
                return Err(ApiError::conflict("choice_needed", format!("Choose one of the models offered for {}.", step.title)));
            }

            step.model = Some(model);
        }
    }

    if found.kind == "conversation" {
        let step = &plan.steps[0];
        let (Some(source), Some(target_group), Some(node_id), Some(agent)) = (found.conversation_id, found.target_group_id, step.node_id, step.agent) else {
            return Err(ApiError::conflict("source_removed", "The conversation that proposed this, or the group it targets, has been removed."));
        };
        let place_group = match step.project_id {
            Some(project) => sqlx::query_scalar::<_, Option<Uuid>>("select group_id from projects where id = $1 and owner = $2").bind(project).bind(owner).fetch_optional(&state.pool).await?.flatten(),
            None => sqlx::query_scalar::<_, Option<Uuid>>("select conversation_group($1, $2, null, $3)").bind(&step.cwd).bind(node_id).bind(owner).fetch_one(&state.pool).await?,
        };

        if place_group != Some(target_group) {
            return Err(ApiError::conflict("target_group_changed", "That place is no longer in the group this was proposed for. Nothing was started."));
        }

        let claimed = sqlx::query("update start_proposals set state = 'started', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

        if claimed.rows_affected() == 0 {
            return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
        }

        let request = mycelium_proto::api::CreateConversationRequest {
            mode: None,
            config: None,
            node_id,
            agent,
            account: step.account.clone(),
            cwd: step.cwd.clone(),
            project_id: step.project_id,
            branch: None,
            worktree_path: step.worktree_path.clone(),
            title: Some(step.title.clone()),
            prompt: Some(step.prompt.clone()),
            parent_id: None,
            side_kind: None,
            model: step.model.clone(),
            routing: step.routing,
            client_id: None,
        };
        let started = crate::conversations::start(state, owner, request).await;

        match started {
            Ok(conversation) => {
                sqlx::query("update start_proposals set started_conversation_id = $2 where id = $1").bind(id).bind(conversation.id).execute(&state.pool).await?;
                let managed = match found.connect {
                    true => crate::conversations::manage(state, owner, source, conversation.id).await?,
                    false => crate::conversations::set_manager(state, owner, source, conversation.id).await?,
                };
                state.publish(Some(owner), StreamMessage::Conversation(managed.to_proto()?));

                return publish(state, owner, find_owned(state, id, owner).await?).await;
            }
            Err(cause) => {
                sqlx::query("update start_proposals set state = 'proposed', decided_at = null where id = $1").bind(id).execute(&state.pool).await?;

                return Err(cause);
            }
        }
    }

    let claimed = sqlx::query("update start_proposals set state = 'started', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let started_by = crate::workflows::actor_of(state, owner).await?;
    let created = crate::workflows::create_owned(state, owner, None, found.conversation_id, plan, started_by, None).await;

    match created {
        Ok(workflow) => {
            sqlx::query("update start_proposals set workflow_id = $2 where id = $1").bind(id).bind(workflow).execute(&state.pool).await?;
        }
        Err(cause) => {
            sqlx::query("update start_proposals set state = 'proposed', decided_at = null where id = $1").bind(id).execute(&state.pool).await?;

            return Err(cause);
        }
    }

    publish(state, owner, find_owned(state, id, owner).await?).await
}

pub async fn approve_and_connect(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<ApproveStartProposalRequest>>) -> ApiResult<Json<StartProposal>> {
    user.require_full_access()?;
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    if found.conversation_id.is_none() {
        return Err(ApiError::conflict("source_removed", "The source conversation has been removed."));
    }

    sqlx::query("update start_proposals set connect = true where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;
    let approved = approve_for(&state, user.id, id, request.map(|Json(request)| request).unwrap_or_default()).await;

    if approved.is_err() {
        sqlx::query("update start_proposals set connect = false where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;
    }

    Ok(Json(approved?))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<StartProposal>> {
    Ok(Json(decline_for(&state, user.id, id).await?))
}

pub async fn decline_for(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<StartProposal> {
    let found = find_owned(state, id, owner).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    sqlx::query("update start_proposals set state = 'declined', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let title = found.plan["title"].as_str().unwrap_or("it");
        let text = format!("I declined starting {title}. Do not start it another way.");
        prompts::tell_agent(state, owner, conversation_id, text).await;
    }

    publish(state, owner, find_owned(state, id, owner).await?).await
}
