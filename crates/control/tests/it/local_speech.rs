use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use crate::common::{Server, eventually, serve};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const MODEL: &str = "example/speech-small";
const RECORDING: &[u8] = b"RIFF-pretend-wav-bytes";

#[derive(Default)]
struct Hub {
    fetched: Mutex<Vec<String>>,
    held: Arc<tokio::sync::Notify>,
    hold_weights: bool,
}

#[derive(Default)]
struct Runner {
    heard: Mutex<Vec<(String, Vec<u8>)>>,
}

fn hub(state: Arc<Hub>) -> Router {
    async fn listing(Path((owner, name)): Path<(String, String)>) -> impl IntoResponse {
        if format!("{owner}/{name}") != MODEL {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Repository not found"})));
        }

        let siblings = json!([
            {"rfilename": "config.json", "size": 2},
            {"rfilename": "model.safetensors", "size": 6},
            {"rfilename": "pytorch_model.bin", "size": 6},
            {"rfilename": "onnx/model.onnx", "size": 6},
            {"rfilename": ".gitattributes", "size": 1},
        ]);

        (StatusCode::OK, Json(json!({"id": MODEL, "siblings": siblings})))
    }

    async fn file(State(state): State<Arc<Hub>>, Path((_, _, file)): Path<(String, String, String)>) -> Vec<u8> {
        state.fetched.lock().unwrap().push(file.clone());

        if state.hold_weights && file == "model.safetensors" {
            state.held.notified().await;
        }

        match file.as_str() {
            "config.json" => b"{}".to_vec(),
            _ => b"weight".to_vec(),
        }
    }

    Router::new().route("/api/models/{owner}/{name}", get(listing)).route("/{owner}/{name}/resolve/main/{file}", get(file)).with_state(state)
}

fn runner(state: Arc<Runner>) -> Router {
    async fn transcribe(State(state): State<Arc<Runner>>, Query(query): Query<HashMap<String, String>>, body: Bytes) -> Json<Value> {
        state.heard.lock().unwrap().push((query.get("model").cloned().unwrap_or_default(), body.to_vec()));

        Json(json!({"text": " heard locally "}))
    }

    Router::new().route("/transcribe", post(transcribe)).with_state(state)
}

fn speech_provider() -> Router {
    Router::new()
        .route("/v1/models", get(|| async { Json(json!({"data": [{"id": "whisper-1"}]})) }))
        .route("/v1/audio/transcriptions", post(|| async { Json(json!({"text": "heard by the provider"})) }))
}

async fn start(hub_state: Arc<Hub>, runner_state: Option<Arc<Runner>>) -> Server {
    let hub_url = serve(hub(hub_state)).await;
    let runner_url = match runner_state {
        Some(state) => Some(serve(runner(state)).await),
        None => None,
    };
    let server = Server::start_with(move |config| {
        config.huggingface_url = hub_url;
        config.local_speech_url = runner_url;
    })
    .await;
    sqlx::query("delete from user_settings").execute(&server.pool).await.unwrap();
    let (status, _) = server.user.json(server.user.put("/api/v1/experiments").json(&json!({"voice": true}))).await;
    assert_eq!(status, 200);

    server
}

async fn choose_local(server: &Server, model_id: &str) -> (u16, Value) {
    server.user.json(server.user.put("/api/v1/speech/local").json(&json!({"model_id": model_id}))).await
}

async fn local_when(server: &Server, state: &'static str) -> Value {
    eventually(state, || async {
        let (_, privacy) = server.user.json(server.user.get("/api/v1/privacy")).await;

        (privacy["speech_to_text_local"]["state"] == state).then(|| privacy["speech_to_text_local"].clone())
    })
    .await
}

async fn transcribe(server: &Server) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/transcriptions").header("content-type", "audio/wav").body(RECORDING.to_vec())).await
}

