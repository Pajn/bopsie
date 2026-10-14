//! An AirPlay speaker as the rest of the server sees it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;
use bopsie_players::health::Event;
use bopsie_players::{
    Capabilities, DeviceInfo, Endpoint, EndpointState, Facts, NowPlaying, Pairing, Players,
};

use crate::sink::AirplaySink;
use crate::sink::CLOCK_PATIENCE;
use crate::speaker::{Speaker, StreamOptions};
use crate::status::Status;
use crate::supervisor::{ClockStart, LATE_TOLERANCE_MS, Launch, Supervisor};

/// The format `cliairplay` takes on every route: 16-bit stereo at 44.1 kHz.
pub const FORMAT: OutputFormat = OutputFormat {
    sample_rate: 44_100,
    channels: 2,
    bits: 16,
};

/// How far ahead playback starts: room for a process to connect and a speaker to buffer, as
/// Music Assistant gives a group that starts cold.
pub const LEAD: Micros = 2_500_000;

const DEFAULT_VOLUME: u8 = 30;

/// How often an idle session is checked for, for the `disconnect_on_idle_s` quirk.
const IDLE_CHECK: Duration = Duration::from_secs(1);

/// The most speakers one PTP clock serves.
const MAX_GROUP_SIZE: usize = 8;

/// What every speaker's process shares.
#[derive(Debug, Clone)]
pub struct Launcher {
    pub binary: PathBuf,
    /// Where command pipes are made.
    pub pipes: PathBuf,
    pub dacp_id: String,
    pub env: Vec<(String, String)>,
    /// Whether the shared PTP daemon is serving.
    pub ptp_shared: Arc<Mutex<bool>>,
}

#[derive(Debug)]
struct State {
    speaker: Speaker,
    discovered: bool,
    volume: u8,
    muted: bool,
    credentials: Option<String>,
    password: Option<String>,
    /// Why the speaker cannot be played, when a connection failed for good.
    failure: Option<String>,
}

struct Shared {
    id: String,
    state: Mutex<State>,
    players: Players,
}

pub struct AirplayEndpoint {
    shared: Arc<Shared>,
    supervisor: Supervisor,
    /// Counts the times discovery found the speaker again.
    sightings: std::sync::atomic::AtomicU64,
}

impl AirplayEndpoint {
    pub fn new(
        speaker: Speaker,
        launcher: Launcher,
        credentials: Option<String>,
        password: Option<String>,
        players: Players,
    ) -> Arc<Self> {
        let id = speaker.id.clone();
        let shared = Arc::new(Shared {
            id: id.clone(),
            state: Mutex::new(State {
                speaker,
                discovered: true,
                volume: DEFAULT_VOLUME,
                muted: false,
                credentials,
                password,
                failure: None,
            }),
            players,
        });
        let for_launch: Weak<Shared> = Arc::downgrade(&shared);
        let for_status = Arc::downgrade(&shared);
        let for_exit = Arc::downgrade(&shared);
        let supervisor = Supervisor::new(
            &id,
            move || {
                let shared = for_launch.upgrade().ok_or("the speaker is gone")?;
                Ok(launch(&shared, &launcher))
            },
            move |status| {
                if let Some(shared) = for_status.upgrade() {
                    shared.observe(status);
                }
            },
            move |expected| {
                if let Some(shared) = for_exit.upgrade() {
                    if !expected {
                        shared.players.record(&shared.id, Event::Exited);
                    }
                    shared.players.changed(&shared.id);
                }
            },
        );
        let endpoint = Arc::new(Self {
            shared,
            supervisor,
            sightings: std::sync::atomic::AtomicU64::new(0),
        });
        let watched = Arc::downgrade(&endpoint);
        std::thread::Builder::new()
            .name("airplay-idle".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(IDLE_CHECK);
                    let Some(endpoint) = watched.upgrade() else {
                        return;
                    };
                    endpoint.disconnect_if_idle();
                }
            })
            .expect("spawning a thread");
        endpoint
    }

    /// Ends the session of a speaker whose `disconnect_on_idle_s` quirk says it has been idle
    /// too long.
    fn disconnect_if_idle(&self) {
        let Some(limit) = self.state().quirks.quirks.disconnect_on_idle_s else {
            return;
        };
        let limit = Duration::from_secs(u64::from(limit));
        if self.supervisor.idle_for().is_some_and(|idle| idle >= limit) {
            tracing::info!(speaker = self.shared.id, "disconnecting an idle speaker");
            self.supervisor.disconnect();
        }
    }

    /// New records for the speaker from discovery.
    pub fn rediscovered(&self, speaker: Speaker) {
        self.sightings
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut state = self.shared.state.lock().unwrap();
        state.speaker = speaker;
        state.discovered = true;
        drop(state);
        self.shared.players.changed(&self.shared.id);
    }

    /// How many times discovery found the speaker again.
    pub fn sightings(&self) -> u64 {
        self.sightings.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The speaker left the network.
    pub fn lost(&self) {
        self.shared.state.lock().unwrap().discovered = false;
        self.supervisor.kill();
        self.shared.players.changed(&self.shared.id);
    }

    pub fn speaker(&self) -> Speaker {
        self.shared.state.lock().unwrap().speaker.clone()
    }

    pub fn set_credentials(&self, credentials: String) {
        let mut state = self.shared.state.lock().unwrap();
        state.credentials = Some(credentials);
        state.failure = None;
        drop(state);
        // The next stream starts a process that pair-verifies with them.
        self.supervisor.kill();
        self.shared.players.changed(&self.shared.id);
    }

    pub fn supervisor(&self) -> &Supervisor {
        &self.supervisor
    }

    /// Sends commands to the speaker's process, if one runs. Values lose their line breaks.
    fn command(&self, lines: Vec<String>) {
        if self.supervisor.is_running() {
            let _ = self.supervisor.command(&lines);
        }
    }
}

