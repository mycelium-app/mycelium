use crate::attachments::{self, MAX_ATTACHMENT_BYTES};
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::git::relay;
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use mycelium_proto::api::{
    AddArtifactVersionRequest, Artifact, ArtifactAuthor, ArtifactComment, ArtifactDataEntry, ArtifactKind, ArtifactVersion, CreateArtifactCommentRequest, CreateArtifactRequest, HostedArtifact, PatchArtifactCommentRequest, PatchArtifactRequest,
};
use mycelium_proto::link::GitCommand;
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_REQUEST_BYTES: usize = MAX_ATTACHMENT_BYTES * 2;
const DESIGN_MEDIA_TYPE: &str = "application/vnd.mycelium.design+json";
const FALLBACK_MEDIA_TYPE: &str = "application/octet-stream";
const MOST_TITLE_CHARACTERS: usize = 200;
const MOST_SLUG_CHARACTERS: usize = 40;
const MOST_COMMENT_CHARACTERS: usize = 10_000;
const MOST_AUTHOR_CHARACTERS: usize = 100;
const MOST_KEY_CHARACTERS: usize = 200;
const MOST_DATA_BYTES: usize = 1024 * 1024;
const MOST_READ_CHARACTERS: usize = 100_000;
const ARTBOARD_GAP: i64 = 80;
const ARTBOARD_WIDTH: i64 = 1440;
const ARTBOARD_HEIGHT: i64 = 900;
const COLUMNS: &str = "id, owner, conversation_id, kind, title, slug, public, latest_version, created_at, updated_at";
const COMMENT_COLUMNS: &str = "id, artifact_id, author_name, body, anchor, resolved, created_at";

#[derive(sqlx::FromRow)]
struct ArtifactRow {
    id: Uuid,
    owner: Uuid,
    conversation_id: Option<Uuid>,
    kind: String,
    title: String,
    slug: String,
    public: bool,
    latest_version: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    version: i32,
    author: String,
    media_type: String,
    sha256: String,
    note: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct CommentRow {
    id: Uuid,
    artifact_id: Uuid,
    author_name: String,
    body: String,
    anchor: Option<Value>,
    resolved: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct DataRow {
    key: String,
    value: Value,
    updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct NewArtifact<'a> {
    pub owner: Uuid,
    pub conversation_id: Option<Uuid>,
    pub author: ArtifactAuthor,
    pub kind: ArtifactKind,
    pub title: &'a str,
    pub content: Option<Vec<u8>>,
    pub media_type: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    kind: Option<String>,
    q: Option<String>,
}

pub struct ArtifactHost;

impl FromRequestParts<AppState> for ArtifactHost {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<ArtifactHost, ApiError> {
        let Some(expected) = &state.config.artifact_host_token else {
            return Err(ApiError::not_found("That address"));
        };
        let given = parts.headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer "));

        match given {
            Some(token) if Sha256::digest(token.as_bytes()) == Sha256::digest(expected.as_bytes()) => Ok(ArtifactHost),
            _ => Err(ApiError::unauthorised()),
        }
    }
}

fn kind_text(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Design => "design",
        ArtifactKind::Page => "page",
        ArtifactKind::Markdown => "markdown",
        ArtifactKind::File => "file",
    }
}

fn parse_kind(text: &str) -> ApiResult<ArtifactKind> {
    match text.trim() {
        "design" => Ok(ArtifactKind::Design),
        "page" => Ok(ArtifactKind::Page),
        "markdown" => Ok(ArtifactKind::Markdown),
        "file" => Ok(ArtifactKind::File),
        _ => Err(ApiError::bad_request("kind_invalid", "Kind must be design, page, markdown or file.")),
    }
}

fn author_text(author: ArtifactAuthor) -> &'static str {
    match author {
        ArtifactAuthor::Agent => "agent",
        ArtifactAuthor::Person => "person",
    }
}

fn parse_author(text: &str) -> ApiResult<ArtifactAuthor> {
    match text {
        "agent" => Ok(ArtifactAuthor::Agent),
        "person" => Ok(ArtifactAuthor::Person),
        other => Err(ApiError::internal(format!("unknown artifact author {other}"))),
    }
}

impl ArtifactRow {
    fn to_proto(self, domain: Option<&str>, versions: Vec<ArtifactVersion>) -> ApiResult<Artifact> {
        Ok(Artifact {
            id: self.id,
            owner: self.owner,
            conversation_id: self.conversation_id,
            kind: parse_kind(&self.kind).map_err(|_| ApiError::internal(format!("unknown artifact kind {}", self.kind)))?,
            url: domain.map(|domain| format!("https://{}.{domain}", self.slug)),
            title: self.title,
            slug: self.slug,
            public: self.public,
            latest_version: self.latest_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            versions,
            deleted: false,
        })
    }
}

