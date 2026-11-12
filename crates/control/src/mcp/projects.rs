use super::approval;
use super::call::{ToolCall, ToolReply};
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::projects;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{CreateProjectRequest, PatchProjectRequest, Project};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {
            "name": "create_project",
            "description": "Add a project: register a folder that is already on a node, make a new folder, or clone a repository from a connected forge. The person approves it in a card in this conversation first. List nodes and existing projects with list_places.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": {"type": "string", "description": "The name or id of the node the project lives on."},
                    "path": {"type": "string", "description": "A folder already on the node, such as ~/Projects/slop/platform."},
                    "new_folder": {"type": "string", "description": "The name of a new folder to make in the node's projects folder, instead of path."},
                    "forge": {"type": "string", "description": "The id or kind (github, gitlab, forgejo) of a connected forge, to clone repository from it."},
                    "repository": {"type": "string", "description": "The repository as owner/name, with forge."},
                    "new_repository": {"type": "boolean", "description": "Create the repository on the forge before cloning it."},
                },
                "required": ["node"],
            },
        },
        {
            "name": "update_project",
            "description": "Change a project's group or test command. The person approves it in a card in this conversation first. Set its logo with set_project_icon.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "The name or id of the project."},
                    "group": {"type": "string", "description": "The name of the group to put it in, or none to take it out of its group."},
                    "test_command": {"type": "string", "description": "The command that runs the project's tests, or an empty string to clear it."},
                },
                "required": ["project"],
            },
        },
    ])
}

async fn described(state: &crate::state::AppState, project: &Project) -> ApiResult<String> {
    let node = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(project.node_id).fetch_one(&state.pool).await?;

    Ok(format!("{} ({}) at {} on {node}", project.name, project.id, project.path))
}

async fn node_id(state: &crate::state::AppState, owner: Uuid, node: &str) -> ApiResult<Option<Uuid>> {
    Ok(sqlx::query_scalar::<_, Uuid>("select id from nodes where created_by = $1 and revoked_at is null and (id::text = lower($2) or name = $2)").bind(owner).bind(node).fetch_optional(&state.pool).await?)
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let field = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    let owner = caller.owner().await?;

    if tool == "create_project" {
        let Some(node) = field("node") else {
            return Ok(ToolReply::invalid("create_project needs node, a string."));
        };
        let Some(node_id) = node_id(state, owner, &node).await? else {
            return Ok(ToolReply::text(&format!("Nothing was added. None of the person's nodes is {node}. List them with list_places."), true));
        };
        let forge_id = match field("forge") {
            Some(forge) => {
                let found = sqlx::query_scalar::<_, Uuid>("select id from forges where owner = $1 and (id::text = lower($2) or kind = lower($2))").bind(owner).bind(&forge).fetch_all(&state.pool).await?;

                match found.as_slice() {
                    [only] => Some(*only),
                    [] => return Ok(ToolReply::text(&format!("Nothing was added. None of the person's forges is {forge}."), true)),
                    _ => return Ok(ToolReply::text(&format!("Nothing was added. More than one forge is {forge}. Give its id."), true)),
                }
            }
            None => None,
        };

        if let Err(refusal) = approval::approved(&caller, tool, &arguments).await? {
            return Ok(ToolReply::text(&refusal, true));
        }

        let request = CreateProjectRequest { node_id, forge_id, repository: field("repository"), path: field("path"), new_repository: arguments["new_repository"].as_bool().unwrap_or(false), new_folder: field("new_folder"), group_id: None };

        return Ok(match projects::create(State(state.clone()), AuthUser::of(state, owner).await?, Json(request)).await {
            Ok((_, Json(created))) => ToolReply::text(&format!("Added the project {}.", described(state, &created.project).await?), false),
            Err(error) => ToolReply::text(&format!("Nothing was added. {}", error.message()), true),
        });
    }

    let Some(project) = field("project") else {
        return Ok(ToolReply::invalid("update_project needs project, a string."));
    };
    let found = sqlx::query_scalar::<_, Uuid>("select id from projects where owner = $1 and (id::text = lower($2) or lower(name) = lower($2))").bind(owner).bind(&project).fetch_all(&state.pool).await?;
    let project_id = match found.as_slice() {
        [only] => *only,
        [] => return Ok(ToolReply::text(&format!("Nothing was changed. None of the person's projects is {project}. List them with list_places."), true)),
        _ => return Ok(ToolReply::text(&format!("Nothing was changed. More than one project is called {project}. Give its id from list_places."), true)),
    };
    let group = field("group");
    let test_command = arguments.get("test_command").and_then(Value::as_str).map(str::to_string);

    if group.is_none() && test_command.is_none() {
        return Ok(ToolReply::text("Nothing was changed. Give group or test_command.", true));
    }

    let group_id = match group.as_deref() {
        Some(name) if name.eq_ignore_ascii_case("none") => None,
        Some(name) => match sqlx::query_scalar::<_, Uuid>("select id from groups where owner = $1 and lower(name) = lower($2)").bind(owner).bind(name).fetch_optional(&state.pool).await? {
            Some(id) => Some(id),
            None => return Ok(ToolReply::text(&format!("Nothing was changed. None of the person's groups is called {name}."), true)),
        },
        None => None,
    };

    if let Err(refusal) = approval::approved(&caller, tool, &arguments).await? {
        return Ok(ToolReply::text(&refusal, true));
    }

    let mut updated = None;

    if let Some(test_command) = test_command {
        updated = Some(projects::patch(State(state.clone()), AuthUser::of(state, owner).await?, Path(project_id), Json(PatchProjectRequest { group_id: None, icon: None, clear_icon: false, test_command: Some(test_command) })).await);
    }

    if group.is_some() && !matches!(updated, Some(Err(_))) {
        updated = Some(projects::patch(State(state.clone()), AuthUser::of(state, owner).await?, Path(project_id), Json(PatchProjectRequest { group_id, icon: None, clear_icon: false, test_command: None })).await);
    }

    Ok(match updated {
        Some(Ok(Json(project))) => ToolReply::text(&format!("Updated the project {}.", described(state, &project).await?), false),
        Some(Err(error)) => ToolReply::text(&format!("Nothing more was changed. {}", error.message()), true),
        None => ToolReply::text("Nothing was changed.", true),
    })
}
