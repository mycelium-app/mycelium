use crate::attachments;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges;
use crate::groups;
use crate::git::relay;
use crate::nodes::node_offline;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow};
use axum::Json;
use axum::extract::{Path, State};
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use mycelium_proto::api::{CheckoutProjectRequest, CreateProjectRequest, PatchProjectRequest, Project, ProjectCheckout};
use mycelium_proto::link::GitCommand;
use mycelium_proto::stream::StreamMessage;
use serde_json::Value;
use uuid::Uuid;

pub const PROJECT_COLUMNS: &str = "id, (select node_id from project_checkouts where project_id = projects.id order by created_at, id limit 1) as node_id, name, (select path from project_checkouts where project_id = projects.id order by created_at, id limit 1) as path, forge_id, repository, group_id, icon, avatar_sha256, test_command, created_at";
pub const CHECKOUT_COLUMNS: &str = "id, project_id, node_id, path, created_at";
pub const MAX_AVATAR_BYTES: usize = 256 * 1024;
const MAX_ICON_CHARACTERS: usize = 40;
const AVATAR_MEDIA_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];
const PROJECTS_FOLDER: &str = "projects";
const FOLDER_OCCUPIED: &str = "already exists and is not empty";

#[derive(Clone, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub node_id: Uuid,
    pub name: String,
    pub path: String,
    pub forge_id: Option<Uuid>,
    pub repository: Option<String>,
    pub group_id: Option<Uuid>,
    pub icon: Option<String>,
    pub avatar_sha256: Option<String>,
    pub test_command: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(skip)]
    pub checkouts: Vec<CheckoutRow>,
}

#[derive(Clone, sqlx::FromRow)]
pub struct CheckoutRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub node_id: Uuid,
    pub path: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl CheckoutRow {
    pub fn to_proto(self) -> ProjectCheckout {
        ProjectCheckout { id: self.id, node_id: self.node_id, path: self.path, created_at: self.created_at }
    }
}

impl ProjectRow {
    pub fn to_proto(self) -> Project {
        let checkouts = self.checkouts.into_iter().map(CheckoutRow::to_proto).collect();

        Project { id: self.id, node_id: self.node_id, name: self.name, path: self.path, forge_id: self.forge_id, repository: self.repository, group_id: self.group_id, icon: self.icon, avatar_sha256: self.avatar_sha256, test_command: self.test_command, checkouts, created_at: self.created_at }
    }

    pub fn checkout_on(&self, node_id: Uuid) -> Option<&CheckoutRow> {
        self.checkouts.iter().find(|checkout| checkout.node_id == node_id)
    }

    pub fn oldest_checkout(&self) -> ApiResult<&CheckoutRow> {
        self.checkout_on(self.node_id).ok_or_else(|| ApiError::internal("This project has no checkout. Add one or remove the project."))
    }

    pub fn checkout_nodes(&self) -> Vec<Uuid> {
        self.checkouts.iter().map(|checkout| checkout.node_id).collect()
    }
}

pub fn not_on_node() -> ApiError {
    ApiError::bad_request("project_not_on_node", "This project is not set up on that node. Choose a node it is set up on.")
}

pub async fn with_checkouts(state: &AppState, mut rows: Vec<ProjectRow>) -> ApiResult<Vec<ProjectRow>> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let checkouts = sqlx::query_as::<_, CheckoutRow>(sqlx::AssertSqlSafe(format!("select {CHECKOUT_COLUMNS} from project_checkouts where project_id = any($1) order by created_at, id"))).bind(&ids).fetch_all(&state.pool).await?;

    for row in &mut rows {
        row.checkouts = checkouts.iter().filter(|checkout| checkout.project_id == row.id).cloned().collect();
    }

    Ok(rows)
}