impl VersionRow {
    fn to_proto(self) -> ApiResult<ArtifactVersion> {
        Ok(ArtifactVersion { version: self.version, author: parse_author(&self.author)?, media_type: self.media_type, sha256: self.sha256, note: self.note, created_at: self.created_at })
    }
}

impl CommentRow {
    fn to_proto(self) -> ArtifactComment {
        ArtifactComment { id: self.id, artifact_id: self.artifact_id, author_name: self.author_name, body: self.body, anchor: self.anchor, resolved: self.resolved, created_at: self.created_at }
    }
}

impl DataRow {
    fn to_proto(self) -> ArtifactDataEntry {
        ArtifactDataEntry { key: self.key, value: self.value, updated_at: self.updated_at }
    }
}

fn announce(state: &AppState, artifact: &Artifact) {
    state.publish(Some(artifact.owner), StreamMessage::Artifact(artifact.clone()));
}

async fn owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ArtifactRow> {
    let row = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from artifacts where id = $1 and owner = $2"))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That artifact"))
}

async fn with_slug(state: &AppState, slug: &str) -> ApiResult<ArtifactRow> {
    let row = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!("select {COLUMNS} from artifacts where slug = $1"))).bind(slug).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That artifact"))
}

async fn versions_of(state: &AppState, id: Uuid) -> ApiResult<Vec<ArtifactVersion>> {
    let rows = sqlx::query_as::<_, VersionRow>("select version, author, media_type, sha256, note, created_at from artifact_versions where artifact_id = $1 order by version desc").bind(id).fetch_all(&state.pool).await?;

    rows.into_iter().map(VersionRow::to_proto).collect()
}

async fn latest_version_of(state: &AppState, row: &ArtifactRow) -> ApiResult<VersionRow> {
    let version = sqlx::query_as::<_, VersionRow>("select version, author, media_type, sha256, note, created_at from artifact_versions where artifact_id = $1 and version = $2").bind(row.id).bind(row.latest_version).fetch_optional(&state.pool).await?;

    version.ok_or_else(|| ApiError::not_found("That version"))
}

async fn latest_content(state: &AppState, row: &ArtifactRow) -> ApiResult<(String, Vec<u8>)> {
    let version = latest_version_of(state, row).await?;
    let bytes = tokio::fs::read(attachments::path_of(state, &version.sha256)).await.map_err(|_| ApiError::not_found("That content"))?;

    Ok((version.media_type, bytes))
}

async fn full(state: &AppState, row: ArtifactRow) -> ApiResult<Artifact> {
    let versions = versions_of(state, row.id).await?;

    row.to_proto(state.config.artifact_domain.as_deref(), versions)
}

fn decode(content: Option<String>, content_base64: Option<String>) -> ApiResult<Option<Vec<u8>>> {
    match (content, content_base64) {
        (Some(_), Some(_)) => Err(ApiError::bad_request("content_ambiguous", "Send either content or content_base64, not both.")),
        (Some(text), None) => Ok(Some(text.into_bytes())),
        (None, Some(encoded)) => {
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|_| ApiError::bad_request("content_invalid", "content_base64 is not valid base64. Encode the file again."))?;

            Ok(Some(bytes))
        }
        (None, None) => Ok(None),
    }
}

fn parse_design(bytes: &[u8]) -> ApiResult<Value> {
    let invalid = || ApiError::bad_request("design_invalid", "The design must be JSON with a pages list, and each page needs an id and an items list.");
    let design: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let well_formed = design["pages"].as_array().is_some_and(|pages| pages.iter().all(|page| page["id"].is_string() && page["items"].is_array()));

    if !well_formed {
        return Err(invalid());
    }

    Ok(design)
}

fn check_content(kind: ArtifactKind, bytes: &[u8]) -> ApiResult<()> {
    if bytes.is_empty() {
        return Err(ApiError::bad_request("artifact_empty", "The artifact has no content. Add some, then try again."));
    }

    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "artifact_too_large", "The artifact is larger than 20 MB. Use a smaller file."));
    }

    match kind {
        ArtifactKind::Page | ArtifactKind::Markdown if std::str::from_utf8(bytes).is_err() => Err(ApiError::bad_request("content_not_text", "A page or a document must be text in UTF-8.")),
        ArtifactKind::Design => parse_design(bytes).map(|_| ()),
        _ => Ok(()),
    }
}

