//! A Cast device against a stand-in: it launches the receiver, which connects back to a real
//! Sendspin server as the receiver app would.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use bopsie_audio::OutputFormat;
use bopsie_audio::timeline::Timeline;
use bopsie_core::clock;
use bopsie_players::{Endpoint, Players};
use bopsie_players_cast::discovery::Sighting;
use bopsie_players_cast::message::{self, CastMessage};
use bopsie_players_cast::{CastProvider, Settings};
use bopsie_players_sendspin::{Hosted, SILENCE_LIMIT, SendspinServer};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const APP: &str = "TESTAPP1";
const UUID: &str = "0123456789abcdef0123456789abcdef";
const CD: OutputFormat = OutputFormat {
    sample_rate: 44_100,
    channels: 2,
    bits: 16,
};
const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

async fn serve(State(server): State<SendspinServer>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        server.serve(socket, None, Some(LOCALHOST)).await;
    })
}

/// A Sendspin server for the receiver to connect to.
async fn sendspin(players: Players, hosted: Hosted) -> SocketAddr {
    let server = SendspinServer {
        players,
        server_id: "server-1".to_owned(),
        name: "Test server".to_owned(),
        silence_limit: SILENCE_LIMIT,
        hosted,
    };
    let app = Router::new()
        .route("/sendspin", get(serve))
        .with_state(server);
    let listener = TcpListener::bind((LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

/// What the stand-in device runs and how loud it is.
#[derive(Default)]
struct DeviceState {
    app: Option<&'static str>,
    level: f64,
    /// Stops the receiver's Sendspin client.
    client: Option<tokio::task::JoinHandle<()>>,
}

/// A stand-in Cast device. Each message it receives is reported as (namespace, payload).
struct FakeDevice {
    port: u16,
    received: mpsc::UnboundedReceiver<(String, Value)>,
    state: Arc<Mutex<DeviceState>>,
    /// Pushes a status as if another sender changed the device.
    push: mpsc::UnboundedSender<Value>,
}

impl FakeDevice {
    async fn start() -> Self {
        let certified = rcgen::generate_simple_self_signed(vec!["cast.local".to_owned()]).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certified.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into()),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind((LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (report, received) = mpsc::unbounded_channel();
        let (push, pushed) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(DeviceState {
            level: 0.25,
            ..DeviceState::default()
        }));
        let device_state = state.clone();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let tls = acceptor.accept(tcp).await.unwrap();
            run_device(tls, report, device_state, pushed).await;
        });
        Self {
            port,
            received,
            state,
            push,
        }
    }

    /// Waits for a message of `kind` on `namespace`, passing over others.
    async fn expect(&mut self, namespace: &str, kind: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (ns, payload) = self.received.recv().await.expect("the device stopped");
                if ns == namespace && payload["type"] == kind {
                    return payload;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {kind} on {namespace}"))
    }
}

fn status(state: &DeviceState) -> Value {
    let applications: Vec<Value> = state
        .app
        .map(|app| {
            json!({"appId": app, "sessionId": format!("session-{app}"),
                "transportId": format!("transport-{app}"), "displayName": app})
        })
        .into_iter()
        .collect();
    json!({"type": "RECEIVER_STATUS", "requestId": 0, "status": {
        "applications": applications,
        "volume": {"level": state.level, "muted": false},
    }})
}

async fn run_device(
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    report: mpsc::UnboundedSender<(String, Value)>,
    state: Arc<Mutex<DeviceState>>,
    mut pushed: mpsc::UnboundedReceiver<Value>,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (incoming, mut inbox) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(message) = message::read(&mut reader).await {
            if incoming.send(message).is_err() {
                break;
            }
        }
    });
    let reply = |source: &str, namespace: &str, payload: Value| {
        CastMessage::new("receiver-0", source, namespace, payload.to_string())
    };
    loop {
        let message: CastMessage = tokio::select! {
            message = inbox.recv() => match message {
                Some(message) => message,
                None => return,
            },
            Some(status) = pushed.recv() => {
                let out = reply("*", "urn:x-cast:com.google.cast.receiver", status);
                message::write(&mut writer, &out).await.unwrap();
                continue;
            }
        };
        let payload: Value = serde_json::from_str(&message.payload).unwrap();
        let _ = report.send((message.namespace.clone(), payload.clone()));
        let receiver = "urn:x-cast:com.google.cast.receiver";
        let answer = match (message.namespace.as_str(), payload["type"].as_str()) {
            ("urn:x-cast:com.google.cast.tp.heartbeat", Some("PING")) => Some(reply(
                &message.source,
                &message.namespace,
                json!({"type": "PONG"}),
            )),
            (_, Some("GET_STATUS")) => Some(reply(
                &message.source,
                receiver,
                status(&state.lock().unwrap()),
            )),
            (_, Some("LAUNCH")) => {
                let mut state = state.lock().unwrap();
                state.app = Some(APP);
                Some(reply(&message.source, receiver, status(&state)))
            }
            (_, Some("STOP")) => {
                let mut state = state.lock().unwrap();
                state.app = None;
                if let Some(client) = state.client.take() {
                    client.abort();
                }
                Some(reply(&message.source, receiver, status(&state)))
            }
            (_, Some("SET_VOLUME")) => {
                let mut state = state.lock().unwrap();
                if let Some(level) = payload["volume"]["level"].as_f64() {
                    state.level = level;
                }
                Some(reply(&message.source, receiver, status(&state)))
            }
            ("urn:x-cast:sendspin", Some("config")) => {
                assert_eq!(message.destination, format!("transport-{APP}"));
                let client = tokio::spawn(receiver_client(payload));
                state.lock().unwrap().client = Some(client);
                None
            }
            _ => None,
        };
        if let Some(answer) = answer {
            message::write(&mut writer, &answer).await.unwrap();
        }
    }
}

