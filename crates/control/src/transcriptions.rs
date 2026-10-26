use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::model_providers;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

pub const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(120);

fn failed() -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "transcription_failed", "The speech provider could not transcribe the recording. Try again, or check the provider in Settings, Providers.")
}

pub async fn create(State(state): State<AppState>, user: AuthUser, body: Bytes) -> ApiResult<Json<Value>> {
    user.require_full_access()?;

    if body.is_empty() {
        return Err(ApiError::bad_request("audio_empty", "The recording is empty. Hold the microphone button and speak, then try again."));
    }

    let providers = model_providers::list_owned(&state, user.id).await?;
    let found = providers.iter().find_map(|provider| provider.speech_model().map(|model| (provider, model)));
    let Some((provider, model)) = found else {
        return Err(ApiError::conflict("no_speech_provider", "Add a provider with a speech-to-text model in Settings, Providers to dictate on this device."));
    };

    let file = reqwest::multipart::Part::bytes(body.to_vec()).file_name("speech.wav").mime_str("audio/wav").map_err(ApiError::internal)?;
    let form = reqwest::multipart::Form::new().part("file", file).text("model", model);
    let request = provider.authorised(state.http.post(format!("{}/audio/transcriptions", provider.address()?)).timeout(TRANSCRIPTION_TIMEOUT).multipart(form))?;
    let Ok(response) = request.send().await else {
        return Err(failed());
    };

    if !response.status().is_success() {
        return Err(failed());
    }

    let Ok(reply) = response.json::<Value>().await else {
        return Err(failed());
    };
    let text = reply["text"].as_str().unwrap_or_default().trim();

    Ok(Json(json!({"text": text})))
}
