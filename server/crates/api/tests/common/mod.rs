//! What the API test files share: a server over the fixture library, and a `/ws` client.

#![allow(dead_code)]

pub mod catalog;
pub mod source;
pub mod speaker;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bopsie_api::{Api, MIGRATIONS, Services, Web, commands, router};
use bopsie_core::config::Config;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_library::Library;
use bopsie_players::{Devices, Players};
use bopsie_providers_filesystem::FilesystemProvider;
use bopsie_queues::{Config as PlaybackConfig, Playback};
use bopsie_sources::Sources;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

pub const USERNAME: &str = "bopsie";
pub const PASSWORD: &str = "bopsie-test";

pub struct Server {
    pub api: Arc<Api>,
    pub addr: SocketAddr,
    /// Where endpoints register, for tests that add their own.
    pub players: Players,
    /// Where sources register, for tests that add their own.
    pub sources: Sources,
    _dir: tempfile::TempDir,
}

/// A server over the fixture library, not yet scanned.
pub async fn start() -> Server {
    start_with(1).await
}

/// A server over the fixture library with tracks `seconds` long, not yet scanned.
pub async fn start_with(seconds: u32) -> Server {
    start_full(seconds, Web::default(), false).await
}

/// A server whose web UI comes from `web`.
pub async fn start_serving(web: Web) -> Server {
    start_full(1, web, false).await
}

/// A set-up server with a Spotify account whose Spotify is nowhere to be reached, and a
/// session token.
pub async fn start_set_up_with_spotify() -> (Server, String) {
    let server = start_full(1, Web::default(), true).await;
    server.api.library.sync().await.unwrap();
    let token = set_up(&server).await;
    (server, token)
}

async fn start_full(seconds: u32, web: Web, spotify: bool) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    bopsie_fixtures::write_library(&music, seconds).unwrap();
    let db =
        Database::open_in_memory(&[MIGRATIONS, &[bopsie_library::MIGRATIONS]].concat()).unwrap();
    let events = EventBus::default();
    let provider = Arc::new(FilesystemProvider::new(music));
    let library = Library::new(
        db.clone(),
        events.clone(),
        vec![provider],
        dir.path().join("artwork"),
    );
    let players = Players::default();
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[bopsie_players::devices::MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    let playback = Playback::start(devices, events.clone(), PlaybackConfig::default());
    playback.report_progress_to(library.clone());
    let sources = Sources::default();
    playback.follow_sources(sources.clone());
    let audiobookshelf = bopsie_sources_audiobookshelf::Account::open(db.clone())
        .await
        .unwrap();
    library.catalogs().add(Arc::new(
        bopsie_sources_audiobookshelf::AudiobookshelfCatalog::new(
            audiobookshelf.clone(),
            library.clone(),
            bopsie_audio::decode::find_ffmpeg(None),
        ),
    ));
    let services = Services {
        library,
        events,
        players: players.clone(),
        playback,
        sources: sources.clone(),
        spotify: if spotify {
            Some(
                bopsie_sources_spotify::SpotifyAccount::open(
                    db.clone(),
                    dir.path().into(),
                    bopsie_sources_spotify::Endpoints {
                        accounts: "http://127.0.0.1:9".into(),
                        api: "http://127.0.0.1:9/v1".into(),
                    },
                )
                .await
                .unwrap(),
            )
        } else {
            None
        },
        audiobookshelf: Some(audiobookshelf),
        airplay: None,
        linkplay: None,
        web,
        ffmpeg: false,
        logs: Default::default(),
    };
    let api = Api::new(db, Config::default(), services, commands::registry())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, router(api.clone())).into_future());
    Server {
        api,
        addr,
        players,
        sources,
        _dir: dir,
    }
}

/// A server with its library scanned and the test user, and a session token for them.
pub async fn start_set_up() -> (Server, String) {
    start_set_up_with(1).await
}

/// As [`start_set_up`], with tracks `seconds` long.
pub async fn start_set_up_with(seconds: u32) -> (Server, String) {
    let server = start_with(seconds).await;
    server.api.library.sync().await.unwrap();
    let token = set_up(&server).await;
    (server, token)
}

/// Creates the test user, and returns a session token for them.
async fn set_up(server: &Server) -> String {
    let (status, body) = server
        .post(
            "/setup",
            None,
            json!({"username": USERNAME, "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

impl Server {
    pub async fn request(&self, request: Request<Body>) -> (StatusCode, Value, String) {
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

    pub async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let (status, body, _) = self
            .request(request.body(Body::from(body.to_string())).unwrap())
            .await;
        (status, body)
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        let (status, body, _) = self
            .request(Request::get(path).body(Body::empty()).unwrap())
            .await;
        (status, body)
    }

    pub async fn connect(&self) -> Client {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", self.addr))
            .await
            .unwrap();
        let mut client = Client {
            socket,
            next_id: 0,
            events: Vec::new(),
        };
        let info = client.receive().await.unwrap();
        assert!(info.get("server_version").is_some(), "{info}");
        client
    }
}

pub struct Client {
    pub socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub next_id: u32,
    /// Events that arrived while waiting for answers.
    pub events: Vec<Value>,
}

impl Client {
    /// The next frame, or `None` once the server has closed the connection.
    pub async fn receive(&mut self) -> Option<Value> {
        loop {
            match self.socket.next().await? {
                Ok(Message::Text(text)) => return Some(serde_json::from_str(&text).unwrap()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    /// Sends a command and returns the frame that answers it.
    pub async fn send(&mut self, command: &str, args: Value) -> Value {
        self.next_id += 1;
        let message_id = self.next_id.to_string();
        let frame = json!({"message_id": message_id, "command": command, "args": args});
        self.socket
            .send(Message::text(frame.to_string()))
            .await
            .unwrap();
        loop {
            let frame = self.receive().await.expect("an answer");
            if frame.get("event").is_some() {
                self.events.push(frame);
                continue;
            }
            assert_eq!(frame["message_id"], message_id, "{frame}");
            return frame;
        }
    }

    pub async fn result(&mut self, command: &str, args: Value) -> Value {
        let answer = self.send(command, args).await;
        assert_eq!(answer["partial"], false, "{answer}");
        answer["result"].clone()
    }

    pub async fn error_code(&mut self, command: &str, args: Value) -> u64 {
        let answer = self.send(command, args).await;
        answer["error_code"]
            .as_u64()
            .unwrap_or_else(|| panic!("{command} succeeded: {answer}"))
    }
}