pub async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProjectRow> {
    let row = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("select {PROJECT_COLUMNS} from projects where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;
    let row = row.ok_or_else(|| ApiError::not_found("That project"))?;

    Ok(with_checkouts(state, vec![row]).await?.remove(0))
}

pub async fn add_checkout(state: &AppState, project_id: Uuid, node_id: Uuid, path: &str) -> ApiResult<CheckoutRow> {
    let inserted = sqlx::query_as::<_, CheckoutRow>(sqlx::AssertSqlSafe(format!("insert into project_checkouts (project_id, node_id, path) values ($1, $2, $3) returning {CHECKOUT_COLUMNS}")))
        .bind(project_id)
        .bind(node_id)
        .bind(path)
        .fetch_one(&state.pool)
        .await;

    match inserted {
        Ok(checkout) => Ok(checkout),
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(ApiError::conflict("project_exists", "This folder is already a project on this node.")),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn clone_repository(state: &AppState, forge: &forges::ForgeRow, repository: &str, node_id: Uuid, folder: Option<&str>) -> ApiResult<String> {
    let credentials = forge.git_credentials()?;
    let url = format!("{}/{repository}.git", forge.base_url);
    let target = folder.map_or_else(|| format!("{PROJECTS_FOLDER}/{repository}"), str::to_string);
    let cloned = relay(state, node_id, target.clone(), GitCommand::Clone { url, credentials: Some(credentials) }).await;

    match cloned {
        Ok(answer) => Ok(answer["path"].as_str().unwrap_or_default().to_string()),
        Err(error) if error.message().contains(FOLDER_OCCUPIED) => {
            let answer = relay(state, node_id, target, GitCommand::Register).await?;
            let registered = answer["path"].as_str().unwrap_or_default().to_string();
            let clone_urls = forges::ForgeApi::from_row(forge)?.clone_urls(&state.http, repository).await;

            check_checkout(&answer, &registered, &forge.base_url, &clone_urls, repository)?;

            Ok(registered)
        }
        Err(error) => Err(error),
    }
}

async fn create_folder(state: &AppState, node_id: Uuid, name: &str) -> ApiResult<String> {
    let answer = relay(state, node_id, PROJECTS_FOLDER.to_string(), GitCommand::CreateDirectory { name: name.to_string() }).await?;

    Ok(answer["path"].as_str().unwrap_or_default().to_string())
}

fn remote_location(url: &str) -> Option<(String, String)> {
    let (authority, path) = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').unwrap_or((rest, "")),
        None => url.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?.split(':').next()?.to_lowercase();
    let path = path.trim_matches('/').trim_end_matches(".git").to_string();

    (!host.is_empty()).then_some((host, path))
}

fn check_checkout(answer: &Value, registered: &str, base_url: &str, clone_urls: &[String], repository: &str) -> ApiResult<()> {
    let choose = format!("Choose the folder that holds {repository}.");

    if answer["repository"] != true {
        return Err(ApiError::bad_request("folder_not_checkout", format!("The folder at {registered} is not a git checkout. {choose}")));
    }

    let forge_host = remote_location(base_url).map(|(host, _)| host).unwrap_or_default();
    let known = match answer["resolved_remotes"].is_array() {
        true => &answer["resolved_remotes"],
        false => &answer["remotes"],
    };
    let remotes: Vec<(String, String)> = known.as_array().into_iter().flatten().filter_map(|remote| remote.as_str().and_then(remote_location)).collect();

    let clones: Vec<(String, String)> = clone_urls.iter().filter_map(|url| remote_location(url)).collect();
    let on_forge = |host: &str| host == forge_host || clones.iter().any(|(clone_host, _)| clone_host == host);

    if remotes.iter().any(|(host, path)| on_forge(host) && path.eq_ignore_ascii_case(repository)) {
        return Ok(());
    }

    match remotes.iter().find(|(host, _)| on_forge(host)) {
        Some((_, other)) => Err(ApiError::bad_request("folder_other_repository", format!("The folder at {registered} is a checkout of {other}, not {repository}. {choose}"))),
        None => Err(ApiError::bad_request("folder_other_repository", format!("The folder at {registered} has no remote on {forge_host}, so Mycelium cannot tell it holds {repository}. {choose}"))),
    }
}

async fn link_conversations(state: &AppState, owner: Uuid, project_id: Uuid, node_id: Uuid) -> ApiResult<()> {
    let linked = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set project_id = $3 where owner = $1 and node_id = $2 and project_id is null and project_for_folder(owner, node_id, cwd) = $3 returning {CONVERSATION_COLUMNS}",
    )))
    .bind(owner)
    .bind(node_id)
    .bind(project_id)
    .fetch_all(&state.pool)
    .await?;

    for conversation in linked {
        state.publish(Some(owner), StreamMessage::Conversation(conversation.to_proto()?));
    }

    Ok(())
}

