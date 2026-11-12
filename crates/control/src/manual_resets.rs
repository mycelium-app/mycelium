use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::store::{enum_text, node_summaries, publish_summaries};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, Utc};
use mycelium_proto::api::{AddManualResetRequest, NodeSummary};
use mycelium_proto::conversation::{AgentId, ResetCredit, ResetCredits};
use uuid::Uuid;

type ResetRow = (Uuid, String, Option<String>, DateTime<Utc>);

fn credit_of((id, kind, model, expires_at): ResetRow) -> ResetCredit {
    let title = match (kind.as_str(), model) {
        ("model", Some(model)) => {
            let mut letters = model.chars();
            let first: String = letters.next().map(|letter| letter.to_uppercase().collect()).unwrap_or_default();

            format!("{first}{} reset", letters.as_str())
        }
        _ => "Full reset".to_string(),
    };

    ResetCredit { id: id.to_string(), expires_at: Some(expires_at), title: Some(title), manual: true }
}

pub async fn add(State(state): State<AppState>, user: AuthUser, Json(request): Json<AddManualResetRequest>) -> ApiResult<(StatusCode, Json<ResetCredit>)> {
    if request.agent != AgentId::Claude {
        return Err(ApiError::bad_request("reset_unsupported", "Only Claude accounts take recorded resets."));
    }

    let account = request.account.trim();

    if account.is_empty() {
        return Err(ApiError::bad_request("account_required", "Choose the account to change."));
    }

    let model = request.model.as_deref().map(str::trim).filter(|model| !model.is_empty());

    match (request.kind.as_str(), model) {
        ("full", _) | ("model", Some(_)) => {}
        ("model", None) => return Err(ApiError::bad_request("reset_model_required", "Enter the model the reset is for.")),
        _ => return Err(ApiError::bad_request("reset_kind_invalid", "Select a kind of reset.")),
    }

    let Ok(day) = NaiveDate::parse_from_str(request.expires_on.trim(), "%Y-%m-%d") else {
        return Err(ApiError::bad_request("reset_date_invalid", "Enter a real date, like 27 11 2026."));
    };
    let zone = crate::wellness::zone_of(&state, user.id, None).await?;

    if day <= Utc::now().with_timezone(&zone).date_naive() {
        return Err(ApiError::bad_request("reset_date_past", "Expiry date must be in the future."));
    }

    let end_of_day = day.and_hms_opt(23, 59, 59).and_then(|moment| moment.and_local_timezone(zone).latest()).map(|moment| moment.with_timezone(&Utc));
    let Some(expires_at) = end_of_day else {
        return Err(ApiError::bad_request("reset_date_invalid", "Enter a real date, like 27 11 2026."));
    };
    let stored_model = model.filter(|_| request.kind == "model");
    let row = sqlx::query_as::<_, ResetRow>(
        "insert into manual_resets (owner, agent, account, kind, model, expires_at) values ($1, $2, $3, $4, $5, $6) returning id, kind, model, expires_at",
    )
    .bind(user.id)
    .bind(enum_text(&request.agent))
    .bind(account)
    .bind(&request.kind)
    .bind(stored_model)
    .bind(expires_at)
    .fetch_one(&state.pool)
    .await?;

    let summaries = node_summaries(&state, None).await?;
    publish_summaries(&state, summaries, Some(user.id)).await?;

    Ok((StatusCode::CREATED, Json(credit_of(row))))
}

pub async fn merge_into(state: &AppState, owner: Uuid, summaries: &mut [NodeSummary]) -> ApiResult<()> {
    let rows = sqlx::query_as::<_, (String, Uuid, String, Option<String>, DateTime<Utc>)>(
        "select account, id, kind, model, expires_at from manual_resets where owner = $1 and agent = 'claude' and used_at is null and expires_at > now() order by expires_at, created_at",
    )
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    for login in summaries.iter_mut().flat_map(|summary| summary.logins.iter_mut()).filter(|login| login.agent == AgentId::Claude) {
        let account = login.account.as_deref().unwrap_or_default();
        let credits: Vec<ResetCredit> = rows.iter().filter(|row| row.0 == account).map(|row| credit_of((row.1, row.2.clone(), row.3.clone(), row.4))).collect();

        if !credits.is_empty() {
            login.reset_credits = Some(ResetCredits { available_count: credits.len() as u32, credits: Some(credits) });
        }
    }

    Ok(())
}

pub async fn remove(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let deleted = sqlx::query("delete from manual_resets where id = $1 and owner = $2").bind(id).bind(user.id).execute(&state.pool).await?;

    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("That reset"));
    }

    let summaries = node_summaries(&state, None).await?;
    publish_summaries(&state, summaries, Some(user.id)).await?;

    Ok(StatusCode::NO_CONTENT)
}
