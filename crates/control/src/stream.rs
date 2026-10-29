use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use axum::http::header::ORIGIN;
use axum::http::{HeaderMap, StatusCode};
use crate::state::{AppState, Streamed};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use tokio::sync::broadcast::{self, error::RecvError};
use uuid::Uuid;

fn origin_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().map(|url| url.origin().ascii_serialization())
}

pub fn check_origin(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    if let Some(origin) = headers.get(ORIGIN) {
        let own = origin_of(&state.config.public_url);

        if origin.to_str().ok().map(str::to_string) != own {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "origin_not_allowed", "This page is not allowed to make that request. Use the address of the control plane itself."));
        }
    }

    Ok(())
}

pub async fn connect(State(state): State<AppState>, headers: HeaderMap, user: AuthUser, upgrade: WebSocketUpgrade) -> ApiResult<Response> {
    check_origin(&state, &headers)?;

    let receiver = state.stream.subscribe();

    Ok(upgrade.on_upgrade(move |socket| forward(state, user, receiver, socket)))
}

async fn session_is_live(state: &AppState, session: Uuid) -> bool {
    let live = sqlx::query_scalar::<_, bool>(
        "select exists (select 1 from web_sessions where id = $1 and expires_at > now())
             or exists (select 1 from client_tokens where id = $1 and revoked_at is null and (expires_at is null or expires_at > now()))",
    )
    .bind(session)
    .fetch_one(&state.pool)
    .await;

    live.unwrap_or(true)
}

async fn forward(state: AppState, user: AuthUser, mut receiver: broadcast::Receiver<Streamed>, mut socket: WebSocket) {
    let user_id = user.id;
    let mut recheck = tokio::time::interval(state.config.stream_recheck_interval);
    recheck.tick().await;

    loop {
        tokio::select! {
            _ = recheck.tick() => {
                if let Some(session) = user.session {
                    if !session_is_live(&state, session).await {
                        break;
                    }
                }
            },
            streamed = receiver.recv() => match streamed {
                Ok(streamed) => {
                    if streamed.owner.is_some_and(|owner| owner != user_id) {
                        continue;
                    }

                    let Ok(text) = serde_json::to_string(&streamed.message) else {
                        continue;
                    };

                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}
