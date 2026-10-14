//! A stand-in for LinkPlay devices: each answers the HTTP API and the UPnP status calls as the
//! real ones do, pulls the stream it is told to play, counts down before it is audible, and
//! groups with the others on its fake network.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use bopsie_players::Players;
use bopsie_players_linkplay::discovery::Sighting;
use bopsie_players_linkplay::transport::{Api, CLIENT_CERTIFICATE};
use bopsie_players_linkplay::upnp::Upnp;
use bopsie_players_linkplay::{Config, LinkplayProvider, ServerAddress};
use futures::StreamExt;
use serde_json::json;
use tokio::net::TcpListener;

#[derive(Default)]
pub struct Device {
    pub name: String,
    pub uuid: String,
    pub mac: String,
    /// The address the fake network knows it by.
    pub ip: String,
    pub wmrm: String,
    pub has_player_status: bool,
    /// From `play` to audible.
    pub delay: Duration,
    pub volume: u8,
    pub muted: bool,
    /// The uuid of the device it follows.
    pub leader: Option<String>,
    pub commands: Vec<String>,
    /// What it was told to play, when, and what it pulled of it.
    pub playing: Option<(String, Instant)>,
    pub pulled: Vec<u8>,
    pub streams: usize,
    fetch: Option<tokio::task::JoinHandle<()>>,
}

pub type Shared = Arc<Mutex<Device>>;

/// Devices by the address the fake network knows them by.
#[derive(Clone, Default)]
pub struct Network {
    devices: Arc<Mutex<HashMap<String, Shared>>>,
}

pub struct Fake {
    pub device: Shared,
    pub port: u16,
    pub sighting: Sighting,
}

