//! `/sendspin` on the API's port: a Sendspin connection that first proves who it is with an
//! API token, as Music Assistant's proxy requires, so that it works wherever the API does.
//! The player it registers belongs to that token's user.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use crate::Api;

/// How long a client has to send its `auth` frame.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// The close code Music Assistant uses for every authentication failure.
const AUTH_FAILED: u16 = 4001;

#[derive(Deserialize)]
struct AuthFrame {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    token: Option<String>,
}

pub async fn upgrade(State(api): State<Arc<Api>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| session(api, socket))
}

async fn reject(mut socket: WebSocket, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: AUTH_FAILED,
            reason: reason.into(),
        })))
        .await;
}

async fn session(api: Arc<Api>, mut socket: WebSocket) {
    let first = match tokio::time::timeout(AUTH_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(message))) => message,
        Ok(_) => return,
        Err(_) => return reject(socket, "Authentication timeout").await,
    };
    let Message::Text(text) = first else {
        return reject(socket, "Expected text message for auth").await;
    };
    let Ok(frame) = serde_json::from_str::<AuthFrame>(&text) else {
        return reject(socket, "First message must be auth").await;
    };
    if frame.kind != "auth" {
        return reject(socket, "First message must be auth").await;
    }
    let Some(token) = frame.token else {
        return reject(socket, "Token required in auth message").await;
    };
    let Ok(session) = api.auth.authenticate(&token).await else {
        return reject(socket, "Invalid or expired token").await;
    };
    let ok = json!({"type": "auth_ok"}).to_string();
    if socket.send(Message::Text(ok.into())).await.is_err() {
        return;
    }
    api.sendspin.serve(socket, Some(session.token_id)).await;
}
