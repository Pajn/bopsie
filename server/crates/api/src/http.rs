//! The HTTP routes. Status codes and bodies follow Music Assistant's webserver, including its
//! mix of JSON and plain-text errors, because scripts written against it check them.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bopsie_core::{Error, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::lifetime::TokenKind;
use crate::auth::{SetupError, User};
use crate::command::{Access, Context};
use crate::commands::auth::{LOGIN_FAILED, UNNAMED_DEVICE};
use crate::{Api, imageproxy, info, sendspin, web, ws};

pub fn router(api: Arc<Api>) -> Router {
    // The streams LinkPlay devices pull, on the API's port so that they reach the server
    // wherever the API does.
    let streams = api
        .linkplay
        .as_ref()
        .map(|linkplay| linkplay.hub().routes());
    let router = Router::new()
        .route("/info", get(server_info))
        .route("/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/api", post(command))
        .route("/api-docs/commands.json", get(commands_json))
        .route("/ws", get(websocket))
        .route("/imageproxy/{proxy_id}", get(imageproxy::serve))
        .route("/sendspin", get(sendspin::upgrade))
        .route("/spotify/callback", get(spotify_callback))
        .route(
            "/artwork",
            post(artwork).layer(axum::extract::DefaultBodyLimit::max(ARTWORK_LIMIT)),
        )
        .route("/backups/{name}", get(download_backup))
        .route(
            "/backups/restore",
            post(restore_backup).layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .fallback(web::fallback)
        .with_state(api);
    match streams {
        Some(streams) => router.merge(streams),
        None => router,
    }
}

type ApiState = State<Arc<Api>>;

#[derive(Deserialize)]
struct SpotifyCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Where Spotify sends the listener back to after logging in to the Web API. It needs no
/// token: the login's `state`, which only this server knows, vouches for it. Answers by going
/// back to the web UI's server page, saying how it went.
async fn spotify_callback(
    State(api): ApiState,
    axum::extract::Query(answer): axum::extract::Query<SpotifyCallback>,
) -> Response {
    let outcome = match (&api.spotify, answer) {
        (None, _) => Err("Spotify is turned off".to_owned()),
        (
            _,
            SpotifyCallback {
                error: Some(error), ..
            },
        ) => Err(error),
        (
            Some(account),
            SpotifyCallback {
                code: Some(code),
                state: Some(state),
                ..
            },
        ) => account
            .callback(&code, &state)
            .await
            .map_err(|error| error.to_string()),
        _ => Err("Spotify sent no code".to_owned()),
    };
    let to = match outcome {
        Ok(()) => {
            // The saved library comes in now rather than at the next sync.
            let library = api.library.clone();
            tokio::spawn(async move {
                if let Err(error) = library.sync().await {
                    tracing::warn!(%error, "library sync after logging in to Spotify failed");
                }
            });
            "/server?spotify=ok".to_owned()
        }
        Err(error) => {
            tracing::warn!(error, "logging in to Spotify failed");
            format!("/server?spotify_error={}", percent(&error))
        }
    };
    (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
}

fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn text(status: StatusCode, body: impl Into<String>) -> Response {
    (status, body.into()).into_response()
}

fn failure(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({"success": false, "error": error}))).into_response()
}

