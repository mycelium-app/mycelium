use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use mycelium_proto::api::{ChooseLocalSpeechRequest, LocalSpeechModel, LocalSpeechState, PrivacySettings};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const MODELS_FOLDER: &str = "speech-models";
const COMPLETE_MARKER: &str = ".complete";
const LISTING_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_TIMEOUT: Duration = Duration::from_secs(3_600);
const TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(120);
const ALTERNATIVE_WEIGHTS: [&str; 4] = [".h5", ".msgpack", ".ot", ".onnx"];
const UNSAFE_WEIGHTS: [&str; 3] = [".bin", ".pt", ".pth"];

#[derive(Clone, Default)]
struct Download {
    attempt: Uuid,
    downloaded: u64,
    total: Option<u64>,
    failure: Option<String>,
}

static DOWNLOADS: LazyLock<Mutex<HashMap<PathBuf, Download>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn downloads() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Download>> {
    DOWNLOADS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn valid_model_id(model_id: &str) -> bool {
    let mut parts = model_id.split('/');
    let valid_part = |part: Option<&str>| part.is_some_and(|part| !part.is_empty() && !part.starts_with('.') && part.chars().all(|letter| letter.is_ascii_alphanumeric() || "._-".contains(letter)));

    valid_part(parts.next()) && valid_part(parts.next()) && parts.next().is_none()
}

fn model_dir(state: &AppState, model_id: &str) -> PathBuf {
    state.config.data_dir.join(MODELS_FOLDER).join(model_id.replace('/', "--"))
}

fn partial_dir(dir: &Path) -> PathBuf {
    let mut name = dir.as_os_str().to_owned();
    name.push(".partial");

    PathBuf::from(name)
}

fn downloaded_size(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(dir.join(COMPLETE_MARKER)).ok().and_then(|size| size.trim().parse().ok())
}

pub async fn chosen(state: &AppState, owner: Uuid) -> ApiResult<Option<String>> {
    let model_id = sqlx::query_scalar::<_, String>("select model_id from local_speech_models where owner = $1").bind(owner).fetch_optional(&state.pool).await?;

    Ok(model_id)
}

pub async fn local_of(state: &AppState, owner: Uuid) -> ApiResult<Option<LocalSpeechModel>> {
    let Some(model_id) = chosen(state, owner).await? else {
        return Ok(None);
    };

    Ok(Some(status(state, &model_id)))
}

fn status(state: &AppState, model_id: &str) -> LocalSpeechModel {
    let dir = model_dir(state, model_id);

    if let Some(size) = downloaded_size(&dir) {
        return LocalSpeechModel { model_id: model_id.to_string(), state: LocalSpeechState::Ready, downloaded_bytes: size, total_bytes: Some(size), failure: None };
    }

    let download = ensure_download(state, model_id, &dir);
    let progress = match download.failure {
        Some(_) => LocalSpeechState::Failed,
        None => LocalSpeechState::Downloading,
    };

    LocalSpeechModel { model_id: model_id.to_string(), state: progress, downloaded_bytes: download.downloaded, total_bytes: download.total, failure: download.failure }
}

fn ensure_download(state: &AppState, model_id: &str, dir: &Path) -> Download {
    let mut running = downloads();

    if let Some(download) = running.get(dir) {
        return download.clone();
    }

    let attempt = Download { attempt: Uuid::new_v4(), ..Download::default() };
    running.insert(dir.to_path_buf(), attempt.clone());
    tokio::spawn(download(state.clone(), model_id.to_string(), dir.to_path_buf(), attempt.attempt));

    attempt
}

fn wanted_files(siblings: &[(String, u64)]) -> Vec<(String, u64)> {
    let has_safetensors = siblings.iter().any(|(name, _)| name.ends_with(".safetensors"));

    siblings
        .iter()
        .filter(|(name, _)| !name.contains('/') && !name.contains('\\') && !name.starts_with('.'))
        .filter(|(name, _)| !ALTERNATIVE_WEIGHTS.iter().any(|suffix| name.ends_with(suffix)))
        .filter(|(name, _)| !has_safetensors || !UNSAFE_WEIGHTS.iter().any(|suffix| name.ends_with(suffix)))
        .cloned()
        .collect()
}

async fn download(state: AppState, model_id: String, dir: PathBuf, attempt: Uuid) {
    let partial = partial_dir(&dir);
    let fetched = fetch_model(&state, &model_id, &dir, &partial, attempt).await;

    if fetched.is_ok() {
        downloads().remove(&dir);

        return;
    }

    let superseded = {
        let mut running = downloads();

        match running.get_mut(&dir) {
            Some(download) if download.attempt == attempt => {
                download.failure = fetched.err();

                false
            }

            Some(_) => true,
            None => false,
        }
    };

    if !superseded {
        let _ = tokio::fs::remove_dir_all(&partial).await;
    }
}

fn not_found(model_id: &str) -> String {
    format!("Hugging Face has no model called {model_id}. Check the id and try again.")
}

fn unreachable() -> String {
    "Hugging Face could not be reached. Try again in a moment.".to_string()
}

fn track(dir: &Path, attempt: Uuid, update: impl FnOnce(&mut Download)) -> Result<(), String> {
    let mut running = downloads();
    let Some(download) = running.get_mut(dir).filter(|download| download.attempt == attempt) else {
        return Err("The download was stopped.".to_string());
    };

    update(download);

    Ok(())
}

async fn fetch_model(state: &AppState, model_id: &str, dir: &Path, partial: &Path, attempt: Uuid) -> Result<(), String> {
    let hub = &state.config.huggingface_url;
    let listing = state.http.get(format!("{hub}/api/models/{model_id}?blobs=true")).header("user-agent", "mycelium").timeout(LISTING_TIMEOUT).send().await.map_err(|_| unreachable())?;

    if listing.status() == reqwest::StatusCode::NOT_FOUND || listing.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(not_found(model_id));
    }

    if !listing.status().is_success() {
        return Err(unreachable());
    }

    let body = listing.json::<Value>().await.map_err(|_| unreachable())?;
    let siblings: Vec<(String, u64)> = body["siblings"].as_array().map(|files| files.iter().filter_map(|file| Some((file["rfilename"].as_str()?.to_string(), file["size"].as_u64().unwrap_or(0)))).collect()).unwrap_or_default();
    let files = wanted_files(&siblings);

    if files.is_empty() {
        return Err(format!("{model_id} has no files to download. Choose another model."));
    }

    let total = files.iter().map(|(_, size)| size).sum();
    let mut written = 0u64;
    track(dir, attempt, |download| download.total = Some(total))?;

    let _ = tokio::fs::remove_dir_all(partial).await;
    tokio::fs::create_dir_all(partial).await.map_err(|error| format!("The server could not save the model: {error}."))?;

    for (name, _) in &files {
        let mut response = state.http.get(format!("{hub}/{model_id}/resolve/main/{name}")).header("user-agent", "mycelium").timeout(FILE_TIMEOUT).send().await.map_err(|_| unreachable())?;

        if !response.status().is_success() {
            return Err(format!("Hugging Face would not send {name} of {model_id}. Try again in a moment."));
        }

        track(dir, attempt, |_| {})?;

        let mut file = tokio::fs::File::create(partial.join(name)).await.map_err(|error| format!("The server could not save the model: {error}."))?;

        while let Some(chunk) = response.chunk().await.map_err(|_| unreachable())? {
            file.write_all(&chunk).await.map_err(|error| format!("The server could not save the model: {error}."))?;
            written += chunk.len() as u64;
            track(dir, attempt, |download| download.downloaded = written)?;
        }

        file.flush().await.map_err(|error| format!("The server could not save the model: {error}."))?;
    }

    tokio::fs::write(partial.join(COMPLETE_MARKER), written.to_string()).await.map_err(|error| format!("The server could not save the model: {error}."))?;
    let _ = tokio::fs::remove_dir_all(dir).await;
    tokio::fs::rename(partial, dir).await.map_err(|error| format!("The server could not save the model: {error}."))?;

    Ok(())
}

