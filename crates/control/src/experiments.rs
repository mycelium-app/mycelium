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
    Tasks,
    Workflows,
    Instructions,
    Walkthrough,
    MoveChats,
    Groups,
    ContinueElsewhere,
    Assistants,
    MemoryBanks,
    ChatAcrossGroups,
    Voice,
}

#[derive(Serialize, Deserialize, Default, Clone, Copy, sqlx::FromRow)]
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
}

impl Experiments {
    pub fn allows(&self, experiment: Experiment) -> bool {
        match experiment {
            Experiment::Tasks => self.tasks,
            Experiment::Workflows => self.workflows,
            Experiment::Instructions => self.instructions,
            Experiment::Walkthrough => self.walkthrough,
            Experiment::MoveChats => self.move_chats,
            Experiment::Groups => self.groups,
            Experiment::ContinueElsewhere => self.continue_elsewhere,
            Experiment::Assistants => self.assistants,
            Experiment::MemoryBanks => self.memory_banks,
            Experiment::ChatAcrossGroups => self.chat_across_groups,
            Experiment::Voice => self.voice,
        }
    }
}

pub fn disabled_message(experiment: Experiment) -> &'static str {
    match experiment {
        Experiment::Tasks => "Enable the Tasks experiment in Settings, Experiments to use this.",
        Experiment::Workflows => "Enable the Workflows experiment in Settings, Experiments to use this.",
        Experiment::Instructions => "Enable the Instructions experiment in Settings, Experiments to use this.",
        Experiment::Walkthrough => "Enable the Walkthroughs experiment in Settings, Experiments to use this.",
        Experiment::MoveChats => "Enable the Move chats experiment in Settings, Experiments to use this.",
        Experiment::Groups => "Enable the Groups experiment in Settings, Experiments to use this.",
        Experiment::ContinueElsewhere => "Enable the Continue elsewhere experiment in Settings, Experiments to use this.",
        Experiment::Assistants => "Enable the Assistants experiment in Settings, Experiments to use this.",
        Experiment::MemoryBanks => "Enable the Memory banks experiment in Settings, Experiments to use this.",
        Experiment::ChatAcrossGroups => "Enable the Chat across groups experiment in Settings, Experiments to use this.",
        Experiment::Voice => "Enable the Voice experiment in Settings, Experiments to use this.",
    }
}

pub fn refusal(experiment: Experiment) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "experiment_disabled", disabled_message(experiment))
}

pub async fn of(state: &AppState, owner: Uuid) -> ApiResult<Experiments> {
    let stored = sqlx::query_as::<_, Experiments>(
        "select experiment_tasks as tasks, experiment_workflows as workflows, experiment_instructions as instructions, experiment_details as details,
                experiment_subagents as subagents, experiment_walkthrough as walkthrough, experiment_move_chats as move_chats, experiment_groups as groups, experiment_continue_elsewhere as continue_elsewhere, experiment_assistants as assistants, experiment_memory_banks as memory_banks, experiment_chat_across_groups as chat_across_groups, experiment_voice as voice
         from user_settings where owner = $1",
    )
    .bind(owner)
    .fetch_optional(&state.pool)
    .await?;

    Ok(stored.unwrap_or_default())
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Experiments>> {
    Ok(Json(of(&state, user.id).await?))
}

pub async fn save(State(state): State<AppState>, user: AuthUser, Json(request): Json<Experiments>) -> ApiResult<Json<Experiments>> {
    user.require_full_access()?;
    sqlx::query(
        "insert into user_settings (owner, experiment_tasks, experiment_workflows, experiment_instructions, experiment_details, experiment_subagents, experiment_walkthrough, experiment_move_chats, experiment_groups, experiment_continue_elsewhere, experiment_assistants, experiment_memory_banks, experiment_chat_across_groups, experiment_voice)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         on conflict (owner) do update set experiment_tasks = excluded.experiment_tasks, experiment_workflows = excluded.experiment_workflows,
             experiment_instructions = excluded.experiment_instructions, experiment_details = excluded.experiment_details, experiment_subagents = excluded.experiment_subagents,
             experiment_walkthrough = excluded.experiment_walkthrough, experiment_move_chats = excluded.experiment_move_chats, experiment_groups = excluded.experiment_groups,
             experiment_continue_elsewhere = excluded.experiment_continue_elsewhere, experiment_assistants = excluded.experiment_assistants, experiment_memory_banks = excluded.experiment_memory_banks, experiment_chat_across_groups = excluded.experiment_chat_across_groups, experiment_voice = excluded.experiment_voice",
    )
    .bind(user.id)
    .bind(request.tasks)
    .bind(request.workflows)
    .bind(request.instructions)
    .bind(request.details)
    .bind(request.subagents)
    .bind(request.walkthrough)
    .bind(request.move_chats)
    .bind(request.groups)
    .bind(request.continue_elsewhere)
    .bind(request.assistants)
    .bind(request.memory_banks)
    .bind(request.chat_across_groups)
    .bind(request.voice)
    .execute(&state.pool)
    .await?;

    Ok(Json(request))
}

fn gated_by(route: &str) -> Option<Experiment> {
    let tail = route.strip_prefix("/api/v1/")?;
    let first = tail.split('/').next().unwrap_or_default();

    match first {
        "tasks" => Some(Experiment::Tasks),
        "workflows" => Some(Experiment::Workflows),
        "instructions" | "instruction-proposals" | "instruction-source" => Some(Experiment::Instructions),
        "groups" if route.ends_with("/instructions") => Some(Experiment::Groups),
        "assistants" => Some(Experiment::Assistants),
        "group-connections" => Some(Experiment::ChatAcrossGroups),
        "message-proposals" if route.ends_with("/connect-groups") => Some(Experiment::ChatAcrossGroups),
        "transcriptions" => Some(Experiment::Voice),
        "memory-banks" => Some(Experiment::MemoryBanks),
        "memories" if route.ends_with("/bank") => Some(Experiment::MemoryBanks),
        "walkthroughs" => Some(Experiment::Walkthrough),
        "conversations" if route.ends_with("/walkthrough") => Some(Experiment::Walkthrough),
        "conversations" if route.ends_with("/move") || route.ends_with("/move-plan") => Some(Experiment::MoveChats),
        "conversations" if route.ends_with("/continue") => Some(Experiment::ContinueElsewhere),
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

    match of(&state, user.id).await {
        Ok(enabled) if enabled.allows(experiment) => next.run(Request::from_parts(parts, body)).await,
        Ok(_) => refusal(experiment).into_response(),
        Err(error) => error.into_response(),
    }
}
