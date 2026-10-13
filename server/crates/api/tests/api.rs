//! The API over real HTTP and WebSocket connections, following the exchanges Bopsie and
//! Music Assistant's own scripts make.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bopsie_api::{Api, MIGRATIONS, commands, router};
use bopsie_core::config::ServerConfig;
use bopsie_core::db::Database;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

const USERNAME: &str = "bopsie";
const PASSWORD: &str = "bopsie-test";

struct Server {
    api: Arc<Api>,
    addr: SocketAddr,
}

async fn start() -> Server {
    let db = Database::open_in_memory(MIGRATIONS).unwrap();
    let api = Api::new(db, ServerConfig::default(), commands::registry())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, router(api.clone())).into_future());
    Server { api, addr }
}

/// A server with the test user, and a session token for them.
async fn start_set_up() -> (Server, String) {
    let server = start().await;
    let (status, body) = server
        .post(
            "/setup",
            None,
            json!({"username": USERNAME, "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"].as_str().unwrap().to_owned();
    (server, token)
}

impl Server {
    async fn request(&self, request: Request<Body>) -> (StatusCode, Value, String) {
        let response = router(self.api.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let auth_header = response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body, auth_header)
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let (status, body, _) = self
            .request(request.body(Body::from(body.to_string())).unwrap())
            .await;
        (status, body)
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let (status, body, _) = self
            .request(Request::get(path).body(Body::empty()).unwrap())
            .await;
        (status, body)
    }

    async fn connect(&self) -> Client {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", self.addr))
            .await
            .unwrap();
        let mut client = Client { socket, next_id: 0 };
        let info = client.receive().await.unwrap();
        assert!(info.get("server_version").is_some(), "{info}");
        client
    }
}

struct Client {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u32,
}

impl Client {
    /// The next frame, or `None` once the server has closed the connection.
    async fn receive(&mut self) -> Option<Value> {
        loop {
            match self.socket.next().await? {
                Ok(Message::Text(text)) => return Some(serde_json::from_str(&text).unwrap()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    /// Sends a command and returns the frame that answers it.
    async fn send(&mut self, command: &str, args: Value) -> Value {
        self.next_id += 1;
        let message_id = self.next_id.to_string();
        let frame = json!({"message_id": message_id, "command": command, "args": args});
        self.socket
            .send(Message::text(frame.to_string()))
            .await
            .unwrap();
        let answer = self.receive().await.expect("an answer");
        assert_eq!(answer["message_id"], message_id, "{answer}");
        answer
    }

    async fn result(&mut self, command: &str, args: Value) -> Value {
        let answer = self.send(command, args).await;
        assert_eq!(answer["partial"], false, "{answer}");
        answer["result"].clone()
    }

    async fn error_code(&mut self, command: &str, args: Value) -> u64 {
        let answer = self.send(command, args).await;
        answer["error_code"]
            .as_u64()
            .unwrap_or_else(|| panic!("{command} succeeded: {answer}"))
    }
}

#[tokio::test]
async fn before_setup_every_door_says_setup_is_required() {
    let server = start().await;

    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", server.addr))
        .await
        .unwrap();
    let mut client = Client { socket, next_id: 0 };
    let info = client.receive().await.unwrap();
    assert_eq!(info["schema_version"], 65);
    assert_eq!(info["min_supported_schema_version"], 28);
    assert_eq!(info["onboard_done"], false);
    assert_eq!(
        client.receive().await.unwrap(),
        json!({"message_id": "connection", "error_code": 503, "details": "Setup required"})
    );
    assert_eq!(client.receive().await, None);

    let (status, body) = server
        .post("/api", None, json!({"command": "players/all"}))
        .await;
    assert_eq!(
        (status, body),
        (StatusCode::SERVICE_UNAVAILABLE, json!("Setup required"))
    );

    let login = json!({"credentials": {"username": USERNAME, "password": PASSWORD}});
    let (status, body) = server.post("/auth/login", None, login).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({"success": false, "error": "Setup required"}));
}

#[tokio::test]
async fn setup_creates_the_admin_once() {
    let server = start().await;
    let (status, body) = server
        .post(
            "/setup",
            None,
            json!({"username": USERNAME, "password": "short"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"success": false, "error": "Password must be at least 8 characters"})
    );

    let (status, body) = server
        .post(
            "/setup",
            None,
            json!({"username": USERNAME, "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["user"]["username"], USERNAME);
    assert_eq!(body["user"]["role"], "admin");
    assert!(body["token"].is_string());

    let (status, body) = server
        .post(
            "/setup",
            None,
            json!({"username": "other", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"success": false, "error": "Setup already completed"})
    );

    let (_, info) = server.get("/info").await;
    assert_eq!(info["onboard_done"], true);
}

/// The exchange in Bopsie's `signIn`: log in anonymously, trade the session for a long-lived
/// token, revoke the session.
#[tokio::test]
async fn bopsie_signs_in() {
    let (server, _) = start_set_up().await;

    let mut anonymous = server.connect().await;
    let wrong =
        json!({"username": USERNAME, "password": "wrong", "device_name": "Bopsie on Android"});
    assert_eq!(
        anonymous.result("auth/login", wrong).await,
        json!({"success": false, "error": "Invalid username or password"})
    );
    let right =
        json!({"username": USERNAME, "password": PASSWORD, "device_name": "Bopsie on Android"});
    let login = anonymous.result("auth/login", right).await;
    assert_eq!(login["success"], true);
    assert_eq!(login["user"]["username"], USERNAME);
    let session_token = login["access_token"].as_str().unwrap().to_owned();
    // Logging in does not authenticate the connection.
    assert_eq!(anonymous.error_code("auth/me", json!({})).await, 20);

    let mut session = server.connect().await;
    let auth = session
        .result("auth", json!({"token": session_token}))
        .await;
    assert_eq!(auth["authenticated"], true);
    assert_eq!(auth["user"]["username"], USERNAME);
    let long_lived = session
        .result("auth/token/create", json!({"name": "Bopsie on Android"}))
        .await;
    let long_lived = long_lived.as_str().expect("a bare token string").to_owned();
    assert_eq!(session.result("auth/logout", json!({})).await, Value::Null);
    assert_eq!(
        session.receive().await,
        None,
        "logout closes the connection"
    );

    let mut revoked = server.connect().await;
    assert_eq!(
        revoked
            .error_code("auth", json!({"token": session_token}))
            .await,
        23
    );

    let mut app = server.connect().await;
    app.result("auth", json!({"token": long_lived})).await;
    assert_eq!(app.result("auth/me", json!({})).await["username"], USERNAME);
}

#[tokio::test]
async fn logging_out_closes_every_connection_on_that_token() {
    let (server, token) = start_set_up().await;
    let mut first = server.connect().await;
    let mut second = server.connect().await;
    first.result("auth", json!({"token": token})).await;
    second.result("auth", json!({"token": token})).await;

    first.result("auth/logout", json!({})).await;
    assert_eq!(second.receive().await, None);
}

#[tokio::test]
async fn commands_need_auth_and_must_exist() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    assert_eq!(client.error_code("players/all", json!({})).await, 20);
    assert_eq!(client.error_code("auth", json!({})).await, 20);
    assert_eq!(
        client
            .error_code("auth", json!({"token": "nonsense"}))
            .await,
        23
    );

    client.result("auth", json!({"access_token": token})).await;
    let unknown = client.send("music/nonexistent", json!({})).await;
    assert_eq!(unknown["error_code"], 12);
    assert_eq!(unknown["details"], "Invalid or unsupported command.");
    assert_eq!(
        client.error_code("player_queues/get", json!({})).await,
        3,
        "missing a required argument"
    );
}

/// What Bopsie asks for as it opens, answered as an empty server would.
#[tokio::test]
async fn an_empty_library_and_no_players() {
    let (server, token) = start_set_up().await;
    let mut client = server.connect().await;
    client.result("auth", json!({"token": token})).await;

    let page = json!({"limit": 100, "offset": 0, "order_by": "sort_name"});
    for kind in ["artists", "albums", "tracks", "playlists"] {
        let items = client
            .result(&format!("music/{kind}/library_items"), page.clone())
            .await;
        assert_eq!(items, json!([]), "{kind}");
        let count = client
            .result(&format!("music/{kind}/count"), json!({}))
            .await;
        assert_eq!(count, 0, "{kind}");
    }
    let players = json!({"return_unavailable": true, "return_disabled": false});
    assert_eq!(client.result("players/all", players).await, json!([]));
    let queue = json!({"queue_id": "phone"});
    assert_eq!(client.result("player_queues/get", queue).await, Value::Null);
    let search = json!({"search_query": "Gamma", "limit": 25});
    assert_eq!(
        client.result("music/search", search).await["tracks"],
        json!([])
    );
}

#[tokio::test]
async fn http_commands_take_a_bearer_token_and_return_the_bare_result() {
    let (server, token) = start_set_up().await;

    let request = Request::post("/api")
        .body(Body::from(json!({"command": "players/all"}).to_string()))
        .unwrap();
    let (status, body, challenge) = server.request(request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!("Authentication required"));
    assert_eq!(challenge, r#"Bearer realm="Music Assistant""#);

    let (status, body) = server
        .post("/api", Some("nonsense"), json!({"command": "players/all"}))
        .await;
    assert_eq!(
        (status, body),
        (StatusCode::UNAUTHORIZED, json!("Authentication failed"))
    );

    let (status, body) = server
        .post(
            "/api",
            Some(&token),
            json!({"message_id": "1", "command": "music/tracks/count", "args": {}}),
        )
        .await;
    assert_eq!((status, body), (StatusCode::OK, json!(0)));

    let (status, body) = server
        .post("/api", Some(&token), json!({"command": "nope"}))
        .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!("Invalid Command: nope"))
    );

    // Public commands need no token.
    let login =
        json!({"command": "auth/login", "args": {"username": USERNAME, "password": PASSWORD}});
    let (status, body) = server.post("/api", None, login).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
}

#[tokio::test]
async fn http_login_returns_a_session_token() {
    let (server, _) = start_set_up().await;
    let login = |password: &str| {
        json!({
            "provider_id": "builtin",
            "credentials": {"username": USERNAME, "password": password},
            "device_name": "e2e",
        })
    };
    let (status, body) = server.post("/auth/login", None, login("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        json!({"success": false, "error": "Invalid username or password"})
    );

    let (status, body) = server.post("/auth/login", None, login(PASSWORD)).await;
    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().unwrap();
    let (status, players) = server
        .post("/api", Some(token), json!({"command": "players/all"}))
        .await;
    assert_eq!((status, players), (StatusCode::OK, json!([])));
}

#[tokio::test]
async fn info_and_the_command_list_need_no_auth() {
    let server = start().await;
    let (status, info) = server.get("/info").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["server_id"], server.api.server_id);
    assert_eq!(info["status"], "running");
    assert_eq!(info["base_url"], Value::Null);

    let (status, docs) = server.get("/api-docs/commands.json").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = docs
        .as_array()
        .unwrap()
        .iter()
        .map(|doc| doc["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "auth/login",
            "auth/logout",
            "auth/me",
            "auth/token/create",
            "music/albums/count",
            "music/albums/library_items",
            "music/artists/count",
            "music/artists/library_items",
            "music/playlists/count",
            "music/playlists/library_items",
            "music/search",
            "music/tracks/count",
            "music/tracks/library_items",
            "player_queues/get",
            "players/all",
        ]
    );
}
