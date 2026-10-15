//! One device's Cast channel: kept open, with heartbeats, for the device's status and volume,
//! and for the receiver launched on it, which the device stops once no sender is connected to
//! it.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bopsie_core::backoff::{self, Backoff};
use serde_json::{Value, json};
use tokio::io::WriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::endpoint::{Command, RunningApp, Shared};
use crate::message::{self, CastMessage};
use crate::tls;

/// The device's own receiver, which launches and stops apps.
const RECEIVER: &str = "receiver-0";
const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const RECEIVER_NAMESPACE: &str = "urn:x-cast:com.google.cast.receiver";
/// The Sendspin receiver's own namespace.
const SENDSPIN: &str = "urn:x-cast:sendspin";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PING_EVERY: Duration = Duration::from_secs(5);
/// How long a device may say nothing, not even answer a ping, before the channel is taken as
/// lost.
const SILENCE_LIMIT: Duration = Duration::from_secs(15);

type Writer = WriteHalf<TlsStream<TcpStream>>;

/// A virtual connection, opened as Cast's own senders open one: a receiver app counts only
/// those that say what they are as connected senders, and stops once it has none.
fn connect() -> Value {
    json!({
        "type": "CONNECT",
        "origin": {},
        "userAgent": "Bopsie",
        "senderInfo": {
            "sdkType": 2,
            "version": "15.605.1.3",
            "browserVersion": "44.0.2403.30",
            "platform": 4,
            "systemVersion": "Bopsie",
            "connectionType": 1,
        },
    })
}

/// Keeps the device's channel open until the endpoint shuts down.
pub(crate) async fn run(shared: Arc<Shared>, mut commands: mpsc::UnboundedReceiver<Command>) {
    let connector = TlsConnector::from(Arc::new(tls::client_config()));
    let sender = format!("sender-{:08x}", rand::random::<u32>());
    let mut backoff = Backoff::new(backoff::Policy::RECONNECT);
    loop {
        let (address, port) = {
            let sighting = shared.sighting.lock().unwrap();
            (sighting.address, sighting.port)
        };
        let mut channel = Channel {
            shared: &shared,
            sender: &sender,
            next_request: 1,
        };
        let ended = channel
            .open(&connector, address, port, &mut commands, &mut backoff)
            .await;
        {
            let mut status = shared.status.lock().unwrap();
            let was_connected = status.connected;
            status.connected = false;
            status.app = None;
            status.configured = None;
            if was_connected {
                shared.players.changed(&shared.id);
            }
        }
        match ended {
            Ended::Shutdown => return,
            Ended::Lost(reason) => {
                tracing::debug!(device = shared.id, %address, reason, "Cast channel lost");
            }
        }
        let wait = backoff.failed(Instant::now());
        let sleep = tokio::time::sleep(wait);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                () = &mut sleep => break,
                command = commands.recv() => match command {
                    None | Some(Command::Shutdown) => return,
                    // Nothing reaches a device whose channel is down.
                    Some(_) => {}
                },
            }
        }
    }
}

enum Ended {
    Shutdown,
    Lost(String),
}

struct Channel<'a> {
    shared: &'a Arc<Shared>,
    sender: &'a str,
    next_request: u64,
}