fn internal_error() -> Response {
    text(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

/// Parses a JSON body. The error is Music Assistant's 400 text for a missing or broken body.
fn parse_body<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, &'static str> {
    if body.is_empty() {
        return Err("Body required");
    }
    serde_json::from_slice(body).map_err(|_| "Invalid request body")
}

async fn server_info(State(api): ApiState) -> Response {
    match info::server_info(&api).await {
        Ok(info) => Json(info).into_response(),
        Err(_) => internal_error(),
    }
}

#[derive(Deserialize)]
struct SetupBody {
    username: String,
    password: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    device_name: Option<String>,
}

/// Creates the first user, an admin, and returns a session token for them.
async fn setup(State(api): ApiState, body: Bytes) -> Response {
    let body: SetupBody = match parse_body(&body) {
        Ok(body) => body,
        Err(reason) => return text(StatusCode::BAD_REQUEST, reason),
    };
    let created = api
        .auth
        .create_first_admin(&body.username, &body.password, body.display_name)
        .await;
    let user = match created {
        Ok(Ok(user)) => user,
        Ok(Err(SetupError::AlreadyDone)) => {
            return failure(StatusCode::BAD_REQUEST, "Setup already completed");
        }
        Ok(Err(SetupError::Invalid(reason))) => return failure(StatusCode::BAD_REQUEST, reason),
        Err(_) => return internal_error(),
    };
    session_response(&api, user, body.device_name.as_deref()).await
}

#[derive(Deserialize)]
struct LoginBody {
    #[serde(default)]
    provider_id: Option<String>,
    credentials: Credentials,
    #[serde(default)]
    device_name: Option<String>,
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

async fn login(State(api): ApiState, body: Bytes) -> Response {
    match api.auth.has_users().await {
        Ok(true) => {}
        Ok(false) => return failure(StatusCode::FORBIDDEN, "Setup required"),
        Err(_) => return internal_error(),
    }
    let body: LoginBody = match parse_body(&body) {
        Ok(body) => body,
        Err(reason) => return text(StatusCode::BAD_REQUEST, reason),
    };
    if body
        .provider_id
        .as_deref()
        .is_some_and(|id| id != "builtin")
    {
        return failure(StatusCode::UNAUTHORIZED, "Unknown authentication provider");
    }
    let user = match api
        .auth
        .verify_login(&body.credentials.username, &body.credentials.password)
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => return failure(StatusCode::UNAUTHORIZED, LOGIN_FAILED),
        Err(_) => return internal_error(),
    };
    session_response(&api, user, body.device_name.as_deref()).await
}

async fn session_response(api: &Api, user: User, device_name: Option<&str>) -> Response {
    let device = device_name.unwrap_or(UNNAMED_DEVICE);
    match api
        .auth
        .issue(&user.user_id, device, TokenKind::Session)
        .await
    {
        Ok(token) => Json(json!({"success": true, "token": token, "user": user})).into_response(),
        Err(_) => internal_error(),
    }
}

#[derive(Deserialize)]
struct CommandBody {
    command: String,
    #[serde(default)]
    args: Value,
}

/// One command per request, with a bearer token. Answers with the bare result.
async fn command(State(api): ApiState, headers: HeaderMap, body: Bytes) -> Response {
    match api.auth.has_users().await {
        Ok(true) => {}
        Ok(false) => return text(StatusCode::SERVICE_UNAVAILABLE, "Setup required"),
        Err(_) => return internal_error(),
    }
    if body.is_empty() {
        return text(StatusCode::BAD_REQUEST, "Body required");
    }
    let body: CommandBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => {
            return text(
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON: {}", String::from_utf8_lossy(&body)),
            );
        }
    };
    let Some(access) = api.commands.access(&body.command) else {
        return text(
            StatusCode::BAD_REQUEST,
            format!("Invalid Command: {}", body.command),
        );
    };

    let session = if access == Access::Public {
        None
    } else {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let Some(token) = token else {
            return unauthorized("Authentication required");
        };
        match api.auth.authenticate(token).await {
            Ok(session) => Some(session),
            Err(_) => return unauthorized("Authentication failed"),
        }
    };

    let ctx = Context {
        api: api.clone(),
        session,
    };
    match api.commands.call(ctx, &body.command, body.args).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => command_error(error),
    }
}

/// The largest artwork that can be uploaded.
const ARTWORK_LIMIT: usize = 10 * 1024 * 1024;