fn media_type_of(kind: ArtifactKind, requested: Option<&str>) -> ApiResult<String> {
    match kind {
        ArtifactKind::Design => Ok(DESIGN_MEDIA_TYPE.to_string()),
        ArtifactKind::Page => Ok("text/html".to_string()),
        ArtifactKind::Markdown => Ok("text/markdown".to_string()),
        ArtifactKind::File => {
            let text = requested.map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()).filter(|value| !value.is_empty());

            match text {
                None => Ok(FALLBACK_MEDIA_TYPE.to_string()),
                Some(text) if text.contains('/') && text.len() <= 100 && !text.contains(|character: char| character.is_whitespace() || character.is_control()) => Ok(text),
                Some(_) => Err(ApiError::bad_request("media_type_invalid", "Give the media type as type/subtype, for example application/pdf.")),
            }
        }
    }
}

fn slug_of(title: &str) -> String {
    let mut slug = String::new();

    for character in title.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }

    let limited: String = slug.chars().take(MOST_SLUG_CHARACTERS).collect();
    let trimmed = limited.trim_matches('-');

    if trimmed.is_empty() { "artifact".to_string() } else { trimmed.to_string() }
}

async fn free_slug(state: &AppState, title: &str) -> ApiResult<String> {
    let base = slug_of(title);
    let mut candidate = base.clone();

    loop {
        let taken = sqlx::query_scalar::<_, bool>("select exists (select 1 from artifacts where slug = $1)").bind(&candidate).fetch_one(&state.pool).await?;

        if !taken {
            return Ok(candidate);
        }

        candidate = format!("{base}-{:06x}", rand::random_range(0..0x100_0000u32));
    }
}

fn clean_title(title: &str) -> ApiResult<&str> {
    let title = title.trim();

    if title.is_empty() {
        return Err(ApiError::bad_request("title_required", "Enter a title for the artifact."));
    }

    if title.chars().count() > MOST_TITLE_CHARACTERS {
        return Err(ApiError::bad_request("title_too_long", "Shorten the title to 200 characters or fewer."));
    }

    Ok(title)
}

pub async fn create_one(state: &AppState, new: NewArtifact<'_>) -> ApiResult<Artifact> {
    let title = clean_title(new.title)?;
    let bytes = match (new.content, new.kind) {
        (Some(bytes), _) => bytes,
        (None, ArtifactKind::Design) => json!({"pages": [{"id": "p1", "name": "Page 1", "items": []}]}).to_string().into_bytes(),
        (None, _) => return Err(ApiError::bad_request("content_required", "Give the content of the artifact.")),
    };

    check_content(new.kind, &bytes)?;

    let media_type = media_type_of(new.kind, new.media_type)?;

    if let Some(conversation_id) = new.conversation_id {
        find_owned(&state.pool, conversation_id, new.owner).await?;
    }

    let (sha256, _, _) = attachments::store(state, new.owner, &bytes, &media_type).await?;
    let slug = free_slug(state, title).await?;
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!("insert into artifacts (owner, conversation_id, kind, title, slug) values ($1, $2, $3, $4, $5) returning {COLUMNS}")))
        .bind(new.owner)
        .bind(new.conversation_id)
        .bind(kind_text(new.kind))
        .bind(title)
        .bind(&slug)
        .fetch_one(&mut *tx)
        .await?;

    sqlx::query("insert into artifact_versions (artifact_id, version, author, media_type, sha256) values ($1, 1, $2, $3, $4)").bind(row.id).bind(author_text(new.author)).bind(&media_type).bind(&sha256).execute(&mut *tx).await?;
    tx.commit().await?;

    let artifact = full(state, row).await?;
    announce(state, &artifact);

    Ok(artifact)
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(request): Json<CreateArtifactRequest>) -> ApiResult<(StatusCode, Json<Artifact>)> {
    let kind = parse_kind(&request.kind)?;
    let content = decode(request.content, request.content_base64)?;
    let artifact = create_one(&state, NewArtifact { owner: user.id, conversation_id: request.conversation_id, author: ArtifactAuthor::Person, kind, title: &request.title, content, media_type: request.media_type.as_deref() }).await?;

    Ok((StatusCode::CREATED, Json(artifact)))
}

