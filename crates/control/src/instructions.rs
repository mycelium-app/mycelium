use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::forges::{self, ForgeApi};
use crate::groups::{self, GroupRow};
use crate::projects::valid_repository;
use crate::state::AppState;
use crate::store::publish_node;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::api::{InstructionLayer, InstructionSource, InstructionVersion, Instructions, SaveInstructionsRequest, SetInstructionSourceRequest};
use mycelium_proto::link::ControlToNode;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const GLOBAL: &str = "global";
const DEFAULT_PATH: &str = "AGENTS.md";

#[derive(sqlx::FromRow)]
struct SourceRow {
    forge_id: Uuid,
    repository: String,
    path: String,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: String,
    text: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

struct Layer {
    key: String,
    name: String,
    group_id: Option<Uuid>,
}

impl Layer {
    fn global() -> Layer {
        Layer { key: GLOBAL.to_string(), name: "Global".to_string(), group_id: None }
    }

    fn of_group(group: GroupRow) -> Layer {
        Layer { key: group.id.to_string(), name: group.name, group_id: Some(group.id) }
    }

    fn path(&self, global_path: &str) -> String {
        if self.group_id.is_none() {
            return global_path.to_string();
        }

        let (directory, file) = global_path.rsplit_once('/').map(|(directory, file)| (format!("{directory}/"), file)).unwrap_or((String::new(), global_path));

        match file.rsplit_once('.') {
            Some((stem, extension)) => format!("{directory}{stem}.{}.{extension}", self.name),
            None => format!("{directory}{file}.{}", self.name),
        }
    }
}

async fn source(state: &AppState) -> ApiResult<Option<SourceRow>> {
    Ok(sqlx::query_as::<_, SourceRow>("select forge_id, repository, path from instruction_source").fetch_optional(&state.pool).await?)
}

async fn latest(state: &AppState, layer: &str) -> ApiResult<Option<VersionRow>> {
    Ok(sqlx::query_as::<_, VersionRow>("select id, text, created_at from instruction_versions where layer = $1 order by seq desc limit 1").bind(layer).fetch_optional(&state.pool).await?)
}

pub async fn current_revision(state: &AppState) -> ApiResult<Option<String>> {
    Ok(sqlx::query_scalar::<_, String>("select id from instruction_versions order by seq desc limit 1").fetch_optional(&state.pool).await?)
}

async fn layers_of(state: &AppState, user: &AuthUser) -> ApiResult<Vec<Layer>> {
    let groups = sqlx::query_as::<_, GroupRow>("select id, name, created_at from groups where owner = $1 order by lower(name), id").bind(user.id).fetch_all(&state.pool).await?;
    let mut layers = vec![Layer::global()];
    layers.extend(groups.into_iter().map(Layer::of_group));

    Ok(layers)
}

async fn find_layer(state: &AppState, user: &AuthUser, key: &str) -> ApiResult<Layer> {
    if key == GLOBAL {
        return Ok(Layer::global());
    }

    let id = key.parse::<Uuid>().map_err(|_| ApiError::not_found("That instructions layer"))?;

    Ok(Layer::of_group(groups::find_owned(state, id, user.id).await?))
}

async fn describe(state: &AppState, user: &AuthUser) -> ApiResult<Instructions> {
    let source = source(state).await?;
    let mut layers = Vec::new();

    for layer in layers_of(state, user).await? {
        let version = latest(state, &layer.key).await?;

        layers.push(InstructionLayer {
            path: source.as_ref().map(|source| layer.path(&source.path)),
            text: version.as_ref().map(|version| version.text.clone()).unwrap_or_default(),
            version: version.map(|version| InstructionVersion { id: version.id, created_at: version.created_at }),
            layer: layer.key,
            name: layer.name,
        });
    }

    Ok(Instructions {
        source: source.map(|source| InstructionSource { forge_id: source.forge_id, repository: source.repository, path: source.path }),
        revision: current_revision(state).await?,
        layers,
    })
}

async fn forge_for(state: &AppState, user: &AuthUser, source: &SourceRow) -> ApiResult<ForgeApi> {
    let forge = forges::find_owned(state, source.forge_id, user.id).await?;

    ForgeApi::from_row(&forge)
}

async fn record(state: &AppState, user: &AuthUser, layer: &Layer, text: &str, forge_commit: Option<String>) -> ApiResult<()> {
    let previous = latest(state, &layer.key).await?.map(|version| version.id).unwrap_or_default();
    let id = forge_commit.unwrap_or_else(|| {
        let digest = Sha256::digest(format!("{}\0{previous}\0{text}\0{}", layer.key, chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()).as_bytes());

        digest.iter().take(20).map(|byte| format!("{byte:02x}")).collect()
    });

    sqlx::query("insert into instruction_versions (id, layer, group_id, text, author) values ($1, $2, $3, $4, $5) on conflict (id) do nothing")
        .bind(id)
        .bind(&layer.key)
        .bind(layer.group_id)
        .bind(text)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok(())
}

pub async fn push_to(state: &AppState, node_id: Uuid) -> ApiResult<()> {
    let Some(revision) = current_revision(state).await? else {
        return Ok(());
    };
    let global = latest(state, GLOBAL).await?.map(|version| version.text);

    state.hub.send(node_id, ControlToNode::Instructions { revision, global });

    Ok(())
}

async fn push_to_every_node(state: &AppState) -> ApiResult<()> {
    let nodes = sqlx::query_scalar::<_, Uuid>("select id from nodes where revoked_at is null").fetch_all(&state.pool).await?;

    for node_id in nodes {
        push_to(state, node_id).await?;
        publish_node(state, node_id).await?;
    }

    Ok(())
}

pub async fn session_layer(state: &AppState, conversation_id: Uuid) -> ApiResult<Option<String>> {
    let revision = current_revision(state).await?;

    sqlx::query("update conversations set instructions_revision = $2 where id = $1").bind(conversation_id).bind(revision).execute(&state.pool).await?;

    let text = sqlx::query_scalar::<_, String>(
        "select v.text from conversations c join projects p on p.id = c.project_id
         join instruction_versions v on v.layer = p.group_id::text
         where c.id = $1 order by v.seq desc limit 1",
    )
    .bind(conversation_id)
    .fetch_optional(&state.pool)
    .await?;

    Ok(text.filter(|text| !text.trim().is_empty()))
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Instructions>> {
    Ok(Json(describe(&state, &user).await?))
}

pub async fn save(State(state): State<AppState>, user: AuthUser, Path(key): Path<String>, Json(request): Json<SaveInstructionsRequest>) -> ApiResult<Json<InstructionLayer>> {
    let layer = find_layer(&state, &user, &key).await?;
    let unchanged = latest(&state, &layer.key).await?.is_some_and(|version| version.text == request.text);

    if !unchanged {
        let source = source(&state).await?;
        let commit = match &source {
            Some(source) => {
                let api = forge_for(&state, &user, source).await?;
                let path = layer.path(&source.path);
                let existing = api.file(&state.http, &source.repository, &path).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;
                let message = format!("docs: update {path}");

                Some(api.write_file(&state.http, &source.repository, &path, &request.text, &message, existing.as_ref()).await.map_err(|error| error.into_api(api.kind, &api.base_url))?)
            }
            None => None,
        };

        record(&state, &user, &layer, &request.text, commit).await?;
        push_to_every_node(&state).await?;
    }

    let described = describe(&state, &user).await?;

    described.layers.into_iter().find(|candidate| candidate.layer == layer.key).map(Json).ok_or_else(|| ApiError::not_found("That instructions layer"))
}

pub async fn pull(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Instructions>> {
    let Some(source) = source(&state).await? else {
        return Err(ApiError::conflict("no_source", "Connect a repository for the instructions before pulling."));
    };
    let api = forge_for(&state, &user, &source).await?;

    for layer in layers_of(&state, &user).await? {
        let file = api.file(&state.http, &source.repository, &layer.path(&source.path)).await.map_err(|error| error.into_api(api.kind, &api.base_url))?;
        let Some(file) = file else {
            continue;
        };
        let current = latest(&state, &layer.key).await?;

        if current.is_none_or(|current| current.text != file.text) {
            record(&state, &user, &layer, &file.text, Some(file.commit)).await?;
        }
    }

    push_to_every_node(&state).await?;

    Ok(Json(describe(&state, &user).await?))
}

pub async fn set_source(State(state): State<AppState>, user: AuthUser, Json(request): Json<SetInstructionSourceRequest>) -> ApiResult<Json<Instructions>> {
    let repository = request.repository.trim();
    let path = request.path.as_deref().map(str::trim).filter(|path| !path.is_empty()).unwrap_or(DEFAULT_PATH);

    if !valid_repository(repository) {
        return Err(ApiError::bad_request("repository_invalid", "Enter the repository as owner/name, for example acme-org/agents-md."));
    }

    if path.starts_with('/') || path.ends_with('/') || path.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..") {
        return Err(ApiError::bad_request("path_invalid", "Enter the path of the file in the repository, for example AGENTS.md."));
    }

    forges::find_owned(&state, request.forge_id, user.id).await?;

    sqlx::query(
        "insert into instruction_source (forge_id, repository, path) values ($1, $2, $3)
         on conflict (singleton) do update set forge_id = excluded.forge_id, repository = excluded.repository, path = excluded.path",
    )
    .bind(request.forge_id)
    .bind(repository)
    .bind(path)
    .execute(&state.pool)
    .await?;

    Ok(Json(describe(&state, &user).await?))
}

pub async fn clear_source(State(state): State<AppState>, _user: AuthUser) -> ApiResult<StatusCode> {
    sqlx::query("delete from instruction_source").execute(&state.pool).await?;

    Ok(StatusCode::NO_CONTENT)
}
