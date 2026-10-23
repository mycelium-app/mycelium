use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mycelium_proto::api::{SessionKind, SessionSummary};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct EndQuery {
    #[serde(default)]
    confirm_current: bool,
}

fn browser_label(user_agent: &str) -> String {
    let browser = ["Firefox", "Edg", "Chrome", "Safari"].into_iter().find(|name| user_agent.contains(name)).map(|name| if name == "Edg" { "Edge" } else { name });
    let system = ["Android", "iPhone", "iPad", "Windows", "Mac OS X", "CrOS", "Linux"].into_iter().find(|name| user_agent.contains(name)).map(|name| if name == "Mac OS X" { "macOS" } else { name });

    match (browser, system) {
        (Some(browser), Some(system)) => format!("{browser} on {system}"),
        (Some(browser), None) => browser.to_string(),
        _ => "Browser".to_string(),
    }
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<SessionSummary>>> {
    let browsers = sqlx::query_as::<_, (Uuid, String, String, chrono::DateTime<chrono::Utc>)>(
        "select id, user_agent, address, coalesce(last_used_at, created_at) from web_sessions where user_id = $1 and expires_at > now()",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let clients = sqlx::query_as::<_, (Uuid, String, String, String, chrono::DateTime<chrono::Utc>)>(
        "select id, label, hostname, address, coalesce(last_used_at, created_at) from client_tokens where user_id = $1 and revoked_at is null",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    let mut sessions: Vec<SessionSummary> = Vec::new();

    for (id, user_agent, address, last_used_at) in browsers {
        sessions.push(SessionSummary { id, kind: SessionKind::Browser, label: browser_label(&user_agent), hostname: String::new(), address, last_used_at, current: user.session == Some(id) });
    }

    for (id, label, hostname, address, last_used_at) in clients {
        sessions.push(SessionSummary { id, kind: SessionKind::Client, label, hostname, address, last_used_at, current: user.session == Some(id) });
    }

    sessions.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_used_at.cmp(&a.last_used_at)));

    Ok(Json(sessions))
}

pub async fn end(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<EndQuery>) -> ApiResult<StatusCode> {
    if user.session == Some(id) && !query.confirm_current {
        return Err(ApiError::conflict("current_session", "This is the session you are using. Confirm to sign out of it."));
    }

    let browser = sqlx::query("delete from web_sessions where id = $1 and user_id = $2").bind(id).bind(user.id).execute(&state.pool).await?;
    let client = sqlx::query("update client_tokens set revoked_at = now() where id = $1 and user_id = $2 and revoked_at is null").bind(id).bind(user.id).execute(&state.pool).await?;

    if browser.rows_affected() + client.rows_affected() == 0 {
        return Err(ApiError::not_found("That session"));
    }

    Ok(StatusCode::NO_CONTENT)
}