fn launch(shared: &Shared, launcher: &Launcher) -> Launch {
    let state = shared.state.lock().unwrap();
    let cmdpipe = launcher.pipes.join(format!("{}.fifo", shared.id));
    let options = StreamOptions {
        dacp_id: launcher.dacp_id.clone(),
        active_remote: format!("{}", rand_u32()),
        sample_rate: FORMAT.sample_rate,
        bit_depth: FORMAT.bits,
        volume: Some(if state.muted { 0 } else { state.volume }),
        credentials: state.credentials.clone(),
        password: state.password.clone(),
        ptp_shared: *launcher.ptp_shared.lock().unwrap(),
        quirks: shared.snapshot(&state).quirks.quirks,
    };
    Launch {
        binary: launcher.binary.clone(),
        args: state.speaker.stream_args(&options, &cmdpipe),
        cmdpipe,
        env: launcher.env.clone(),
    }
}

/// An Active-Remote id: any number, new for each process.
fn rand_u32() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() | 1)
}

impl Shared {
    /// The endpoint's state, as `state` has it.
    fn snapshot(&self, state: &State) -> EndpointState {
        let records = &state.speaker.records;
        let mut endpoint = EndpointState {
            name: state.speaker.name.clone(),
            provider: "airplay",
            available: state.discovered && state.failure.is_none(),
            volume: Some(state.volume),
            muted: Some(state.muted),
            device: DeviceInfo {
                model: records.model().map(str::to_owned),
                manufacturer: records.manufacturer().map(str::to_owned),
                software_version: records.firmware().map(str::to_owned),
            },
            app: false,
            owner_token: None,
            facts: Facts {
                mac: self.id.strip_prefix("ap").map(str::to_owned),
                serial: records.serial().map(str::to_owned),
                uuid: records.uuid().map(str::to_owned),
                address: Some(state.speaker.address),
                features: Some(records.features()),
                status_flags: Some(records.status_flags()),
            },
            capabilities: Capabilities {
                formats: vec![FORMAT],
                lead: LEAD,
                sync_group: true,
                max_group_size: Some(MAX_GROUP_SIZE),
                volume: true,
                mute: true,
                pairing: state.speaker.pairing(),
                native_group: None,
                withdrawn: Vec::new(),
            },
            quirks: Default::default(),
            native_leader: None,
        };
        endpoint.quirks =
            self.players
                .resolve_quirks(&self.id, &endpoint, &state.speaker.quirk_defaults());
        // A speaker that pairs with a PIN plays once it has been paired.
        if endpoint.quirks.quirks.pairing == Some(Pairing::Pin) && state.credentials.is_none() {
            endpoint.available = false;
        }
        endpoint
    }
    fn observe(&self, status: &Status) {
        if let Some(event) = telemetry(status) {
            self.players.record(&self.id, event);
        }
        match status {
            Status::Connected => {
                let had_failure = self.state.lock().unwrap().failure.take().is_some();
                if had_failure {
                    self.players.changed(&self.id);
                }
            }
            Status::Error { code, detail, .. } if status.is_terminal() => {
                let reason = match code.as_str() {
                    "auth_required" => "the speaker needs a password".to_owned(),
                    "auth_failed" => "the speaker refused the password or pairing".to_owned(),
                    _ => format!("cannot connect: {detail}"),
                };
                // Connection failures may pass; authentication failures need the user.
                if code.starts_with("auth") {
                    self.state.lock().unwrap().failure = Some(reason);
                    self.players.changed(&self.id);
                }
            }
            Status::ClockReady { state, .. } if state == "stalled" => {
                tracing::warn!(
                    speaker = self.id,
                    "the speaker does not follow our clock and will play silence; are UDP ports 319 and 320 reachable from it?"
                );
            }
            _ => {}
        }
    }
}

