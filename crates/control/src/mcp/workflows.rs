use super::call::{ToolCall, ToolReply};
use crate::error::{ApiError, ApiResult};
use crate::start_proposals;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {"name":"start_workflow","description":"Propose a native workflow or subagent; the person approves it on a card first and nothing starts before. Steps may name node and account by name or id (see list_places). Steps use ordinary conversations across agents and nodes. Dependencies fan out automatically; gate steps wait for the person. Prompts reference {{step_key.output}} and {{previous}}. Nested workflow steps contain another plan. Use max_rounds (1 to 20) and optional stop_when text for bounded debate loops.","inputSchema":{"type":"object","properties":{"plan":{"type":"object"}},"required":["plan"]}},
        {"name":"start_subagent","description":"Start a native subagent as an ordinary conversation, independently of the current agent harness. Supply a step with title, prompt, node_id, agent (claude or codex), cwd, optional model (as list_places names it), difficulty, account, routing (any_model, claude_any or codex_any), project_id and branch, or project_id and worktree_path to open a worktree the project already has on that node. With no model or account named it uses Any model, moving between accounts when one reaches its usage limit; a named model keeps its agent and moves between that agent's accounts; a named account stays on it. Or provider_id and model for a chat-model subagent. Name node and account by name or id (see list_places). The person approves the card first: nothing starts until they do. Read its conversation and result with get_workflow.","inputSchema":{"type":"object","properties":{"step":{"type":"object"}},"required":["step"]}},
        {"name":"list_places","description":"List the person's nodes with whether each is online, their agents with the models each last offered, signed-in accounts, chat models and projects with their checkouts, one per node the project is on. Use it to name a node, agent, model and account for start_subagent or start_workflow. It only reads and needs no approval.","inputSchema":{"type":"object","properties":{}}},
        {"name":"get_workflow","description":"Read a native workflow, its step conversations, outputs and gates.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let owner = caller.owner().await?;

    Ok(match tool {
        "list_places" => {
            let nodes = sqlx::query_as::<_, (Uuid, String)>("select id, name from nodes where created_by = $1 and revoked_at is null order by name").bind(owner).fetch_all(&state.pool).await?;
            let ids: Vec<Uuid> = nodes.iter().map(|(id, _)| *id).collect();
            let agents = sqlx::query_as::<_, (Uuid, String)>("select node_id, agent from agents where node_id = any($1) and available order by agent").bind(&ids).fetch_all(&state.pool).await?;
            let logins = sqlx::query_as::<_, (Uuid, String, String)>("select node_id, agent, account from logins where node_id = any($1) and state = 'signed_in' and account is not null order by account").bind(&ids).fetch_all(&state.pool).await?;
            let projects = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>)>("select p.id, p.name, p.repository, f.kind from projects p left join forges f on f.id = p.forge_id where p.owner = $1 order by p.name, p.id").bind(owner).fetch_all(&state.pool).await?;
            let checkouts = sqlx::query_as::<_, (Uuid, Uuid, String)>("select c.project_id, c.node_id, c.path from project_checkouts c join projects p on p.id = c.project_id where p.owner = $1 order by c.created_at, c.id").bind(owner).fetch_all(&state.pool).await?;
            let providers = sqlx::query_as::<_, (Uuid, String, Vec<String>)>("select id, name, models from model_providers where owner = $1 order by name").bind(owner).fetch_all(&state.pool).await?;
            let mut agent_models = std::collections::HashMap::new();

            for (node_id, agent) in &agents {
                let offered = crate::prompts::offered_models(state, owner, *node_id, agent).await?;
                let models: Vec<Value> = offered.iter().filter_map(|entry| Some(json!({"model": entry.get("modelId")?, "name": entry.get("name")}))).collect();

                agent_models.entry(*node_id).or_insert_with(serde_json::Map::new).insert(agent.clone(), json!(models));
            }

            let node_names: std::collections::HashMap<Uuid, &String> = nodes.iter().map(|(node_id, name)| (*node_id, name)).collect();
            let nodes: Vec<Value> = nodes
                .iter()
                .map(|(node_id, name)| {
                    let accounts: Vec<Value> = logins.iter().filter(|login| login.0 == *node_id).map(|(_, agent, account)| json!({"agent": agent, "account": account})).collect();
                    json!({"id": node_id, "name": name, "online": state.hub.is_online(*node_id), "agents": agents.iter().filter(|agent| agent.0 == *node_id).map(|agent| agent.1.clone()).collect::<Vec<_>>(), "agent_models": agent_models.get(node_id).cloned().unwrap_or_default(), "signed_in_accounts": accounts})
                })
                .collect();
            let projects: Vec<Value> = projects
                .iter()
                .map(|(project_id, name, repository, forge)| {
                    let held: Vec<Value> = checkouts
                        .iter()
                        .filter(|checkout| checkout.0 == *project_id)
                        .filter_map(|(_, node_id, path)| Some(json!({"node": node_names.get(node_id)?, "node_id": node_id, "path": path})))
                        .collect();

                    json!({"project_id": project_id, "name": name, "repository": repository, "forge": forge, "checkouts": held})
                })
                .collect();
            let places = json!({"nodes": nodes, "projects": projects, "chat_models": providers.iter().map(|(provider_id, name, models)| json!({"provider_id": provider_id, "provider": name, "models": models})).collect::<Vec<_>>()});

            ToolReply::text(&places.to_string(), false)
        }
        "start_workflow" | "start_subagent" => {
            let plan = if tool == "start_subagent" {
                let mut step = arguments.get("step").cloned().unwrap_or(Value::Null);

                if let Some(step) = step.as_object_mut() {
                    step.insert("key".into(), json!("subagent"));
                }

                json!({"title":step.get("title").cloned().unwrap_or(json!("Subagent")),"steps":[step]})
            } else {
                arguments.get("plan").cloned().unwrap_or(Value::Null)
            };
            let Some(conversation) = caller.conversation_id().await? else {
                return Ok(ToolReply::text("Nothing was proposed. A start proposal belongs to a conversation, and this call came from none.", true));
            };
            let kind = if tool == "start_subagent" { "subagent" } else { "workflow" };

            match start_proposals::propose(state, owner, conversation, kind, plan).await? {
                Ok(proposal) => ToolReply::text(&format!("Waiting for the person to approve starting {} (proposal {}). Nothing has started.", proposal.title, proposal.id), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        _ => {
            let Some(workflow) = arguments.get("id").and_then(Value::as_str).and_then(|id| id.parse().ok()) else {
                return Ok(ToolReply::invalid("get_workflow needs a workflow id."));
            };
            let found = crate::workflows::load(state, owner, workflow).await?;
            let value = serde_json::to_value(found).map_err(ApiError::internal)?;

            ToolReply::text(&value.to_string(), false)
        }
    })
}
