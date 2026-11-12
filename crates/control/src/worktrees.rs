use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::projects::{CheckoutRow, ProjectRow, find_owned, not_on_node};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use mycelium_proto::api::{ExistingWorktree, Worktree};
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

pub async fn create(state: &AppState, checkout: &CheckoutRow, branch: &str, issue_number: Option<i64>) -> ApiResult<WorktreeRow> {
    let branch = branch.trim();

    if branch.is_empty() {
        return Err(ApiError::bad_request("branch_required", "Enter a name for the new branch."));
    }

    let answer = relay(state, checkout.node_id, checkout.path.clone(), GitCommand::AddWorktree { branch: branch.to_string() }).await?;

    record(state, checkout, branch, answer["path"].as_str().unwrap_or_default(), issue_number).await
}

pub async fn create_from_remote_branch(state: &AppState, checkout: &CheckoutRow, branch: &str) -> ApiResult<WorktreeRow> {
    let answer = relay(state, checkout.node_id, checkout.path.clone(), GitCommand::AddRemoteWorktree { branch: branch.to_string() }).await?;

    record(state, checkout, branch, answer["path"].as_str().unwrap_or_default(), None).await
}

async fn record(state: &AppState, checkout: &CheckoutRow, branch: &str, path: &str, issue_number: Option<i64>) -> ApiResult<WorktreeRow> {
    let row = sqlx::query_as::<_, WorktreeRow>(sqlx::AssertSqlSafe(format!(
        "insert into worktrees (project_id, checkout_id, branch, path, issue_number) values ($1, $2, $3, $4, $5)
         on conflict (checkout_id, branch) do update set path = excluded.path, issue_number = excluded.issue_number
         returning {}",
        WORKTREE_COLUMNS.replace("worktrees.", ""),
    )))
    .bind(checkout.project_id)
    .bind(checkout.id)
    .bind(branch)
    .bind(path)
    .bind(issue_number)
    .fetch_one(&state.pool)
    .await?;

    Ok(row)
}

async fn existing_on_node(state: &AppState, checkout: &CheckoutRow) -> ApiResult<Vec<ExistingWorktree>> {
    let answer = relay(state, checkout.node_id, checkout.path.clone(), GitCommand::ExistingWorktrees).await?;

    serde_json::from_value(answer["worktrees"].clone()).map_err(|_| ApiError::internal("The node answered with a worktree list that could not be read."))
}

#[derive(serde::Deserialize)]
pub struct ExistingQuery {
    node_id: Option<Uuid>,
}

pub async fn existing(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<ExistingQuery>) -> ApiResult<Json<Vec<ExistingWorktree>>> {
    let project = find_owned(&state, id, user.id).await?;
    let checkout = project.checkout_on(query.node_id.unwrap_or(project.node_id)).ok_or_else(not_on_node)?;

    Ok(Json(existing_on_node(&state, checkout).await?))
}

pub async fn find_existing(state: &AppState, project: &ProjectRow, node_id: Uuid, path: &str) -> ApiResult<ExistingWorktree> {
    let wanted = path.trim().trim_end_matches('/');
    let checkout = project.checkout_on(node_id).ok_or_else(not_on_node)?;

    existing_on_node(state, checkout)
        .await?
        .into_iter()
        .find(|worktree| worktree.path.trim_end_matches('/') == wanted)
        .ok_or_else(|| ApiError::bad_request("worktree_not_found", format!("{wanted} is not a worktree of {} on this node. Choose one of the project's worktrees.", project.name)))
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