/// What a status line says about the speaker's health.
fn telemetry(status: &Status) -> Option<Event> {
    match status {
        Status::Connected => Some(Event::Connected),
        Status::Error { .. } if status.is_terminal() => Some(Event::ConnectFailed),
        Status::ClockReady { state, .. } => match state.as_str() {
            "ready" => Some(Event::ClockReady),
            "stalled" => Some(Event::ClockStalled),
            _ => None,
        },
        Status::Started {
            requested_unix_ms,
            at_unix_ms,
        } => Some(if *at_unix_ms > requested_unix_ms + LATE_TOLERANCE_MS {
            Event::LateStart
        } else {
            Event::Started
        }),
        _ => None,
    }
}

/// Strips what would end a command line.
fn clean(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

impl Endpoint for AirplayEndpoint {
    fn id(&self) -> &str {
        &self.shared.id
    }

    fn state(&self) -> EndpointState {
        let state = self.shared.state.lock().unwrap();
        self.shared.snapshot(&state)
    }

    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink> {
        let adjust_ms = self.state().quirks.quirks.sync_adjust_ms.unwrap_or(0);
        Box::new(AirplaySink::new(
            self.supervisor.clone(),
            format,
            timeline,
            adjust_ms,
        ))
    }

    fn set_volume(&self, volume: u8) {
        self.shared.state.lock().unwrap().volume = volume;
        self.command(vec![format!("VOLUME={volume}")]);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_muted(&self, muted: bool) {
        let volume = {
            let mut state = self.shared.state.lock().unwrap();
            state.muted = muted;
            if muted { 0 } else { state.volume }
        };
        self.command(vec![format!("VOLUME={volume}")]);
        self.shared.players.changed(&self.shared.id);
    }

    fn set_now_playing(&self, now_playing: Option<&NowPlaying>) {
        let Some(now) = now_playing else {
            return;
        };
        let mut lines = vec![
            format!("TITLE={}", clean(&now.title)),
            format!("ARTIST={}", clean(now.artist.as_deref().unwrap_or(""))),
            format!("ALBUM={}", clean(now.album.as_deref().unwrap_or(""))),
            format!("DURATION={}", now.duration / 1000),
            format!("PROGRESS={}", now.position / 1000),
        ];
        if let Some(url) = &now.artwork_url {
            lines.push(format!("ARTWORKFILE={}", clean(url)));
        }
        lines.push("ACTION=SENDMETA".into());
        self.command(lines);
    }

    fn set_group(&self, _playing: bool, _group_id: &str) {
        // AirPlay speakers are not told about each other: each plays its own stream.
    }

    fn prepare(&self) -> Option<Micros> {
        if let Err(error) = self.supervisor.ensure_connected() {
            tracing::warn!(speaker = self.shared.id, %error, "cannot connect");
            return None;
        }
        // Wait a while for the speaker to say when its clock will follow ours.
        let deadline = std::time::Instant::now() + CLOCK_PATIENCE;
        loop {
            match self.supervisor.clock_start() {
                ClockStart::From(unix_ms) => {
                    let now_ms = clock::unix_ms(clock::now()) as i64;
                    return Some(clock::now() + (unix_ms as i64 - now_ms) * 1_000);
                }
                ClockStart::NotMeasured => return None,
                ClockStart::Unknown if std::time::Instant::now() >= deadline => return None,
                ClockStart::Unknown => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
    }
}
