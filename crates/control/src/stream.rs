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
            return Err(ApiError::new(StatusCode::FORBIDDEN, "origin_not_allowed", "This page is not allowed to open the live stream. Use the address of the control plane itself."));
        }
    }

    Ok(())
}

pub async fn connect(State(state): State<AppState>, headers: HeaderMap, user: AuthUser, upgrade: WebSocketUpgrade) -> ApiResult<Response> {
    check_origin(&state, &headers)?;

    let receiver = state.stream.subscribe();

    Ok(upgrade.on_upgrade(move |socket| forward(user.id, receiver, socket)))
}

async fn forward(user_id: Uuid, mut receiver: broadcast::Receiver<Streamed>, mut socket: WebSocket) {
    loop {
        tokio::select! {
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
