use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::projects::ProjectRow;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use mycelium_proto::api::Worktree;
use mycelium_proto::link::GitCommand;
use uuid::Uuid;

const WORKTREE_COLUMNS: &str = "worktrees.id, worktrees.project_id, worktrees.branch, worktrees.path, worktrees.issue_number, worktrees.created_at";

#[derive(sqlx::FromRow)]
pub struct WorktreeRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch: String,
    pub path: String,
    pub issue_number: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl WorktreeRow {
    pub fn to_proto(self) -> Worktree {
        Worktree { id: self.id, project_id: self.project_id, branch: self.branch, path: self.path, issue_number: self.issue_number, created_at: self.created_at }
    }
}

pub async fn create(state: &AppState, project: &ProjectRow, branch: &str, issue_number: Option<i64>) -> ApiResult<WorktreeRow> {
    let branch = branch.trim();

    if branch.is_empty() {
        return Err(ApiError::bad_request("branch_required", "Enter a name for the new branch."));
    }

    let answer = relay(state, project.node_id, project.path.clone(), GitCommand::AddWorktree { branch: branch.to_string() }).await?;

    record(state, project, branch, answer["path"].as_str().unwrap_or_default(), issue_number).await
}

pub async fn create_from_remote_branch(state: &AppState, project: &ProjectRow, branch: &str) -> ApiResult<WorktreeRow> {
    let answer = relay(state, project.node_id, project.path.clone(), GitCommand::AddRemoteWorktree { branch: branch.to_string() }).await?;

    record(state, project, branch, answer["path"].as_str().unwrap_or_default(), None).await
}

async fn record(state: &AppState, project: &ProjectRow, branch: &str, path: &str, issue_number: Option<i64>) -> ApiResult<WorktreeRow> {
    let row = sqlx::query_as::<_, WorktreeRow>(sqlx::AssertSqlSafe(format!(
        "insert into worktrees (project_id, branch, path, issue_number) values ($1, $2, $3, $4)
         on conflict (project_id, branch) do update set path = excluded.path, issue_number = excluded.issue_number
         returning {}",
        WORKTREE_COLUMNS.replace("worktrees.", ""),
    )))
    .bind(project.id)
    .bind(branch)
    .bind(path)
    .bind(issue_number)
    .fetch_one(&state.pool)
    .await?;

    Ok(row)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Worktree>>> {
    let rows = sqlx::query_as::<_, WorktreeRow>(sqlx::AssertSqlSafe(format!(
        "select {WORKTREE_COLUMNS} from worktrees join projects on projects.id = worktrees.project_id where projects.owner = $1 order by worktrees.created_at, worktrees.id",
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(WorktreeRow::to_proto).collect()))
}
