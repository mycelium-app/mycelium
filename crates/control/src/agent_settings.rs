use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{enum_text, parse_enum};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use mycelium_proto::api::ModelChoice;
use mycelium_proto::conversation::AgentId;
use mycelium_proto::routing::{AccountMode, AgentSettings, DefaultModes, SaveAgentSettingsRequest, SetAccountModeRequest};
use sqlx::types::Json as Sql;
use uuid::Uuid;

const CLAUDE_MODES: [&str; 4] = ["default", "plan", "acceptEdits", "bypassPermissions"];
const CODEX_MODES: [&str; 3] = ["read-only", "auto", "full-access"];
const ANY_MODEL_LIMIT: usize = 20;

fn modes_of(agent: AgentId) -> &'static [&'static str] {
    match agent {
        AgentId::Claude => &CLAUDE_MODES,
        AgentId::Codex => &CODEX_MODES,
    }
}

fn agent_name(agent: AgentId) -> &'static str {
    match agent {
        AgentId::Claude => "Claude Code",
        AgentId::Codex => "Codex",
    }
}

fn valid_mode(agent: AgentId, mode: &str) -> ApiResult<()> {
    if modes_of(agent).contains(&mode) {
        return Ok(());
    }

    let choices = match agent {
        AgentId::Claude => "Default, Plan, Auto or Bypass",
        AgentId::Codex => "Default, Auto or Bypass",
    };

    Err(ApiError::bad_request("mode_invalid", format!("Choose {choices} as the default mode for {}.", agent_name(agent))))
}

fn valid_entry(entry: &ModelChoice) -> ApiResult<()> {
    let kinds = [entry.agent.is_some(), entry.provider_id.is_some()].into_iter().filter(|kind| *kind).count();

    if kinds != 1 {
        return Err(ApiError::bad_request("any_model_entry_invalid", "Each entry in the Any model list is either Claude Code, Codex or a chat model. Remove the entry and add it again."));
    }

    Ok(())
}

pub async fn get(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<AgentSettings>> {
    Ok(Json(read(&state, user.id).await?))
}

pub async fn read(state: &AppState, owner: Uuid) -> ApiResult<AgentSettings> {
    let stored = sqlx::query_as::<_, (Option<String>, Option<String>, Sql<Vec<ModelChoice>>)>("select default_mode_claude, default_mode_codex, any_model from user_settings where owner = $1").bind(owner).fetch_optional(&state.pool).await?;
    let (claude, codex, any_model) = stored.map_or((None, None, Vec::new()), |(claude, codex, any_model)| (claude, codex, any_model.0));
    let overrides = sqlx::query_as::<_, (String, String, String)>("select agent, account, default_mode from account_settings where owner = $1 and default_mode is not null order by agent, account").bind(owner).fetch_all(&state.pool).await?;
    let mut account_modes = Vec::new();

    for (agent, account, mode) in overrides {
        account_modes.push(AccountMode { agent: parse_enum(&agent)?, account, mode });
    }

    Ok(AgentSettings { default_modes: DefaultModes { claude, codex }, account_modes, any_model })
}

pub async fn save(State(state): State<AppState>, user: AuthUser, Json(request): Json<SaveAgentSettingsRequest>) -> ApiResult<Json<AgentSettings>> {
    if let Some(mode) = &request.default_modes.claude {
        valid_mode(AgentId::Claude, mode)?;
    }

    if let Some(mode) = &request.default_modes.codex {
        valid_mode(AgentId::Codex, mode)?;
    }

    if request.any_model.len() > ANY_MODEL_LIMIT {
        return Err(ApiError::bad_request("any_model_too_long", format!("The Any model list holds up to {ANY_MODEL_LIMIT} entries. Remove one to add another.")));
    }

    for entry in &request.any_model {
        valid_entry(entry)?;
    }

    sqlx::query(
        "insert into user_settings (owner, default_mode_claude, default_mode_codex, any_model) values ($1, $2, $3, $4)
         on conflict (owner) do update set default_mode_claude = excluded.default_mode_claude, default_mode_codex = excluded.default_mode_codex, any_model = excluded.any_model",
    )
    .bind(user.id)
    .bind(&request.default_modes.claude)
    .bind(&request.default_modes.codex)
    .bind(Sql(&request.any_model))
    .execute(&state.pool)
    .await?;

    Ok(Json(read(&state, user.id).await?))
}

pub async fn set_account_mode(State(state): State<AppState>, user: AuthUser, Json(request): Json<SetAccountModeRequest>) -> ApiResult<StatusCode> {
    let account = request.account.trim();

    if account.is_empty() {
        return Err(ApiError::bad_request("account_required", "Choose the account to change."));
    }

    if let Some(mode) = &request.mode {
        valid_mode(request.agent, mode)?;
    }

    sqlx::query(
        "insert into account_settings (owner, agent, account, default_mode) values ($1, $2, $3, $4)
         on conflict (owner, agent, account) do update set default_mode = excluded.default_mode",
    )
    .bind(user.id)
    .bind(enum_text(&request.agent))
    .bind(account)
    .bind(&request.mode)
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn default_mode(state: &AppState, owner: Uuid, agent: AgentId, account: Option<&str>) -> ApiResult<Option<String>> {
    let overridden = match account {
        Some(account) => sqlx::query_scalar::<_, Option<String>>("select default_mode from account_settings where owner = $1 and agent = $2 and account = $3").bind(owner).bind(enum_text(&agent)).bind(account).fetch_optional(&state.pool).await?.flatten(),
        None => None,
    };

    if overridden.is_some() {
        return Ok(overridden);
    }

    let column = match agent {
        AgentId::Claude => "default_mode_claude",
        AgentId::Codex => "default_mode_codex",
    };
    let chosen = sqlx::query_scalar::<_, Option<String>>(sqlx::AssertSqlSafe(format!("select {column} from user_settings where owner = $1"))).bind(owner).fetch_optional(&state.pool).await?.flatten();

    Ok(chosen)
}

pub async fn apply_default_mode(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let row = crate::store::find_owned(&state.pool, conversation_id, owner).await?;

    if row.side_kind.as_deref() == Some("subagent") {
        return Ok(());
    }

    let Ok(agent) = parse_enum::<AgentId>(&row.agent) else {
        return Ok(());
    };
    let Some(wanted) = default_mode(state, owner, agent, row.account.as_deref()).await? else {
        return Ok(());
    };
    let offered = row.options.as_ref().and_then(|options| options["modes"]["availableModes"].as_array()).is_some_and(|modes| modes.iter().any(|mode| mode["id"].as_str() == Some(wanted.as_str())));

    if !offered || row.mode.as_deref() == Some(wanted.as_str()) {
        return Ok(());
    }

    crate::prompts::set_config(state, owner, conversation_id, mycelium_proto::api::ConfigRequest { mode: Some(wanted), model: None, config: None }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_must_belong_to_its_agent() {
        assert!(valid_mode(AgentId::Claude, "plan").is_ok());
        assert!(valid_mode(AgentId::Codex, "plan").is_err());
        assert!(valid_mode(AgentId::Codex, "full-access").is_ok());
    }

    #[test]
    fn an_any_model_entry_names_one_kind_of_choice() {
        let both = ModelChoice { agent: Some(AgentId::Claude), provider_id: Some(Uuid::nil()), model: None, account: None, any_model: false };
        let neither = ModelChoice { agent: None, provider_id: None, model: None, account: None, any_model: false };

        assert!(valid_entry(&both).is_err());
        assert!(valid_entry(&neither).is_err());
    }
}
