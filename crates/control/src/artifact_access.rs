use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{
    ApproveArtifactAccessRequest, ArtifactAccessRequest, ArtifactLink, LinkArtifactRequest,
};
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use uuid::Uuid;

const REQUEST_COLUMNS: &str =
    "id, artifact_id, conversation_id, permission, reason, state, created_at, decided_at";

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: Uuid,
    artifact_id: Uuid,
    project_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
    permission: String,
}

impl From<LinkRow> for ArtifactLink {
    fn from(row: LinkRow) -> Self {
        Self {
            id: row.id,
            artifact_id: row.artifact_id,
            project_id: row.project_id,
            conversation_id: row.conversation_id,
            permission: row.permission,
        }
    }
}

#[derive(sqlx::FromRow)]
struct RequestRow {
    id: Uuid,
    artifact_id: Uuid,
    conversation_id: Uuid,
    permission: String,
    reason: String,
    state: String,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<RequestRow> for ArtifactAccessRequest {
    fn from(row: RequestRow) -> Self {
        Self {
            id: row.id,
            artifact_id: row.artifact_id,
            conversation_id: row.conversation_id,
            permission: row.permission,
            reason: row.reason,
            state: row.state,
            created_at: row.created_at,
            decided_at: row.decided_at,
        }
    }
}

fn permission(value: &str) -> ApiResult<()> {
    if !matches!(value, "view" | "edit") {
        return Err(ApiError::bad_request(
            "permission_invalid",
            "Choose view or edit access.",
        ));
    }

    Ok(())
}

pub async fn require(
    state: &AppState,
    owner: Uuid,
    conversation: Uuid,
    artifact: Uuid,
    edit: bool,
) -> ApiResult<()> {
    let allowed: bool = sqlx::query_scalar("select exists(select 1 from artifact_links l join artifacts a on a.id = l.artifact_id join conversations c on c.id = $3 and c.owner = $2 where a.id = $1 and a.owner = $2 and (l.conversation_id = c.id or l.project_id = c.project_id) and (not $4 or l.permission = 'edit'))")
        .bind(artifact).bind(owner).bind(conversation).bind(edit).fetch_one(&state.pool).await?;

    if !allowed {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "artifact_access_required",
            "This conversation does not have the required artefact access. Call request_artifact_access with view or edit and a reason.",
        ));
    }

    Ok(())
}

pub async fn find(
    state: &AppState,
    owner: Uuid,
    conversation: Uuid,
    query: &str,
) -> ApiResult<String> {
    find_owned(&state.pool, conversation, owner).await?;
    let rows: Vec<(Uuid, String, String, String, bool)> = sqlx::query_as("select a.id, a.title, a.slug, a.kind, bool_or(l.permission = 'edit') from artifacts a join artifact_links l on l.artifact_id = a.id join conversations c on c.id = $2 and c.owner = $1 where a.owner = $1 and (l.conversation_id = c.id or l.project_id = c.project_id) and ($3 = '' or strpos(lower(a.title || ' ' || a.slug), lower($3)) > 0) group by a.id order by a.updated_at desc limit 100")
        .bind(owner).bind(conversation).bind(query.trim()).fetch_all(&state.pool).await?;
    let artifacts: Vec<_> = rows.into_iter().map(|(id, title, slug, kind, edit)| json!({"id": id, "title": title, "slug": slug, "kind": kind, "permission": if edit { "edit" } else { "view" }})).collect();

    Ok(json!({"artifacts": artifacts}).to_string())
}

pub async fn request(
    state: &AppState,
    owner: Uuid,
    conversation: Uuid,
    artifact: Uuid,
    access: &str,
    reason: &str,
) -> ApiResult<String> {
    permission(access)?;
    crate::artifacts::find_owned(state, artifact, owner).await?;
    find_owned(&state.pool, conversation, owner).await?;

    if reason.trim().is_empty() || reason.len() > 4000 {
        return Err(ApiError::bad_request(
            "reason_required",
            "Give a reason of 1 to 4,000 characters.",
        ));
    }

    if require(state, owner, conversation, artifact, access == "edit")
        .await
        .is_ok()
    {
        return Ok("This conversation already has the requested artefact access.".to_string());
    }

    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("insert into artifact_access_requests (owner, artifact_id, conversation_id, permission, reason) values ($1, $2, $3, $4, $5) on conflict (artifact_id, conversation_id, permission) where state = 'requested' do update set reason = excluded.reason returning {REQUEST_COLUMNS}")))
        .bind(owner).bind(artifact).bind(conversation).bind(access).bind(reason.trim()).fetch_one(&state.pool).await?;
    let request: ArtifactAccessRequest = row.into();
    state.publish(
        Some(owner),
        StreamMessage::ArtifactAccessRequest(request.clone()),
    );

    Ok(format!(
        "Requested {access} access to artefact {artifact}. The person can approve or decline request {} in Artefacts. Wait for their decision before using it.",
        request.id
    ))
}