async fn add_version(state: &AppState, row: &ArtifactRow, author: ArtifactAuthor, bytes: &[u8], note: Option<&str>) -> ApiResult<Artifact> {
    let kind = parse_kind(&row.kind)?;

    check_content(kind, bytes)?;

    let media_type = latest_version_of(state, row).await?.media_type;
    let (sha256, _, _) = attachments::store(state, row.owner, bytes, &media_type).await?;
    let note = note.map(str::trim).filter(|note| !note.is_empty()).map(crate::store::text_without_nul);
    let mut tx = state.pool.begin().await?;
    let latest = sqlx::query_scalar::<_, i32>("select latest_version from artifacts where id = $1 for update").bind(row.id).fetch_one(&mut *tx).await?;
    let version = latest + 1;

    sqlx::query("insert into artifact_versions (artifact_id, version, author, media_type, sha256, note) values ($1, $2, $3, $4, $5, $6)")
        .bind(row.id)
        .bind(version)
        .bind(author_text(author))
        .bind(&media_type)
        .bind(&sha256)
        .bind(note)
        .execute(&mut *tx)
        .await?;

    let updated = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!("update artifacts set latest_version = $2, updated_at = now() where id = $1 returning {COLUMNS}"))).bind(row.id).bind(version).fetch_one(&mut *tx).await?;

    tx.commit().await?;

    let artifact = full(state, updated).await?;
    announce(state, &artifact);

    Ok(artifact)
}

pub async fn create_version(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<AddArtifactVersionRequest>) -> ApiResult<(StatusCode, Json<Artifact>)> {
    let row = owned(&state, id, user.id).await?;
    let content = decode(request.content, request.content_base64)?.ok_or_else(|| ApiError::bad_request("content_required", "Give the content of the new version."))?;
    let artifact = add_version(&state, &row, ArtifactAuthor::Person, &content, request.note.as_deref()).await?;

    Ok((StatusCode::CREATED, Json(artifact)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser, Query(query): Query<ListQuery>) -> ApiResult<Json<Vec<Artifact>>> {
    let kind = match query.kind.as_deref().map(str::trim).filter(|kind| !kind.is_empty()) {
        Some(text) => Some(kind_text(parse_kind(text)?)),
        None => None,
    };
    let words = query.q.as_deref().map(str::trim).filter(|words| !words.is_empty());
    let rows = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!(
        "select {COLUMNS} from artifacts
         where owner = $1 and ($2::text is null or kind = $2)
           and ($3::text is null or strpos(lower(title), lower($3)) > 0 or strpos(slug, lower($3)) > 0)
         order by updated_at desc, id"
    )))
    .bind(user.id)
    .bind(kind)
    .bind(words)
    .fetch_all(&state.pool)
    .await?;
    let domain = state.config.artifact_domain.as_deref();
    let artifacts = rows.into_iter().map(|row| row.to_proto(domain, Vec::new())).collect::<ApiResult<Vec<_>>>()?;

    Ok(Json(artifacts))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Artifact>> {
    let row = owned(&state, id, user.id).await?;

    Ok(Json(full(&state, row).await?))
}

fn content_response(kind: &str, media_type: &str, sha256: &str, bytes: Vec<u8>) -> Response {
    let content_type = match kind {
        "page" => "text/html; charset=utf-8",
        "markdown" => "text/markdown; charset=utf-8",
        _ => media_type,
    };
    let sandbox = if kind == "page" { "sandbox allow-scripts" } else { "sandbox" };

    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CACHE_CONTROL, "private, no-cache".to_string()),
            (header::ETAG, format!("\"{sha256}\"")),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, sandbox.to_string()),
        ],
        bytes,
    )
        .into_response()
}

pub async fn content(State(state): State<AppState>, user: AuthUser, Path((id, version)): Path<(Uuid, i32)>) -> ApiResult<Response> {
    let row = owned(&state, id, user.id).await?;
    let stored = sqlx::query_as::<_, (String, String)>("select media_type, sha256 from artifact_versions where artifact_id = $1 and version = $2").bind(row.id).bind(version).fetch_optional(&state.pool).await?;
    let Some((media_type, sha256)) = stored else {
        return Err(ApiError::not_found("That version"));
    };
    let bytes = tokio::fs::read(attachments::path_of(&state, &sha256)).await.map_err(|_| ApiError::not_found("That content"))?;

    Ok(content_response(&row.kind, &media_type, &sha256, bytes))
}

