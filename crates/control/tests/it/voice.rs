use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use crate::common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const AUDIO: &[u8] = b"ID3-pretend-mp3-bytes";

#[derive(Default)]
struct Asked {
    requests: Mutex<Vec<Value>>,
    fail: bool,
}

fn speech_server(asked: Arc<Asked>, models: Vec<&'static str>) -> Router {
    async fn speak(State(asked): State<Arc<Asked>>, Json(request): Json<Value>) -> Response {
        asked.requests.lock().unwrap().push(request);

        if asked.fail {
            return (StatusCode::INTERNAL_SERVER_ERROR, "down").into_response();
        }

        ([(header::CONTENT_TYPE, "audio/mpeg")], AUDIO).into_response()
    }

    Router::new()
        .route("/v1/models", get(move || {
            let listed: Vec<Value> = models.iter().map(|id| json!({"id": id})).collect();

            async move { Json(json!({"data": listed})) }
        }))
        .route("/v1/audio/speech", post(speak))
        .with_state(asked)
}

async fn add_provider(server: &Server, router: Router, name: &str) -> Value {
    let base = serve(router).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": name, "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

async fn speak(server: &Server, body: Value) -> (u16, String, Vec<u8>) {
    let response = server.user.post("/api/v1/speech").json(&body).send().await.unwrap();
    let status = response.status().as_u16();
    let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();

    (status, content_type, response.bytes().await.unwrap().to_vec())
}

#[tokio::test]
async fn voice_is_off_until_the_person_turns_it_on_and_the_privacy_page_names_the_providers() {
    let server = Server::start().await;
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();
    let (status, before) = server.user.json(server.user.get("/api/v1/privacy")).await;
    assert_eq!((status, before["voice_enabled"].clone(), before["speech_to_text_provider"].clone(), before["text_to_speech_provider"].clone()), (200, json!(false), Value::Null, Value::Null));

    let provider = add_provider(&server, speech_server(Arc::default(), vec!["whisper-1", "gpt-4o-mini-tts"]), "Speech").await;
    let (status, after) = server.user.json(server.user.put("/api/v1/privacy").json(&json!({"voice_enabled": true}))).await;
    assert_eq!((status, after["voice_enabled"].clone()), (200, json!(true)));
    assert_eq!(after["speech_to_text_provider"], json!({"provider_id": provider["id"], "name": "Speech", "model": "whisper-1"}));
    assert_eq!(after["text_to_speech_provider"], json!({"provider_id": provider["id"], "name": "Speech", "model": "gpt-4o-mini-tts"}));

    let (_, read_back) = server.user.json(server.user.get("/api/v1/privacy")).await;
    assert_eq!(read_back["voice_enabled"], json!(true));
}

#[tokio::test]
async fn transcription_is_allowed_with_voice_on_while_the_voice_experiment_is_off() {
    let server = Server::start().await;
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();
    let (_, refused) = server.user.json(server.user.post("/api/v1/transcriptions").header("content-type", "audio/wav").body(AUDIO.to_vec())).await;
    assert_eq!(refused["error"], json!("experiment_disabled"));

    server.user.json(server.user.put("/api/v1/privacy").json(&json!({"voice_enabled": true}))).await;
    let (status, answer) = server.user.json(server.user.post("/api/v1/transcriptions").header("content-type", "audio/wav").body(AUDIO.to_vec())).await;
    assert_eq!((status, answer["error"].clone()), (409, json!("no_speech_provider")));
}

#[tokio::test]
async fn the_voice_assistant_is_built_in_with_default_instructions_it_cannot_be_renamed_or_archived() {
    let server = Server::start().await;
    let (status, voice) = server.user.json(server.user.get("/api/v1/assistants/builtin/voice")).await;
    assert_eq!((status, voice["name"].clone(), voice["builtin_key"].clone()), (200, json!("Voice"), json!("voice")));
    assert!(voice["instructions"].as_str().unwrap().contains("open_screen"));

    let id = voice["id"].as_str().unwrap();
    let (_, listed) = server.user.json(server.user.get("/api/v1/assistants")).await;
    assert!(listed.as_array().unwrap().iter().any(|assistant| assistant["id"] == voice["id"]));

    let (renamed, refused) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"name": "Buddy"}))).await;
    assert_eq!((renamed, refused["error"].clone()), (409, json!("assistant_built_in")));
    let (archived, refused) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"archived": true}))).await;
    assert_eq!((archived, refused["error"].clone()), (409, json!("assistant_built_in")));
}