pub async fn links(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ArtifactLink>>> {
    crate::artifacts::find_owned(&state, id, user.id).await?;
    let rows = sqlx::query_as::<_, LinkRow>("select id, artifact_id, project_id, conversation_id, permission from artifact_links where artifact_id = $1 order by id").bind(id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn link(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<LinkArtifactRequest>,
) -> ApiResult<Json<ArtifactLink>> {
    crate::artifacts::find_owned(&state, id, user.id).await?;
    permission(&request.permission)?;

    if request.project_id.is_some() == request.conversation_id.is_some() {
        return Err(ApiError::bad_request(
            "target_required",
            "Choose one project or conversation.",
        ));
    }

    if let Some(project) = request.project_id {
        let exists: bool = sqlx::query_scalar(
            "select exists(select 1 from projects where id = $1 and owner = $2)",
        )
        .bind(project)
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;

        if !exists {
            return Err(ApiError::not_found("That project"));
        }
    }

    if let Some(conversation) = request.conversation_id {
        find_owned(&state.pool, conversation, user.id).await?;
    }

    let row = sqlx::query_as::<_, LinkRow>("insert into artifact_links (artifact_id, project_id, conversation_id, permission) values ($1, $2, $3, $4) on conflict do nothing returning id, artifact_id, project_id, conversation_id, permission")
        .bind(id).bind(request.project_id).bind(request.conversation_id).bind(&request.permission).fetch_optional(&state.pool).await?;
    let row = match row {
        Some(row) => row,
        None => sqlx::query_as::<_, LinkRow>("update artifact_links set permission = $4 where artifact_id = $1 and (project_id = $2 or conversation_id = $3) returning id, artifact_id, project_id, conversation_id, permission")
            .bind(id).bind(request.project_id).bind(request.conversation_id).bind(request.permission).fetch_one(&state.pool).await?,
    };
    crate::artifacts::republish(&state, id).await?;

    Ok(Json(row.into()))
}

pub async fn unlink(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, link_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    crate::artifacts::find_owned(&state, id, user.id).await?;
    let deleted = sqlx::query("delete from artifact_links where id = $1 and artifact_id = $2")
        .bind(link_id)
        .bind(id)
        .execute(&state.pool)
        .await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That artefact link"));
    }

    crate::artifacts::republish(&state, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_requests(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ArtifactAccessRequest>>> {
    let rows = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from artifact_access_requests where owner = $1 order by created_at desc limit 200"))).bind(user.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn approve(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(choice): Json<ApproveArtifactAccessRequest>,
) -> ApiResult<Json<ArtifactAccessRequest>> {
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("select {REQUEST_COLUMNS} from artifact_access_requests where id = $1 and owner = $2 for update"))).bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(|| ApiError::not_found("That request"))?;

    if row.state != "requested" {
        return Err(ApiError::conflict(
            "request_decided",
            "This request has already been decided.",
        ));
    }

    let project: Option<Uuid> =
        sqlx::query_scalar("select project_id from conversations where id = $1 and owner = $2")
            .bind(row.conversation_id)
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;

    if choice.project && project.is_none() {
        return Err(ApiError::bad_request(
            "project_required",
            "This conversation has no project. Grant access to the conversation instead.",
        ));
    }

    let project = if choice.project { project } else { None };
    let conversation = if choice.project {
        None
    } else {
        Some(row.conversation_id)
    };
    sqlx::query("insert into artifact_links (artifact_id, project_id, conversation_id, permission) values ($1, $2, $3, $4) on conflict do nothing").bind(row.artifact_id).bind(project).bind(conversation).bind(&row.permission).execute(&mut *tx).await?;
    sqlx::query("update artifact_links set permission = 'edit' where artifact_id = $1 and (project_id = $2 or conversation_id = $3) and $4 = 'edit'").bind(row.artifact_id).bind(project).bind(conversation).bind(&row.permission).execute(&mut *tx).await?;
    let decided = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("update artifact_access_requests set state = 'approved', decided_at = now() where id = $1 returning {REQUEST_COLUMNS}"))).bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let request: ArtifactAccessRequest = decided.into();
    state.publish(
        Some(user.id),
        StreamMessage::ArtifactAccessRequest(request.clone()),
    );
    crate::artifacts::republish(&state, request.artifact_id).await?;
    crate::prompts::tell_agent(
        &state,
        user.id,
        request.conversation_id,
        format!(
            "Artefact access approved: you may {} artefact {}.",
            request.permission, request.artifact_id
        ),
    )
    .await;

    Ok(Json(request))
}

pub async fn decline(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ArtifactAccessRequest>> {
    let row = sqlx::query_as::<_, RequestRow>(sqlx::AssertSqlSafe(format!("update artifact_access_requests set state = 'declined', decided_at = now() where id = $1 and owner = $2 and state = 'requested' returning {REQUEST_COLUMNS}"))).bind(id).bind(user.id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::conflict("request_decided", "This request was not found or has already been decided."))?;
    let request: ArtifactAccessRequest = row.into();
    state.publish(
        Some(user.id),
        StreamMessage::ArtifactAccessRequest(request.clone()),
    );
    crate::prompts::tell_agent(
        &state,
        user.id,
        request.conversation_id,
        format!("Artefact access declined for {}.", request.artifact_id),
    )
    .await;

    Ok(Json(request))
}
