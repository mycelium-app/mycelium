use crate::assistants;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::model_providers;
use crate::outbound::{AUDIO_LIMIT, read_limited};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mycelium_proto::api::{Assistant, PrivacySettings, SavePrivacyRequest, SpeechProvider, SpeechRequest, SpeechVoices};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

pub const VOICE: &str = "voice";
const SPEECH_TIMEOUT: Duration = Duration::from_secs(60);
const SPEECH_TEXT_LIMIT: usize = 4_000;
const DEFAULT_VOICE: &str = "alloy";
const DEFAULT_AUDIO_TYPE: &str = "audio/wav";
const DESCRIPTION: &str = "Answers when you press the microphone, and acts across Mycelium for you.";
const VOICE_INSTRUCTIONS: &str = "You are Voice, the person's spoken assistant. They talk to you by pressing the microphone, and your replies are often read aloud, so keep them short, plain and easy to hear: no tables, no code blocks, no long lists. Write in British English.

You can act across Mycelium for them. Use find_conversations to look at their work, start_conversation to begin a new conversation, send_to_conversation to message an existing one, assistant_write_file to keep notes in your own storage, and open_screen to show them a screen in the app.

For what waits on the person in the Inbox, call list_needs_you once: its first item becomes the outlined one. When they say approve, decline or give an answer, call resolve_needs_you at once on the outlined item, without asking again, because saying it is their clear yes. When they say next or name a conversation, call focus_needs_you to move the outline, then act on the item you outlined last. Questions and proposals can be settled by voice. A permission request runs a command and a workflow gate waits for a person, so neither is ever settled by voice: outline it and tell them it needs a tap on Approve. Say what happened in one short sentence.

Ask before you do anything that sends a message, deletes something or spends money or usage, and wait for a clear yes. Opening a screen and reading do not need asking. If you did not catch what was said or the request is unclear, ask one short question rather than guessing.";

pub fn default_instructions(builtin_key: &str) -> Option<&'static str> {
    match builtin_key {
        VOICE => Some(VOICE_INSTRUCTIONS),
        crate::assistant_actions::WELLNESS => Some(crate::assistant_actions::WELLNESS_INSTRUCTIONS),
        _ => None,
    }
}

pub async fn enabled(state: &AppState, owner: Uuid) -> ApiResult<bool> {
    let enabled = sqlx::query_scalar::<_, bool>("select voice_enabled from user_settings where owner = $1").bind(owner).fetch_optional(&state.pool).await?;

    Ok(enabled.unwrap_or(false))
}

pub async fn ensure_voice(state: &AppState, owner: Uuid) -> ApiResult<Uuid> {
    let existing = sqlx::query_scalar::<_, Uuid>("select id from assistants where owner = $1 and builtin_key = $2").bind(owner).bind(VOICE).fetch_optional(&state.pool).await?;
    let id = match existing {
        Some(id) => id,
        None => {
            let mut created = None;

            for name in ["Voice", "Voice (built in)"] {
                created = sqlx::query_scalar::<_, Uuid>("insert into assistants (owner, name, description, builtin_key) values ($1, $3, $4, $2) on conflict do nothing returning id")
                    .bind(owner)
                    .bind(VOICE)
                    .bind(name)
                    .bind(DESCRIPTION)
                    .fetch_optional(&state.pool)
                    .await?;

                if created.is_some() {
                    break;
                }
            }

            match created {
                Some(id) => id,
                None => sqlx::query_scalar::<_, Uuid>("select id from assistants where owner = $1 and builtin_key = $2").bind(owner).bind(VOICE).fetch_one(&state.pool).await?,
            }
        }
    };

    let instructions = assistants::instructions_of(state, id).await?;

    if instructions.trim().is_empty() {
        assistants::store_file(state, id, assistants::INSTRUCTIONS_PATH, VOICE_INSTRUCTIONS.as_bytes()).await?;
        assistants::publish(state, owner, id).await?;
    }

    Ok(id)
}

