//! Sendspin clients against the server role, streaming a session from the engine.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use bopsie_audio::engine::{self, Command, Realtime, Settings, TrackRequest, TrackSource};
use bopsie_audio::timeline::Timeline;
use bopsie_audio::{OutputFormat, PcmFormat};
use bopsie_core::clock;
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::{Change, Players};
use bopsie_players_sendspin::{SILENCE_LIMIT, SendspinServer};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

async fn serve(State(server): State<SendspinServer>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| async move { server.serve(socket, None, None).await })
}

async fn start() -> (SocketAddr, Players) {
    start_with(Players::default()).await
}

async fn start_with(players: Players) -> (SocketAddr, Players) {
    start_listening(players, SILENCE_LIMIT).await
}

/// A server that takes a client that says nothing for `silence_limit` as gone.
async fn start_listening(players: Players, silence_limit: Duration) -> (SocketAddr, Players) {
    let server = SendspinServer {
        players: players.clone(),
        server_id: "server-1".to_owned(),
        name: "Test server".to_owned(),
        silence_limit,
    };
    let app = Router::new()
        .route("/sendspin", get(serve))
        .with_state(server);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    (addr, players)
}

struct Client {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// Audio chunks: when they arrived, their timestamp and their audio.
    chunks: Vec<(i64, i64, Vec<u8>)>,
    messages: VecDeque<Value>,
}

impl Client {
    async fn connect(addr: SocketAddr, id: &str, buffer_capacity: u64) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/sendspin"))
            .await
            .unwrap();
        let mut client = Self {
            socket,
            chunks: Vec::new(),
            messages: VecDeque::new(),
        };
        let format = json!({"codec": "pcm", "channels": 2, "sample_rate": 48000, "bit_depth": 16});
        client
            .send(json!({"type": "client/hello", "payload": {
                "client_id": id, "name": id, "version": 1,
                "supported_roles": ["player@v1", "controller@v1", "metadata@v1"],
                "device_info": {"product_name": "Mobile Application", "manufacturer": "Bopsie", "software_version": "0"},
                "player@v1_support": {"supported_formats": [format], "buffer_capacity": buffer_capacity,
                    "supported_commands": ["volume", "mute"]}}}))
            .await;
        let hello = client.message("server/hello").await;
        assert_eq!(
            hello["payload"]["active_roles"],
            json!(["player@v1", "metadata@v1"])
        );
        client
            .send(
                json!({"type": "client/state", "payload": {"available": true,
                "player": {"volume": 50, "muted": false, "static_delay_ms": 0,
                    "required_lead_time_ms": 300, "min_buffer_ms": 300}}}),
            )
            .await;
        client
    }

    async fn send(&mut self, message: Value) {
        self.socket
            .send(Message::text(message.to_string()))
            .await
            .unwrap();
    }

    /// Reads one frame, storing chunks and messages.
    async fn read(&mut self) -> bool {
        match self.socket.next().await {
            Some(Ok(Message::Binary(bytes))) => {
                assert_eq!(bytes[0], 4);
                let timestamp = i64::from_be_bytes(bytes[1..9].try_into().unwrap());
                self.chunks
                    .push((clock::now(), timestamp, bytes[9..].to_vec()));
                true
            }
            Some(Ok(Message::Text(text))) => {
                self.messages
                    .push_back(serde_json::from_str(&text).unwrap());
                true
            }
            Some(Ok(_)) => true,
            _ => false,
        }
    }

    /// Waits for a message of `kind`, keeping what arrives meanwhile.
    async fn message(&mut self, kind: &str) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(i) = self.messages.iter().position(|m| m["type"] == kind) {
                return self.messages.remove(i).unwrap();
            }
            tokio::time::timeout_at(deadline, self.read())
                .await
                .unwrap_or_else(|_| panic!("no {kind}"));
        }
    }
}

struct One(Option<TrackRequest>);

impl TrackSource for One {
    fn next(&mut self) -> Option<TrackRequest> {
        self.0.take()
    }
}

const SESSION: PcmFormat = PcmFormat {
    sample_rate: 48_000,
    channels: 2,
};
const OUTPUT: OutputFormat = OutputFormat {
    sample_rate: 48_000,
    channels: 2,
    bits: 16,
};