pub(crate) fn valid_icon(icon: &str) -> bool {
    icon.len() <= MAX_ICON_CHARACTERS && icon.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
}

pub fn valid_repository(repository: &str) -> bool {
    let segments: Vec<&str> = repository.split('/').collect();

    segments.len() >= 2 && segments.iter().all(|segment| !segment.is_empty() && *segment != "." && *segment != ".." && segment.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<Project>>> {
    let rows = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("select {PROJECT_COLUMNS} from projects where owner = $1 order by created_at, id"))).bind(user.id).fetch_all(&state.pool).await?;
    let rows = with_checkouts(&state, rows).await?;

    Ok(Json(rows.into_iter().map(ProjectRow::to_proto).collect()))
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateProjectRequest>) -> ApiResult<(StatusCode, Json<Project>)> {
    tokio::spawn(create_project(state, user, request)).await.map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "project_create_aborted", "Setting up the project stopped unexpectedly. Check the project list, then try again."))?
}

async fn create_project(state: AppState, user: AuthUser, request: CreateProjectRequest) -> ApiResult<(StatusCode, Json<Project>)> {
    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(request.node_id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    if !state.hub.is_online(request.node_id) {
        return Err(node_offline());
    }

    let new_folder = request.new_folder.as_deref();

    let (name, path, forge_id, repository) = match (request.forge_id, request.repository.as_deref().map(str::trim), request.path.as_deref().map(str::trim)) {
        (Some(forge_id), Some(repository), Some(path)) if !path.is_empty() && !request.new_repository && new_folder.is_none() => {
            if !valid_repository(repository) {
                return Err(ApiError::bad_request("repository_invalid", "Enter the repository as owner/name, for example acme-org/acme-backups."));
            }

            let forge = forges::find_owned(&state, forge_id, user.id).await?;
            let answer = relay(&state, request.node_id, path.to_string(), GitCommand::Register).await?;
            let registered = answer["path"].as_str().unwrap_or_default().to_string();

            let clone_urls = forges::ForgeApi::from_row(&forge)?.clone_urls(&state.http, repository).await;

            check_checkout(&answer, &registered, &forge.base_url, &clone_urls, repository)?;

            let name = repository.rsplit('/').next().unwrap_or(repository).to_string();

            (name, registered, Some(forge_id), Some(repository.to_string()))
        }
        (Some(forge_id), Some(repository), None) => {
            if !valid_repository(repository) {
                return Err(ApiError::bad_request("repository_invalid", "Enter the repository as owner/name, for example acme-org/acme-backups."));
            }

            let forge = forges::find_owned(&state, forge_id, user.id).await?;

            if request.new_repository {
                let api = forges::ForgeApi::from_row(&forge)?.cached(&state.forge_cache);

                api.create_repository(&state.http, &forge.account, repository).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;
            }

            let folder = match new_folder {
                Some(name) => Some(create_folder(&state, request.node_id, name).await?),
                None => None,
            };
            let path = clone_repository(&state, &forge, repository, request.node_id, folder.as_deref()).await?;
            let name = repository.rsplit('/').next().unwrap_or(repository).to_string();

            (name, path, Some(forge_id), Some(repository.to_string()))
        }
        (None, None, path) if path.is_some_and(|path| !path.is_empty()) || new_folder.is_some() => {
            let path = match new_folder {
                Some(name) => create_folder(&state, request.node_id, name).await?,
                None => path.unwrap_or_default().to_string(),
            };
            let answer = relay(&state, request.node_id, path, GitCommand::Register).await?;
            let registered = answer["path"].as_str().unwrap_or_default().to_string();
            let name = registered.rsplit('/').next().unwrap_or(&registered).to_string();

            (name, registered, None, None)
        }
        _ => return Err(ApiError::bad_request("project_source_required", "Choose a forge repository or enter the folder on the node.")),
    };

    let inserted = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path, forge_id, repository, group_id) values ($1, $2, $3, $4, $5, $6, $7) returning id")
    .bind(user.id)
    .bind(request.node_id)
    .bind(&name)
    .bind(&path)
    .bind(forge_id)
    .bind(&repository)
    .bind(request.group_id)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok(project_id) => {
            link_conversations(&state, user.id, project_id, request.node_id).await?;
            let owner = user.id;
            let project = find_owned(&state, project_id, owner).await?.to_proto();
            state.publish(Some(owner), StreamMessage::Project(project.clone()));
            let background = state.clone();
            tokio::spawn(async move { find_automatic_icon(&background, owner, project_id).await });

            Ok((StatusCode::CREATED, Json(project)))
        }
        Err(sqlx::Error::Database(cause)) if cause.is_unique_violation() => Err(ApiError::conflict("project_exists", "This folder is already a project on this node.")),
        Err(cause) => Err(cause.into()),
    }
}