impl Network {
    /// A device known as `ip` on the fake network, answering on a local port.
    pub async fn device(&self, name: &str, ip: &str, uuid: &str, player_status: bool) -> Fake {
        let device = Arc::new(Mutex::new(Device {
            name: name.to_owned(),
            uuid: uuid.to_owned(),
            mac: format!("AA:BB:CC:00:00:{}", &uuid[uuid.len() - 2..]),
            ip: ip.to_owned(),
            wmrm: "4.2".to_owned(),
            has_player_status: player_status,
            delay: Duration::from_millis(600),
            volume: 30,
            ..Device::default()
        }));
        self.devices
            .lock()
            .unwrap()
            .insert(ip.to_owned(), device.clone());
        let app = Router::new()
            .route("/httpapi.asp", get(httpapi))
            .route("/upnp/control/{service}", post(upnp))
            .with_state((device.clone(), self.clone()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(axum::serve(listener, app).into_future());
        let sighting = Sighting {
            fullname: format!("{name}._linkplay._tcp.local."),
            name: name.to_owned(),
            address: ip.parse().unwrap(),
            mac: Some(device.lock().unwrap().mac.clone()),
            uuid: Some(uuid.to_owned()),
        };
        Fake {
            device,
            port,
            sighting,
        }
    }

    fn by_ip(&self, ip: &str) -> Option<Shared> {
        self.devices.lock().unwrap().get(ip).cloned()
    }

    fn by_uuid(&self, uuid: &str) -> Option<Shared> {
        self.devices
            .lock()
            .unwrap()
            .values()
            .find(|d| d.lock().unwrap().uuid == uuid)
            .cloned()
    }

    fn followers(&self, uuid: &str) -> Vec<Shared> {
        self.devices
            .lock()
            .unwrap()
            .values()
            .filter(|d| d.lock().unwrap().leader.as_deref() == Some(uuid))
            .cloned()
            .collect()
    }
}

impl Fake {
    /// Adds the device to `provider`, reached at its local port.
    pub fn add_to(&self, provider: &LinkplayProvider) {
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let api = Api::with_probes(local, CLIENT_CERTIFICATE, &[("http", self.port)]).unwrap();
        let upnp = Upnp::new(local, self.port);
        provider.add(
            self.sighting.clone(),
            self.sighting.id().unwrap(),
            api,
            upnp,
        );
    }

    pub fn id(&self) -> String {
        self.sighting.id().unwrap()
    }

    pub fn commands(&self) -> Vec<String> {
        self.device.lock().unwrap().commands.clone()
    }
}

/// A provider whose devices pull streams from a server it starts. Returns it with the players.
pub async fn provider() -> (LinkplayProvider, Players) {
    let players = Players::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    let mut config = Config::new(ServerAddress {
        port: address.port(),
        base_url: Some(format!("http://{address}")),
    });
    config.lost_grace = Duration::ZERO;
    let provider = LinkplayProvider::new(players.clone(), config).unwrap();
    tokio::spawn(axum::serve(listener, provider.hub().routes()).into_future());
    (provider, players)
}

fn status_ex(device: &Device, network: &Network) -> String {
    let leader = device
        .leader
        .as_ref()
        .and_then(|uuid| network.by_uuid(uuid));
    let mut status = json!({
        "DeviceName": device.name, "GroupName": device.name, "project": "Fake_Speaker",
        "firmware": "Linkplay.1.0", "uuid": device.uuid, "MAC": device.mac,
        "wmrm_version": device.wmrm, "streams": "0x1", "group": "0",
    });
    if let Some(leader) = leader {
        let leader = leader.lock().unwrap();
        status["group"] = json!("1");
        status["master_uuid"] = json!(leader.uuid);
        status["master_ip"] = json!(leader.ip);
    }
    status.to_string()
}

/// Where its countdown stands, and whether it plays.
fn transport(device: &Device) -> (&'static str, i64) {
    match &device.playing {
        None => ("stop", 0),
        Some((_, at)) => {
            let audible = *at + device.delay;
            let position = Instant::now()
                .saturating_duration_since(audible)
                .as_millis() as i64
                - audible
                    .saturating_duration_since(Instant::now())
                    .as_millis() as i64;
            ("play", position)
        }
    }
}

async fn httpapi(
    State((device, network)): State<(Shared, Network)>,
    Query(query): Query<HashMap<String, String>>,
) -> String {
    let command = query.get("command").cloned().unwrap_or_default();
    device.lock().unwrap().commands.push(command.clone());
    let (verb, rest) = command.split_once(':').unwrap_or((command.as_str(), ""));
    match verb {
        "getStatusEx" => {
            // Not locked while the leader is looked up, which locks every device in turn.
            let snapshot = {
                let d = device.lock().unwrap();
                Device {
                    name: d.name.clone(),
                    uuid: d.uuid.clone(),
                    mac: d.mac.clone(),
                    ip: d.ip.clone(),
                    wmrm: d.wmrm.clone(),
                    leader: d.leader.clone(),
                    ..Device::default()
                }
            };
            status_ex(&snapshot, &network)
        }
        "getPlayerStatus" => {
            let d = device.lock().unwrap();
            if !d.has_player_status {
                return "unknown command".to_owned();
            }
            let (status, position) = transport(&d);
            json!({"status": status, "curpos": position.to_string(), "vol": d.volume.to_string(),
                "mute": if d.muted { "1" } else { "0" }, "mode": "10", "Title": ""})
            .to_string()
        }
        "setPlayerCmd" => {
            let (action, value) = rest.split_once(':').unwrap_or((rest, ""));
            let mut d = device.lock().unwrap();
            match action {
                "vol" => d.volume = value.parse().unwrap_or(d.volume),
                "mute" => d.muted = value == "1",
                "stop" => {
                    if let Some(fetch) = d.fetch.take() {
                        fetch.abort();
                    }
                    d.playing = None;
                }
                "play" => {
                    if let Some(fetch) = d.fetch.take() {
                        fetch.abort();
                    }
                    d.playing = Some((value.to_owned(), Instant::now()));
                    d.pulled.clear();
                    d.streams += 1;
                    let (url, puller) = (value.to_owned(), device.clone());
                    d.fetch = Some(tokio::spawn(async move {
                        let Ok(response) = reqwest::get(&url).await else {
                            return;
                        };
                        let mut body = response.bytes_stream();
                        while let Some(Ok(chunk)) = body.next().await {
                            puller.lock().unwrap().pulled.extend_from_slice(&chunk);
                        }
                    }));
                }
                _ => return "unknown command".to_owned(),
            }
            "OK".to_owned()
        }
        "ConnectMasterAp" => {
            let Some(ip) = rest
                .strip_prefix("JoinGroupMaster:eth")
                .and_then(|r| r.split(':').next())
            else {
                return "unknown command".to_owned();
            };
            let Some(leader) = network.by_ip(ip) else {
                return "failed".to_owned();
            };
            let uuid = leader.lock().unwrap().uuid.clone();
            device.lock().unwrap().leader = Some(uuid);
            "OK".to_owned()
        }
        "multiroom" => {
            let uuid = device.lock().unwrap().uuid.clone();
            if rest == "getSlaveList" {
                let list: Vec<_> = network
                    .followers(&uuid)
                    .iter()
                    .map(|f| {
                        let f = f.lock().unwrap();
                        json!({"name": f.name, "uuid": f.uuid, "ip": f.ip, "volume": f.volume, "mute": 0})
                    })
                    .collect();
                return json!({"slaves": list.len(), "wmrm_version": "4.2", "slave_list": list})
                    .to_string();
            }
            if let Some(ip) = rest.strip_prefix("SlaveKickout:") {
                if let Some(follower) = network.by_ip(ip) {
                    follower.lock().unwrap().leader = None;
                }
                return "OK".to_owned();
            }
            if rest == "Ungroup" {
                device.lock().unwrap().leader = None;
                for follower in network.followers(&uuid) {
                    follower.lock().unwrap().leader = None;
                }
                return "OK".to_owned();
            }
            "unknown command".to_owned()
        }
        _ => "unknown command".to_owned(),
    }
}

async fn upnp(State((device, _)): State<(Shared, Network)>, headers: HeaderMap) -> String {
    let action = headers
        .get("SOAPACTION")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim_matches('"').split('#').nth(1))
        .unwrap_or_default()
        .to_owned();
    let d = device.lock().unwrap();
    let body = match action.as_str() {
        "GetTransportInfo" => {
            let state = if d.playing.is_some() {
                "PLAYING"
            } else {
                "STOPPED"
            };
            format!("<CurrentTransportState>{state}</CurrentTransportState>")
        }
        "GetPositionInfo" => {
            // Whole seconds since it became audible, as the devices count.
            let seconds = transport(&d).1.max(0) / 1_000;
            format!(
                "<RelTime>{:02}:{:02}:{:02}</RelTime>",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        }
        "GetVolume" => format!("<CurrentVolume>{}</CurrentVolume>", d.volume),
        "GetMute" => format!("<CurrentMute>{}</CurrentMute>", u8::from(d.muted)),
        _ => String::new(),
    };
    format!(
        "<s:Envelope><s:Body><u:{action}Response>{body}</u:{action}Response></s:Body></s:Envelope>"
    )
}

/// Polls `holds` for up to five seconds.
pub async fn eventually(mut holds: impl FnMut() -> bool) -> bool {
    for _ in 0..250 {
        if holds() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}
