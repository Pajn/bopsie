//! An AirPlay speaker as the rest of the server sees it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_players::{DeviceInfo, Endpoint, EndpointState, NowPlaying, Players};

use crate::sink::AirplaySink;
use crate::speaker::{Speaker, StreamOptions};
use crate::status::Status;
use crate::supervisor::{Launch, Supervisor};

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
            move || {
                if let Some(shared) = for_exit.upgrade() {
                    shared.players.changed(&shared.id);
                }
            },
        );
        Arc::new(Self { shared, supervisor })
    }

    /// New records for the speaker from discovery.
    pub fn rediscovered(&self, speaker: Speaker) {
        let mut state = self.shared.state.lock().unwrap();
        state.speaker = speaker;
        state.discovered = true;
        drop(state);
        self.shared.players.changed(&self.shared.id);
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
    fn observe(&self, status: &Status) {
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
        let records = &state.speaker.records;
        let needs_pairing = records.requires_pin() && state.credentials.is_none();
        EndpointState {
            name: state.speaker.name.clone(),
            provider: "airplay",
            available: state.discovered && state.failure.is_none() && !needs_pairing,
            volume: Some(state.volume),
            muted: Some(state.muted),
            device: DeviceInfo {
                model: records.model().map(str::to_owned),
                manufacturer: records.manufacturer().map(str::to_owned),
                software_version: None,
            },
            app: false,
            owner_token: None,
            formats: vec![FORMAT],
            lead: LEAD,
        }
    }

    fn output(&self, format: OutputFormat, timeline: Timeline) -> Box<dyn Sink> {
        Box::new(AirplaySink::new(self.supervisor.clone(), format, timeline))
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
}