/// Plays `frames` of noise to the given clients' endpoints, as one session.
fn play(
    players: &Players,
    ids: &[&str],
    dir: &std::path::Path,
    frames: usize,
) -> engine::EngineHandle {
    let path = dir.join("a.wav");
    write_wav(&path, 48_000, 2, &noise(1, frames * 2)).unwrap();
    let lead = ids
        .iter()
        .map(|id| players.get(id).unwrap().state().capabilities.lead)
        .max()
        .unwrap();
    let timeline = Timeline::new(48_000, clock::now());
    let sinks = ids
        .iter()
        .map(|id| {
            (
                id.to_string(),
                players.get(id).unwrap().output(OUTPUT, timeline),
            )
        })
        .collect();
    let settings = Settings {
        format: SESSION,
        ffmpeg: None,
        ring_frames: 48_000,
        realtime: Some(Realtime {
            timeline,
            lead,
            clock: clock::now,
        }),
    };
    let source = One(Some(TrackRequest {
        id: "a".to_owned(),
        path,
        start: Duration::ZERO,
    }));
    let handle = engine::spawn(settings, Box::new(source), sinks);
    handle.send(Command::Restart {
        epoch: 1,
        not_before: None,
    });
    handle
}

async fn registered(players: &Players, id: &str) {
    let mut changes = players.subscribe();
    if players.get(id).is_some() {
        return;
    }
    loop {
        if let Ok(Change::Added(added)) = changes.recv().await
            && added == id
        {
            return;
        }
    }
}

#[tokio::test]
async fn a_client_registers_and_gets_time_replies() {
    let (addr, players) = start().await;
    let mut client = Client::connect(addr, "phone", 1_048_576).await;
    registered(&players, "phone").await;
    let state = players.get("phone").unwrap().state();
    assert!(state.available && state.app);
    assert_eq!(state.capabilities.lead, 300_000);
    assert_eq!(state.capabilities.formats, [OUTPUT]);
    assert!(state.capabilities.volume && state.capabilities.mute);
    assert_eq!(state.capabilities.max_group_size, None);
    assert_eq!(state.volume, Some(50));

    let before = clock::now();
    client
        .send(json!({"type": "client/time", "payload": {"client_transmitted": 42}}))
        .await;
    let time = client.message("server/time").await;
    let after = clock::now();
    let payload = &time["payload"];
    assert_eq!(payload["client_transmitted"], 42);
    let (received, transmitted) = (
        payload["server_received"].as_i64().unwrap(),
        payload["server_transmitted"].as_i64().unwrap(),
    );
    assert!(before <= received && received <= transmitted && transmitted <= after);

    players.get("phone").unwrap().set_volume(70);
    let command = client.message("server/command").await;
    assert_eq!(
        command["payload"]["player"],
        json!({"command": "volume", "volume": 70})
    );
}

#[tokio::test]
async fn a_session_streams_timed_chunks_and_ends_after_the_buffer_plays() {
    let (addr, players) = start().await;
    let mut client = Client::connect(addr, "phone", 1_048_576).await;
    registered(&players, "phone").await;
    let dir = tempfile::tempdir().unwrap();
    let begun = clock::now();
    let handle = play(&players, &["phone"], dir.path(), 48_000);

    let start = client.message("stream/start").await;
    assert_eq!(
        start["payload"]["player"],
        json!({"codec": "pcm", "sample_rate": 48000, "channels": 2, "bit_depth": 16})
    );
    let _ = client.message("stream/end").await;
    let ended = clock::now();
    handle.shutdown();

    let chunks = &client.chunks;
    let audio: usize = chunks.iter().map(|(_, _, audio)| audio.len()).sum();
    assert_eq!(audio, 48_000 * 4, "one second of audio");
    // The first chunk plays one lead after the session started, the rest follow on, in 25 ms
    // chunks on the timeline's grid after the first.
    let first = chunks[0].1;
    assert!(
        first >= begun + 300_000 && first < begun + 400_000,
        "{}",
        first - begun
    );
    for pair in chunks.windows(2) {
        let duration = (pair[0].2.len() / 4) as i64 * 1_000_000 / 48_000;
        assert!(
            (pair[1].1 - (pair[0].1 + duration)).abs() <= 1,
            "contiguous"
        );
    }
    let middle = &chunks[1..chunks.len() - 1];
    assert!(middle.iter().all(|(_, _, audio)| audio.len() == 4_800));
    // stream/end waits for the last chunk to play.
    assert!(ended >= first + 1_000_000);
}

