use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{FromRequestParts, MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Experiment {
    Voice,
}

#[derive(Serialize, Deserialize, Default, Clone, Copy)]
#[serde(default)]
pub struct Experiments {
    pub tasks: bool,
    pub workflows: bool,
    pub instructions: bool,
    pub details: bool,
    pub subagents: bool,
    pub walkthrough: bool,
    pub move_chats: bool,
    pub groups: bool,
    pub continue_elsewhere: bool,
    pub assistants: bool,
    pub memory_banks: bool,
    pub chat_across_groups: bool,
    pub voice: bool,
    pub kubernetes_nodes: bool,
}

impl Experiments {
    pub fn graduated(voice: bool, kubernetes_nodes: bool) -> Self {
        Self {
            tasks: true,
            workflows: true,
            instructions: true,
            details: true,
            subagents: true,
            walkthrough: true,
            move_chats: true,
            groups: true,
            continue_elsewhere: true,
            assistants: true,
            memory_banks: true,
            chat_across_groups: true,
            voice,
            kubernetes_nodes,
        }
    }

    pub fn allows(&self, experiment: Experiment) -> bool {
        match experiment {
            Experiment::Voice => self.voice,
        }
    }
}

pub fn disabled_message(experiment: Experiment) -> &'static str {
    match experiment {
        Experiment::Voice => "Enable the Voice experiment in Settings, Experiments to use this.",
    }
}

pub fn refusal(experiment: Experiment) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "experiment_disabled", disabled_message(experiment))
}

pub async fn of(state: &AppState, owner: Uuid) -> ApiResult<Experiments> {
    let settings = sqlx::query_as::<_, (bool, bool)>("select experiment_voice, experiment_kubernetes_nodes from user_settings where owner = $1").bind(owner).fetch_optional(&state.pool).await?;
    let (voice, kubernetes_nodes) = settings.unwrap_or((false, false));

    Ok(Experiments::graduated(voice, kubernetes_nodes))
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Experiments>> {
    Ok(Json(of(&state, user.id).await?))
}

pub async fn save(State(state): State<AppState>, user: AuthUser, Json(request): Json<Experiments>) -> ApiResult<Json<Experiments>> {
    user.require_full_access()?;
    sqlx::query("insert into user_settings (owner, experiment_voice, experiment_kubernetes_nodes) values ($1, $2, $3) on conflict (owner) do update set experiment_voice = excluded.experiment_voice, experiment_kubernetes_nodes = excluded.experiment_kubernetes_nodes")
        .bind(user.id)
        .bind(request.voice)
        .bind(request.kubernetes_nodes)
        .execute(&state.pool)
        .await?;

    Ok(Json(Experiments::graduated(request.voice, request.kubernetes_nodes)))
}

fn gated_by(route: &str) -> Option<Experiment> {
    let tail = route.strip_prefix("/api/v1/")?;
    let first = tail.split('/').next().unwrap_or_default();

    match first {
        "transcriptions" => Some(Experiment::Voice),
        _ => None,
    }
}

pub async fn gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let route = request.extensions().get::<MatchedPath>().map(|matched| matched.as_str().to_string()).unwrap_or_default();
    let Some(experiment) = gated_by(&route) else {
        return next.run(request).await;
    };

    let (mut parts, body) = request.into_parts();
    let Ok(user) = AuthUser::from_request_parts(&mut parts, &state).await else {
        return next.run(Request::from_parts(parts, body)).await;
    };

    let talking = crate::voice::enabled(&state, user.id).await.unwrap_or(false);

    match of(&state, user.id).await {
        Ok(enabled) if enabled.allows(experiment) || talking => next.run(Request::from_parts(parts, body)).await,
        Ok(_) => refusal(experiment).into_response(),
        Err(error) => error.into_response(),
    }
}
