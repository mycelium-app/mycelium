mod common;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use common::{Server, serve};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const RECORDING: &[u8] = b"RIFF-pretend-wav-bytes";

#[derive(Default)]
struct Heard {
    bodies: Mutex<Vec<Vec<u8>>>,
    fail: bool,
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn speech_server(heard: Arc<Heard>, models: Vec<&'static str>) -> Router {
    async fn transcribe(State(heard): State<Arc<Heard>>, body: Bytes) -> (StatusCode, Json<Value>) {
        heard.bodies.lock().unwrap().push(body.to_vec());

        if heard.fail {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "down"})));
        }

        (StatusCode::OK, Json(json!({"text": " fix the build "})))
    }

    Router::new()
        .route("/v1/models", get(move || {
            let listed: Vec<Value> = models.iter().map(|id| json!({"id": id})).collect();

            async move { Json(json!({"data": listed})) }
        }))
        .route("/v1/audio/transcriptions", post(transcribe))
        .with_state(heard)
}

async fn add_provider(server: &Server, router: Router, name: &str) -> Value {
    let base = serve(router).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": name, "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

async fn voice_on(server: &Server) {
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();
    let (status, _) = server.user.json(server.user.put("/api/v1/experiments").json(&json!({"voice": true}))).await;
    assert_eq!(status, 200);
}

async fn transcribe(server: &Server, audio: &[u8]) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/transcriptions").header("content-type", "audio/wav").body(audio.to_vec())).await
}

#[tokio::test]
async fn transcription_is_refused_until_the_voice_experiment_is_on() {
    let server = Server::start().await;
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();
    add_provider(&server, speech_server(Arc::default(), vec!["whisper-1"]), "Speech").await;
    let (status, refused) = transcribe(&server, RECORDING).await;
    assert_eq!((status, refused["error"].clone()), (403, json!("experiment_disabled")));
    voice_on(&server).await;
    let (status, heard) = transcribe(&server, RECORDING).await;
    assert_eq!((status, heard["text"].clone()), (200, json!("fix the build")));
}

#[tokio::test]
async fn a_recording_is_transcribed_by_the_persons_speech_provider() {
    let server = Server::start().await;
    voice_on(&server).await;
    let heard = Arc::new(Heard::default());
    add_provider(&server, speech_server(heard.clone(), vec!["gpt-4o-mini", "whisper-1"]), "Speech").await;
    let (status, reply) = transcribe(&server, RECORDING).await;
    assert_eq!((status, reply["text"].clone()), (200, json!("fix the build")));
    let bodies = heard.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert!(contains(&bodies[0], b"whisper-1"));
    assert!(contains(&bodies[0], RECORDING));
}

#[tokio::test]
async fn transcription_without_a_speech_provider_says_how_to_add_one() {
    let server = Server::start().await;
    voice_on(&server).await;
    add_provider(&server, speech_server(Arc::default(), vec!["gpt-4o-mini"]), "Chat only").await;
    let (status, refused) = transcribe(&server, RECORDING).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("no_speech_provider")));
    assert!(refused["message"].as_str().unwrap().contains("Settings, Providers"));
}

#[tokio::test]
async fn an_empty_recording_is_refused() {
    let server = Server::start().await;
    voice_on(&server).await;
    add_provider(&server, speech_server(Arc::default(), vec!["whisper-1"]), "Speech").await;
    let (status, refused) = transcribe(&server, b"").await;
    assert_eq!((status, refused["error"].clone()), (400, json!("audio_empty")));
}

#[tokio::test]
async fn a_failing_speech_provider_is_reported_as_a_bad_gateway() {
    let server = Server::start().await;
    voice_on(&server).await;
    add_provider(&server, speech_server(Arc::new(Heard { fail: true, ..Heard::default() }), vec!["whisper-1"]), "Speech").await;
    let (status, failed) = transcribe(&server, RECORDING).await;
    assert_eq!((status, failed["error"].clone()), (502, json!("transcription_failed")));
}

#[tokio::test]
async fn providers_report_their_speech_model() {
    let server = Server::start().await;
    add_provider(&server, speech_server(Arc::default(), vec!["gpt-4o-mini", "whisper-1"]), "Speech").await;
    add_provider(&server, speech_server(Arc::default(), vec!["gpt-4o-mini"]), "Chat only").await;
    let (_, providers) = server.user.json(server.user.get("/api/v1/model-providers")).await;
    let speech: Vec<Value> = providers.as_array().unwrap().iter().map(|provider| provider["speech_model"].clone()).collect();
    assert_eq!(speech, vec![json!("whisper-1"), Value::Null]);
}
