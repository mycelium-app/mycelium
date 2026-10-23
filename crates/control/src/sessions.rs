use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::message_proposals;
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

#[derive(sqlx::FromRow)]
struct ClientRow {
    id: Uuid,
    label: String,
    hostname: String,
    address: String,
    last_used_at: chrono::DateTime<chrono::Utc>,
    scopes: Option<Vec<String>>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    conversation_id: Option<Uuid>,
    conversation_title: Option<String>,
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
    let clients = sqlx::query_as::<_, ClientRow>(
        "select t.id, t.label, t.hostname, t.address, coalesce(t.last_used_at, t.created_at) as last_used_at, t.scopes, t.expires_at, t.conversation_id, c.title as conversation_title
         from client_tokens t left join conversations c on c.id = t.conversation_id
         where t.user_id = $1 and t.revoked_at is null and (t.expires_at is null or t.expires_at > now())",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    let mut sessions: Vec<SessionSummary> = Vec::new();

    for (id, user_agent, address, last_used_at) in browsers {
        sessions.push(SessionSummary { id, kind: SessionKind::Browser, label: browser_label(&user_agent), hostname: String::new(), address, last_used_at, current: user.session == Some(id), scopes: Vec::new(), expires_at: None, conversation_id: None, conversation_title: None });
    }

    for client in clients {
        let kind = if client.scopes.is_some() { SessionKind::Agent } else { SessionKind::Client };

        sessions.push(SessionSummary {
            id: client.id,
            kind,
            label: client.label,
            hostname: client.hostname,
            address: client.address,
            last_used_at: client.last_used_at,
            current: user.session == Some(client.id),
            scopes: client.scopes.unwrap_or_default(),
            expires_at: client.expires_at,
            conversation_id: client.conversation_id,
            conversation_title: client.conversation_id.map(|id| message_proposals::label(id, &client.conversation_title.unwrap_or_default())),
        });
    }

    sessions.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_used_at.cmp(&a.last_used_at)));

    Ok(Json(sessions))
}

pub async fn end(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<EndQuery>) -> ApiResult<StatusCode> {
    user.require_full_access()?;

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
