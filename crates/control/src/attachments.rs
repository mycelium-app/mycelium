use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::find_owned;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use mycelium_proto::conversation::Attachment;
use mycelium_proto::link::PromptAttachment;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;
pub const MAX_MESSAGE_ATTACHMENT_BYTES: i64 = 40 * 1024 * 1024;
const MAX_NAME_CHARACTERS: usize = 200;
const FALLBACK_MEDIA_TYPE: &str = "application/octet-stream";

#[derive(Deserialize)]
pub struct UploadQuery {
    name: Option<String>,
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn path_of(state: &AppState, sha256: &str) -> std::path::PathBuf {
    state.config.data_dir.join("attachments").join(sha256)
}

fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let limited: String = base.chars().filter(|character| !character.is_control()).take(MAX_NAME_CHARACTERS).collect();

    if limited.is_empty() { "attachment".to_string() } else { limited }
}

fn clean_media_type(media_type: Option<&HeaderValue>) -> String {
    let text = media_type.and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).map(|value| value.trim().to_ascii_lowercase()).unwrap_or_default();

    if text.contains('/') && !text.contains(char::is_whitespace) { text } else { FALLBACK_MEDIA_TYPE.to_string() }
}

pub async fn store(state: &AppState, owner: Uuid, body: &[u8], media_type: &str) -> ApiResult<(String, String, i64)> {
    let sha256 = hex_digest(body);
    let path = path_of(state, &sha256);

    if !path.exists() {
        let directory = state.config.data_dir.join("attachments");
        tokio::fs::create_dir_all(&directory).await.map_err(ApiError::internal)?;
        let partial = directory.join(format!("{sha256}.{}.partial", Uuid::new_v4()));
        tokio::fs::write(&partial, body).await.map_err(ApiError::internal)?;
        tokio::fs::rename(&partial, &path).await.map_err(ApiError::internal)?;
    }

    let size = body.len() as i64;
    let stored = sqlx::query_as::<_, (String, i64)>(
        "insert into attachments (owner, sha256, media_type, size) values ($1, $2, $3, $4)
         on conflict (owner, sha256) do update set media_type = attachments.media_type returning media_type, size",
    )
    .bind(owner)
    .bind(&sha256)
    .bind(media_type)
    .bind(size)
    .fetch_one(&state.pool)
    .await?;

    Ok((sha256, stored.0, stored.1))
}

pub fn media_type_of(headers: &HeaderMap) -> String {
    clean_media_type(headers.get(header::CONTENT_TYPE))
}

pub async fn upload(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<UploadQuery>, headers: HeaderMap, body: Bytes) -> ApiResult<(StatusCode, Json<Attachment>)> {
    find_owned(&state.pool, id, user.id).await?;

    if body.is_empty() {
        return Err(ApiError::bad_request("attachment_empty", "The file is empty. Choose a file that has content."));
    }

    if body.len() > MAX_ATTACHMENT_BYTES {
        return Err(too_large());
    }

    let (sha256, media_type, size) = store(&state, user.id, &body, &media_type_of(&headers)).await?;
    let name = clean_name(query.name.as_deref().unwrap_or_default());

    Ok((StatusCode::CREATED, Json(Attachment { sha256, name, media_type, size })))
}

fn too_large() -> ApiError {
    ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "attachment_too_large", "The file is larger than 20 MB. Choose a smaller file.")
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(sha256): Path<String>) -> ApiResult<Response> {
    let media_type = sqlx::query_scalar::<_, String>("select media_type from attachments where owner = $1 and sha256 = $2").bind(user.id).bind(&sha256).fetch_optional(&state.pool).await?;
    let Some(media_type) = media_type else {
        return Err(ApiError::not_found("That attachment"));
    };

    let bytes = tokio::fs::read(path_of(&state, &sha256)).await.map_err(|_| ApiError::not_found("That attachment"))?;
    let shown_inline = media_type.starts_with("image/") && media_type != "image/svg+xml";
    let disposition = if shown_inline { "inline" } else { "attachment" };
    let served_type = if shown_inline { media_type.as_str() } else { FALLBACK_MEDIA_TYPE };

    Ok((
        [
            (header::CONTENT_TYPE, served_type.to_string()),
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
            (header::ETAG, format!("\"{sha256}\"")),
            (header::CONTENT_DISPOSITION, disposition.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
        ],
        bytes,
    )
        .into_response())
}

pub async fn confirm(state: &AppState, owner: Uuid, requested: &[Attachment]) -> ApiResult<Vec<Attachment>> {
    if requested.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(ApiError::bad_request("too_many_attachments", "Attach no more than 10 files to one message."));
    }

    let mut confirmed = Vec::new();

    for attachment in requested {
        let row = sqlx::query_as::<_, (String, i64)>("select media_type, size from attachments where owner = $1 and sha256 = $2").bind(owner).bind(&attachment.sha256).fetch_optional(&state.pool).await?;
        let Some((media_type, size)) = row else {
            return Err(ApiError::bad_request("attachment_missing", format!("{} was not uploaded. Attach it again.", attachment.name)));
        };

        confirmed.push(Attachment { sha256: attachment.sha256.clone(), name: clean_name(&attachment.name), media_type, size });
    }

    let total: i64 = confirmed.iter().map(|attachment| attachment.size).sum();

    if total > MAX_MESSAGE_ATTACHMENT_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "attachments_too_large", "The files together are larger than 40 MB. Send them in separate messages."));
    }

    Ok(confirmed)
}

pub async fn read(state: &AppState, attachment: &Attachment) -> ApiResult<Vec<u8>> {
    tokio::fs::read(path_of(state, &attachment.sha256)).await.map_err(|_| ApiError::conflict("attachment_lost", format!("{} is no longer stored. Attach it again.", attachment.name)))
}

pub async fn for_node(state: &AppState, attachments: &[Attachment]) -> ApiResult<Vec<PromptAttachment>> {
    let mut loaded = Vec::new();

    for attachment in attachments {
        let bytes = read(state, attachment).await?;
        loaded.push(PromptAttachment { name: attachment.name.clone(), media_type: attachment.media_type.clone(), data: base64::engine::general_purpose::STANDARD.encode(bytes) });
    }

    Ok(loaded)
}
