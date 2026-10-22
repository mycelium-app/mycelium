use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::projects;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{CreateConversationRequest, CreateTaskRequest, Task};
use mycelium_proto::conversation::Conversation;
use uuid::Uuid;

const TASK_COLUMNS: &str = "id, node_id, agent, cwd, project_id, branch, title, prompt, created_at";

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: Uuid,
    node_id: Uuid,
    agent: String,
    cwd: String,
    project_id: Option<Uuid>,
    branch: Option<String>,
    title: String,
    prompt: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl TaskRow {
    fn to_proto(self) -> ApiResult<Task> {
        Ok(Task {
            id: self.id,
            node_id: self.node_id,
            agent: parse_enum(&self.agent)?,
            cwd: self.cwd,
            project_id: self.project_id,
            branch: self.branch,
            title: self.title,
            prompt: self.prompt,
            created_at: self.created_at,
        })
    }
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Task>>> {
    let rows = sqlx::query_as::<_, TaskRow>(sqlx::AssertSqlSafe(format!("select {TASK_COLUMNS} from tasks where owner = $1 and started_at is null order by created_at, id"))).bind(user.id).fetch_all(&state.pool).await?;
    let mut tasks = Vec::new();

    for row in rows {
        tasks.push(row.to_proto()?);
    }

    Ok(Json(tasks))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateTaskRequest>) -> ApiResult<(StatusCode, Json<Task>)> {
    let title = request.title.trim();

    if title.is_empty() {
        return Err(ApiError::bad_request("title_required", "Enter a title for this task."));
    }

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

    let prompt = request.prompt.as_deref().map(str::trim).filter(|text| !text.is_empty());
    let branch = request.branch.as_deref().map(str::trim).filter(|branch| !branch.is_empty());

    if branch.is_some() && request.project_id.is_none() {
        return Err(ApiError::bad_request("project_required", "Choose a project to start a worktree from."));
    }

    let row = sqlx::query_as::<_, TaskRow>(sqlx::AssertSqlSafe(format!(
        "insert into tasks (owner, node_id, agent, cwd, project_id, branch, title, prompt) values ($1, $2, $3, $4, $5, $6, $7, $8) returning {TASK_COLUMNS}",
    )))
    .bind(user.id)
    .bind(request.node_id)
    .bind(enum_text(&request.agent))
    .bind(request.cwd.trim())
    .bind(request.project_id)
    .bind(branch)
    .bind(title)
    .bind(prompt)
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.to_proto()?)))
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let claimed = sqlx::query_as::<_, TaskRow>(sqlx::AssertSqlSafe(format!("update tasks set started_at = now() where id = $1 and owner = $2 and started_at is null returning {TASK_COLUMNS}")))
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;
    let Some(row) = claimed else {
        return Err(ApiError::not_found("That task"));
    };

    let task = row.to_proto()?;
    let prompt = task.prompt.clone().unwrap_or_else(|| task.title.clone());
    let request = CreateConversationRequest { node_id: task.node_id, agent: task.agent, cwd: task.cwd, project_id: task.project_id, branch: task.branch, title: Some(task.title), prompt: Some(prompt) };
    let started = conversations::start(&state, &user, request).await;

    match started {
        Ok(conversation) => {
            sqlx::query("update tasks set conversation_id = $2 where id = $1").bind(id).bind(conversation.id).execute(&state.pool).await?;

            Ok((StatusCode::CREATED, Json(conversation)))
        }
        Err(cause) => {
            sqlx::query("update tasks set started_at = null where id = $1").bind(id).execute(&state.pool).await?;

            Err(cause)
        }
    }
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from tasks where id = $1 and owner = $2 and started_at is null").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That task"));
    }

    Ok(StatusCode::NO_CONTENT)
}
