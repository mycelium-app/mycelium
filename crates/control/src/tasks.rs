use crate::auth::AuthUser;
use crate::conversations;
use crate::error::{ApiError, ApiResult};
use crate::projects;
use crate::prompts;
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{AnswerTaskRequest, CreateConversationRequest, CreateTaskRequest, PromptRequest, Task, TaskStatus};
use mycelium_proto::conversation::Conversation;
use uuid::Uuid;

const TASK_SELECT: &str = "select t.id, t.node_id, t.agent, t.cwd, t.project_id, t.branch, t.title, t.prompt, t.status, t.options, t.answer, t.source_id, s.title as source_title, t.conversation_id, t.created_at from tasks t left join conversations s on s.id = t.source_id";

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
    status: String,
    options: Vec<String>,
    answer: Option<String>,
    source_id: Option<Uuid>,
    source_title: Option<String>,
    conversation_id: Option<Uuid>,
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
            status: parse_enum(&self.status)?,
            options: self.options,
            answer: self.answer,
            source_id: self.source_id,
            source_title: self.source_title,
            conversation_id: self.conversation_id,
            created_at: self.created_at,
        })
    }
}

async fn load(state: &AppState, id: Uuid) -> ApiResult<Task> {
    let row = sqlx::query_as::<_, TaskRow>(sqlx::AssertSqlSafe(format!("{TASK_SELECT} where t.id = $1"))).bind(id).fetch_one(&state.pool).await?;

    row.to_proto()
}

async fn move_to(state: &AppState, user: &AuthUser, id: Uuid, from: TaskStatus, to: TaskStatus) -> ApiResult<Task> {
    let moved = sqlx::query("update tasks set status = $3 where id = $1 and owner = $2 and status = $4").bind(id).bind(user.id).bind(enum_text(&to)).bind(enum_text(&from)).execute(&state.pool).await?;

    if moved.rows_affected() == 0 {
        return Err(ApiError::not_found("That task"));
    }

    load(state, id).await
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Task>>> {
    let rows = sqlx::query_as::<_, TaskRow>(sqlx::AssertSqlSafe(format!("{TASK_SELECT} where t.owner = $1 order by t.created_at, t.id"))).bind(user.id).fetch_all(&state.pool).await?;
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

    let id = sqlx::query_scalar::<_, Uuid>("insert into tasks (owner, node_id, agent, cwd, project_id, branch, title, prompt) values ($1, $2, $3, $4, $5, $6, $7, $8) returning id")
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

    Ok((StatusCode::CREATED, Json(load(&state, id).await?)))
}

pub async fn record(state: &AppState, conversation_id: Uuid, status: TaskStatus, title: &str, prompt: Option<&str>, options: &[String], project_name: Option<&str>) -> ApiResult<Result<Task, String>> {
    let (owner, node_id, agent, cwd, conversation_project) =
        sqlx::query_as::<_, (Uuid, Uuid, String, String, Option<Uuid>)>("select owner, node_id, agent, cwd, project_id from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let mut place = (node_id, cwd, conversation_project);

    if let Some(name) = project_name {
        let found = sqlx::query_as::<_, (Uuid, Uuid, String)>("select id, node_id, path from projects where owner = $1 and lower(name) = lower($2)").bind(owner).bind(name.trim()).fetch_optional(&state.pool).await?;
        let Some((project_id, project_node, path)) = found else {
            return Ok(Err(format!("Nothing was stored. There is no project called {name}. Leave the project out to use this conversation's own.")));
        };

        place = (project_node, path, Some(project_id));
    }

    let id = sqlx::query_scalar::<_, Uuid>("insert into tasks (owner, node_id, agent, cwd, project_id, title, prompt, status, options, source_id) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning id")
        .bind(owner)
        .bind(place.0)
        .bind(agent)
        .bind(place.1)
        .bind(place.2)
        .bind(title)
        .bind(prompt)
        .bind(enum_text(&status))
        .bind(options)
        .bind(conversation_id)
        .fetch_one(&state.pool)
        .await?;

    Ok(Ok(load(state, id).await?))
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<Conversation>)> {
    let task = move_to(&state, &user, id, TaskStatus::Open, TaskStatus::Claimed).await?;
    let prompt = task.prompt.clone().unwrap_or_else(|| task.title.clone());
    let request = CreateConversationRequest { account: None, node_id: task.node_id, agent: task.agent, cwd: task.cwd, project_id: task.project_id, branch: task.branch, title: Some(task.title), prompt: Some(prompt), parent_id: None, side_kind: None };
    let started = conversations::start(&state, &user, request).await;

    match started {
        Ok(conversation) => {
            sqlx::query("update tasks set conversation_id = $2 where id = $1").bind(id).bind(conversation.id).execute(&state.pool).await?;

            Ok((StatusCode::CREATED, Json(conversation)))
        }
        Err(cause) => {
            sqlx::query("update tasks set status = 'open' where id = $1").bind(id).execute(&state.pool).await?;

            Err(cause)
        }
    }
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Task>> {
    Ok(Json(move_to(&state, &user, id, TaskStatus::Proposed, TaskStatus::Open).await?))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Task>> {
    Ok(Json(move_to(&state, &user, id, TaskStatus::Proposed, TaskStatus::Declined).await?))
}

pub async fn answer(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<AnswerTaskRequest>) -> ApiResult<Json<Task>> {
    let text = request.answer.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("answer_required", "Enter an answer to send."));
    }

    let asked = move_to(&state, &user, id, TaskStatus::Question, TaskStatus::Done).await?;
    let Some(source_id) = asked.source_id else {
        sqlx::query("update tasks set status = 'question' where id = $1").bind(id).execute(&state.pool).await?;

        return Err(ApiError::conflict("asker_gone", "The conversation that asked this question no longer exists, so it cannot receive an answer."));
    };

    let delivered = prompts::prompt(State(state.clone()), user, Path(source_id), Json(PromptRequest { text: format!("Answer to your question \"{}\": {text}", asked.title) })).await;

    match delivered {
        Ok(_) => {
            sqlx::query("update tasks set answer = $2 where id = $1").bind(id).bind(text).execute(&state.pool).await?;

            Ok(Json(load(&state, id).await?))
        }
        Err(cause) => {
            sqlx::query("update tasks set status = 'question' where id = $1").bind(id).execute(&state.pool).await?;

            Err(cause)
        }
    }
}

pub async fn done(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Task>> {
    Ok(Json(move_to(&state, &user, id, TaskStatus::Claimed, TaskStatus::Done).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from tasks where id = $1 and owner = $2 and status = 'open'").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That task"));
    }

    Ok(StatusCode::NO_CONTENT)
}