pub async fn patch(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<PatchArtifactRequest>) -> ApiResult<Json<Artifact>> {
    owned(&state, id, user.id).await?;

    let title = request.title.as_deref().map(clean_title).transpose()?;
    let row = sqlx::query_as::<_, ArtifactRow>(sqlx::AssertSqlSafe(format!("update artifacts set title = coalesce($2, title), public = coalesce($3, public), updated_at = now() where id = $1 returning {COLUMNS}")))
        .bind(id)
        .bind(title)
        .bind(request.public)
        .fetch_one(&state.pool)
        .await?;
    let artifact = full(&state, row).await?;

    announce(&state, &artifact);

    Ok(Json(artifact))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let row = owned(&state, id, user.id).await?;

    sqlx::query("delete from artifacts where id = $1").bind(id).execute(&state.pool).await?;

    let mut artifact = row.to_proto(state.config.artifact_domain.as_deref(), Vec::new())?;

    artifact.deleted = true;
    announce(&state, &artifact);

    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_comment(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<CreateArtifactCommentRequest>) -> ApiResult<(StatusCode, Json<ArtifactComment>)> {
    let row = owned(&state, id, user.id).await?;
    let body = request.body.trim();

    if body.is_empty() {
        return Err(ApiError::bad_request("comment_required", "Write the comment before sending it."));
    }

    if body.chars().count() > MOST_COMMENT_CHARACTERS {
        return Err(ApiError::bad_request("comment_too_long", "Shorten the comment to 10,000 characters or fewer."));
    }

    let author_name: String = request.author_name.as_deref().map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&user.name).chars().take(MOST_AUTHOR_CHARACTERS).collect();
    let comment = sqlx::query_as::<_, CommentRow>(sqlx::AssertSqlSafe(format!("insert into artifact_comments (artifact_id, author_name, body, anchor) values ($1, $2, $3, $4) returning {COMMENT_COLUMNS}")))
        .bind(row.id)
        .bind(crate::store::text_without_nul(&author_name))
        .bind(crate::store::text_without_nul(body))
        .bind(request.anchor)
        .fetch_one(&state.pool)
        .await?
        .to_proto();

    state.publish(Some(row.owner), StreamMessage::ArtifactComment(comment.clone()));

    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn comments(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ArtifactComment>>> {
    let row = owned(&state, id, user.id).await?;
    let rows = sqlx::query_as::<_, CommentRow>(sqlx::AssertSqlSafe(format!("select {COMMENT_COLUMNS} from artifact_comments where artifact_id = $1 order by created_at, id"))).bind(row.id).fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(CommentRow::to_proto).collect()))
}

pub async fn patch_comment(State(state): State<AppState>, user: AuthUser, Path((id, comment_id)): Path<(Uuid, Uuid)>, Json(request): Json<PatchArtifactCommentRequest>) -> ApiResult<Json<ArtifactComment>> {
    let row = owned(&state, id, user.id).await?;
    let comment = sqlx::query_as::<_, CommentRow>(sqlx::AssertSqlSafe(format!("update artifact_comments set resolved = $3 where id = $2 and artifact_id = $1 returning {COMMENT_COLUMNS}")))
        .bind(row.id)
        .bind(comment_id)
        .bind(request.resolved)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("That comment"))?
        .to_proto();

    state.publish(Some(row.owner), StreamMessage::ArtifactComment(comment.clone()));

    Ok(Json(comment))
}

async fn put_data(state: &AppState, artifact_id: Uuid, key: &str, value: Value) -> ApiResult<ArtifactDataEntry> {
    let key = key.trim();

    if key.is_empty() || key.chars().count() > MOST_KEY_CHARACTERS || key.chars().any(char::is_control) {
        return Err(ApiError::bad_request("data_key_invalid", "Use a key of 1 to 200 characters without control characters."));
    }

    let size = serde_json::to_vec(&value).map_err(ApiError::internal)?.len();

    if size > MOST_DATA_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "data_too_large", "The value is larger than 1 MB. Store less."));
    }

    let row = sqlx::query_as::<_, DataRow>(
        "insert into artifact_data (artifact_id, key, value) values ($1, $2, $3)
         on conflict (artifact_id, key) do update set value = excluded.value, updated_at = now() returning key, value, updated_at",
    )
    .bind(artifact_id)
    .bind(key)
    .bind(value)
    .fetch_one(&state.pool)
    .await?;

    Ok(row.to_proto())
}

async fn data_of(state: &AppState, artifact_id: Uuid) -> ApiResult<Vec<ArtifactDataEntry>> {
    let rows = sqlx::query_as::<_, DataRow>("select key, value, updated_at from artifact_data where artifact_id = $1 order by key").bind(artifact_id).fetch_all(&state.pool).await?;

    Ok(rows.into_iter().map(DataRow::to_proto).collect())
}

pub async fn replace_data(State(state): State<AppState>, user: AuthUser, Path((id, key)): Path<(Uuid, String)>, Json(value): Json<Value>) -> ApiResult<Json<ArtifactDataEntry>> {
    let row = owned(&state, id, user.id).await?;

    Ok(Json(put_data(&state, row.id, &key, value).await?))
}