pub async fn checkout(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<CheckoutProjectRequest>) -> ApiResult<(StatusCode, Json<Project>)> {
    let project = find_owned(&state, id, user.id).await?;

    if project.checkout_on(request.node_id).is_some() {
        return Ok((StatusCode::OK, Json(project.to_proto())));
    }

    let (Some(forge_id), Some(repository)) = (project.forge_id, project.repository.clone()) else {
        return Err(ApiError::bad_request("project_not_cloneable", "This project has no forge repository to clone. Register its folder on that node instead."));
    };

    let sibling = sqlx::query_scalar::<_, Uuid>("select p.id from projects p join project_checkouts c on c.project_id = p.id where p.owner = $1 and c.node_id = $2 and p.forge_id = $3 and lower(p.repository) = lower($4) order by p.created_at, p.id limit 1")
        .bind(user.id)
        .bind(request.node_id)
        .bind(forge_id)
        .bind(&repository)
        .fetch_optional(&state.pool)
        .await?;

    if let Some(sibling) = sibling {
        return Ok((StatusCode::OK, Json(find_owned(&state, sibling, user.id).await?.to_proto())));
    }

    create_project(state, user, CreateProjectRequest { node_id: request.node_id, forge_id: Some(forge_id), repository: Some(repository), path: None, new_repository: false, new_folder: None, group_id: None }).await
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchProjectRequest>) -> ApiResult<Json<Project>> {
    let changes_icon = request.icon.is_some() || request.clear_icon;

    if let Some(group_id) = request.group_id {
        groups::find_owned(&state, group_id, user.id).await?;
    }

    let icon = request.icon.as_deref().map(str::trim).filter(|icon| !request.clear_icon && !icon.is_empty());

    if icon.is_some_and(|icon| !valid_icon(icon)) {
        return Err(ApiError::bad_request("icon_invalid", "Choose an icon from the list."));
    }

    let row = if let Some(test_command) = request.test_command.as_deref() {
        sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set test_command = nullif(btrim($3), '') where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
            .bind(id)
            .bind(user.id)
            .bind(test_command)
            .fetch_optional(&state.pool)
            .await?
    } else if changes_icon {
        sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set icon = $3, avatar_sha256 = null, avatar_automatic = false where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
            .bind(id)
            .bind(user.id)
            .bind(icon)
            .fetch_optional(&state.pool)
            .await?
    } else {
        sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("update projects set group_id = $3 where id = $1 and owner = $2 returning {PROJECT_COLUMNS}")))
            .bind(id)
            .bind(user.id)
            .bind(request.group_id)
            .fetch_optional(&state.pool)
            .await?
    };

    let Some(row) = row else {
        return Err(ApiError::not_found("That project"));
    };

    publish_project(&state, user.id, id).await?;
    let row = with_checkouts(&state, vec![row]).await?.remove(0);

    Ok(Json(row.to_proto()))
}

