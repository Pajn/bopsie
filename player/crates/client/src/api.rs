//! The server's WebSocket command API at `/ws`, as much of it as signing in needs.
//!
//! The server opens with its server info. Commands carry a `message_id` the answer repeats;
//! `auth` authenticates the connection with a token. Events and answers to other commands are
//! skipped, since one command is in flight at a time.

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::Error;
use crate::address::{normalize_server_url, socket_url};

/// The oldest server schema this client understands, as for the app.
const MIN_SCHEMA_VERSION: u32 = 28;

#[derive(Debug, Clone, Deserialize)]
struct ServerInfo {
    server_id: String,
    schema_version: u32,
}

pub(crate) struct CommandSocket {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    info: ServerInfo,
    next_id: u64,
}

impl CommandSocket {
    pub(crate) async fn open(server_url: &str) -> Result<Self, Error> {
        let (mut socket, _) = connect_async(socket_url(server_url, "/ws")?)
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;
        let info = loop {
            let message = next_json(&mut socket).await?;
            if message.get("message_id").and_then(Value::as_str) == Some("connection")
                && message.get("error_code").and_then(Value::as_u64) == Some(503)
            {
                return Err(Error::SetupRequired);
            }
            if message.get("server_id").is_some() {
                break serde_json::from_value::<ServerInfo>(message)
                    .map_err(|e| Error::Protocol(format!("server info: {e}")))?;
            }
        };
        Ok(Self {
            socket,
            info,
            next_id: 0,
        })
    }

    pub(crate) async fn authenticate(&mut self, token: &str) -> Result<(), Error> {
        self.send("auth", "auth", json!({ "token": token })).await?;
        self.answer("auth")
            .await
            .map(drop)
            .map_err(|error| match error {
                Error::Command { .. } => Error::Unauthorized,
                other => other,
            })
    }

    pub(crate) async fn command(&mut self, command: &str, args: Value) -> Result<Value, Error> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        self.send(&id, command, args).await?;
        self.answer(&id).await
    }

    async fn send(&mut self, id: &str, command: &str, args: Value) -> Result<(), Error> {
        let text = json!({ "message_id": id, "command": command, "args": args }).to_string();
        self.socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| Error::Connection(e.to_string()))
    }

    /// Waits for the answer to `id`, concatenating the batches of a streaming command.
    async fn answer(&mut self, id: &str) -> Result<Value, Error> {
        let mut partial = Vec::new();
        loop {
            let mut message = next_json(&mut self.socket).await?;
            if message.get("message_id").and_then(Value::as_str) != Some(id) {
                continue;
            }
            if let Some(code) = message.get("error_code") {
                return Err(Error::Command {
                    code: code.as_i64().unwrap_or_default(),
                    details: message
                        .get("details")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                });
            }
            let result = message
                .get_mut("result")
                .map(Value::take)
                .unwrap_or(Value::Null);
            let is_partial = message.get("partial").and_then(Value::as_bool) == Some(true);
            match result {
                Value::Array(items) if is_partial => partial.extend(items),
                Value::Array(items) if !partial.is_empty() => {
                    partial.extend(items);
                    return Ok(Value::Array(partial));
                }
                _ if is_partial => {}
                result => return Ok(result),
            }
        }
    }

    pub(crate) async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

async fn next_json(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<Value, Error> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|e| Error::Protocol(e.to_string()));
            }
            Some(Ok(Message::Close(frame))) => {
                let reason = frame.map(|f| f.reason.to_string()).unwrap_or_default();
                return Err(Error::Connection(format!("closed by the server: {reason}")));
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Error::Connection(e.to_string())),
            None => return Err(Error::Connection("closed".to_owned())),
        }
    }
}

/// A completed sign-in: the long-lived token and who it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignIn {
    /// The normalized server address.
    pub server_url: String,
    pub server_id: String,
    pub user_id: String,
    pub username: String,
    pub token: String,
}

#[derive(Deserialize)]
struct Login {
    success: bool,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    user: Option<User>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct User {
    user_id: String,
    username: String,
}

/// Signs in with a username and password, then trades the session for a long-lived token named
/// after the device, as the app does. `device_name` is what the server's list of sign-ins shows,
/// such as `Bopsie on a Mac`.
pub async fn sign_in(
    server_url: &str,
    username: &str,
    password: &str,
    device_name: &str,
) -> Result<SignIn, Error> {
    let server_url = normalize_server_url(server_url)?;
    let mut anonymous = CommandSocket::open(&server_url).await?;
    let result = sign_in_on(&mut anonymous, &server_url, username, password, device_name).await;
    anonymous.close().await;
    result
}

async fn sign_in_on(
    anonymous: &mut CommandSocket,
    server_url: &str,
    username: &str,
    password: &str,
    device_name: &str,
) -> Result<SignIn, Error> {
    if anonymous.info.schema_version < MIN_SCHEMA_VERSION {
        return Err(Error::ServerTooOld);
    }
    let login = anonymous
        .command(
            "auth/login",
            json!({ "username": username, "password": password, "device_name": device_name }),
        )
        .await?;
    let login: Login =
        serde_json::from_value(login).map_err(|e| Error::Protocol(format!("auth/login: {e}")))?;
    if !login.success {
        return Err(Error::WrongCredentials(
            login
                .error
                .unwrap_or_else(|| "Wrong username or password.".to_owned()),
        ));
    }
    let (Some(session_token), Some(user)) = (login.access_token, login.user) else {
        return Err(Error::Protocol(
            "auth/login succeeded without a token".to_owned(),
        ));
    };

    let mut session = CommandSocket::open(server_url).await?;
    let token = async {
        session.authenticate(&session_token).await?;
        let token = session
            .command("auth/token/create", json!({ "name": device_name }))
            .await?;
        // The session token has served its purpose; revoking it keeps the server's list of
        // sign-ins to one entry per device.
        let _ = session.command("auth/logout", json!({})).await;
        token
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Protocol("auth/token/create did not return a token".to_owned()))
    }
    .await;
    session.close().await;

    Ok(SignIn {
        server_url: server_url.to_owned(),
        server_id: anonymous.info.server_id.clone(),
        user_id: user.user_id,
        username: user.username,
        token: token?,
    })
}