pub async fn data(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ArtifactDataEntry>>> {
    let row = owned(&state, id, user.id).await?;

    Ok(Json(data_of(&state, row.id).await?))
}

pub async fn host_artifact(State(state): State<AppState>, _host: ArtifactHost, Path(slug): Path<String>) -> ApiResult<Json<HostedArtifact>> {
    let row = with_slug(&state, &slug).await?;
    let latest = latest_version_of(&state, &row).await?;
    let artifact = full(&state, row).await?;

    Ok(Json(HostedArtifact { artifact, media_type: latest.media_type, sha256: latest.sha256 }))
}

pub async fn host_content(State(state): State<AppState>, _host: ArtifactHost, Path(slug): Path<String>) -> ApiResult<Response> {
    let row = with_slug(&state, &slug).await?;
    let latest = latest_version_of(&state, &row).await?;
    let bytes = tokio::fs::read(attachments::path_of(&state, &latest.sha256)).await.map_err(|_| ApiError::not_found("That content"))?;

    Ok(content_response(&row.kind, &latest.media_type, &latest.sha256, bytes))
}

pub async fn host_replace_data(State(state): State<AppState>, _host: ArtifactHost, Path((slug, key)): Path<(String, String)>, Json(value): Json<Value>) -> ApiResult<Json<ArtifactDataEntry>> {
    let row = with_slug(&state, &slug).await?;

    Ok(Json(put_data(&state, row.id, &key, value).await?))
}

pub async fn host_data(State(state): State<AppState>, _host: ArtifactHost, Path(slug): Path<String>) -> ApiResult<Json<Vec<ArtifactDataEntry>>> {
    let row = with_slug(&state, &slug).await?;

    Ok(Json(data_of(&state, row.id).await?))
}

pub async fn editor(State(state): State<AppState>, user: Result<AuthUser, ApiError>, request: Request) -> Response {
    if user.is_err() {
        return Redirect::to("/auth/login").into_response();
    }

    let (mut parts, body) = request.into_parts();
    let path = parts.uri.path().strip_prefix("/design").filter(|path| !path.is_empty()).unwrap_or("/");
    let target = match parts.uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };

    parts.uri = target.parse::<Uri>().unwrap_or_else(|_| Uri::from_static("/"));

    let mut service = crate::web::single_page_app(&state.config.editor_dir);

    match service.try_call(Request::from_parts(parts, body)).await {
        Ok(response) => response.into_response(),
        Err(cause) => ApiError::internal(cause).into_response(),
    }
}

fn argument<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

fn number_argument(arguments: &Value, name: &str) -> Option<i64> {
    arguments.get(name).and_then(Value::as_f64).map(|number| number.round() as i64)
}

async fn resolve(state: &AppState, owner: Uuid, arguments: &Value) -> ApiResult<ArtifactRow> {
    let Some(reference) = argument(arguments, "artifact") else {
        return Err(ApiError::bad_request("artifact_required", "Give the artifact, its id or its slug."));
    };

    match Uuid::parse_str(reference) {
        Ok(id) => owned(state, id, owner).await,
        Err(_) => {
            let row = with_slug(state, reference).await?;

            if row.owner != owner {
                return Err(ApiError::not_found("That artifact"));
            }

            Ok(row)
        }
    }
}

async fn content_from(state: &AppState, owner: Uuid, conversation_id: Uuid, arguments: &Value) -> ApiResult<Option<Vec<u8>>> {
    let text = arguments.get("content").and_then(Value::as_str);
    let path = argument(arguments, "path");

    match (text, path) {
        (Some(_), Some(_)) => Err(ApiError::bad_request("content_ambiguous", "Give content or path, not both.")),
        (Some(text), None) => Ok(Some(text.as_bytes().to_vec())),
        (None, Some(path)) => {
            let conversation = find_owned(&state.pool, conversation_id, owner).await?;
            let answer = relay(state, conversation.node_id, conversation.cwd, GitCommand::ReadFile { path: path.to_string() }).await?;
            let encoded = answer["data"].as_str().unwrap_or_default();
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(ApiError::internal)?;

            Ok(Some(bytes))
        }
        (None, None) => Ok(None),
    }
}

fn media_type_of_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => FALLBACK_MEDIA_TYPE,
    }
}

async fn edit_design(state: &AppState, owner: Uuid, arguments: &Value, edit: impl FnOnce(&mut Value) -> ApiResult<String>) -> ApiResult<String> {
    let row = resolve(state, owner, arguments).await?;

    if row.kind != "design" {
        return Err(ApiError::bad_request("not_a_design", "That artifact is not a design."));
    }

    let (_, bytes) = latest_content(state, &row).await?;
    let mut design = parse_design(&bytes)?;
    let detail = edit(&mut design)?;
    let saved = serde_json::to_vec(&design).map_err(ApiError::internal)?;
    let artifact = add_version(state, &row, ArtifactAuthor::Agent, &saved, None).await?;

    Ok(format!("Saved {detail} in \"{}\" as version {}.", artifact.title, artifact.latest_version))
}

