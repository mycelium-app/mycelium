use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::local_speech;
use crate::model_providers;
use crate::state::AppState;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use base64::Engine;
use serde_json::{Value, json};
use std::time::Duration;

pub const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(120);

fn failed() -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "transcription_failed", "The speech provider could not transcribe the recording. Try again, or check the provider in Settings, Voice.")
}

fn openrouter_body(model: &str, audio: &[u8]) -> Value {
    json!({"model": model, "input_audio": {"data": base64::engine::general_purpose::STANDARD.encode(audio), "format": "wav"}})
}

pub async fn create(State(state): State<AppState>, user: AuthUser, body: Bytes) -> ApiResult<Json<Value>> {
    user.require_full_access()?;

    if body.is_empty() {
        return Err(ApiError::bad_request("audio_empty", "The recording is empty. Hold the microphone button and speak, then try again."));
    }

    if let Some(model_id) = local_speech::chosen(&state, user.id).await? {
        let text = local_speech::transcribe(&state, &model_id, body.to_vec()).await?;

        return Ok(Json(json!({"text": text})));
    }

    let found = model_providers::speech_model(&state, user.id).await?;
    let Some((provider, model)) = found else {
        return Err(ApiError::conflict("no_speech_provider", "Choose speech to text in Settings, Voice to dictate on this device."));
    };

    let address = format!("{}/audio/transcriptions", provider.address()?);
    let request = if provider.is_openrouter() {
        state.http.post(address).json(&openrouter_body(&model, &body))
    } else {
        let file = reqwest::multipart::Part::bytes(body.to_vec()).file_name("speech.wav").mime_str("audio/wav").map_err(ApiError::internal)?;

        state.http.post(address).multipart(reqwest::multipart::Form::new().part("file", file).text("model", model))
    };
    let request = provider.authorised(request.timeout(TRANSCRIPTION_TIMEOUT))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_takes_the_recording_as_base64_wav_in_json() {
        let body = openrouter_body("mistralai/voxtral-mini-transcribe-3-0", b"RIFF");

        assert_eq!(body, json!({"model": "mistralai/voxtral-mini-transcribe-3-0", "input_audio": {"data": "UklGRg==", "format": "wav"}}));
    }
}