pub async fn voice_assistant(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Assistant>> {
    let id = ensure_voice(&state, user.id).await?;
    let mut assistant = assistants::find_owned(&state, id, user.id).await?;

    assistant.instructions = Some(assistants::instructions_of(&state, id).await?);

    Ok(Json(assistant))
}

pub async fn reset_instructions(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Assistant>> {
    user.require_full_access()?;

    let mut assistant = assistants::find_owned(&state, id, user.id).await?;
    let default = assistant.builtin_key.as_deref().and_then(default_instructions);
    let Some(default) = default else {
        return Err(ApiError::conflict("no_default_instructions", format!("{} has no default instructions to go back to. Edit its instructions instead.", assistant.name)));
    };

    assistants::store_file(&state, id, assistants::INSTRUCTIONS_PATH, default.as_bytes()).await?;
    assistant = assistants::publish(&state, user.id, id).await?;
    assistant.instructions = Some(assistants::instructions_of(&state, id).await?);

    Ok(Json(assistant))
}

pub async fn privacy_of(state: &AppState, owner: Uuid) -> ApiResult<PrivacySettings> {
    let speech_to_text_provider = model_providers::speech_model(state, owner).await?.map(|(provider, model)| SpeechProvider { provider_id: provider.id, name: provider.name, model });
    let text_to_speech_provider = model_providers::tts_model(state, owner).await?.map(|(provider, model)| SpeechProvider { provider_id: provider.id, name: provider.name, model });

    let speech_to_text_local = crate::local_speech::local_of(state, owner).await?;

    Ok(PrivacySettings { voice_enabled: enabled(state, owner).await?, speech_to_text_provider, text_to_speech_provider, speech_to_text_local })
}

pub async fn get_privacy(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<PrivacySettings>> {
    Ok(Json(privacy_of(&state, user.id).await?))
}

pub async fn save_privacy(State(state): State<AppState>, user: AuthUser, Json(request): Json<SavePrivacyRequest>) -> ApiResult<Json<PrivacySettings>> {
    user.require_full_access()?;

    sqlx::query("insert into user_settings (owner, voice_enabled) values ($1, $2) on conflict (owner) do update set voice_enabled = excluded.voice_enabled")
        .bind(user.id)
        .bind(request.voice_enabled)
        .execute(&state.pool)
        .await?;

    Ok(Json(privacy_of(&state, user.id).await?))
}

pub async fn voices(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<SpeechVoices>> {
    let speaker = model_providers::tts_model(&state, user.id).await?;
    let voices = match speaker {
        Some(_) => model_providers::SPEECH_VOICES.iter().map(|voice| voice.to_string()).collect(),
        None => Vec::new(),
    };

    Ok(Json(SpeechVoices { voices }))
}

fn speech_failed() -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "speech_failed", "The speech provider could not read that aloud. Try again, or check the provider in Settings, Voice.")
}

fn speed_of(name: Option<&str>) -> f64 {
    match name {
        Some("slower") => 0.85,
        Some("faster") => 1.2,
        _ => 1.0,
    }
}

pub async fn speak(State(state): State<AppState>, user: AuthUser, Json(request): Json<SpeechRequest>) -> ApiResult<Response> {
    user.require_full_access()?;

    let text = request.text.trim();

    if text.is_empty() {
        return Err(ApiError::bad_request("text_required", "Enter some text to read aloud."));
    }

    if text.chars().count() > SPEECH_TEXT_LIMIT {
        return Err(ApiError::bad_request("text_too_long", "That is too long to read aloud in one go. Read a shorter part."));
    }

    let found = model_providers::tts_model(&state, user.id).await?;
    let Some((provider, model)) = found else {
        return Err(ApiError::conflict("no_tts_provider", "Choose a text-to-speech model in Settings, Voice to have replies read aloud."));
    };
    let voice = request.voice.as_deref().map(str::trim).filter(|voice| !voice.is_empty()).unwrap_or(DEFAULT_VOICE).to_string();
    let body = json!({"model": model, "input": text, "voice": voice, "speed": speed_of(request.speed.as_deref()), "response_format": "wav"});
    let sent = provider.authorised(state.http.post(format!("{}/audio/speech", provider.address()?)).timeout(SPEECH_TIMEOUT).json(&body))?;
    let Ok(response) = sent.send().await else {
        return Err(speech_failed());
    };

    if !response.status().is_success() {
        return Err(speech_failed());
    }

    let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).filter(|value| value.starts_with("audio/")).unwrap_or(DEFAULT_AUDIO_TYPE).to_string();
    let Ok(audio) = read_limited(response, AUDIO_LIMIT).await else {
        return Err(speech_failed());
    };

    Ok(([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-store".to_string()), (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string())], audio).into_response())
}