pub async fn set_avatar(state: &AppState, owner: Uuid, id: Uuid, media_type: &str, body: &[u8]) -> ApiResult<Project> {
    find_owned(state, id, owner).await?;
    check_avatar(media_type, body)?;

    let row = store_avatar(state, owner, id, media_type, body, false).await?.ok_or_else(|| ApiError::not_found("That project"))?;

    publish_project(state, owner, id).await?;
    let row = with_checkouts(state, vec![row]).await?.remove(0);

    Ok(row.to_proto())
}

pub(crate) fn check_avatar(media_type: &str, body: &[u8]) -> ApiResult<()> {
    if !AVATAR_MEDIA_TYPES.contains(&media_type) {
        return Err(ApiError::bad_request("avatar_not_image", "The logo must be a PNG, JPEG or WebP image. Choose another file."));
    }

    if body.is_empty() {
        return Err(ApiError::bad_request("avatar_empty", "The file is empty. Choose an image that has content."));
    }

    if body.len() > MAX_AVATAR_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "avatar_too_large", "The logo is larger than 256 KB. Choose a smaller image."));
    }

    Ok(())
}

pub async fn upload_avatar(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, headers: HeaderMap, body: Bytes) -> ApiResult<Json<Project>> {
    let media_type = attachments::media_type_of(&headers);

    Ok(Json(set_avatar(&state, user.id, id, &media_type, &body).await?))
}

async fn store_avatar(state: &AppState, owner: Uuid, id: Uuid, media_type: &str, body: &[u8], automatic: bool) -> ApiResult<Option<ProjectRow>> {
    let (sha256, _, _) = attachments::store(state, owner, body, media_type).await?;
    let row = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!(
        "update projects set avatar_sha256 = $3, icon = null, avatar_automatic = $4 where id = $1 and owner = $2 and (not $4 or avatar_automatic or (icon is null and avatar_sha256 is null)) returning {PROJECT_COLUMNS}",
    )))
    .bind(id)
    .bind(owner)
    .bind(&sha256)
    .bind(automatic)
    .fetch_optional(&state.pool)
    .await?;

    Ok(row)
}

pub async fn find_icon(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Project>> {
    find_owned(&state, id, user.id).await?;
    find_automatic_icon(&state, user.id, id).await;

    Ok(Json(find_owned(&state, id, user.id).await?.to_proto()))
}

pub async fn find_automatic_icon(state: &AppState, owner: Uuid, id: Uuid) {
    use base64::Engine;

    let wanting = sqlx::query_as::<_, (Uuid, String, String)>("select c.node_id, c.path, n.version from projects p join project_checkouts c on c.project_id = p.id join nodes n on n.id = c.node_id where p.id = $1 and p.owner = $2 and (p.avatar_automatic or (p.icon is null and p.avatar_sha256 is null)) order by c.created_at, c.id").bind(id).bind(owner).fetch_all(&state.pool).await;
    let Ok(wanting) = wanting else {
        return;
    };
    let reachable = wanting.into_iter().find(|(node_id, _, version)| {
        let supported = semver::Version::parse(version.trim_start_matches('v')).is_ok_and(|version| version >= semver::Version::new(0, 9, 0));

        supported && state.hub.is_online(*node_id)
    });
    let Some((node_id, path, _)) = reachable else {
        return;
    };

    let Ok(answer) = relay(state, node_id, path, GitCommand::FindLogo).await else {
        return;
    };
    let (Some(media_type), Some(data)) = (answer["media_type"].as_str(), answer["data"].as_str()) else {
        return;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        return;
    };

    if !AVATAR_MEDIA_TYPES.contains(&media_type) || bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return;
    }

    if let Ok(Some(_)) = store_avatar(state, owner, id, media_type, &bytes, true).await {
        let _ = publish_project(state, owner, id).await;
    }
}

async fn publish_project(state: &AppState, owner: Uuid, project_id: Uuid) -> ApiResult<()> {
    let project = find_owned(state, project_id, owner).await?;
    state.publish(Some(owner), StreamMessage::Project(project.to_proto()));

    let conversations = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where project_id = $1 and owner = $2")))
        .bind(project_id)
        .bind(owner)
        .fetch_all(&state.pool)
        .await?;

    for conversation in conversations {
        state.publish(Some(owner), StreamMessage::Conversation(conversation.to_proto()?));
    }

    Ok(())
}