#[tokio::test]
async fn a_local_model_downloads_its_top_level_files_once_and_is_reused() {
    let hub_state = Arc::new(Hub::default());
    let server = start(hub_state.clone(), None).await;

    let (status, chosen) = choose_local(&server, MODEL).await;
    assert_eq!((status, chosen["speech_to_text_local"]["model_id"].clone()), (200, json!(MODEL)));

    let ready = local_when(&server, "ready").await;
    assert_eq!((ready["downloaded_bytes"].clone(), ready["total_bytes"].clone()), (json!(8), json!(8)));
    assert!(server.data_dir.join("speech-models/example--speech-small/model.safetensors").exists());

    let provider_base = serve(speech_provider()).await;
    let (_, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Speech", "base_url": format!("{provider_base}/v1")}))).await;
    server.user.json(server.user.put("/api/v1/speech/choices/speech_to_text").json(&json!({"provider_id": provider["id"], "model": "whisper-1"}))).await;
    let (_, privacy) = server.user.json(server.user.get("/api/v1/privacy")).await;
    assert_eq!(privacy["speech_to_text_local"], Value::Null);

    let (_, again) = choose_local(&server, MODEL).await;
    assert_eq!(again["speech_to_text_local"]["state"], json!("ready"));

    let mut fetched = hub_state.fetched.lock().unwrap().clone();
    fetched.sort();
    assert_eq!(fetched, ["config.json", "model.safetensors"]);
}

#[tokio::test]
async fn transcription_goes_to_the_local_model_when_it_is_chosen_and_to_the_provider_otherwise() {
    let runner_state = Arc::new(Runner::default());
    let server = start(Arc::default(), Some(runner_state.clone())).await;
    let provider_base = serve(speech_provider()).await;
    let (_, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Speech", "base_url": format!("{provider_base}/v1")}))).await;

    choose_local(&server, MODEL).await;
    local_when(&server, "ready").await;

    let (status, reply) = transcribe(&server).await;
    assert_eq!((status, reply["text"].clone()), (200, json!("heard locally")));
    assert_eq!(runner_state.heard.lock().unwrap().clone(), vec![("example--speech-small".to_string(), RECORDING.to_vec())]);

    server.user.json(server.user.put("/api/v1/speech/choices/speech_to_text").json(&json!({"provider_id": provider["id"], "model": "whisper-1"}))).await;
    let (status, reply) = transcribe(&server).await;
    assert_eq!((status, reply["text"].clone()), (200, json!("heard by the provider")));
    assert_eq!(runner_state.heard.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_model_hugging_face_does_not_have_fails_with_the_reason_and_can_be_tried_again() {
    let server = start(Arc::default(), None).await;

    choose_local(&server, "example/no-such-model").await;
    let failed = local_when(&server, "failed").await;
    assert_eq!(failed["failure"], json!("Hugging Face has no model called example/no-such-model. Check the id and try again."));

    let (status, retried) = choose_local(&server, "example/no-such-model").await;
    assert_eq!(status, 200);
    assert_eq!((retried["speech_to_text_local"]["state"].clone(), retried["speech_to_text_local"]["failure"].clone()), (json!("downloading"), Value::Null));
    local_when(&server, "failed").await;
}

#[tokio::test]
async fn a_model_id_that_is_not_owner_and_name_is_refused() {
    let server = start(Arc::default(), None).await;

    for entered in ["", "speech-small", "example/../../etc", "a/b/c"] {
        let (status, refused) = choose_local(&server, entered).await;
        assert_eq!((status, refused["error"].clone()), (400, json!("local_model_invalid")), "{entered}");
    }
}

#[tokio::test]
async fn removing_the_local_model_deletes_its_files_and_the_choice() {
    let server = start(Arc::default(), None).await;
    choose_local(&server, MODEL).await;
    local_when(&server, "ready").await;

    let (status, privacy) = server.user.json(server.user.delete("/api/v1/speech/local")).await;
    assert_eq!((status, privacy["speech_to_text_local"].clone()), (200, Value::Null));
    assert!(!server.data_dir.join("speech-models/example--speech-small").exists());
}

#[tokio::test]
async fn a_local_model_without_the_speech_runner_says_how_to_set_it_up() {
    let server = start(Arc::default(), None).await;
    choose_local(&server, MODEL).await;
    local_when(&server, "ready").await;

    let (status, refused) = transcribe(&server).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("local_speech_unavailable")));
    assert!(refused["message"].as_str().unwrap().contains("MYCELIUM_LOCAL_SPEECH_URL"));
}

#[tokio::test]
async fn choosing_a_provider_during_a_download_stops_it_and_keeps_nothing() {
    let hub_state = Arc::new(Hub { hold_weights: true, ..Hub::default() });
    let server = start(hub_state.clone(), None).await;
    let provider_base = serve(speech_provider()).await;
    let (_, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Speech", "base_url": format!("{provider_base}/v1")}))).await;

    choose_local(&server, MODEL).await;
    let held = hub_state.clone();
    eventually("the weights to be asked for", || {
        let held = held.clone();

        async move { held.fetched.lock().unwrap().iter().any(|file| file == "model.safetensors").then_some(()) }
    })
    .await;
    let (status, refused) = transcribe(&server).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("local_model_downloading")));

    server.user.json(server.user.put("/api/v1/speech/choices/speech_to_text").json(&json!({"provider_id": provider["id"], "model": "whisper-1"}))).await;
    hub_state.held.notify_one();

    let models = server.data_dir.join("speech-models");
    eventually("the partial download to go", || {
        let models = models.clone();

        async move { (std::fs::read_dir(&models).map(|entries| entries.count()).unwrap_or(0) == 0).then_some(()) }
    })
    .await;
}