#[tokio::test]
async fn grouped_clients_get_the_same_audio_at_the_same_time() {
    let (addr, players) = start().await;
    let mut kitchen = Client::connect(addr, "kitchen", 1_048_576).await;
    let mut bedroom = Client::connect(addr, "bedroom", 1_048_576).await;
    registered(&players, "kitchen").await;
    registered(&players, "bedroom").await;
    let dir = tempfile::tempdir().unwrap();
    let handle = play(&players, &["kitchen", "bedroom"], dir.path(), 24_000);

    let _ = kitchen.message("stream/end").await;
    let _ = bedroom.message("stream/end").await;
    handle.shutdown();
    let strip = |c: &Client| -> Vec<(i64, Vec<u8>)> {
        c.chunks.iter().map(|(_, t, a)| (*t, a.clone())).collect()
    };
    let audio: usize = kitchen.chunks.iter().map(|(_, _, a)| a.len()).sum();
    assert_eq!(audio, 24_000 * 4);
    assert!(
        strip(&kitchen) == strip(&bedroom),
        "both play the same audio at the same times"
    );
}

#[tokio::test]
async fn a_sync_adjustment_moves_when_a_client_plays() {
    let settings = bopsie_core::config::PlayerConfig {
        quirks: bopsie_players::quirks::Quirks {
            sync_adjust_ms: Some(40),
            ..Default::default()
        },
        ..Default::default()
    };
    let (addr, players) = start_with(Players::new([("kitchen".to_owned(), settings)].into())).await;
    let mut kitchen = Client::connect(addr, "kitchen", 1_048_576).await;
    let mut bedroom = Client::connect(addr, "bedroom", 1_048_576).await;
    registered(&players, "kitchen").await;
    registered(&players, "bedroom").await;
    let dir = tempfile::tempdir().unwrap();
    let handle = play(&players, &["kitchen", "bedroom"], dir.path(), 12_000);

    let _ = kitchen.message("stream/end").await;
    let _ = bedroom.message("stream/end").await;
    handle.shutdown();
    assert_eq!(kitchen.chunks.len(), bedroom.chunks.len());
    for ((_, late, a), (_, on_time, b)) in kitchen.chunks.iter().zip(&bedroom.chunks) {
        assert_eq!(a, b);
        assert_eq!(late - on_time, 40_000, "40 ms later");
    }
}

#[tokio::test]
async fn never_more_is_outstanding_than_the_client_can_hold() {
    let (addr, players) = start().await;
    // Room for about eight chunks.
    let capacity = 8 * 4_809;
    let mut client = Client::connect(addr, "small", capacity).await;
    registered(&players, "small").await;
    let dir = tempfile::tempdir().unwrap();
    let handle = play(&players, &["small"], dir.path(), 48_000);
    let _ = client.message("stream/end").await;
    handle.shutdown();

    let chunks = &client.chunks;
    let audio: usize = chunks.iter().map(|(_, _, audio)| audio.len()).sum();
    assert_eq!(audio, 48_000 * 4);
    for (i, (arrived, _, _)) in chunks.iter().enumerate() {
        // Chunks received by now that have not finished playing.
        let outstanding: u64 = chunks[..=i]
            .iter()
            .filter(|(_, timestamp, _)| timestamp + 25_000 > *arrived)
            .map(|(_, _, audio)| (audio.len() + 9) as u64)
            .sum();
        assert!(
            outstanding <= capacity,
            "chunk {i}: {outstanding} bytes outstanding"
        );
    }
}

#[tokio::test]
async fn a_client_that_leaves_becomes_unavailable() {
    let (addr, players) = start().await;
    let mut client = Client::connect(addr, "phone", 1_048_576).await;
    registered(&players, "phone").await;
    let mut changes = players.subscribe();
    client
        .send(json!({"type": "client/goodbye", "payload": {"reason": "user_request"}}))
        .await;
    while changes.recv().await.unwrap() != Change::Updated("phone".to_owned()) {}
    assert!(!players.get("phone").unwrap().state().available);
    assert!(players.health("phone").counts.is_empty(), "it said goodbye");
}

#[tokio::test]
async fn a_client_whose_connection_drops_is_counted() {
    let (addr, players) = start().await;
    let client = Client::connect(addr, "phone", 1_048_576).await;
    registered(&players, "phone").await;
    let mut changes = players.subscribe();
    drop(client);
    while changes.recv().await.unwrap() != Change::Updated("phone".to_owned()) {}
    let health = players.health("phone");
    assert_eq!(health.counts[&bopsie_players::health::Event::Dropped], 1);
}

#[tokio::test]
async fn a_client_that_goes_silent_is_taken_as_gone() {
    let (addr, players) = start_listening(Players::default(), Duration::from_millis(400)).await;
    // Connected, then never reads again, so never answers a ping.
    let _client = Client::connect(addr, "phone", 1_048_576).await;
    registered(&players, "phone").await;
    assert!(players.get("phone").unwrap().state().available);
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert!(!players.get("phone").unwrap().state().available);
    assert_eq!(
        players.health("phone").counts[&bopsie_players::health::Event::Dropped],
        1
    );
}