fn describe_item(item: &Value) -> String {
    let id = item["id"].as_str().unwrap_or_default();
    let kind = item["type"].as_str().unwrap_or("item");
    let size = match (item["w"].as_f64(), item["h"].as_f64()) {
        (Some(width), Some(height)) => format!(" {width}x{height}", width = width.round(), height = height.round()),
        (Some(width), None) => format!(" w{}", width.round()),
        _ => String::new(),
    };
    let label = item["name"].as_str().or_else(|| item["text"].as_str()).map(|text| format!(" \"{}\"", text.chars().take(60).collect::<String>().replace('\n', " "))).unwrap_or_default();

    format!("  {kind} {id}{label}{size}")
}

pub async fn tool(state: &AppState, owner: Uuid, conversation_id: Uuid, name: &str, arguments: &Value) -> ApiResult<String> {
    match name {
        "create_artifact" => {
            let kind = parse_kind(argument(arguments, "kind").unwrap_or_default())?;
            let content = content_from(state, owner, conversation_id, arguments).await?;
            let requested = argument(arguments, "media_type").or_else(|| argument(arguments, "path").map(media_type_of_path));
            let artifact = create_one(state, NewArtifact { owner, conversation_id: Some(conversation_id), author: ArtifactAuthor::Agent, kind, title: argument(arguments, "title").unwrap_or_default(), content, media_type: requested }).await?;

            Ok(format!("Created {} \"{}\" (artifact {}, version 1).", kind_text(artifact.kind), artifact.title, artifact.id))
        }
        "update_artifact" => {
            let row = resolve(state, owner, arguments).await?;
            let content = content_from(state, owner, conversation_id, arguments).await?.ok_or_else(|| ApiError::bad_request("content_required", "Give content or path."))?;
            let artifact = add_version(state, &row, ArtifactAuthor::Agent, &content, argument(arguments, "note")).await?;

            Ok(format!("Updated \"{}\" to version {}.", artifact.title, artifact.latest_version))
        }
        "design_set_artboard" => {
            let Some(html) = arguments.get("html").and_then(Value::as_str).filter(|html| !html.trim().is_empty()) else {
                return Err(ApiError::bad_request("html_required", "Give the artboard's html."));
            };

            edit_design(state, owner, arguments, |design| {
                let pages = design["pages"].as_array_mut().ok_or_else(|| ApiError::bad_request("design_invalid", "The design has no pages."))?;

                if pages.is_empty() {
                    pages.push(json!({"id": "p1", "name": "Page 1", "items": []}));
                }

                let existing_ids: Vec<String> = pages.iter().flat_map(|page| page["items"].as_array().into_iter().flatten()).filter_map(|item| item["id"].as_str().map(String::from)).collect();
                let index = match argument(arguments, "page") {
                    Some(page_id) => pages.iter().position(|page| page["id"] == page_id).ok_or_else(|| ApiError::bad_request("page_not_found", format!("The design has no page {page_id}.")))?,
                    None => 0,
                };
                let items = pages[index]["items"].as_array_mut().ok_or_else(|| ApiError::bad_request("design_invalid", "The page has no items."))?;

                let existing = match argument(arguments, "artboard") {
                    Some(artboard_id) => Some(items.iter().position(|item| item["id"] == artboard_id && item["type"] == "artboard").ok_or_else(|| ApiError::bad_request("artboard_not_found", format!("The page has no artboard {artboard_id}.")))?),
                    None => None,
                };

                if let Some(position) = existing {
                    let artboard = &mut items[position];

                    if let Some(name) = argument(arguments, "name") {
                        artboard["name"] = json!(name);
                    }

                    for field in ["x", "y", "w", "h"] {
                        if let Some(number) = number_argument(arguments, field) {
                            artboard[field] = json!(number);
                        }
                    }

                    artboard["html"] = json!(html);

                    return Ok(format!("artboard \"{}\" ({})", artboard["name"].as_str().unwrap_or_default(), artboard["id"].as_str().unwrap_or_default()));
                }

                let artboards = items.iter().filter(|item| item["type"] == "artboard").count();
                let mut counter = artboards + 1;
                let id = loop {
                    let candidate = format!("a{counter}");

                    if !existing_ids.contains(&candidate) {
                        break candidate;
                    }

                    counter += 1;
                };
                let right_edge = items.iter().filter(|item| item["type"] == "artboard").map(|item| (item["x"].as_f64().unwrap_or(0.0) + item["w"].as_f64().unwrap_or(0.0)).round() as i64).max();
                let name = argument(arguments, "name").map(String::from).unwrap_or_else(|| format!("Artboard {}", artboards + 1));

                items.push(json!({
                    "id": id,
                    "type": "artboard",
                    "name": name,
                    "x": number_argument(arguments, "x").unwrap_or_else(|| right_edge.map_or(0, |edge| edge + ARTBOARD_GAP)),
                    "y": number_argument(arguments, "y").unwrap_or(0),
                    "w": number_argument(arguments, "w").unwrap_or(ARTBOARD_WIDTH),
                    "h": number_argument(arguments, "h").unwrap_or(ARTBOARD_HEIGHT),
                    "html": html,
                }));

                Ok(format!("artboard \"{name}\" ({id})"))
            })
            .await
        }
        "design_remove_item" => {
            let Some(item_id) = argument(arguments, "item") else {
                return Err(ApiError::bad_request("item_required", "Give the id of the item to remove."));
            };

            edit_design(state, owner, arguments, |design| {
                let pages = design["pages"].as_array_mut().ok_or_else(|| ApiError::bad_request("design_invalid", "The design has no pages."))?;

                for page in pages {
                    let Some(items) = page["items"].as_array_mut() else {
                        continue;
                    };

                    if let Some(position) = items.iter().position(|item| item["id"] == item_id) {
                        items.remove(position);

                        return Ok(format!("the removal of item {item_id}"));
                    }
                }

                Err(ApiError::bad_request("item_not_found", format!("The design has no item {item_id}.")))
            })
            .await
        }
        "read_artifact" => {
            let row = resolve(state, owner, arguments).await?;
            let (media_type, bytes) = latest_content(state, &row).await?;

            match row.kind.as_str() {
                "design" => {
                    let design = parse_design(&bytes)?;
                    let pages = design["pages"].as_array().cloned().unwrap_or_default();

                    if let Some(wanted) = argument(arguments, "artboard") {
                        let found = pages.iter().flat_map(|page| page["items"].as_array().into_iter().flatten()).find(|item| item["type"] == "artboard" && (item["id"] == wanted || item["name"].as_str().is_some_and(|name| name.eq_ignore_ascii_case(wanted))));

                        return match found {
                            Some(artboard) => Ok(artboard["html"].as_str().unwrap_or_default().to_string()),
                            None => Err(ApiError::bad_request("artboard_not_found", format!("The design has no artboard {wanted}."))),
                        };
                    }

                    let mut lines = vec![format!("Design \"{}\" (artifact {}), version {}.", row.title, row.id, row.latest_version)];

                    for page in &pages {
                        lines.push(format!("Page \"{}\" ({}):", page["name"].as_str().unwrap_or_default(), page["id"].as_str().unwrap_or_default()));
                        lines.extend(page["items"].as_array().into_iter().flatten().map(describe_item));
                    }

                    Ok(lines.join("\n"))
                }
                "page" | "markdown" => {
                    let text = String::from_utf8_lossy(&bytes);

                    if text.chars().count() > MOST_READ_CHARACTERS {
                        let shown: String = text.chars().take(MOST_READ_CHARACTERS).collect();

                        return Ok(format!("{shown}\n\n[Shortened: the artifact is {} bytes.]", bytes.len()));
                    }

                    Ok(text.into_owned())
                }
                _ => Ok(format!("File \"{}\" ({media_type}, {} bytes, version {}). Its content is not text, so it is not shown.", row.title, bytes.len(), row.latest_version)),
            }
        }
        "read_artifact_feedback" => {
            let row = resolve(state, owner, arguments).await?;
            let comments = sqlx::query_as::<_, CommentRow>(sqlx::AssertSqlSafe(format!("select {COMMENT_COLUMNS} from artifact_comments where artifact_id = $1 and not resolved order by created_at, id"))).bind(row.id).fetch_all(&state.pool).await?;
            let entries = data_of(state, row.id).await?;
            let mut lines = Vec::new();

            if comments.is_empty() {
                lines.push("No unresolved comments.".to_string());
            } else {
                lines.push("Unresolved comments:".to_string());
                lines.extend(comments.iter().map(|comment| {
                    let anchor = comment.anchor.as_ref().map(|anchor| format!(" (anchor {anchor})")).unwrap_or_default();

                    format!("- {} by {}: {}{anchor}", comment.id, comment.author_name, comment.body.replace('\n', " "))
                }));
            }

            if entries.is_empty() {
                lines.push("The data store is empty.".to_string());
            } else {
                lines.push("Data store:".to_string());
                lines.extend(entries.iter().map(|entry| format!("- {} = {}", entry.key, entry.value)));
            }

            Ok(lines.join("\n"))
        }
        _ => Err(ApiError::bad_request("unknown_tool", "Unknown artifact tool.")),
    }
}