#[tokio::test]
async fn resetting_instructions_saves_the_default_as_a_new_version_and_keeps_the_edit_in_history() {
    let server = Server::start().await;
    let (_, voice) = server.user.json(server.user.get("/api/v1/assistants/builtin/voice")).await;
    let id = voice["id"].as_str().unwrap();
    let default = voice["instructions"].as_str().unwrap().to_string();
    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"instructions": "Only talk about tea."}))).await;
    assert_eq!(status, 200);

    let (status, reset) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/instructions/reset"))).await;
    assert_eq!((status, reset["instructions"].clone()), (200, json!(default)));

    let versions: i64 = sqlx::query_scalar("select count(*) from instruction_versions where text = 'Only talk about tea.'").fetch_one(&server.pool).await.unwrap();
    assert_eq!(versions, 1);
}

#[tokio::test]
async fn an_assistant_without_a_built_in_default_cannot_be_reset() {
    let server = Server::start().await;
    let (status, created) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Stand-up", "instructions": "Collect updates."}))).await;
    assert_eq!(status, 201, "{created}");

    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/assistants/{}/instructions/reset", created["id"].as_str().unwrap())).json(&json!({}))).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("no_default_instructions")));
}

#[tokio::test]
async fn speech_is_proxied_to_the_tts_provider_with_the_chosen_voice_and_speed() {
    let server = Server::start().await;
    let asked = Arc::new(Asked::default());
    add_provider(&server, speech_server(asked.clone(), vec!["whisper-1", "gpt-4o-mini-tts"]), "Speech").await;

    let (status, content_type, audio) = speak(&server, json!({"text": "  Your build passed.  ", "voice": "nova", "speed": "faster"})).await;
    assert_eq!((status, content_type.as_str(), audio.as_slice()), (200, "audio/mpeg", AUDIO));

    let requests = asked.requests.lock().unwrap();
    assert_eq!((requests[0]["model"].clone(), requests[0]["input"].clone(), requests[0]["voice"].clone(), requests[0]["speed"].clone()), (json!("gpt-4o-mini-tts"), json!("Your build passed."), json!("nova"), json!(1.2)));
    assert_eq!(requests[0]["response_format"], json!("wav"));
}

#[tokio::test]
async fn speech_lists_the_voices_of_the_tts_provider_and_none_without_one() {
    let server = Server::start().await;
    let (_, none) = server.user.json(server.user.get("/api/v1/speech/voices")).await;
    assert_eq!(none["voices"], json!([]));

    add_provider(&server, speech_server(Arc::default(), vec!["tts-1"]), "Speech").await;
    let (_, some) = server.user.json(server.user.get("/api/v1/speech/voices")).await;
    assert!(some["voices"].as_array().unwrap().contains(&json!("nova")));
}

#[tokio::test]
async fn speech_without_a_tts_provider_says_how_to_add_one() {
    let server = Server::start().await;
    add_provider(&server, speech_server(Arc::default(), vec!["whisper-1"]), "Speech to text only").await;

    let (status, _, body) = speak(&server, json!({"text": "Hello"})).await;
    let refused: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!((status, refused["error"].clone()), (409, json!("no_tts_provider")));
    assert!(refused["message"].as_str().unwrap().contains("Settings, Providers"));
}

#[tokio::test]
async fn a_failing_tts_provider_is_reported_as_a_bad_gateway() {
    let server = Server::start().await;
    add_provider(&server, speech_server(Arc::new(Asked { fail: true, ..Asked::default() }), vec!["tts-1"]), "Speech").await;

    let (status, _, body) = speak(&server, json!({"text": "Hello"})).await;
    let failed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!((status, failed["error"].clone()), (502, json!("speech_failed")));
}
