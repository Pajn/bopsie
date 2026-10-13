//! One `/ws` connection.
//!
//! The server speaks first with server info. The client then sends commands as
//! `{message_id, command, args}` and gets `{message_id, result, partial}` or
//! `{message_id, error_code, details}` back. Commands run concurrently, so answers can arrive
//! out of order; the `message_id` ties them together. The `auth` command authenticates the
//! connection and is handled here, in order, because it changes who later commands run as.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use bopsie_core::Error;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;

use crate::Api;
use crate::auth::Session;
use crate::command::{Context, authentication_required};
use crate::info::server_info;

/// How long a closing connection waits for answers already being computed.
const DRAIN: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct CommandMessage {
    /// Any JSON value; it is echoed back as it came.
    message_id: Value,
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Deserialize)]
struct AuthArgs {
    token: Option<String>,
    access_token: Option<String>,
}

fn result_frame(message_id: &Value, result: Value) -> String {
    json!({"message_id": message_id, "result": result, "partial": false}).to_string()
}

fn error_frame(message_id: &Value, error: &Error) -> String {
    json!({
        "message_id": message_id,
        "error_code": error.code.as_u16(),
        "details": error.details,
    })
    .to_string()
}

pub async fn serve(api: Arc<Api>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    let (info, has_users) = match (server_info(&api).await, api.auth.has_users().await) {
        (Ok(info), Ok(has_users)) => (info, has_users),
        _ => return,
    };
    let info = serde_json::to_string(&info).expect("server info serializes");
    if sink.send(Message::Text(info.into())).await.is_err() {
        return;
    }
    if !has_users {
        let frame =
            json!({"message_id": "connection", "error_code": 503, "details": "Setup required"});
        let _ = sink.send(Message::Text(frame.to_string().into())).await;
        let _ = sink.close().await;
        return;
    }

    // Answers come from many tasks; one writer owns the socket.
    let (out, mut outbox) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(frame) = outbox.recv().await {
            if sink.send(Message::Text(frame.into())).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });

    let mut session: Option<Session> = None;
    let mut revocations = api.auth.revocations();
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    receive(&api, &text, &mut session, &out, &mut running).await;
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            revoked = revocations.recv() => match revoked {
                Ok(token_id) if session.as_ref().is_some_and(|s| s.token_id == token_id) => break,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            Some(_) = running.join_next(), if !running.is_empty() => {}
        }
    }

    // Let answers already on their way out reach the client, then stop the rest.
    drop(out);
    let _ = tokio::time::timeout(DRAIN, async {
        while running.join_next().await.is_some() {}
    })
    .await;
    running.abort_all();
    let _ = tokio::time::timeout(DRAIN, writer).await;
}

async fn receive(
    api: &Arc<Api>,
    text: &str,
    session: &mut Option<Session>,
    out: &mpsc::Sender<String>,
    running: &mut JoinSet<()>,
) {
    let message: CommandMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            let message_id = serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| value.get("message_id").cloned());
            match message_id {
                Some(message_id) => {
                    let error = Error::invalid_data(format!("Invalid message: {error}"));
                    let _ = out.send(error_frame(&message_id, &error)).await;
                }
                None => tracing::debug!(%error, "ignoring a frame that is not a command"),
            }
            return;
        }
    };

    if message.command == "auth" {
        let frame = match authenticate(api, message.args).await {
            Ok(authenticated) => {
                let user = serde_json::to_value(&authenticated.user).expect("users serialize");
                *session = Some(authenticated);
                result_frame(
                    &message.message_id,
                    json!({"authenticated": true, "user": user}),
                )
            }
            Err(error) => error_frame(&message.message_id, &error),
        };
        let _ = out.send(frame).await;
        return;
    }

    let ctx = Context {
        api: api.clone(),
        session: session.clone(),
    };
    let api = api.clone();
    let out = out.clone();
    running.spawn(async move {
        let frame = match api.commands.call(ctx, &message.command, message.args).await {
            Ok(result) => result_frame(&message.message_id, result),
            Err(error) => error_frame(&message.message_id, &error),
        };
        let _ = out.send(frame).await;
    });
}

async fn authenticate(api: &Api, args: Value) -> Result<Session, Error> {
    let args: AuthArgs = serde_json::from_value(args).map_err(|_| authentication_required())?;
    let token = args
        .token
        .or(args.access_token)
        .ok_or_else(authentication_required)?;
    api.auth.authenticate(&token).await
}