/// The receiver app's Sendspin client, as the config tells it to connect.
async fn receiver_client(config: Value) {
    let url = format!(
        "{}/sendspin",
        config["serverUrl"].as_str().unwrap().replace("http", "ws")
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let send = |message: Value| Message::text(message.to_string());
    let format = json!({"codec": "pcm", "channels": 2, "sample_rate": 44100, "bit_depth": 16});
    socket
        .send(send(json!({"type": "client/hello", "payload": {
            "client_id": config["playerId"], "name": config["playerName"], "version": 1,
            "supported_roles": ["player@v1", "metadata@v1"],
            "player@v1_support": {"supported_formats": [format], "buffer_capacity": 2_097_152,
                "supported_commands": ["volume", "mute"]}}})))
        .await
        .unwrap();
    socket
        .send(send(
            json!({"type": "client/state", "payload": {"available": true,
            "player": {"volume": 25, "muted": false, "static_delay_ms": 0,
                "required_lead_time_ms": 400, "min_buffer_ms": 0}}}),
        ))
        .await
        .unwrap();
    while let Some(Ok(_)) = socket.next().await {}
}

struct Setup {
    players: Players,
    provider: CastProvider,
    device: FakeDevice,
    endpoint: Arc<dyn Endpoint>,
}

async fn setup() -> Setup {
    let players = Players::default();
    let hosted = Hosted::default();
    let sendspin = sendspin(players.clone(), hosted.clone()).await;
    let device = FakeDevice::start().await;
    let provider = CastProvider::new(
        players.clone(),
        hosted,
        Settings {
            app_id: APP.to_owned(),
            sendspin_port: sendspin.port(),
            sendspin_host: None,
            release_after: Duration::from_millis(300),
            launch_timeout: Duration::from_secs(5),
        },
    );
    provider.see(Sighting {
        fullname: "Living-Room._googlecast._tcp.local.".to_owned(),
        uuid: UUID.to_owned(),
        name: "Living Room".to_owned(),
        model: Some("WiiM AMP".to_owned()),
        address: LOCALHOST,
        port: device.port,
    });
    let endpoint = players.get(&format!("cast{UUID}")).expect("registered");
    Setup {
        players,
        provider,
        device,
        endpoint,
    }
}

async fn eventually(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the condition did not hold in time");
}

async fn prepare(endpoint: &Arc<dyn Endpoint>) -> Option<i64> {
    let endpoint = endpoint.clone();
    tokio::task::spawn_blocking(move || endpoint.prepare())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_is_a_player_that_launches_its_receiver_when_played_to() {
    let Setup {
        players,
        provider,
        mut device,
        endpoint,
    } = setup().await;
    device
        .expect("urn:x-cast:com.google.cast.tp.connection", "CONNECT")
        .await;
    eventually(|| endpoint.state().volume.is_some()).await;
    let state = endpoint.state();
    assert!(state.available);
    assert_eq!(state.name, "Living Room");
    assert_eq!(state.provider, "sendspin");
    assert_eq!(state.volume, Some(25), "the device's own volume");
    assert_eq!(state.device.model.as_deref(), Some("WiiM AMP"));
    assert_eq!(state.facts.address, Some(LOCALHOST));
    assert!(state.capabilities.launched);
    assert!(
        device.state.lock().unwrap().app.is_none(),
        "nothing launched yet"
    );

    let start = prepare(&endpoint).await;
    assert!(
        start.is_some(),
        "it says when it can start, having just connected"
    );
    let launch = device
        .expect("urn:x-cast:com.google.cast.receiver", "LAUNCH")
        .await;
    assert_eq!(launch["appId"], APP);
    let config = device.expect("urn:x-cast:sendspin", "config").await;
    assert_eq!(config["playerId"], format!("cast{UUID}"));
    assert_eq!(config["playerName"], "Living Room");
    assert_eq!(config["codecs"], json!(["pcm"]));

    // The receiver plays through this endpoint, not as a player of its own.
    let state = endpoint.state();
    assert_eq!(
        state.capabilities.formats.len(),
        1,
        "the receiver's own formats"
    );
    assert_eq!(state.capabilities.lead, 400_000);
    assert_eq!(players.all().len(), 1);
    assert_eq!(prepare(&endpoint).await, None, "already connected");

    endpoint.set_volume(40);
    let volume = device
        .expect("urn:x-cast:com.google.cast.receiver", "SET_VOLUME")
        .await;
    assert_eq!(volume["volume"]["level"], 0.4);
    assert_eq!(endpoint.state().volume, Some(40));

    // Idle for long enough, it is stopped, freeing the device.
    endpoint.set_group(true, "group");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        device.state.lock().unwrap().app.is_some(),
        "kept while playing"
    );
    endpoint.set_group(false, "group");
    let stop = device
        .expect("urn:x-cast:com.google.cast.receiver", "STOP")
        .await;
    assert_eq!(stop["sessionId"], format!("session-{APP}"));
    eventually(|| endpoint.state().capabilities.formats.len() == 2).await;
    assert!(endpoint.state().available, "still there to be played to");
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn another_app_taking_the_device_is_not_fought() {
    let Setup {
        provider,
        mut device,
        endpoint,
        ..
    } = setup().await;
    eventually(|| endpoint.state().available).await;
    assert!(prepare(&endpoint).await.is_some());
    device.expect("urn:x-cast:sendspin", "config").await;

    // Another sender casts to the device.
    let taken = {
        let mut state = device.state.lock().unwrap();
        state.app = Some("OTHERAPP");
        if let Some(client) = state.client.take() {
            client.abort();
        }
        status(&state)
    };
    device.push.send(taken).unwrap();
    eventually(|| endpoint.diagnostics()["wanted"] == false).await;
    // The group it played in asks for it again, and is not given it.
    let _ = endpoint.output(CD, Timeline::new(44_100, clock::now()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(device.state.lock().unwrap().app, Some("OTHERAPP"));

    // Played to again, it takes the device back.
    assert!(prepare(&endpoint).await.is_some());
    device
        .expect("urn:x-cast:com.google.cast.receiver", "LAUNCH")
        .await;
    assert_eq!(device.state.lock().unwrap().app, Some(APP));
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_receiver_that_stopped_by_itself_is_launched_again_for_its_group() {
    let Setup {
        provider,
        mut device,
        endpoint,
        ..
    } = setup().await;
    eventually(|| endpoint.state().available).await;
    assert!(prepare(&endpoint).await.is_some());
    device.expect("urn:x-cast:sendspin", "config").await;

    let stopped = {
        let mut state = device.state.lock().unwrap();
        state.app = None;
        if let Some(client) = state.client.take() {
            client.abort();
        }
        status(&state)
    };
    device.push.send(stopped).unwrap();
    eventually(|| endpoint.diagnostics()["wanted"] == false).await;

    // Its group takes it back, as it does a member whose output failed.
    let _ = endpoint.output(CD, Timeline::new(44_100, clock::now()));
    device
        .expect("urn:x-cast:com.google.cast.receiver", "LAUNCH")
        .await;
    device.expect("urn:x-cast:sendspin", "config").await;
    eventually(|| endpoint.state().capabilities.formats.len() == 1).await;
    provider.shutdown();
}