impl Channel<'_> {
    async fn open(
        &mut self,
        connector: &TlsConnector,
        address: IpAddr,
        port: u16,
        commands: &mut mpsc::UnboundedReceiver<Command>,
        backoff: &mut Backoff,
    ) -> Ended {
        let tcp = match tokio::time::timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect(SocketAddr::new(address, port)),
        )
        .await
        {
            Ok(Ok(tcp)) => tcp,
            Ok(Err(error)) => return Ended::Lost(error.to_string()),
            Err(_) => return Ended::Lost("connecting timed out".to_owned()),
        };
        let local = tcp.local_addr().map(|a| a.ip()).ok();
        let tls =
            match tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(address.into(), tcp))
                .await
            {
                Ok(Ok(tls)) => tls,
                Ok(Err(error)) => return Ended::Lost(format!("TLS: {error}")),
                Err(_) => return Ended::Lost("the TLS handshake timed out".to_owned()),
            };
        let (mut reader, mut writer) = tokio::io::split(tls);
        let (incoming, mut inbox) = mpsc::channel::<CastMessage>(64);
        let reading = tokio::spawn(async move {
            while let Ok(message) = message::read(&mut reader).await {
                if incoming.send(message).await.is_err() {
                    break;
                }
            }
        });
        let ended = self
            .serve(&mut writer, &mut inbox, commands, local, backoff)
            .await;
        reading.abort();
        ended
    }

    async fn serve(
        &mut self,
        writer: &mut Writer,
        inbox: &mut mpsc::Receiver<CastMessage>,
        commands: &mut mpsc::UnboundedReceiver<Command>,
        local: Option<IpAddr>,
        backoff: &mut Backoff,
    ) -> Ended {
        let shared = self.shared;
        let host = shared.settings.sendspin_host.or(local);
        let server_url = host.map(|host| {
            let host = match host {
                IpAddr::V6(v6) => format!("[{v6}]"),
                IpAddr::V4(v4) => v4.to_string(),
            };
            format!("http://{host}:{}", shared.settings.sendspin_port)
        });
        if let Err(ended) = self.send(writer, CONNECTION, RECEIVER, connect()).await {
            return ended;
        }
        if let Err(ended) = self
            .request(writer, RECEIVER_NAMESPACE, json!({"type": "GET_STATUS"}))
            .await
        {
            return ended;
        }
        {
            let mut status = shared.status.lock().unwrap();
            status.connected = true;
            status.server_url = server_url;
        }
        backoff.working(Instant::now());
        shared.players.changed(&shared.id);
        tracing::debug!(device = shared.id, "Cast channel open");

        let mut heard = Instant::now();
        let mut ping = tokio::time::interval(PING_EVERY);
        loop {
            let step = tokio::select! {
                message = inbox.recv() => match message {
                    Some(message) => {
                        heard = Instant::now();
                        self.receive(writer, message).await
                    }
                    None => Err(Ended::Lost("the device closed the channel".to_owned())),
                },
                command = commands.recv() => match command {
                    None | Some(Command::Shutdown) => Err(Ended::Shutdown),
                    Some(command) => self.command(writer, command).await,
                },
                _ = ping.tick() => {
                    if heard.elapsed() > SILENCE_LIMIT {
                        Err(Ended::Lost("the device went silent".to_owned()))
                    } else {
                        self.send(writer, HEARTBEAT, RECEIVER, json!({"type": "PING"})).await
                    }
                }
            };
            if let Err(ended) = step {
                return ended;
            }
        }
    }

    async fn send(
        &mut self,
        writer: &mut Writer,
        namespace: &str,
        destination: &str,
        payload: Value,
    ) -> Result<(), Ended> {
        let message = CastMessage::new(self.sender, destination, namespace, payload.to_string());
        message::write(writer, &message)
            .await
            .map_err(|error| Ended::Lost(error.to_string()))
    }

    /// Sends to the device's receiver, numbering the request as it requires.
    async fn request(
        &mut self,
        writer: &mut Writer,
        namespace: &str,
        mut payload: Value,
    ) -> Result<(), Ended> {
        payload["requestId"] = json!(self.next_request);
        self.next_request += 1;
        self.send(writer, namespace, RECEIVER, payload).await
    }

    async fn command(&mut self, writer: &mut Writer, command: Command) -> Result<(), Ended> {
        let shared = self.shared;
        match command {
            Command::Launch => {
                let running = {
                    let mut status = shared.status.lock().unwrap();
                    status.wanted = true;
                    status.seen = false;
                    // A receiver that is running is told again, in case its client gave up.
                    status.configured = None;
                    status
                        .app
                        .as_ref()
                        .is_some_and(|app| app.app_id == shared.settings.app_id)
                };
                if running {
                    self.configure(writer).await
                } else {
                    tracing::info!(device = shared.id, "launching the Cast receiver");
                    let app_id = shared.settings.app_id.clone();
                    self.request(
                        writer,
                        RECEIVER_NAMESPACE,
                        json!({"type": "LAUNCH", "appId": app_id}),
                    )
                    .await
                }
            }
            Command::Stop => {
                let session = {
                    let mut status = shared.status.lock().unwrap();
                    status.wanted = false;
                    status.configured = None;
                    status
                        .app
                        .as_ref()
                        .filter(|app| app.app_id == shared.settings.app_id)
                        .map(|app| app.session_id.clone())
                };
                match session {
                    Some(session) => {
                        tracing::info!(device = shared.id, "stopping the Cast receiver");
                        self.request(
                            writer,
                            RECEIVER_NAMESPACE,
                            json!({"type": "STOP", "sessionId": session}),
                        )
                        .await
                    }
                    None => Ok(()),
                }
            }
            Command::Volume(volume) => {
                self.request(
                    writer,
                    RECEIVER_NAMESPACE,
                    json!({"type": "SET_VOLUME", "volume": {"level": f64::from(volume.min(100)) / 100.0}}),
                )
                .await
            }
            Command::Muted(muted) => {
                self.request(
                    writer,
                    RECEIVER_NAMESPACE,
                    json!({"type": "SET_VOLUME", "volume": {"muted": muted}}),
                )
                .await
            }
            Command::Shutdown => Err(Ended::Shutdown),
        }
    }

    async fn receive(&mut self, writer: &mut Writer, message: CastMessage) -> Result<(), Ended> {
        let shared = self.shared;
        tracing::trace!(
            device = shared.id,
            source = message.source,
            namespace = message.namespace,
            payload = message.payload,
            "from the Cast device"
        );
        let payload: Value = serde_json::from_str(&message.payload).unwrap_or(Value::Null);
        let kind = payload["type"].as_str().unwrap_or_default();
        match (message.namespace.as_str(), kind) {
            (HEARTBEAT, "PING") => {
                self.send(writer, HEARTBEAT, &message.source, json!({"type": "PONG"}))
                    .await
            }
            (CONNECTION, "CLOSE") if message.source == RECEIVER => {
                Err(Ended::Lost("the device closed the channel".to_owned()))
            }
            (CONNECTION, "CLOSE") => {
                let mut status = shared.status.lock().unwrap();
                if status.configured.as_deref() == Some(message.source.as_str()) {
                    status.configured = None;
                }
                Ok(())
            }
            (RECEIVER_NAMESPACE, "RECEIVER_STATUS") => {
                self.update(&payload["status"]);
                self.configure(writer).await
            }
            (RECEIVER_NAMESPACE, "LAUNCH_ERROR" | "INVALID_REQUEST") => {
                tracing::warn!(
                    device = shared.id,
                    app = shared.settings.app_id,
                    reason = %payload["reason"],
                    "the Cast receiver could not be launched"
                );
                shared.status.lock().unwrap().wanted = false;
                Ok(())
            }
            (SENDSPIN, _) => {
                if payload["state"] == "error" {
                    tracing::warn!(
                        device = shared.id,
                        message = %payload["message"],
                        "the Cast receiver failed"
                    );
                }
                let mut status = shared.status.lock().unwrap();
                // It reports every second while it plays; only a new state is logged.
                if status.report.as_ref().map(|r| &r["state"]) != Some(&payload["state"]) {
                    tracing::debug!(device = shared.id, report = %payload, "Cast receiver");
                }
                status.report = Some(payload);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Takes in the device's status: its volume, and the app it runs.
    fn update(&self, status: &Value) {
        let shared = self.shared;
        let app = status["applications"]
            .as_array()
            .and_then(|apps| apps.first())
            .and_then(|app| {
                Some(RunningApp {
                    app_id: app["appId"].as_str()?.to_owned(),
                    session_id: app["sessionId"].as_str()?.to_owned(),
                    transport_id: app["transportId"].as_str()?.to_owned(),
                })
            });
        let mut current = shared.status.lock().unwrap();
        if let Some(level) = status["volume"]["level"].as_f64() {
            current.volume = Some((level.clamp(0.0, 1.0) * 100.0).round() as u8);
        }
        if let Some(muted) = status["volume"]["muted"].as_bool() {
            current.muted = Some(muted);
        }
        let ours = app
            .as_ref()
            .is_some_and(|app| app.app_id == shared.settings.app_id);
        if current.wanted && ours {
            current.seen = true;
        } else if current.wanted && current.seen {
            // It stopped, or another app took the device: it is not taken back.
            tracing::info!(device = shared.id, "the Cast receiver stopped");
            current.wanted = false;
            current.seen = false;
        }
        if !ours {
            current.configured = None;
        }
        current.app = app;
        drop(current);
        shared.players.changed(&shared.id);
    }

    /// Tells a running receiver that is wanted where to connect, once per launch.
    async fn configure(&mut self, writer: &mut Writer) -> Result<(), Ended> {
        let shared = self.shared;
        let (transport, server_url) = {
            let status = shared.status.lock().unwrap();
            let Some(app) = status
                .app
                .as_ref()
                .filter(|app| status.wanted && app.app_id == shared.settings.app_id)
            else {
                return Ok(());
            };
            if status.configured.as_deref() == Some(app.transport_id.as_str()) {
                return Ok(());
            }
            let Some(server_url) = status.server_url.clone() else {
                tracing::warn!(
                    device = shared.id,
                    "no address to give the Cast receiver for the server"
                );
                return Ok(());
            };
            (app.transport_id.clone(), server_url)
        };
        self.send(writer, CONNECTION, &transport, connect()).await?;
        let name = shared.sighting.lock().unwrap().name.clone();
        self.send(
            writer,
            SENDSPIN,
            &transport,
            json!({
                "type": "config",
                "serverUrl": server_url,
                "playerId": shared.id,
                "playerName": name,
                "syncDelay": 0,
                "codecs": ["pcm"],
            }),
        )
        .await?;
        tracing::info!(
            device = shared.id,
            server_url,
            "the Cast receiver is told where to connect"
        );
        shared.status.lock().unwrap().configured = Some(transport);
        Ok(())
    }
}