pub async fn choose(State(state): State<AppState>, user: AuthUser, Json(request): Json<ChooseLocalSpeechRequest>) -> ApiResult<Json<PrivacySettings>> {
    user.require_full_access()?;

    let model_id = request.model_id.trim();

    if !valid_model_id(model_id) {
        return Err(ApiError::bad_request("local_model_invalid", "Enter a Hugging Face model id, such as owner/name."));
    }

    let previous = chosen(&state, user.id).await?;

    sqlx::query("insert into local_speech_models (owner, model_id) values ($1, $2) on conflict (owner) do update set model_id = excluded.model_id, chosen_at = now()")
        .bind(user.id)
        .bind(model_id)
        .execute(&state.pool)
        .await?;

    if let Some(previous) = previous.filter(|previous| previous != model_id) {
        stop_unfinished_download(&state, &previous).await?;
    }

    let dir = model_dir(&state, model_id);
    let failed = downloads().get(&dir).is_some_and(|download| download.failure.is_some());

    if failed {
        downloads().remove(&dir);
    }

    Ok(Json(crate::voice::privacy_of(&state, user.id).await?))
}

pub async fn remove(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<PrivacySettings>> {
    user.require_full_access()?;

    let removed = sqlx::query_scalar::<_, String>("delete from local_speech_models where owner = $1 returning model_id").bind(user.id).fetch_optional(&state.pool).await?;

    if let Some(model_id) = removed {
        if !in_use(&state, &model_id).await? {
            let dir = model_dir(&state, &model_id);
            downloads().remove(&dir);
            let _ = tokio::fs::remove_dir_all(partial_dir(&dir)).await;
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }
    }

    Ok(Json(crate::voice::privacy_of(&state, user.id).await?))
}

pub async fn clear_choice(state: &AppState, owner: Uuid) -> ApiResult<()> {
    let cleared = sqlx::query_scalar::<_, String>("delete from local_speech_models where owner = $1 returning model_id").bind(owner).fetch_optional(&state.pool).await?;

    if let Some(model_id) = cleared {
        stop_unfinished_download(state, &model_id).await?;
    }

    Ok(())
}

async fn in_use(state: &AppState, model_id: &str) -> ApiResult<bool> {
    let used = sqlx::query_scalar::<_, bool>("select exists (select 1 from local_speech_models where model_id = $1)").bind(model_id).fetch_one(&state.pool).await?;

    Ok(used)
}

async fn stop_unfinished_download(state: &AppState, model_id: &str) -> ApiResult<()> {
    let dir = model_dir(state, model_id);

    if downloaded_size(&dir).is_none() && !in_use(state, model_id).await? {
        downloads().remove(&dir);
    }

    Ok(())
}

fn runner_failed() -> ApiError {
    ApiError::new(axum::http::StatusCode::BAD_GATEWAY, "transcription_failed", "The local model could not transcribe the recording. Try again, or check the model in Settings, Voice.")
}

pub async fn transcribe(state: &AppState, model_id: &str, audio: Vec<u8>) -> ApiResult<String> {
    let model = status(state, model_id);

    match model.state {
        LocalSpeechState::Ready => {}
        LocalSpeechState::Downloading => return Err(ApiError::conflict("local_model_downloading", "The local model is still downloading. Try again when it is ready.")),
        LocalSpeechState::Failed => return Err(ApiError::conflict("local_model_failed", "The local model could not be downloaded. Try again in Settings, Voice.")),
    }

    let Some(runner) = state.config.local_speech_url.as_deref() else {
        return Err(ApiError::conflict("local_speech_unavailable", "Local speech to text needs the speech runner beside the server. Set MYCELIUM_LOCAL_SPEECH_URL, or choose a provider in Settings, Voice."));
    };
    let folder = model_id.replace('/', "--");
    let sent = state.http.post(format!("{runner}/transcribe")).query(&[("model", folder.as_str())]).header("content-type", "audio/wav").timeout(TRANSCRIPTION_TIMEOUT).body(audio).send().await;
    let Ok(response) = sent else {
        return Err(runner_failed());
    };

    if !response.status().is_success() {
        return Err(runner_failed());
    }

    let Ok(reply) = response.json::<Value>().await else {
        return Err(runner_failed());
    };

    Ok(reply["text"].as_str().unwrap_or_default().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_owner_and_name_only() {
        assert!(valid_model_id("example/speech-small"));
        assert!(valid_model_id("Example.Org/model_v2.1"));
        assert!(!valid_model_id("speech-small"));
        assert!(!valid_model_id("example/../secrets"));
        assert!(!valid_model_id("example/.hidden"));
        assert!(!valid_model_id("a/b/c"));
        assert!(!valid_model_id("example/name with space"));
        assert!(!valid_model_id(""));
    }

    #[test]
    fn only_top_level_files_are_wanted_and_safetensors_wins_over_pickled_weights() {
        let siblings: Vec<(String, u64)> = ["config.json", "model.safetensors", "pytorch_model.bin", "tf_model.h5", ".gitattributes", "onnx/model.onnx", "tokenizer.json", "modeling_custom.py"].iter().map(|name| (name.to_string(), 1)).collect();
        let wanted: Vec<String> = wanted_files(&siblings).into_iter().map(|(name, _)| name).collect();

        assert_eq!(wanted, ["config.json", "model.safetensors", "tokenizer.json", "modeling_custom.py"]);
    }

    #[test]
    fn pickled_weights_are_kept_when_there_is_no_safetensors() {
        let siblings: Vec<(String, u64)> = ["config.json", "pytorch_model.bin"].iter().map(|name| (name.to_string(), 1)).collect();

        assert_eq!(wanted_files(&siblings).len(), 2);
    }
}
