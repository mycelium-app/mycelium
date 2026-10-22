use crate::auth::AuthUser;
use crate::state::{AppState, Streamed};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use tokio::sync::broadcast::{self, error::RecvError};
use uuid::Uuid;

pub async fn connect(State(state): State<AppState>, user: AuthUser, upgrade: WebSocketUpgrade) -> Response {
    let receiver = state.stream.subscribe();

    upgrade.on_upgrade(move |socket| forward(user.id, receiver, socket))
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
