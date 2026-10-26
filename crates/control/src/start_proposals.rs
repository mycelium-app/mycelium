use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::state::AppState;
use crate::workflows::Plan;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{ApproveStartProposalRequest, PromptRequest, StartProposal, StartProposalNode, StartProposalState, StartProposalStep};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const LISTED: i64 = 100;
const COLUMNS: &str = "id, conversation_id, kind, plan, choices, connect, state, workflow_id, created_at, decided_at";

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
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn node_of(state: &AppState, names: &[(Uuid, String)], id: Uuid) -> StartProposalNode {
    let name = names.iter().find(|(node, _)| *node == id).map(|(_, name)| name.clone()).unwrap_or_default();

    StartProposalNode { id, name, online: state.hub.is_online(id) }
}

async fn to_proto(state: &AppState, row: ProposalRow) -> ApiResult<StartProposal> {
    let plan: Plan = serde_json::from_value(row.plan.clone()).map_err(|_| ApiError::not_found("That proposal"))?;
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
            let folder = step.project_id.and_then(|id| projects.iter().find(|(project, _)| *project == id).map(|(_, name)| name.clone())).or_else(|| Some(step.cwd.clone()).filter(|cwd| !cwd.is_empty()));

            StartProposalStep {
                key: step.key.clone(),
                title: step.title.clone(),
                depends_on: step.depends_on.clone(),
                gate: step.gate,
                node: step.node_id.map(|id| node_of(state, &names, id)),
                folder,
                agent: step.agent.map(|agent| agent.as_str().to_string()),
                account: step.account.clone(),
                model: step.model.clone(),
                first_message: step.prompt.clone(),
                node_choices,
                account_choices,
            }
        })
        .collect();

    let proposal_state = match row.state.as_str() {
        "started" => StartProposalState::Started,
        "declined" => StartProposalState::Declined,
        _ => StartProposalState::Proposed,
    };

    Ok(StartProposal { id: row.id, conversation_id: row.conversation_id, kind: row.kind, title: plan.title, steps, state: proposal_state, connect: row.connect, workflow_id: row.workflow_id, created_at: row.created_at, decided_at: row.decided_at })
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

pub async fn propose(state: &AppState, owner: Uuid, conversation_id: Uuid, kind: &str, plan: Value) -> ApiResult<Result<StartProposal, String>> {
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
        }

        if !step_choices.is_empty() {
            choices.insert(key.clone(), Value::Object(step_choices));
        }
    }

    let parsed = match serde_json::from_value::<Plan>(plan.clone()) {
        Ok(parsed) => parsed,
        Err(error) => return Ok(Err(format!("Nothing was proposed. Enter a workflow plan: {error}"))),
    };

    if let Err(error) = crate::workflows::validate(&parsed, 0) {
        return Ok(Err(format!("Nothing was proposed. {}", error.message())));
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into start_proposals (owner, conversation_id, kind, plan, choices) values ($1, $2, $3, $4, $5) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(kind)
        .bind(json!(parsed))
        .bind(Value::Object(choices))
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
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let mut plan: Plan = serde_json::from_value(found.plan.clone()).map_err(|_| ApiError::not_found("That proposal"))?;

    for step in &mut plan.steps {
        let choice = request.choices.get(&step.key);
        let open = &found.choices[&step.key];
        let nodes: Vec<Uuid> = open["node"].as_array().cloned().unwrap_or_default().iter().filter_map(|node| node["id"].as_str().and_then(|id| id.parse().ok())).collect();
        let accounts: Vec<String> = open["account"].as_array().cloned().unwrap_or_default().iter().filter_map(|account| account.as_str().map(String::from)).collect();

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
    }

    let claimed = sqlx::query("update start_proposals set state = 'started', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let created = crate::workflows::create_owned(&state, user.id, None, found.conversation_id, plan).await;

    match created {
        Ok(workflow) => {
            sqlx::query("update start_proposals set workflow_id = $2 where id = $1").bind(id).bind(workflow).execute(&state.pool).await?;
        }
        Err(cause) => {
            sqlx::query("update start_proposals set state = 'proposed', decided_at = null where id = $1").bind(id).execute(&state.pool).await?;

            return Err(cause);
        }
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?).await?))
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
    let approved = approve(State(state.clone()), user, Path(id), request).await;

    if approved.is_err() {
        sqlx::query("update start_proposals set connect = false where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;
    }

    approved
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<StartProposal>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    sqlx::query("update start_proposals set state = 'declined', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let title = found.plan["title"].as_str().unwrap_or("it");
        let text = format!("I declined starting {title}. Do not start it another way.");
        let _ = prompts::prompt(State(state.clone()), user.clone(), Path(conversation_id), Json(PromptRequest { text, ..Default::default() })).await;
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?).await?))
}