/// `POST /artwork`: an admin uploads an image, JPEG or PNG, as the raw request body. It is kept
/// in the artwork cache and named by the proxy id answered, for an edit's `image`.
/// Checks the request's bearer token is an admin's; the response to send when it is not.
async fn admin(api: &Api, headers: &HeaderMap) -> Result<(), Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Err(unauthorized("Authentication required"));
    };
    let session = match api.auth.authenticate(token).await {
        Ok(session) => session,
        Err(_) => return Err(unauthorized("Authentication failed")),
    };
    if !session.allows(crate::auth::Scope::Admin) {
        return Err(text(StatusCode::FORBIDDEN, "This needs the admin scope"));
    }
    Ok(())
}

/// A backup from the backups directory, as a download. Admins only.
async fn download_backup(
    State(api): ApiState,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if let Err(refused) = admin(&api, &headers).await {
        return refused;
    }
    let path = match api.backups.path_of(&name) {
        Ok(path) => path,
        Err(error) => return text(StatusCode::NOT_FOUND, &error.details),
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(_) => return internal_error(),
    };
    let size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_LENGTH, size.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response()
}

/// Restores the backup in the request's body, with its passphrase in `X-Backup-Passphrase`
/// when it is encrypted: checks it, answers, then the server restarts and restores it. Admins
/// only.
async fn restore_backup(
    State(api): ApiState,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    if let Err(refused) = admin(&api, &headers).await {
        return refused;
    }
    let passphrase = headers
        .get("x-backup-passphrase")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .filter(|p| !p.is_empty());
    // Written to disk as it comes, as a backup is larger than is held in memory.
    let path = api.backups.upload_path();
    let written = async {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(&path).await?;
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk.map_err(std::io::Error::other)?)
                .await?;
        }
        file.sync_all().await
    }
    .await;
    if let Err(error) = written {
        let _ = tokio::fs::remove_file(&path).await;
        tracing::warn!(%error, "cannot receive a backup");
        return text(StatusCode::BAD_REQUEST, "The backup did not arrive whole");
    }
    let restored = api.backups.restore(path.clone(), passphrase).await;
    let _ = tokio::fs::remove_file(&path).await;
    match restored {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({"restarting": true}))).into_response(),
        Err(error) => {
            let status = match error.code {
                ErrorCode::AuthenticationFailed => StatusCode::UNAUTHORIZED,
                ErrorCode::InvalidData => StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::ResourceBusy => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            text(status, &error.details)
        }
    }
}

async fn artwork(State(api): ApiState, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(refused) = admin(&api, &headers).await {
        return refused;
    }
    let extension = match image::guess_format(&body) {
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::Png) => "png",
        _ => {
            return text(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Artwork is a JPEG or PNG image",
            );
        }
    };
    // Checked whole, so that what is kept is an image the proxy can resize.
    let data = body.to_vec();
    let check = data.clone();
    let decodes = tokio::task::spawn_blocking(move || image::load_from_memory(&check).is_ok())
        .await
        .unwrap_or(false);
    if !decodes {
        return text(StatusCode::BAD_REQUEST, "The image cannot be read");
    }
    match api.library.store_artwork(data, extension).await {
        Ok(proxy_id) => Json(json!({"proxy_id": proxy_id})).into_response(),
        Err(_) => internal_error(),
    }
}

fn unauthorized(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            r#"Bearer realm="Music Assistant""#,
        )],
        reason.to_owned(),
    )
        .into_response()
}

fn command_error(error: Error) -> Response {
    match error.code {
        ErrorCode::InsufficientPermissions => text(StatusCode::FORBIDDEN, error.details),
        ErrorCode::InvalidData | ErrorCode::UserNotFound => {
            text(StatusCode::BAD_REQUEST, error.details)
        }
        _ => internal_error(),
    }
}

async fn commands_json(State(api): ApiState) -> Response {
    Json(api.commands.docs()).into_response()
}

async fn websocket(State(api): ApiState, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| ws::serve(api, socket))
}
