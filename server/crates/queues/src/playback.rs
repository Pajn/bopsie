//! Playback across devices: one queue per device, sync groups that a leader's queue plays to,
//! and what the API reports of both.
//!
//! A leader's queue session runs one engine, as the group planner says: over one protocol, in
//! one format, with a sink for each device it carries, through the endpoint it chose. A device
//! that joins while the group plays is given what is still to play; one the plan cannot carry
//! is refused, with the planner's reason. Whatever a session reports is passed on as Music
//! Assistant's events, and to the devices themselves as group state and what is playing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bopsie_audio::PcmFormat;
use bopsie_audio::engine::{Input, LIVE_AHEAD, Playable, QUEUE_AHEAD};
use bopsie_audio::engine::{Realtime, Settings};
use bopsie_audio::sink::Sink;
use bopsie_audio::stretch::{MAX_RATE, MIN_RATE};
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::backoff::{self, Backoff};
use bopsie_core::clock;
use bopsie_core::db::Database;
use bopsie_core::events::{Event, EventBus, EventType};
use bopsie_core::media::Track;
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_library::Library;
use bopsie_library::catalog::Progress;
use bopsie_players::devices::PROTOCOLS;
use bopsie_players::planner::{self, Plan, Policy, Strategy};
use bopsie_players::{
    Change, Controls, Device, Devices, Endpoint, Member, NowPlaying, Remote, Repeat,
};
use bopsie_sources::{Control, Happened, Source, SourceEvent, Sources};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::{Value, json};

use crate::calibration::{self, Calibration, Scheduled};
use crate::model::{self, Grouping, PlayerPlayback, PlayerView, SourceEntry, SourcePlayback};
use crate::session::{Notice, QueueSession};
use crate::state::{Effect, PlaybackState, Queue, QueueError, QueueOption, RepeatMode};
use crate::store::{self, SavedQueue};

/// Recent audio kept for members that join a playing group.
const RING_SECONDS: usize = 10;
/// Crashes of one queue's engine within [`CRASH_WINDOW`] after which it stops.
const MAX_CRASHES: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// How long a leader may be away and still play on when it comes back.
const RESUME_WITHIN: Duration = Duration::from_secs(60);
/// How often what changed is saved.
const SAVE_EVERY: Duration = Duration::from_secs(1);
/// How long a device's own report of its native group is ignored after the server changed it.
const SETTLING: Duration = Duration::from_secs(15);
/// How long a device that left its native group has to bring back a protocol it withdrew.
const WITHDRAWN_RETURN: Duration = Duration::from_secs(30);
/// The setting [`Playback::mixed_airplay_ms`] is kept under.
const MIXED_AIRPLAY_MS: &str = "playback.mixed_airplay_ms";
/// The setting how fast each audiobook last played is kept under.
const SPEEDS: &str = "playback.speeds";
/// The setting [`Playback::last_calibration`] is kept under.
const LAST_CALIBRATION: &str = "playback.last_calibration";
/// The provider whose speakers keep in step on PTP.
pub const AIRPLAY: &str = "airplay";

/// A queue's sleep timer.
#[derive(Debug, Clone, PartialEq)]
struct Sleep {
    until: SleepUntil,
}

#[derive(Debug, Clone, PartialEq)]
enum SleepUntil {
    /// On the server's clock.
    At(Micros),
    /// The end of the chapter `chapter` of item `item`, or of the item when it has none.
    EndOf {
        item: String,
        chapter: Option<usize>,
    },
}

/// When a queue's sleep timer pauses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SleepAt {
    /// Unix seconds.
    Unix(f64),
    EndOfChapter,
}

/// What was last reported of a queue's current audiobook.
#[derive(Debug, Clone, PartialEq)]
struct Reported {
    uri: String,
    playing: bool,
    /// How fast it plays.
    rate: f64,
    /// The queue played it to its end.
    ended: bool,
    position: Duration,
    duration: Duration,
    at: Instant,
}

/// How long an item plays before its play counts; half of it, for an item shorter than twice
/// that.
const PLAY_COUNTS_AFTER: Duration = Duration::from_secs(30);

/// How far a position may move from where playing for the time between reports takes it
/// before it counts as a seek.
const SEEK_SLACK: Duration = Duration::from_secs(3);
/// How near its end an item left counts as played to its end.
const END_SLACK: Duration = Duration::from_secs(5);

/// What to report of a queue's audiobooks, from what was reported last and what is now: of one
/// left, that it ended or was stopped; of the one now, that it plays, paused or moved.
fn progress_between(before: Option<&Reported>, now: Option<&Reported>) -> Vec<(String, Progress)> {
    let mut reports = Vec::new();
    let same = matches!((before, now), (Some(b), Some(n)) if b.uri == n.uri);
    if let Some(before) = before.filter(|_| !same)
        && !before.ended
    {
        let progress = if before.position + END_SLACK >= before.duration {
            Progress::Ended
        } else {
            Progress::Stopped(before.position)
        };
        reports.push((before.uri.clone(), progress));
    }
    let Some(now) = now else {
        return reports;
    };
    let before = before.filter(|_| same);
    if now.ended {
        if !before.is_some_and(|b| b.ended) {
            reports.push((now.uri.clone(), Progress::Ended));
        }
        return reports;
    }
    // Where it would be had nothing moved it.
    let expected = before.map(|b| {
        if b.playing {
            b.position + now.at.saturating_duration_since(b.at).mul_f64(b.rate)
        } else {
            b.position
        }
    });
    let moved = expected.is_some_and(|expected| now.position.abs_diff(expected) > SEEK_SLACK);
    let progress = if moved {
        Progress::Seeked(now.position)
    } else if now.playing {
        Progress::Playing(now.position)
    } else if before.is_none_or(|b| b.playing) {
        Progress::Paused(now.position)
    } else {
        return reports;
    };
    reports.push((now.uri.clone(), progress));
    reports
}

/// How often a playing queue's position is saved.
const POSITION_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub ffmpeg: Option<PathBuf>,
    /// Where clients reach the server, for artwork URLs sent to players.
    pub base_url: Option<String>,
    /// What the group planner may choose from.
    pub policy: Policy,
    /// The loudness tracks are brought to, in LUFS; `None` plays them as they are.
    pub normalise: Option<f32>,
}

/// A device leading a group, or playing on its own.
#[derive(Debug, Clone)]
pub struct Group {
    pub leader: String,
    /// The devices following it, in the order they joined.
    pub members: Vec<String>,
    /// How it plays, while it has an engine.
    pub plan: Option<Plan>,
}

#[derive(Clone)]
struct Entry {
    session: QueueSession,
    /// The leader this device follows.
    synced_to: Option<String>,
    /// The devices following this one, in the order they joined.
    members: Vec<String>,
    /// How the leader's engine plays, while it has one.
    plan: Option<Plan>,
}

struct Inner {
    devices: Devices,
    events: EventBus,
    config: Config,
    entries: Mutex<BTreeMap<String, Entry>>,
    /// Whether each device could play when last seen, to tell when one comes back.
    available: Mutex<BTreeMap<String, bool>>,
    /// How long each device whose output failed waits before it rejoins.
    backoffs: Mutex<BTreeMap<String, Backoff>>,
    /// Devices waiting to rejoin.
    rejoining: Mutex<BTreeSet<String>>,
    /// When each queue's engine recently crashed.
    crashes: Mutex<BTreeMap<String, Vec<Instant>>>,
    /// Queues paused because their leader went away, and since when.
    absent: Mutex<BTreeMap<String, Instant>>,
    /// Where what outlasts a restart is kept, once [`Playback::persist`] is called.
    db: OnceLock<Database>,
    /// How much later AirPlay speakers play in a group that mixes protocols, in ms: they keep
    /// in step with each other on PTP, and as a whole with the others by this.
    mixed_airplay_ms: std::sync::atomic::AtomicI32,
    /// What the last calibration set, as the API reports it.
    last_calibration: Mutex<Option<serde_json::Value>>,
    /// Where listening progress in a catalog's items, such as audiobooks, is reported.
    progress_to: OnceLock<Library>,
    /// What was last reported of each queue's item.
    reported: Mutex<HashMap<String, Reported>>,
    /// The item of each queue whose play was counted last, by queue item id.
    counted: Mutex<HashMap<String, String>>,
    /// How fast each audiobook last played, by URI, for it to play so again.
    speeds: Mutex<HashMap<String, f32>>,
    /// When each queue with a sleep timer pauses.
    sleeps: Mutex<HashMap<String, Sleep>>,
    /// Queues from an earlier run for devices not here yet.
    restored: Mutex<BTreeMap<String, Queue>>,
    /// Groups from an earlier run, by member, waiting for their devices.
    saved_groups: Mutex<BTreeMap<String, String>>,
    /// Volumes from an earlier run for devices not here yet.
    saved_volumes: Mutex<BTreeMap<String, (u8, bool)>>,
    /// What was last written of each queue, without its position, and when.
    last_saved: Mutex<BTreeMap<String, (SavedQueue, Instant)>>,
    last_groups: Mutex<Option<BTreeMap<String, String>>>,
    last_volumes: Mutex<BTreeMap<String, (u8, bool)>>,
    /// Shutting down: nothing more is saved.
    closing: AtomicBool,
    /// Devices whose native groups the server is changing or last changed, and when.
    settling: Mutex<BTreeMap<String, Instant>>,
    /// Groups playing a calibration, by leader: nothing else plays to them meanwhile.
    calibrating: Mutex<BTreeSet<String>>,
    /// Endpoints the server just moved a device off, as from LinkPlay to its Cast receiver, or
    /// woke it through, and when: the device leaving the old one's input is the server's doing,
    /// not someone's at it.
    left: Mutex<BTreeMap<String, Instant>>,
    rng: Mutex<StdRng>,
    /// Sources that play to groups, once [`Playback::follow_sources`] is called.
    sources: OnceLock<Sources>,
}

#[derive(Clone)]
pub struct Playback {
    inner: Arc<Inner>,
}

/// What a queue command asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Seconds into the current item.
    Seek(f64),
    /// Seconds forward, or back when negative.
    Skip(f64),
    Shuffle(bool),
    Repeat(RepeatMode),
    Clear,
}

/// How far a speaker's volume buttons move its volume.
const REMOTE_VOLUME_STEP: u8 = 5;

/// How long after its speakers are ready a calibration starts.
const CALIBRATION_MARGIN: Micros = 500_000;

fn calibrating() -> Error {
    Error::new(
        ErrorCode::UnsupportedFeature,
        "The group is calibrating; it plays again once that is done",
    )
}

/// Writes one speaker's part of a calibration, and holds its output until the calibration
/// ends, so that a sink that stops when finished plays it all.
fn play_calibration(mut sink: Box<dyn Sink>, index: usize, count: usize, ends_at: Micros) {
    let format = sink.format();
    let samples = calibration::track(index, count, format.sample_rate, format.channels);
    let channels = usize::from(format.channels);
    // Twenty milliseconds at a time, as the engine writes.
    let block = format.sample_rate as usize / 50 * channels;
    let mut bytes = Vec::new();
    for (i, chunk) in samples.chunks(block).enumerate() {
        bytes.clear();
        bopsie_audio::pack::pack(chunk, format.bits, &mut bytes);
        if let Err(error) = sink.write((i * block / channels) as u64, &bytes) {
            tracing::warn!(%error, "a speaker failed during a calibration");
            let _ = sink.finish();
            return;
        }
    }
    let _ = sink.drain();
    let wait = ends_at - clock::now();
    if wait > 0 {
        std::thread::sleep(Duration::from_micros(wait as u64));
    }
    let _ = sink.finish();
}

fn not_found(id: &str) -> Error {
    Error::new(
        ErrorCode::PlayerUnavailable,
        format!("Player {id} not found"),
    )
}

fn unavailable() -> Error {
    Error::new(ErrorCode::PlayerUnavailable, "The player is not available")
}

fn queue_error(error: QueueError) -> Error {
    Error::invalid_data(error.to_string())
}

/// Plays on from `position`, as a group whose plan changed does.
fn resume_at(position: Duration) -> impl FnOnce(&mut Queue, &mut StdRng) -> ((), Effect) {
    move |q, _| {
        q.state = PlaybackState::Idle;
        q.resume_position = position;
        ((), q.play())
    }
}

impl Playback {
    /// Follows `devices`, giving each a queue. Needs a Tokio runtime.
    pub fn start(devices: Devices, events: EventBus, config: Config) -> Self {
        let playback = Self {
            inner: Arc::new(Inner {
                devices: devices.clone(),
                events,
                config,
                entries: Mutex::default(),
                available: Mutex::default(),
                backoffs: Mutex::default(),
                rejoining: Mutex::default(),
                crashes: Mutex::default(),
                absent: Mutex::default(),
                db: OnceLock::new(),
                mixed_airplay_ms: Default::default(),
                last_calibration: Mutex::default(),
                progress_to: OnceLock::new(),
                reported: Mutex::default(),
                counted: Mutex::default(),
                speeds: Mutex::default(),
                sleeps: Mutex::default(),
                restored: Mutex::default(),
                saved_groups: Mutex::default(),
                saved_volumes: Mutex::default(),
                last_saved: Mutex::default(),
                last_groups: Mutex::default(),
                last_volumes: Mutex::default(),
                closing: AtomicBool::new(false),
                settling: Mutex::default(),
                calibrating: Mutex::default(),
                left: Mutex::default(),
                rng: Mutex::new(StdRng::from_rng(&mut rand::rng())),
                sources: OnceLock::new(),
            }),
        };
        let mut changes = devices.subscribe();
        let mut remotes = devices.players().remotes();
        for device in devices.all() {
            playback.add_player(&device.id);
        }
        let watcher = playback.clone();
        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(Change::Added(id)) => watcher.add_player(&id),
                    Ok(Change::Updated(id)) => watcher.player_changed(&id),
                    Ok(Change::Removed(id)) => watcher.remove_player(&id),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let listener = playback.clone();
        tokio::spawn(async move {
            loop {
                match remotes.recv().await {
                    Ok((endpoint, remote)) => listener.remote(&endpoint, remote),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let ticker = playback.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                ticker.tick();
            }
        });
        playback
    }

    /// Carries out what a speaker's own controls asked for, on the group it plays in: a pause
    /// pressed on one speaker pauses all of them, as it would in the app.
    fn remote(&self, endpoint: &str, remote: Remote) {
        let Some(device) = self.inner.devices.device_of(endpoint) else {
            return;
        };
        let Some(queue) = self.leader_of(&device.id) else {
            return;
        };
        let playing = match self.live_of(&queue) {
            Some(source) => source.state().playing,
            None => self
                .entry(&queue)
                .is_ok_and(|entry| entry.session.snapshot().state == PlaybackState::Playing),
        };
        if remote == Remote::Released
            && self
                .inner
                .left
                .lock()
                .unwrap()
                .get(endpoint)
                .is_some_and(|at| at.elapsed() < SETTLING)
        {
            tracing::debug!(
                endpoint,
                "an endpoint the server moved its device off released it"
            );
            return;
        }
        let result = match remote {
            // Some speakers answer the server's own play or pause with the same command;
            // one that changes nothing is taken for that.
            Remote::Play if playing => Ok(()),
            Remote::Pause if !playing => Ok(()),
            Remote::Play => self.command(&queue, QueueCommand::Play),
            Remote::Pause => self.command(&queue, QueueCommand::Pause),
            Remote::PlayPause => self.command(&queue, QueueCommand::PlayPause),
            Remote::Stop => self.command(&queue, QueueCommand::Stop),
            Remote::Next => self.command(&queue, QueueCommand::Next),
            Remote::Previous => self.command(&queue, QueueCommand::Previous),
            Remote::VolumeUp | Remote::VolumeDown => {
                let current = self
                    .active(&device.id)
                    .ok()
                    .and_then(|(_, member)| member.state.volume)
                    .unwrap_or(0);
                let next = if remote == Remote::VolumeUp {
                    current.saturating_add(REMOTE_VOLUME_STEP).min(100)
                } else {
                    current.saturating_sub(REMOTE_VOLUME_STEP)
                };
                self.volume_set(&device.id, next)
            }
            Remote::Volume(volume) => self.group_volume(&device.id, volume),
            Remote::Mute(muted) => {
                for id in self.group_of(&queue) {
                    if let Ok((_, member)) = self.active(&id) {
                        member.endpoint.set_muted(muted);
                    }
                }
                Ok(())
            }
            Remote::Repeat(repeat) => self.command(
                &queue,
                QueueCommand::Repeat(match repeat {
                    Repeat::Off => RepeatMode::Off,
                    Repeat::One => RepeatMode::One,
                    Repeat::All => RepeatMode::All,
                }),
            ),
            Remote::Shuffle(on) => self.command(&queue, QueueCommand::Shuffle(on)),
            Remote::Seek(ms) => self.command(&queue, QueueCommand::Seek(ms as f64 / 1_000.0)),
            // A speaker that went to another source leaves its group, which plays on; a
            // speaker playing on its own pauses where it was.
            Remote::Released if queue != device.id => self.ungroup(&device.id),
            Remote::Released if playing => self.command(&queue, QueueCommand::Pause),
            Remote::Released => Ok(()),
        };
        if let Err(error) = result {
            tracing::warn!(device = device.id, ?remote, %error, "cannot do what the speaker asked");
        }
    }

    /// Plays sources to the groups of the players they are announced for. Needs a Tokio
    /// runtime.
    pub fn follow_sources(&self, sources: Sources) {
        let mut events = sources.subscribe();
        if self.inner.sources.set(sources).is_err() {
            return;
        }
        let listener = self.clone();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => listener.source_event(event),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    fn source(&self, id: &str) -> Option<Arc<dyn Source>> {
        self.inner.sources.get()?.get(id)
    }

    /// The sources announced for `player`.
    fn sources_of(&self, player: &str) -> Vec<Arc<dyn Source>> {
        self.inner
            .sources
            .get()
            .map(|sources| sources.of_player(player))
            .unwrap_or_default()
    }

    /// The source the group led by `leader` plays, if it plays one.
    fn live_of(&self, leader: &str) -> Option<Arc<dyn Source>> {
        let id = self.entry(leader).ok()?.session.live_source()?;
        self.source(&id)
    }

    fn source_event(&self, event: SourceEvent) {
        let Some(source) = self.source(&event.source) else {
            // A source that is gone ends what it played.
            for (leader, entry) in self.inner.entries.lock().unwrap().clone() {
                if entry.session.live_source().as_deref() == Some(event.source.as_str()) {
                    self.end_source(&leader);
                }
            }
            return;
        };
        let Some(leader) = self.leader_of(source.player()) else {
            return;
        };
        let live = self.live_of(&leader).is_some_and(|s| s.id() == source.id());
        match event.happened {
            Happened::Playing => self.play_source(&leader, source),
            Happened::Paused if live => self.pause_source(&leader, source.as_ref()),
            // A seek while paused, as the rewind after a pause, waits for play.
            Happened::Seeked if live && source.state().playing => {
                if let Ok(entry) = self.entry(&leader) {
                    entry.session.restart_live(Input::decoder(source.input()));
                }
            }
            Happened::MediaChanged if live => self.announce_source(&leader, source),
            Happened::Volume(volume) if live => {
                if let Err(error) = self.group_volume(&leader, volume) {
                    tracing::warn!(queue = leader, %error, "cannot follow the source's volume");
                }
            }
            Happened::Ended if live => self.end_source(&leader),
            _ => {}
        }
    }

    /// Plays `source` to the group led by `leader`, in place of its queue or another source.
    fn play_source(&self, leader: &str, source: Arc<dyn Source>) {
        let Ok(entry) = self.entry(leader) else {
            return;
        };
        if let Some(other) = self.live_of(leader)
            && other.id() != source.id()
        {
            other.control(Control::Release);
        }
        tracing::info!(queue = leader, source = source.id(), "playing a source");
        self.inner.absent.lock().unwrap().remove(leader);
        let session = entry.session;
        let epoch = session.play_live(source.id(), Input::decoder(source.input()));
        let playback = self.clone();
        let leader = leader.to_owned();
        tokio::spawn(async move {
            let not_before = playback.prepare(&leader).await;
            match playback.ensure_engine(&leader) {
                Ok(()) => session.release(epoch, not_before),
                Err(error) => {
                    tracing::warn!(queue = leader, %error, "cannot play the source");
                    session.abandon(epoch);
                }
            }
            playback.announce_source(&leader, source);
            playback.changed(&leader);
        });
    }

    /// The source paused: the speakers stop at once, and the source goes back by what they
    /// had not played yet.
    fn pause_source(&self, leader: &str, source: &dyn Source) {
        let Ok(entry) = self.entry(leader) else {
            return;
        };
        let unheard = entry.session.ahead();
        entry.session.pause_live();
        source.unheard(unheard);
        for id in self.group_of(leader) {
            if let Ok((_, member)) = self.active(&id) {
                member.endpoint.set_group(false, leader);
            }
        }
        self.changed(leader);
    }

    /// The group stops playing its source; its queue rests, paused, as it was.
    fn end_source(&self, leader: &str) {
        let Ok(entry) = self.entry(leader) else {
            return;
        };
        if let Some(source) = entry.session.end_live() {
            tracing::info!(queue = leader, source, "the source stopped playing");
        }
        for id in self.group_of(leader) {
            if let Ok((_, member)) = self.active(&id) {
                member.endpoint.set_group(false, leader);
                member.endpoint.set_now_playing(None);
            }
        }
        self.changed(leader);
    }

    /// Tells the group's devices what the source plays, when they can hear it: what the
    /// source says now, the speakers play after the audio already on its way.
    fn announce_source(&self, leader: &str, source: Arc<dyn Source>) {
        let delay = self
            .entry(leader)
            .map(|entry| entry.session.ahead())
            .unwrap_or_default();
        let playback = self.clone();
        let leader = leader.to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if playback
                .live_of(&leader)
                .is_none_or(|s| s.id() != source.id())
            {
                return;
            }
            let state = source.state();
            let now_playing = state.media.as_ref().map(|media| NowPlaying {
                title: media.title.clone(),
                artist: media.artist.clone(),
                album: media.album.clone(),
                album_artist: media.artist.clone(),
                year: None,
                track_number: None,
                artwork_url: media.artwork_url.clone(),
                duration: media.duration_ms,
                position: state.position_at(clock::now()),
                playing: state.playing,
            });
            for id in playback.group_of(&leader) {
                if let Ok((_, member)) = playback.active(&id) {
                    member.endpoint.set_group(state.playing, &leader);
                    member.endpoint.set_now_playing(now_playing.as_ref());
                }
            }
            playback.changed(&leader);
        });
    }

    fn entry(&self, id: &str) -> Result<Entry> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }

    fn device(&self, id: &str) -> Result<Device> {
        self.inner.devices.get(id).ok_or_else(|| not_found(id))
    }

    /// The device's endpoint in use: the one its group's plan plays through, else the one it
    /// is shown as.
    fn active(&self, id: &str) -> Result<(Device, Member)> {
        self.active_among(id, &self.inner.devices.all())
    }

    /// As [`Playback::active`], among `devices`.
    fn active_among(&self, id: &str, devices: &[Device]) -> Result<(Device, Member)> {
        let device = find(devices, id).ok_or_else(|| not_found(id))?.clone();
        let carrier = self
            .leader_of(id)
            .and_then(|leader| self.entry(&leader).ok()?.plan)
            .and_then(|plan| plan.carrier(id).map(|c| c.endpoint.clone()));
        let member = carrier
            .and_then(|endpoint| device.member(&endpoint).cloned())
            .unwrap_or_else(|| device.primary().clone());
        Ok((device, member))
    }

    fn add_player(&self, id: &str) {
        let session = {
            let mut entries = self.inner.entries.lock().unwrap();
            if entries.contains_key(id) {
                drop(entries);
                self.player_changed(id);
                return;
            }
            let restored = self.inner.restored.lock().unwrap().remove(id);
            let session = QueueSession::new(restored.unwrap_or_default());
            entries.insert(
                id.to_owned(),
                Entry {
                    session: session.clone(),
                    synced_to: None,
                    members: Vec::new(),
                    plan: None,
                },
            );
            session
        };
        let available = self.device(id).is_ok_and(|d| d.available());
        self.inner
            .available
            .lock()
            .unwrap()
            .insert(id.to_owned(), available);
        let mut notices = session.notices();
        let follower = self.clone();
        let queue_id = id.to_owned();
        tokio::spawn(async move {
            while let Ok(notice) = notices.recv().await {
                follower.on_notice(&queue_id, notice);
            }
        });
        self.restore_device(id);
        self.enforce_cap(id);
        self.publish(EventType::PlayerAdded, id, self.player(id, None));
        self.publish(EventType::QueueAdded, id, self.queue(id));
    }

    /// Gives a device that appears what an earlier run left for it: its volume, once, and
    /// the groups it was in, when their other devices are here too.
    fn restore_device(&self, id: &str) {
        let volume = self.inner.saved_volumes.lock().unwrap().remove(id);
        if let Some((volume, muted)) = volume
            && let Ok((_, member)) = self.active(id)
        {
            let volume = volume.min(self.cap(id));
            if member.state.volume != Some(volume) {
                member.endpoint.set_volume(volume);
            }
            if muted && member.state.muted != Some(true) {
                member.endpoint.set_muted(true);
            }
        }
        let ready: Vec<(String, String)> = self
            .inner
            .saved_groups
            .lock()
            .unwrap()
            .iter()
            .filter(|(member, leader)| {
                [member, leader]
                    .iter()
                    .all(|d| self.entry(d).is_ok() && self.device(d).is_ok_and(|d| d.available()))
            })
            .map(|(m, l)| (m.clone(), l.clone()))
            .collect();
        for (member, leader) in ready {
            self.inner.saved_groups.lock().unwrap().remove(&member);
            if let Err(error) = self.group(&member, &leader) {
                tracing::info!(member, leader, %error, "a group from before cannot form again");
            }
        }
    }

    /// Restores what an earlier run kept in `db`, with each queued track as `library` has it
    /// now, and keeps what changes from now on. Nothing plays on its own: queues come back
    /// idle where they were.
    pub async fn persist(&self, db: Database, library: Library) -> Result<()> {
        let mixed: Option<String> = db
            .call(|conn| Ok(bopsie_core::db::setting(conn, MIXED_AIRPLAY_MS)?))
            .await?;
        if let Some(ms) = mixed.and_then(|ms| ms.parse().ok()) {
            self.inner.mixed_airplay_ms.store(ms, Ordering::Relaxed);
        }
        let last: Option<String> = db
            .call(|conn| Ok(bopsie_core::db::setting(conn, LAST_CALIBRATION)?))
            .await?;
        let speeds: Option<String> = db
            .call(|conn| Ok(bopsie_core::db::setting(conn, SPEEDS)?))
            .await?;
        if let Some(speeds) = speeds.and_then(|speeds| serde_json::from_str(&speeds).ok()) {
            *self.inner.speeds.lock().unwrap() = speeds;
        }
        *self.inner.last_calibration.lock().unwrap() =
            last.and_then(|last| serde_json::from_str(&last).ok());
        let saved = store::load(&db).await?;
        let mut found: HashMap<String, Option<(Track, Playable)>> = HashMap::new();
        for queue in saved.queues.values() {
            for item in &queue.items {
                if !found.contains_key(&item.uri) {
                    let track = library
                        .playable(&item.uri)
                        .await
                        .ok()
                        .and_then(|tracks| tracks.into_iter().next());
                    found.insert(item.uri.clone(), track);
                }
            }
        }
        for (id, queue) in &saved.queues {
            let queue = queue.restore(|uri| found.get(uri).cloned().flatten());
            let existing = self.entry(id).ok();
            match existing {
                // A device already here gets its queue now, unless it already has one.
                Some(entry) => {
                    let current = entry.session.snapshot();
                    if current.items.is_empty() && current.state == PlaybackState::Idle {
                        entry.session.replace(queue);
                        self.changed(id);
                    }
                }
                None => {
                    self.inner
                        .restored
                        .lock()
                        .unwrap()
                        .insert(id.clone(), queue);
                }
            }
        }
        *self.inner.saved_groups.lock().unwrap() = saved.groups;
        *self.inner.saved_volumes.lock().unwrap() = saved.volumes;
        for device in self.inner.devices.all() {
            self.restore_device(&device.id);
        }
        let _ = self.inner.db.set(db);
        let saver = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SAVE_EVERY);
            loop {
                interval.tick().await;
                saver.save(false).await;
            }
        });
        Ok(())
    }

    /// Saves everything, as it is now, then stops every session: outputs finish, AirPlay
    /// speakers are flushed and Sendspin clients told the stream ended. Nothing is saved after,
    /// so the next start finds each queue where it was playing, not stopped.
    pub async fn shutdown(&self) {
        self.save(true).await;
        self.inner.closing.store(true, Ordering::SeqCst);
        let sessions: Vec<QueueSession> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|entry| entry.session.clone())
            .collect();
        let _ = tokio::task::spawn_blocking(move || {
            for session in sessions {
                if session.snapshot().state != PlaybackState::Idle {
                    session.stop();
                }
                session.shutdown();
            }
        })
        .await;
    }

    /// Writes what changed since the last save, or everything with `all`; the position of
    /// a playing queue at most every [`POSITION_EVERY`].
    pub async fn save(&self, all: bool) {
        let Some(db) = self.inner.db.get() else {
            return;
        };
        if self.inner.closing.load(Ordering::SeqCst) {
            return;
        }
        let entries: Vec<(String, Entry)> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect();
        let now = Instant::now();
        for (id, entry) in &entries {
            let queue = entry.session.snapshot();
            let saved = SavedQueue::of(&queue, entry.session.position(clock::now()));
            let unplaced = SavedQueue {
                position_ms: 0,
                ..saved.clone()
            };
            let due = {
                let last = self.inner.last_saved.lock().unwrap();
                match last.get(id) {
                    None => true,
                    Some((before, at)) => {
                        all || *before != unplaced
                            || (queue.state == PlaybackState::Playing
                                && now.duration_since(*at) >= POSITION_EVERY)
                    }
                }
            };
            if !due {
                continue;
            }
            match store::save_queue(db, id.clone(), saved).await {
                Ok(()) => {
                    self.inner
                        .last_saved
                        .lock()
                        .unwrap()
                        .insert(id.clone(), (unplaced, now));
                }
                Err(error) => tracing::warn!(queue = id, %error, "cannot save a queue"),
            }
        }

        // Groups formed now, and those from before still waiting for their devices.
        let mut groups: BTreeMap<String, String> = self.inner.saved_groups.lock().unwrap().clone();
        for (id, entry) in &entries {
            if let Some(leader) = &entry.synced_to {
                groups.insert(id.clone(), leader.clone());
            }
        }
        let groups_changed = all || *self.inner.last_groups.lock().unwrap() != Some(groups.clone());
        if groups_changed {
            match store::save_groups(db, groups.clone()).await {
                Ok(()) => *self.inner.last_groups.lock().unwrap() = Some(groups),
                Err(error) => tracing::warn!(%error, "cannot save groups"),
            }
        }

        let volumes: BTreeMap<String, (u8, bool)> = entries
            .iter()
            .filter_map(|(id, _)| {
                let (_, member) = self.active(id).ok()?;
                Some((
                    id.clone(),
                    (member.state.volume?, member.state.muted.unwrap_or(false)),
                ))
            })
            .filter(|(id, volume)| {
                all || self.inner.last_volumes.lock().unwrap().get(id) != Some(volume)
            })
            .collect();
        if !volumes.is_empty() {
            match store::save_volumes(db, volumes.clone()).await {
                Ok(()) => self.inner.last_volumes.lock().unwrap().extend(volumes),
                Err(error) => tracing::warn!(%error, "cannot save volumes"),
            }
        }
    }

    fn player_changed(&self, id: &str) {
        self.enforce_cap(id);
        self.carrier_turned_off(id);
        let available = self.device(id).is_ok_and(|d| d.available());
        let was = self
            .inner
            .available
            .lock()
            .unwrap()
            .insert(id.to_owned(), available);
        // A device that comes back gets a new sink in the group it plays in, and a leader
        // that went away for a moment plays on.
        if available && was == Some(false) {
            self.reconnect(id);
            self.restore_device(id);
            let paused = self.inner.absent.lock().unwrap().remove(id);
            if paused.is_some_and(|since| since.elapsed() < RESUME_WITHIN)
                && let Ok(entry) = self.entry(id)
                && entry.session.snapshot().state == PlaybackState::Paused
            {
                tracing::info!(queue = id, "the group's leader is back; playing on");
                if let Err(error) = self.begin(id, |q, _| ((), q.play())) {
                    tracing::warn!(queue = id, %error, "cannot play on");
                }
            }
        }
        // A leader that goes leaves its group with nothing to follow: it pauses where it was.
        if !available
            && was == Some(true)
            && let Ok(entry) = self.entry(id)
            && entry.synced_to.is_none()
            && entry.session.snapshot().state == PlaybackState::Playing
        {
            tracing::warn!(queue = id, "the group's leader is gone; pausing");
            entry.session.pause();
            self.inner
                .absent
                .lock()
                .unwrap()
                .insert(id.to_owned(), Instant::now());
        }
        self.reconcile_native(id);
        self.publish(EventType::PlayerUpdated, id, self.player(id, None));
        self.publish(EventType::QueueUpdated, id, self.queue(id));
        if let Some(leader) = self.leader_of(id) {
            self.send_controls(&leader);
        }
    }

    /// A device whose carrier in a playing group was turned off in its settings: its output
    /// leaves the stream, and the group carries it another way if it has one. Its output
    /// still works, so nothing else would take it out.
    fn carrier_turned_off(&self, id: &str) {
        let Some(leader) = self.leader_of(id) else {
            return;
        };
        let Ok(leading) = self.entry(&leader) else {
            return;
        };
        let Some(carrier) = leading.plan.as_ref().and_then(|p| p.carrier(id)).cloned() else {
            return;
        };
        let Ok(device) = self.device(id) else {
            return;
        };
        if carrier.native || !device.member(&carrier.endpoint).is_some_and(|m| m.disabled) {
            return;
        }
        tracing::info!(
            device = id,
            endpoint = carrier.endpoint,
            "its endpoint was turned off; it leaves the stream"
        );
        if leader == id {
            // The whole group streams anew, over what the leader has left; a leader with
            // nothing left pauses, as one that went away does.
            if device.available() && leading.session.snapshot().state == PlaybackState::Playing {
                let position = leading.session.position(clock::now());
                if let Err(error) = self.begin(&leader, resume_at(position)) {
                    tracing::warn!(queue = leader, %error, "cannot play on");
                }
            }
            return;
        }
        if let Some(entry) = self.inner.entries.lock().unwrap().get_mut(&leader)
            && let Some(plan) = &mut entry.plan
        {
            plan.carriers.retain(|c| c.device != id);
        }
        leading.session.remove_sink(id);
        if device.available() {
            self.reconnect(id);
        }
    }

    fn reconnect(&self, id: &str) {
        if let Some(leader) = self.leader_of(id)
            && let Err(error) = self.carry(&leader, id)
        {
            tracing::warn!(device = id, %error, "cannot put a device back in its group");
        }
    }

    /// Puts `device`, the leader or a member of `leader`'s group, into the group's running
    /// engine: through its carrier, when the plan carries it; else the group is planned again,
    /// and if it must change how it plays to carry the device, it plays on from where it was.
    fn carry(&self, leader: &str, device: &str) -> Result<()> {
        let leading = self.entry(leader)?;
        let (Some(running), Some((_, Some(timeline)))) =
            (leading.plan.clone(), leading.session.engine())
        else {
            // Not playing: the next start plans the group with it.
            return Ok(());
        };
        let plan = if running.carrier(device).is_some() {
            running.clone()
        } else {
            self.plan_for(leader, &leading.members, Some(running.strategy))?
        };
        // The stream plays on when every device it carries is carried as it was, though the
        // group may now mix protocols to carry this one; unless that moves its AirPlay
        // speakers by their offset beside other protocols.
        let mixing = (running.strategy == Strategy::Mixed) != (plan.strategy == Strategy::Mixed);
        let moves_airplay = mixing
            && self.mixed_airplay_ms() != 0
            && running.streamed().any(|carrier| {
                self.device(&carrier.device).is_ok_and(|device| {
                    device
                        .member(&carrier.endpoint)
                        .is_some_and(|member| member.endpoint.state().provider == AIRPLAY)
                })
            });
        let same_stream = running.sample_rate == plan.sample_rate
            && running.channels == plan.channels
            && !moves_airplay
            && running
                .carriers
                .iter()
                .all(|carrier| plan.carrier(&carrier.device) == Some(carrier));
        if same_stream {
            if let Some(carrier) = plan.carrier(device).filter(|c| !c.native)
                && let Some(member) = self.device(device)?.member(&carrier.endpoint)
            {
                self.enforce_cap(device);
                let timeline = self.carrier_timeline(&plan, &member.endpoint, timeline);
                leading.session.add_sink(
                    device.to_owned(),
                    member.endpoint.output(carrier.format, timeline),
                );
            }
            self.set_plan(leader, Some(plan));
        } else if leading.session.snapshot().state == PlaybackState::Playing {
            tracing::info!(
                queue = leader,
                from = ?running.strategy,
                to = ?plan.strategy,
                "the group changes how it plays to carry a member"
            );
            let now = Instant::now();
            let mut left = self.inner.left.lock().unwrap();
            for carrier in &running.carriers {
                if plan.carrier(&carrier.device).map(|c| &c.endpoint) != Some(&carrier.endpoint) {
                    left.insert(carrier.endpoint.clone(), now);
                }
            }
            drop(left);
            let position = leading.session.position(clock::now());
            self.begin(leader, resume_at(position))?;
        }
        Ok(())
    }

    /// An output of `queue_id`'s group failed and left its engine: it is put back after a
    /// wait that grows while it keeps failing, unless by then it no longer plays in the group,
    /// the group stopped or its engine was replaced.
    fn output_failed(&self, queue_id: &str, device: &str) {
        if !self
            .inner
            .rejoining
            .lock()
            .unwrap()
            .insert(device.to_owned())
        {
            return;
        }
        let wait = self
            .inner
            .backoffs
            .lock()
            .unwrap()
            .entry(device.to_owned())
            .or_insert_with(|| Backoff::new(backoff::Policy::RECONNECT))
            .failed(Instant::now());
        tracing::info!(
            queue = queue_id,
            device,
            ?wait,
            "an output failed; it rejoins after a wait"
        );
        let engine = self
            .entry(queue_id)
            .ok()
            .and_then(|entry| entry.session.engine());
        let (playback, queue_id, device) = (self.clone(), queue_id.to_owned(), device.to_owned());
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            playback.inner.rejoining.lock().unwrap().remove(&device);
            let Ok(entry) = playback.entry(&queue_id) else {
                return;
            };
            let wanted = playback.leader_of(&device).as_deref() == Some(queue_id.as_str())
                && entry.session.snapshot().state == PlaybackState::Playing
                && entry.session.engine() == engine
                && playback.device(&device).is_ok_and(|d| d.available());
            if !wanted {
                // A device that comes back is put back by player_changed; a new engine
                // already has it.
                return;
            }
            playback.reconnect(&device);
            if let Some(backoff) = playback.inner.backoffs.lock().unwrap().get_mut(&device) {
                backoff.working(Instant::now());
            }
        });
    }

    /// `queue_id`'s engine crashed: the group plays on from where it was, unless it keeps
    /// crashing.
    fn crashed(&self, queue_id: &str, error: &str) {
        let now = Instant::now();
        let recent = {
            let mut crashes = self.inner.crashes.lock().unwrap();
            let times = crashes.entry(queue_id.to_owned()).or_default();
            times.retain(|at| now.duration_since(*at) < CRASH_WINDOW);
            times.push(now);
            times.len()
        };
        let Ok(entry) = self.entry(queue_id) else {
            return;
        };
        if recent > MAX_CRASHES {
            tracing::error!(
                queue = queue_id,
                error,
                "the engine keeps crashing; stopping"
            );
            entry.session.stop();
            return;
        }
        if entry.session.snapshot().state != PlaybackState::Playing {
            return;
        }
        tracing::warn!(queue = queue_id, error, "the engine crashed; playing on");
        let position = entry.session.position(clock::now());
        if let Err(error) = self.begin(queue_id, resume_at(position)) {
            tracing::warn!(queue = queue_id, %error, "cannot play on after a crash");
        }
    }

    /// A device that turned out to be part of another: its queue and group go.
    fn remove_player(&self, id: &str) {
        let _ = self.ungroup(id);
        let removed = self.inner.entries.lock().unwrap().remove(id);
        if let Some(entry) = removed {
            entry.session.stop();
            entry.session.shutdown();
        }
        self.inner.available.lock().unwrap().remove(id);
        self.publish(EventType::PlayerRemoved, id, Some(json!(id)));
    }

    fn publish(&self, event: EventType, id: &str, data: Option<Value>) {
        if let Some(data) = data {
            self.inner.events.publish(Event {
                event,
                object_id: Some(id.to_owned()),
                data,
            });
        }
    }

    /// The leader whose queue `id` plays: the one it follows, or itself.
    fn leader_of(&self, id: &str) -> Option<String> {
        let entries = self.inner.entries.lock().unwrap();
        let entry = entries.get(id)?;
        Some(entry.synced_to.clone().unwrap_or_else(|| id.to_owned()))
    }

    /// The leader and its members.
    fn group_of(&self, leader: &str) -> Vec<String> {
        let members = self
            .entry(leader)
            .map(|entry| entry.members)
            .unwrap_or_default();
        std::iter::once(leader.to_owned()).chain(members).collect()
    }

    /// Announces everything about a queue and the players in its group.
    fn changed(&self, queue_id: &str) {
        self.publish(EventType::QueueUpdated, queue_id, self.queue(queue_id));
        self.publish(EventType::QueueItemsUpdated, queue_id, self.queue(queue_id));
        for id in self.group_of(queue_id) {
            self.publish(EventType::PlayerUpdated, &id, self.player(&id, None));
        }
        self.send_controls(queue_id);
    }

    /// Tells every endpoint of the group playing `queue_id` the group's volume, mute, repeat
    /// and shuffle, for those that control the group.
    fn send_controls(&self, queue_id: &str) {
        let Ok(entry) = self.entry(queue_id) else {
            return;
        };
        let queue = entry.session.snapshot();
        let (devices, active): (Vec<Device>, Vec<Member>) = self
            .group_of(queue_id)
            .iter()
            .filter_map(|id| self.active(id).ok())
            .unzip();
        let controls = Controls {
            volume: active
                .iter()
                .filter_map(|m| m.state.volume)
                .max()
                .unwrap_or(0),
            muted: !active.is_empty() && active.iter().all(|m| m.state.muted == Some(true)),
            repeat: match queue.repeat_mode {
                RepeatMode::Off => Repeat::Off,
                RepeatMode::One => Repeat::One,
                RepeatMode::All => Repeat::All,
            },
            shuffle: queue.shuffle_enabled,
            seek_max_ms: queue
                .current()
                .map(|item| u64::from(item.track.duration) * 1_000)
                .filter(|ms| *ms > 0),
        };
        for device in &devices {
            for member in &device.members {
                member.endpoint.set_controls(&controls);
            }
        }
        if let Some(source) = self.live_of(queue_id) {
            source.control(Control::Volume(controls.volume));
        }
    }

    fn on_notice(&self, queue_id: &str, notice: Notice) {
        match notice {
            Notice::Started(_) | Notice::Seeked => self.announce(queue_id),
            Notice::Finished | Notice::Stopped => {
                for id in self.group_of(queue_id) {
                    if let Ok((_, member)) = self.active(&id) {
                        member.endpoint.set_group(false, queue_id);
                        member.endpoint.set_now_playing(None);
                    }
                }
            }
            Notice::TakenBack(source) => {
                if let Some(source) = self.source(&source) {
                    tracing::info!(
                        queue = queue_id,
                        source = source.id(),
                        "the queue plays again"
                    );
                    source.control(Control::Release);
                }
            }
            Notice::OutputFailed(device) => self.output_failed(queue_id, &device),
            Notice::Crashed(error) => self.crashed(queue_id, &error),
            Notice::Failed {
                queue_item_id,
                error,
            } => {
                tracing::warn!(
                    queue = queue_id,
                    item = queue_item_id,
                    error,
                    "skipped a track"
                );
            }
        }
        self.changed(queue_id);
    }

    /// Tells the group's devices what is playing.
    fn announce(&self, queue_id: &str) {
        let Ok(entry) = self.entry(queue_id) else {
            return;
        };
        let queue = entry.session.snapshot();
        let Some(item) = queue.current() else {
            return;
        };
        let now_playing = now_playing(
            &item.track,
            entry.session.position(clock::now()),
            queue.state == PlaybackState::Playing,
            self.inner.config.base_url.as_deref(),
        );
        for id in self.group_of(queue_id) {
            if let Ok((_, member)) = self.active(&id) {
                member
                    .endpoint
                    .set_group(queue.state == PlaybackState::Playing, queue_id);
                member.endpoint.set_now_playing(Some(&now_playing));
            }
        }
    }

    /// Reports listening progress in a catalog's items, such as an Audiobookshelf book, to
    /// `library`, which passes it to the catalog.
    pub fn report_progress_to(&self, library: Library) {
        let _ = self.inner.progress_to.set(library);
    }

    /// Tells the catalog of each queue's current audiobook how far into it listening is, and
    /// of one the queue left, that it was left; from what changed since the last report.
    fn report_progress(&self) {
        let Some(library) = self.inner.progress_to.get() else {
            return;
        };
        let now = clock::now();
        let current: Vec<(String, Option<Reported>)> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            // Starting, it is not yet where it will be heard from: nothing to say until it is,
            // lest a book be reported back at its beginning.
            .filter(|(_, entry)| !entry.session.buffering())
            .map(|(id, entry)| {
                let queue = entry.session.snapshot();
                let book = queue.current().filter(|item| {
                    item.track.fields.media_type == bopsie_core::media::MediaType::Audiobook
                });
                let reported = book.map(|item| Reported {
                    uri: item.track.fields.uri.clone(),
                    rate: f64::from(item.rate),
                    playing: queue.state == PlaybackState::Playing,
                    ended: queue.ended,
                    position: entry.session.position(now),
                    duration: Duration::from_secs(u64::from(item.track.duration)),
                    at: Instant::now(),
                });
                (id.clone(), reported)
            })
            .collect();
        let mut reported = self.inner.reported.lock().unwrap();
        for (queue_id, now) in current {
            let before = reported.remove(&queue_id);
            for (uri, progress) in progress_between(before.as_ref(), now.as_ref()) {
                library.report_progress(&uri, progress);
            }
            if let Some(now) = now {
                reported.insert(queue_id, now);
            }
        }
    }

    /// Counts a play of each queue's current item once it has played long enough, with its
    /// album and artists, in the library's play history.
    fn count_plays(&self) {
        let Some(library) = self.inner.progress_to.get() else {
            return;
        };
        let now = clock::now();
        let heard: Vec<(String, String, Vec<String>)> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| !entry.session.buffering())
            .filter_map(|(id, entry)| {
                let queue = entry.session.snapshot();
                if queue.state != PlaybackState::Playing {
                    return None;
                }
                let item = queue.current()?;
                let enough =
                    PLAY_COUNTS_AFTER.min(Duration::from_secs(u64::from(item.track.duration)) / 2);
                if entry.session.position(now) < enough {
                    return None;
                }
                let track = &item.track;
                let uris = std::iter::once(track.fields.uri.clone())
                    .chain(track.album.iter().map(|album| album.uri.clone()))
                    .chain(track.artists.iter().map(|artist| artist.uri.clone()))
                    .collect();
                Some((id.clone(), item.queue_item_id.clone(), uris))
            })
            .collect();
        let mut counted = self.inner.counted.lock().unwrap();
        for (queue_id, item_id, uris) in heard {
            if counted.get(&queue_id) == Some(&item_id) {
                continue;
            }
            library.played(uris.iter().map(String::as_str));
            counted.insert(queue_id, item_id);
        }
    }

    /// Pauses `queue_id`'s group `after` from now, or at the end of what plays: the chapter of
    /// a book, else the item. `None` for both cancels it.
    pub fn sleep(
        &self,
        queue_id: &str,
        after: Option<Duration>,
        end_of_chapter: bool,
    ) -> Result<()> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        let session = self.entry(&leader)?.session;
        let until = if let Some(after) = after {
            Some(SleepUntil::At(clock::now() + after.as_micros() as Micros))
        } else if end_of_chapter {
            let queue = session.snapshot();
            let item = queue.current().ok_or_else(|| {
                bopsie_core::Error::new(bopsie_core::ErrorCode::QueueEmpty, "Nothing plays")
            })?;
            let ms = session.position(clock::now()).as_millis() as u64;
            let chapter = item
                .track
                .fields
                .metadata
                .chapters
                .as_deref()
                .and_then(|chapters| bopsie_core::media::chapter_at(chapters, ms));
            Some(SleepUntil::EndOf {
                item: item.queue_item_id.clone(),
                chapter,
            })
        } else {
            None
        };
        let mut sleeps = self.inner.sleeps.lock().unwrap();
        match until {
            Some(until) => {
                sleeps.insert(leader.clone(), Sleep { until });
            }
            None => {
                sleeps.remove(&leader);
            }
        }
        drop(sleeps);
        self.changed(&leader);
        Ok(())
    }

    /// When `queue_id`'s group pauses, by its sleep timer: in Unix seconds, or at the end of
    /// what plays.
    pub fn sleep_at(&self, queue_id: &str) -> Option<SleepAt> {
        let sleeps = self.inner.sleeps.lock().unwrap();
        Some(match &sleeps.get(queue_id)?.until {
            SleepUntil::At(at) => {
                let seconds = (*at - clock::now()) as f64 / 1_000_000.0;
                SleepAt::Unix(model::unix_now() + seconds.max(0.0))
            }
            SleepUntil::EndOf { .. } => SleepAt::EndOfChapter,
        })
    }

    /// Pauses the groups whose sleep timer is due.
    fn sleep_due(&self) {
        let due: Vec<(String, Option<Duration>)> = {
            let sleeps = self.inner.sleeps.lock().unwrap();
            sleeps
                .iter()
                .filter_map(|(queue_id, sleep)| {
                    let session = self.entry(queue_id).ok()?.session;
                    let queue = session.snapshot();
                    if queue.state != PlaybackState::Playing {
                        return None;
                    }
                    match &sleep.until {
                        SleepUntil::At(at) => {
                            (clock::now() >= *at).then(|| (queue_id.clone(), None))
                        }
                        SleepUntil::EndOf { item, chapter } => {
                            let current = queue.current()?;
                            if current.queue_item_id != *item {
                                // On to the next item: it pauses at its start.
                                return Some((queue_id.clone(), Some(Duration::ZERO)));
                            }
                            let chapters = current.track.fields.metadata.chapters.as_deref()?;
                            let ms = session.position(clock::now()).as_millis() as u64;
                            let now = bopsie_core::media::chapter_at(chapters, ms);
                            (now != *chapter).then(|| {
                                let start = now
                                    .and_then(|index| chapters.get(index))
                                    .map(|next| Duration::from_millis(next.start_ms));
                                (queue_id.clone(), start)
                            })
                        }
                    }
                })
                .collect()
        };
        for (queue_id, start) in due {
            self.inner.sleeps.lock().unwrap().remove(&queue_id);
            tracing::info!(queue = queue_id, "the sleep timer pauses");
            if let Err(error) = self.command(&queue_id, QueueCommand::Pause) {
                tracing::warn!(queue = queue_id, %error, "the sleep timer cannot pause");
                continue;
            }
            // At the start of what comes next, where it was heard into for a moment.
            if let Some(start) = start {
                let _ = self.command(&queue_id, QueueCommand::Seek(start.as_secs_f64()));
            }
        }
    }

    fn tick(&self) {
        self.report_progress();
        self.count_plays();
        self.sleep_due();
        let playing: Vec<(String, f64)> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| entry.synced_to.is_none())
            .filter(|(_, entry)| entry.session.snapshot().state == PlaybackState::Playing)
            // Its time stands still until it is heard, as its last update says.
            .filter(|(_, entry)| !entry.session.buffering())
            .map(|(id, entry)| {
                (
                    id.clone(),
                    entry.session.position(clock::now()).as_secs_f64(),
                )
            })
            .collect();
        for (id, elapsed) in playing {
            self.publish(EventType::QueueTimeUpdated, &id, Some(json!(elapsed)));
        }
    }

    /// How `leader` and `members` would play together, preferring `current`'s strategy.
    fn plan_for(
        &self,
        leader: &str,
        members: &[String],
        current: Option<Strategy>,
    ) -> Result<Plan> {
        self.plan_among(leader, members, current, &self.inner.devices.all())
    }

    /// As [`Playback::plan_for`], among `devices`.
    fn plan_among(
        &self,
        leader: &str,
        members: &[String],
        current: Option<Strategy>,
        devices: &[Device],
    ) -> Result<Plan> {
        let leading = find(devices, leader).ok_or_else(|| not_found(leader))?;
        let members: Vec<Device> = members
            .iter()
            .filter_map(|id| find(devices, id).cloned())
            .collect();
        planner::plan(leading, &members, &self.inner.config.policy, current).ok_or_else(unavailable)
    }

    /// Plans the leader's group afresh and starts an engine for it, unless one already plays
    /// that plan.
    fn ensure_engine(&self, leader: &str) -> Result<()> {
        let entry = self.entry(leader)?;
        let plan = self.plan_for(leader, &entry.members, None)?;
        // A source's live input is read only a little ahead, a queue's well ahead.
        let max_ahead = if entry.session.live_source().is_some() {
            LIVE_AHEAD
        } else {
            QUEUE_AHEAD
        };
        if entry.session.engine().is_some()
            && entry.plan.as_ref() == Some(&plan)
            && entry.session.max_ahead() == Some(max_ahead)
        {
            return Ok(());
        }
        for dropped in &plan.dropped {
            tracing::info!(
                queue = leader,
                device = dropped.device,
                reason = %dropped.reason,
                "the group plays without a member"
            );
        }
        let format = PcmFormat {
            sample_rate: plan.sample_rate,
            channels: plan.channels,
        };
        // Each speaker is at most at its maximum before it hears anything.
        for carrier in &plan.carriers {
            self.enforce_cap(&carrier.device);
        }
        let timeline = Timeline::new(format.sample_rate, clock::now());
        let sinks = plan
            .streamed()
            .filter_map(|carrier| {
                let device = self.inner.devices.get(&carrier.device)?;
                let member = device.member(&carrier.endpoint)?;
                let timeline = self.carrier_timeline(&plan, &member.endpoint, timeline);
                Some((
                    carrier.device.clone(),
                    member.endpoint.output(carrier.format, timeline),
                ))
            })
            .collect();
        let settings = Settings {
            format,
            ffmpeg: self.inner.config.ffmpeg.clone(),
            ring_frames: format.sample_rate as usize * RING_SECONDS,
            max_ahead,
            realtime: Some(Realtime {
                timeline,
                lead: plan.lead,
                clock: clock::now,
            }),
        };
        entry.session.normalise_to(self.inner.config.normalise);
        entry.session.attach(settings, sinks);
        self.set_plan(leader, Some(plan));
        Ok(())
    }

    fn set_plan(&self, leader: &str, plan: Option<Plan>) {
        if let Some(entry) = self.inner.entries.lock().unwrap().get_mut(leader) {
            entry.plan = plan;
        }
    }

    /// Runs `f` on the queue `queue_id` plays from, then announces the change.
    fn update<T>(
        &self,
        queue_id: &str,
        f: impl FnOnce(&mut Queue, &mut StdRng) -> (T, Effect),
    ) -> Result<T> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        self.inner.absent.lock().unwrap().remove(&leader);
        let session = self.entry(&leader)?.session;
        let value = session.update(|queue| f(queue, &mut self.inner.rng.lock().unwrap()));
        self.changed(&leader);
        Ok(value)
    }

    /// Runs `f`, which may start playback, on the queue `queue_id` plays from. The queue
    /// changes at once; the group's devices get ready and playback starts in the background,
    /// as with Music Assistant, where a play request returns before the music does. Anything
    /// done to the queue in the meantime, such as a stop, supersedes the start.
    fn begin(
        &self,
        queue_id: &str,
        f: impl FnOnce(&mut Queue, &mut StdRng) -> ((), Effect),
    ) -> Result<()> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        if !self.device(&leader)?.available() {
            return Err(unavailable());
        }
        if self.inner.calibrating.lock().unwrap().contains(&leader) {
            return Err(calibrating());
        }
        let session = self.entry(&leader)?.session;
        let ((), held) = session.update_held(|queue| f(queue, &mut self.inner.rng.lock().unwrap()));
        self.changed(&leader);
        let Some(epoch) = held else {
            return Ok(());
        };
        let playback = self.clone();
        tokio::spawn(async move {
            let not_before = playback.prepare(&leader).await;
            match playback.ensure_engine(&leader) {
                Ok(()) => {
                    session.release(epoch, not_before);
                    playback.changed(&leader);
                }
                Err(error) => {
                    tracing::warn!(queue = leader, %error, "cannot start playback");
                    session.abandon(epoch);
                    playback.changed(&leader);
                }
            }
        });
        Ok(())
    }

    /// Gets every endpoint the group would play through ready, together, and returns when the
    /// slowest can start, if any needs longer than its lead.
    async fn prepare(&self, leader: &str) -> Option<Micros> {
        let members = self.entry(leader).ok()?.members;
        let plan = self.plan_for(leader, &members, None).ok()?;
        let preparing: Vec<_> = plan
            .streamed()
            .filter_map(|carrier| {
                let device = self.inner.devices.get(&carrier.device)?;
                let member = device.member(&carrier.endpoint)?;
                // A receiver that has to be launched, on a device with its own multiroom, as a
                // Cast receiver on a WiiM: the device is woken through that first, which the
                // WiiM needs to sound the receiver, and which ends a receiver running on it,
                // so that it is launched afresh.
                let wake = member
                    .state
                    .capabilities
                    .launched
                    .then(|| {
                        device.members.iter().find(|m| {
                            m.id != member.id
                                && m.state.available
                                && m.state.capabilities.native_group.is_some()
                        })
                    })
                    .flatten()
                    .map(|native| native.endpoint.clone());
                if let Some(native) = &wake {
                    self.inner
                        .left
                        .lock()
                        .unwrap()
                        .insert(native.id().to_owned(), Instant::now());
                }
                Some((wake, member.endpoint.clone()))
            })
            .map(|(wake, endpoint)| {
                tokio::task::spawn_blocking(move || {
                    if let Some(native) = wake {
                        native.wake();
                    }
                    endpoint.prepare()
                })
            })
            .collect();
        let mut latest = None;
        for prepared in preparing {
            if let Ok(Some(at)) = prepared.await {
                latest = latest.max(Some(at));
            }
        }
        latest
    }

    pub fn play_media(
        &self,
        queue_id: &str,
        tracks: Vec<(Track, Playable)>,
        option: QueueOption,
        start_at: Option<usize>,
    ) -> Result<()> {
        let speeds = self.inner.speeds.lock().unwrap().clone();
        let enqueue = move |queue: &mut Queue, rng: &mut StdRng| {
            let effect = queue.enqueue(tracks, option, rng);
            // A book plays as fast as it last did.
            for item in &mut queue.items {
                if let Some(&speed) = speeds.get(&item.track.fields.uri) {
                    item.rate = speed;
                }
            }
            match (effect, start_at) {
                (Effect::PlayFrom(first), Some(offset)) if offset > 0 => {
                    ((), queue.play_index(first + offset))
                }
                _ => ((), effect),
            }
        };
        if matches!(option, QueueOption::Play | QueueOption::Replace) {
            if let Some(leader) = self.leader_of(queue_id) {
                self.inner.sleeps.lock().unwrap().remove(&leader);
            }
            self.begin(queue_id, enqueue)
        } else {
            self.update(queue_id, enqueue)
        }
    }

    /// Plays audiobook `queue_item_id` of `queue_id`'s queue, or its current item, at `speed`,
    /// keeping its pitch, and plays the book so from now on. A playing item carries on from
    /// where it is at once.
    pub async fn set_playback_speed(
        &self,
        queue_id: &str,
        speed: f32,
        queue_item_id: Option<&str>,
    ) -> Result<()> {
        if !(MIN_RATE..=MAX_RATE).contains(&speed) {
            return Err(bopsie_core::Error::invalid_data(format!(
                "Playback speed must be between {MIN_RATE} and {MAX_RATE}, got {speed}"
            )));
        }
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        let session = self.entry(&leader)?.session;
        let (uri, current, playing) = {
            let mut found = None;
            session.update(|queue| {
                let index = match queue_item_id {
                    Some(id) => queue.index_of(id),
                    None => queue.current_index,
                };
                if let Some(index) = index
                    && let Some(item) = queue.items.get_mut(index)
                    && item.track.fields.media_type == bopsie_core::media::MediaType::Audiobook
                {
                    let changed = (item.rate - speed).abs() > 0.001;
                    item.rate = speed;
                    found = Some((
                        item.track.fields.uri.clone(),
                        Some(index) == queue.current_index && changed,
                        queue.state == PlaybackState::Playing,
                    ));
                }
                ((), Effect::None)
            });
            found.ok_or_else(|| {
                bopsie_core::Error::new(
                    bopsie_core::ErrorCode::InvalidCommand,
                    "Playback speed is only for audiobooks",
                )
            })?
        };
        let speeds = {
            let mut speeds = self.inner.speeds.lock().unwrap();
            speeds.insert(uri, speed);
            serde_json::to_string(&*speeds).unwrap_or_default()
        };
        if let Some(db) = self.inner.db.get() {
            db.call(move |conn| Ok(bopsie_core::db::set_setting(conn, SPEEDS, Some(&speeds))?))
                .await?;
        }
        if current && playing {
            let position = session.position(clock::now());
            self.begin(&leader, resume_at(position))?;
        }
        self.changed(&leader);
        Ok(())
    }

    pub fn command(&self, queue_id: &str, command: QueueCommand) -> Result<()> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        // Whatever the user asks for replaces playing on when the leader comes back.
        self.inner.absent.lock().unwrap().remove(&leader);
        // Playing again, or stopping, cancels the sleep timer.
        if matches!(
            command,
            QueueCommand::Play | QueueCommand::PlayPause | QueueCommand::Stop
        ) {
            self.inner.sleeps.lock().unwrap().remove(&leader);
        }
        let session = self.entry(&leader)?.session;
        // While the group plays a source, its transport controls the source.
        if let Some(source) = self.live_of(&leader)
            && let Some(control) = source_control(&command, &source.state())
        {
            source.control(control);
            self.changed(&leader);
            return Ok(());
        }
        match command {
            QueueCommand::Play => self.begin(&leader, |q, _| ((), q.play()))?,
            QueueCommand::Pause => session.pause(),
            QueueCommand::PlayPause => {
                if session.snapshot().state == PlaybackState::Playing {
                    session.pause();
                } else {
                    self.begin(&leader, |q, _| ((), q.play()))?;
                }
            }
            QueueCommand::Stop => session.stop(),
            QueueCommand::Next | QueueCommand::Previous => {
                let next = command == QueueCommand::Next;
                let at = session.position(clock::now());
                // Within a book, by chapter.
                let chapter = session
                    .snapshot()
                    .current()
                    .and_then(|item| item.track.fields.metadata.chapters.clone())
                    .and_then(|chapters| {
                        bopsie_core::media::chapter_skip(&chapters, at.as_millis() as u64, next)
                    });
                match chapter {
                    Some(start) => session.seek(Duration::from_millis(start)),
                    None if next => self.begin(&leader, |q, _| ((), q.next_track()))?,
                    None => {
                        let elapsed = at.as_secs_f64();
                        self.begin(&leader, move |q, _| ((), q.previous_track(elapsed)))?;
                    }
                }
            }
            QueueCommand::Seek(seconds) => {
                session.seek(Duration::from_secs_f64(seconds.max(0.0)));
            }
            QueueCommand::Skip(seconds) => {
                let at = session.position(clock::now()).as_secs_f64() + seconds;
                session.seek(Duration::from_secs_f64(at.max(0.0)));
            }
            QueueCommand::Shuffle(enabled) => {
                self.update(&leader, |q, rng| ((), q.set_shuffle(enabled, rng)))?;
            }
            QueueCommand::Repeat(mode) => {
                self.update(&leader, |q, _| ((), q.set_repeat(mode)))?;
            }
            QueueCommand::Clear => self.update(&leader, |q, _| ((), q.clear()))?,
        }
        self.changed(&leader);
        Ok(())
    }

    /// Plays the item at `index`, or with the id `index` names.
    pub fn play_index(&self, queue_id: &str, item: &str) -> Result<()> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        let queue = self.entry(&leader)?.session.snapshot();
        let index = item
            .parse::<usize>()
            .ok()
            .filter(|i| *i < queue.items.len())
            .or_else(|| queue.index_of(item))
            .ok_or_else(|| queue_error(QueueError::NotFound(item.to_owned())))?;
        self.begin(&leader, move |q, _| ((), q.play_index(index)))
    }

    pub fn move_item(&self, queue_id: &str, queue_item_id: &str, shift: i64) -> Result<()> {
        self.update(queue_id, |q, _| {
            (q.move_item(queue_item_id, shift), Effect::None)
        })?
        .map_err(queue_error)
    }

    pub fn delete_item(&self, queue_id: &str, item: &str) -> Result<()> {
        self.update(queue_id, |q, _| {
            let id = item
                .parse::<usize>()
                .ok()
                .and_then(|i| q.items.get(i))
                .map_or_else(|| item.to_owned(), |i| i.queue_item_id.clone());
            (q.delete_item(&id), Effect::None)
        })?
        .map_err(queue_error)
    }

    /// Moves a queue, its position and its settings to another device, which plays on if the
    /// source was playing. The source is left empty.
    pub fn transfer(&self, source: &str, target: &str) -> Result<()> {
        if self.entry(target)?.synced_to.is_some() {
            self.ungroup(target)?;
        }
        let from = self.entry(source)?.session;
        let moved = from.snapshot();
        let position = from.position(clock::now());
        let was_playing = moved.state == PlaybackState::Playing;
        self.update(source, |q, _| ((), q.clear()))?;
        let take_over = move |q: &mut Queue, _: &mut StdRng| {
            *q = moved;
            q.state = PlaybackState::Idle;
            q.index_in_buffer = None;
            q.resume_position = position;
            let effect = if was_playing { q.play() } else { Effect::None };
            ((), effect)
        };
        if was_playing {
            self.begin(target, take_over)
        } else {
            self.update(target, take_over)
        }
    }

    /// Adds `player` to the group `target` plays in, if the group's plan can carry it; else
    /// fails with the planner's reason. The player stops what it was playing. A join the
    /// group's protocol or format cannot take on changes them, and the group plays on from
    /// where it was.
    pub fn group(&self, player: &str, target: &str) -> Result<()> {
        let leader = self.leader_of(target).ok_or_else(|| not_found(target))?;
        if leader == player {
            return Ok(());
        }
        self.device(player)?;
        let leading = self.entry(&leader)?;
        let mut members = leading.members.clone();
        members.retain(|m| m != player);
        members.push(player.to_owned());
        let current = leading.plan.as_ref().map(|p| p.strategy);
        let plan = self.plan_for(&leader, &members, current)?;
        if let Some(dropped) = plan.dropped.iter().find(|d| d.device == player) {
            // One that follows another natively may have the protocol it lacks hidden until it
            // leaves that group: it leaves, and joins once the protocol is back.
            if let Some(protocol) = self.withdrawn_for(player, &leader) {
                self.rejoin_over(player, &leader, protocol);
                return Ok(());
            }
            let leader_name = self.device(&leader)?.name;
            return Err(Error::new(
                ErrorCode::UnsupportedFeature,
                dropped.message(&leader_name),
            ));
        }

        if self.entry(player)?.synced_to.is_some() {
            self.ungroup(player)?;
        }
        let native = plan
            .carrier(player)
            .filter(|c| c.native)
            .zip(plan.carriers.first());
        if native.is_some() {
            // Before it is linked: what the device reports from then on, until it has joined,
            // still says it follows no one.
            self.settling(player);
        }
        self.link(player, &leader);
        let playing = self.entry(&leader)?.session.snapshot().state == PlaybackState::Playing;
        if let Some((carrier, head)) = native {
            self.join_natively(player, &carrier.endpoint, &head.endpoint, &leader);
        }
        self.carry(&leader, player)?;
        if playing {
            self.announce(&leader);
        }
        self.changed(&leader);
        Ok(())
    }

    /// Takes `player` out of its group. A leader hands its queue, and its followers, to the
    /// first of them. The group keeps how it plays until it next starts.
    pub fn ungroup(&self, player: &str) -> Result<()> {
        let entry = self.entry(player)?;
        if let Some(leader) = entry.synced_to {
            let endpoint = self.active(player).ok().map(|(_, member)| member.endpoint);
            if let Some(native) = self.native_follower(player) {
                self.leave_natively(player, native);
            }
            self.unlink(player, &leader);
            if let Some(endpoint) = endpoint {
                endpoint.set_group(false, player);
                endpoint.set_now_playing(None);
            }
            return Ok(());
        }
        let Some((heir, rest)) = entry.members.split_first() else {
            return Ok(());
        };
        let (heir, rest) = (heir.clone(), rest.to_vec());
        for member in std::iter::once(&heir).chain(&rest) {
            self.ungroup(member)?;
        }
        self.transfer(player, &heir)?;
        for member in &rest {
            self.group(member, &heir)?;
        }
        Ok(())
    }

    /// Records `player` as following `leader`; it stops what it played itself.
    fn link(&self, player: &str, leader: &str) {
        if let Ok(own) = self.entry(player)
            && own.session.snapshot().state != PlaybackState::Idle
        {
            own.session.stop();
        }
        let mut entries = self.inner.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(player) {
            entry.synced_to = Some(leader.to_owned());
        }
        if let Some(entry) = entries.get_mut(leader)
            && !entry.members.iter().any(|m| m == player)
        {
            entry.members.push(player.to_owned());
        }
    }

    /// Records that `player` no longer follows `leader`, and takes its output out of the
    /// group's engine.
    fn unlink(&self, player: &str, leader: &str) {
        {
            let mut entries = self.inner.entries.lock().unwrap();
            if let Some(leading) = entries.get_mut(leader) {
                leading.members.retain(|m| m != player);
                if let Some(plan) = &mut leading.plan {
                    plan.carriers.retain(|c| c.device != player);
                }
            }
            if let Some(own) = entries.get_mut(player) {
                own.synced_to = None;
            }
        }
        if let Ok(leading) = self.entry(leader) {
            leading.session.remove_sink(player);
        }
        self.changed(leader);
        self.changed(player);
    }

    /// The endpoint by which `player` follows another device natively, if it does.
    fn native_follower(&self, player: &str) -> Option<Arc<dyn bopsie_players::Endpoint>> {
        self.device(player)
            .ok()?
            .members
            .iter()
            .find(|m| m.state.native_leader.is_some())
            .map(|m| m.endpoint.clone())
    }

    /// Notes that the devices of `player` are being changed by the server, so that what they
    /// report meanwhile, and for [`SETTLING`] after, is not taken for a change made elsewhere:
    /// a report asked for before the change can arrive after it.
    fn settling(&self, player: &str) {
        self.inner
            .settling
            .lock()
            .unwrap()
            .insert(player.to_owned(), Instant::now());
    }

    /// Tells endpoint `follower` of `player` to follow endpoint `leading` of `leader`, in the
    /// background; if it will not, `player` leaves the group again.
    fn join_natively(&self, player: &str, follower: &str, leading: &str, leader: &str) {
        let Some(endpoint) = self
            .device(player)
            .ok()
            .and_then(|d| d.member(follower).map(|m| m.endpoint.clone()))
        else {
            return;
        };
        self.settling(player);
        let (playback, player, leading, leader) = (
            self.clone(),
            player.to_owned(),
            leading.to_owned(),
            leader.to_owned(),
        );
        tokio::spawn(async move {
            let joined = tokio::task::spawn_blocking(move || endpoint.join_native(&leading))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            playback.settling(&player);
            if let Err(error) = joined {
                tracing::warn!(
                    device = player,
                    leader,
                    error,
                    "cannot join the group natively"
                );
                if playback.leader_of(&player).as_deref() == Some(leader.as_str()) {
                    playback.unlink(&player, &leader);
                }
            }
        });
    }

    /// Tells `player`'s endpoint `endpoint` to leave its native group, in the background.
    fn leave_natively(&self, player: &str, endpoint: Arc<dyn bopsie_players::Endpoint>) {
        self.settling(player);
        let (playback, player) = (self.clone(), player.to_owned());
        tokio::spawn(async move {
            let left = tokio::task::spawn_blocking(move || endpoint.leave_native())
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            playback.settling(&player);
            if let Err(error) = left {
                tracing::warn!(device = player, error, "cannot leave the native group");
            }
        });
    }

    /// The protocol `player` withdrew while following natively that `leader`'s group plays
    /// over, if any: then it can join once it leaves its native group.
    fn withdrawn_for(&self, player: &str, leader: &str) -> Option<&'static str> {
        let device = self.device(player).ok()?;
        let leading = self.device(leader).ok()?;
        device
            .members
            .iter()
            .flat_map(|m| m.state.capabilities.withdrawn.iter().copied())
            .find(|protocol| {
                leading
                    .members
                    .iter()
                    .any(|m| m.state.provider == *protocol && m.state.available)
            })
    }

    /// Takes `player` out of its native group, waits for `protocol` to come back on it, and
    /// joins it to `leader`'s group: in the background, as the device takes a while.
    fn rejoin_over(&self, player: &str, leader: &str, protocol: &'static str) {
        tracing::info!(
            device = player,
            leader,
            protocol,
            "leaving a native group to join over a protocol it withdrew"
        );
        if let Some(own_leader) = self.leader_of(player).filter(|l| l != player) {
            self.unlink(player, &own_leader);
        }
        let Some(native) = self.native_follower(player) else {
            return;
        };
        self.settling(player);
        let (playback, player, leader) = (self.clone(), player.to_owned(), leader.to_owned());
        tokio::spawn(async move {
            let left = tokio::task::spawn_blocking(move || native.leave_native())
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            playback.settling(&player);
            if let Err(error) = left {
                tracing::warn!(device = player, error, "cannot leave the native group");
                return;
            }
            let deadline = Instant::now() + WITHDRAWN_RETURN;
            while Instant::now() < deadline {
                let back = playback.device(&player).is_ok_and(|d| {
                    d.members
                        .iter()
                        .any(|m| m.state.provider == protocol && m.state.available)
                });
                if back {
                    if let Err(error) = playback.group(&player, &leader) {
                        tracing::warn!(device = player, leader, %error, "cannot join");
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            tracing::warn!(device = player, protocol, "the protocol did not come back");
        });
    }

    /// Takes a device's own report of its native group as what is so: a group made in its
    /// vendor's app is shown as it is, and one left there is left here too.
    fn reconcile_native(&self, id: &str) {
        let Ok(device) = self.device(id) else {
            return;
        };
        let Some(native) = device
            .members
            .iter()
            .find(|m| m.state.capabilities.native_group.is_some())
        else {
            return;
        };
        let reported = native
            .state
            .native_leader
            .as_deref()
            .and_then(|endpoint| self.inner.devices.device_of(endpoint))
            .map(|d| d.id);
        let Ok(entry) = self.entry(id) else {
            return;
        };
        // Checked after the group is read: the server notes it is changing a group before it
        // changes it, so a change seen here is one whose settling is seen too.
        let recently = self
            .inner
            .settling
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|at| at.elapsed() < SETTLING);
        if recently {
            return;
        }
        match (reported, entry.synced_to) {
            (Some(leader), current) if current.as_deref() != Some(leader.as_str()) => {
                if self.entry(&leader).is_err() {
                    return;
                }
                tracing::info!(
                    device = id,
                    leader,
                    "following a leader it was grouped with elsewhere"
                );
                if let Some(current) = current {
                    self.unlink(id, &current);
                }
                self.link(id, &leader);
                self.changed(&leader);
            }
            (None, Some(leader)) => {
                let leader_native = self.device(&leader).is_ok_and(|d| {
                    d.members
                        .iter()
                        .any(|m| m.state.capabilities.native_group.is_some())
                });
                let playing_natively = self
                    .entry(&leader)
                    .ok()
                    .and_then(|e| e.plan)
                    .is_none_or(|plan| plan.carrier(id).is_none_or(|c| c.native));
                if leader_native && playing_natively {
                    tracing::info!(device = id, leader, "no longer following, as it says");
                    self.unlink(id, &leader);
                }
            }
            _ => {}
        }
    }

    pub fn volume_set(&self, player: &str, volume: u8) -> Result<()> {
        let volume = volume.min(self.cap(player));
        self.active(player)?.1.endpoint.set_volume(volume);
        Ok(())
    }

    /// The loudest `device` may be set: its settings' maximum, else 100.
    fn cap(&self, device: &str) -> u8 {
        self.device(device)
            .ok()
            .and_then(|d| d.max_volume)
            .unwrap_or(100)
            .min(100)
    }

    /// Turns down any endpoint of `device` that is louder than its maximum, whoever turned it
    /// up: an app, the speaker's own remote, or a protocol's default at connecting.
    fn enforce_cap(&self, device: &str) {
        let Ok(found) = self.device(device) else {
            return;
        };
        let Some(cap) = found.max_volume else {
            return;
        };
        for member in &found.members {
            if member.state.capabilities.volume && member.state.volume.is_some_and(|v| v > cap) {
                tracing::info!(
                    device,
                    endpoint = member.id,
                    cap,
                    "turned down to its maximum"
                );
                member.endpoint.set_volume(cap);
            }
        }
    }

    pub fn volume_mute(&self, player: &str, muted: bool) -> Result<()> {
        self.active(player)?.1.endpoint.set_muted(muted);
        Ok(())
    }

    /// Moves every member's volume by the same proportion, so that the loudest ends at
    /// `volume`.
    pub fn group_volume(&self, player: &str, volume: u8) -> Result<()> {
        let leader = self.leader_of(player).ok_or_else(|| not_found(player))?;
        let members: Vec<(String, Member)> = self
            .group_of(&leader)
            .into_iter()
            .filter_map(|id| {
                let member = self.active(&id).ok()?.1;
                Some((id, member))
            })
            .collect();
        let loudest = members
            .iter()
            .filter_map(|(_, m)| m.state.volume)
            .max()
            .unwrap_or(0);
        for (id, member) in members {
            let current = member.state.volume.unwrap_or(loudest);
            let scaled = if loudest == 0 {
                volume
            } else {
                ((f64::from(current) * f64::from(volume) / f64::from(loudest)).round() as u8)
                    .min(100)
            };
            member.endpoint.set_volume(scaled.min(self.cap(&id)));
        }
        Ok(())
    }

    /// Plays a calibration on `id`'s group: each speaker the server streams to chirps in turn,
    /// on the group's timeline, for a phone to hear; see [`calibration`]. What the group
    /// played is paused, and stays paused. Returns once the chirps are scheduled, before they
    /// play.
    /// How much later AirPlay speakers play in a group that mixes protocols, in ms.
    pub fn mixed_airplay_ms(&self) -> i32 {
        self.inner.mixed_airplay_ms.load(Ordering::Relaxed)
    }

    /// Sets how much later AirPlay speakers play in a group that mixes protocols, from the next
    /// play, and keeps it.
    pub async fn set_mixed_airplay_ms(&self, ms: i32) -> Result<()> {
        self.inner.mixed_airplay_ms.store(ms, Ordering::Relaxed);
        if let Some(db) = self.inner.db.get() {
            db.call(move |conn| {
                Ok(bopsie_core::db::set_setting(
                    conn,
                    MIXED_AIRPLAY_MS,
                    Some(&ms.to_string()),
                )?)
            })
            .await?;
        }
        Ok(())
    }

    /// What the last calibration set, as the API reported it, if one was saved.
    pub fn last_calibration(&self) -> Option<serde_json::Value> {
        self.inner.last_calibration.lock().unwrap().clone()
    }

    /// Keeps what a calibration set, to show later.
    pub async fn record_calibration(&self, calibration: serde_json::Value) -> Result<()> {
        let text = calibration.to_string();
        *self.inner.last_calibration.lock().unwrap() = Some(calibration);
        if let Some(db) = self.inner.db.get() {
            db.call(move |conn| {
                Ok(bopsie_core::db::set_setting(
                    conn,
                    LAST_CALIBRATION,
                    Some(&text),
                )?)
            })
            .await?;
        }
        Ok(())
    }

    /// The timeline an endpoint carrying for `plan` plays: AirPlay's later by
    /// [`Playback::mixed_airplay_ms`] when the plan mixes protocols.
    fn carrier_timeline(
        &self,
        plan: &Plan,
        endpoint: &Arc<dyn Endpoint>,
        timeline: Timeline,
    ) -> Timeline {
        if plan.strategy == Strategy::Mixed && endpoint.state().provider == AIRPLAY {
            timeline.delayed(self.mixed_airplay_ms())
        } else {
            timeline
        }
    }

    pub async fn calibrate(&self, id: &str) -> Result<Calibration> {
        let leader = self.leader_of(id).ok_or_else(|| not_found(id))?;
        let entry = self.entry(&leader)?;
        let plan = self.plan_for(&leader, &entry.members, None)?;
        let devices = self.inner.devices.all();
        let mut speakers = Vec::new();
        for carrier in plan.streamed() {
            let device =
                find(&devices, &carrier.device).ok_or_else(|| not_found(&carrier.device))?;
            let member = device
                .member(&carrier.endpoint)
                .ok_or_else(|| not_found(&carrier.endpoint))?;
            speakers.push((device.clone(), member.clone(), carrier.format));
        }
        if speakers.len() < 2 {
            return Err(Error::new(
                ErrorCode::UnsupportedFeature,
                "Calibrating needs a group of at least two speakers the server streams to",
            ));
        }
        let skipped = plan
            .carriers
            .iter()
            .filter(|carrier| carrier.native)
            .filter_map(|carrier| find(&devices, &carrier.device).map(|d| d.name.clone()))
            .collect();
        if !self
            .inner
            .calibrating
            .lock()
            .unwrap()
            .insert(leader.clone())
        {
            return Err(calibrating());
        }

        // The engine lets go of the speakers, so that the calibration has them to itself.
        if entry.session.snapshot().state == PlaybackState::Playing {
            entry.session.pause();
        }
        let session = entry.session.clone();
        let _ = tokio::task::spawn_blocking(move || session.shutdown()).await;
        let ready = self.prepare(&leader).await;

        let now = clock::now();
        let start = (now + plan.lead).max(ready.unwrap_or(now)) + CALIBRATION_MARGIN;
        let timeline = Timeline::new(plan.sample_rate, start);
        let count = speakers.len();
        let ends_at = start + calibration::length(count);
        let mut chirps = Vec::new();
        let mut writers = Vec::new();
        for (index, (device, member, format)) in speakers.into_iter().enumerate() {
            for round in 0..calibration::ROUNDS {
                chirps.push(Scheduled {
                    device: device.id.clone(),
                    name: device.name.clone(),
                    endpoint: member.id.clone(),
                    at: start + calibration::offset(round, index, count),
                });
            }
            let sink = member.endpoint.output(
                format,
                self.carrier_timeline(&plan, &member.endpoint, timeline),
            );
            writers.push(tokio::task::spawn_blocking(move || {
                play_calibration(sink, index, count, ends_at);
            }));
        }
        chirps.sort_by_key(|chirp| chirp.at);
        tracing::info!(queue = leader, speakers = count, "calibrating");
        let playback = self.clone();
        tokio::spawn(async move {
            for writer in writers {
                let _ = writer.await;
            }
            playback.inner.calibrating.lock().unwrap().remove(&leader);
        });
        Ok(Calibration {
            chirp: calibration::CHIRP,
            chirps,
            ends_at,
            skipped,
        })
    }

    /// Every device, with its endpoints, as playback sees them.
    pub fn devices(&self) -> Devices {
        self.inner.devices.clone()
    }

    /// Each device that leads a group or plays, with its members and how it plays.
    pub fn groups(&self) -> Vec<Group> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| !entry.members.is_empty() || entry.plan.is_some())
            .map(|(leader, entry)| Group {
                leader: leader.clone(),
                members: entry.members.clone(),
                plan: entry.plan.clone(),
            })
            .collect()
    }

    /// The queue `player` plays from: its leader's.
    pub fn active_queue(&self, player: &str) -> Option<String> {
        self.leader_of(player)
    }

    pub fn queue(&self, queue_id: &str) -> Option<Value> {
        let entry = self.entry(queue_id).ok()?;
        let device = self.device(queue_id).ok()?;
        let queue = entry.session.snapshot();
        let mut value = model::queue(
            queue_id,
            &device.name,
            device.available(),
            &queue,
            entry.session.position(clock::now()),
        );
        // While a source plays to the group, the queue rests.
        value["active"] = json!(entry.session.live_source().is_none());
        // Bopsie's own: meant to be playing, but nothing heard yet, so its elapsed time stands
        // still. Music Assistant's states have no such state.
        value["buffering"] = json!(entry.session.buffering());
        // Bopsie's own: when the sleep timer pauses the queue.
        let (sleep_at, sleep_end_of_chapter) = match self.sleep_at(queue_id) {
            Some(SleepAt::Unix(at)) => (Some(at), false),
            Some(SleepAt::EndOfChapter) => (None, true),
            None => (None, false),
        };
        value["sleep_at"] = json!(sleep_at);
        value["sleep_end_of_chapter"] = json!(sleep_end_of_chapter);
        // Bopsie's own: the chapter of the current book playing, by index.
        value["current_chapter"] = json!(
            queue
                .current()
                .and_then(|item| item.track.fields.metadata.chapters.as_deref())
                .and_then(|chapters| bopsie_core::media::chapter_at(
                    chapters,
                    entry.session.position(clock::now()).as_millis() as u64
                ))
        );
        if let Some(applied) = queue
            .current()
            .and_then(|item| entry.session.normalised(&item.queue_item_id))
        {
            value["current_item"]["streamdetails"] = model::stream_details(&applied);
        }
        Some(value)
    }

    pub fn queue_items(&self, queue_id: &str, limit: usize, offset: usize) -> Option<Vec<Value>> {
        let queue = self.entry(queue_id).ok()?.session.snapshot();
        Some(
            queue
                .items
                .iter()
                .enumerate()
                .skip(offset)
                .take(limit)
                .map(|(index, item)| model::queue_item(queue_id, item, index))
                .collect(),
        )
    }

    /// Whether `token_id` connected the app player `player`.
    pub fn owned_by(&self, player: &str, token_id: &str) -> bool {
        self.inner.devices.get(player).is_some_and(|device| {
            device
                .members
                .iter()
                .any(|m| m.state.owner_token.as_deref() == Some(token_id))
        })
    }

    /// The devices that could join `id`'s group as it is: those its plan would carry.
    fn can_group_with(&self, id: &str, devices: &[Device]) -> Vec<String> {
        let Some(leader) = self.leader_of(id) else {
            return Vec::new();
        };
        let Ok(entry) = self.entry(&leader) else {
            return Vec::new();
        };
        let current = entry.plan.as_ref().map(|p| p.strategy);
        devices
            .iter()
            .filter(|other| other.id != id && other.id != leader && other.available())
            .filter(|other| {
                let mut members = entry.members.clone();
                members.retain(|m| *m != other.id);
                members.push(other.id.clone());
                self.plan_among(&leader, &members, current, devices)
                    .is_ok_and(|plan| plan.carrier(&other.id).is_some())
                    || self.withdrawn_for(&other.id, &leader).is_some()
            })
            .map(|other| other.id.clone())
            .collect()
    }

    /// The player as the holder of `viewer_token` sees it.
    pub fn player(&self, id: &str, viewer_token: Option<&str>) -> Option<Value> {
        let devices = self.inner.devices.all();
        self.player_among(id, viewer_token, &devices)
    }

    fn player_among(
        &self,
        id: &str,
        viewer_token: Option<&str>,
        devices: &[Device],
    ) -> Option<Value> {
        let (device, member) = self.active_among(id, devices).ok()?;
        let entry = self.entry(id).ok()?;
        let leader = entry.synced_to.clone().unwrap_or_else(|| id.to_owned());
        let leading = self.entry(&leader).ok()?;
        let queue = leading.session.snapshot();
        let elapsed = leading.session.position(clock::now());
        let live = self
            .live_of(&leader)
            .map(|source| (source.id().to_owned(), source.state()));
        let group_volume = (!entry.members.is_empty())
            .then(|| {
                self.group_of(id)
                    .iter()
                    .filter_map(|m| self.active_among(m, devices).ok()?.1.state.volume)
                    .max()
            })
            .flatten();
        let viewer_owns =
            viewer_token.is_some_and(|token| member.state.owner_token.as_deref() == Some(token));
        let health = self.inner.devices.players().health(&member.id);
        let playing_through = leading
            .plan
            .as_ref()
            .and_then(|plan| plan.carrier(id))
            .map(|c| c.endpoint.as_str());
        let view = PlayerView {
            id,
            name: &device.name,
            kind: "player",
            enabled: device.enabled,
            available: device.available(),
            endpoint: &member.state,
            health: &health,
            features: features(&device, &member),
            output_protocols: output_protocols(&device),
            active_output_protocol: playing_through,
        };
        Some(model::player(
            &view,
            &Grouping {
                synced_to: entry.synced_to.as_deref(),
                members: &entry.members,
                can_group_with: self.can_group_with(id, devices),
                group_volume,
            },
            &PlayerPlayback {
                state: queue.state,
                queue_id: &leader,
                current: queue.current(),
                elapsed,
                buffering: live.is_none() && leading.session.buffering(),
                source: live
                    .as_ref()
                    .map(|(source, state)| SourcePlayback { id: source, state }),
                sources: self
                    .sources_of(id)
                    .iter()
                    .map(|source| SourceEntry {
                        id: source.id().to_owned(),
                        name: source.name(),
                    })
                    .collect(),
            },
            viewer_owns,
            self.inner.config.base_url.as_deref(),
        ))
    }

    /// One endpoint of a device that has several, on its own.
    fn protocol_player(&self, device: &Device, member: &Member) -> Value {
        let health = self.inner.devices.players().health(&member.id);
        let view = PlayerView {
            id: &member.id,
            name: &member.state.name,
            kind: "protocol",
            enabled: device.enabled,
            available: member.state.available,
            endpoint: &member.state,
            health: &health,
            features: features(device, member),
            output_protocols: Vec::new(),
            active_output_protocol: None,
        };
        model::player(
            &view,
            &Grouping {
                synced_to: None,
                members: &[],
                can_group_with: Vec::new(),
                group_volume: None,
            },
            &PlayerPlayback {
                state: PlaybackState::Idle,
                queue_id: &device.id,
                current: None,
                elapsed: Duration::ZERO,
                buffering: false,
                source: None,
                sources: Vec::new(),
            },
            false,
            self.inner.config.base_url.as_deref(),
        )
    }

    /// Every device as a player. With `protocols`, also the endpoints of devices reached over
    /// more than one, each on its own, as Music Assistant's `return_protocol_players` gives.
    pub fn players(&self, viewer_token: Option<&str>, protocols: bool) -> Vec<Value> {
        let devices = self.inner.devices.all();
        let mut players: Vec<Value> = devices
            .iter()
            .filter_map(|device| self.player_among(&device.id, viewer_token, &devices))
            .collect();
        if protocols {
            for device in devices.iter().filter(|d| d.members.len() > 1) {
                for member in device.members.iter().filter(|m| m.id != device.id) {
                    players.push(self.protocol_player(device, member));
                }
            }
        }
        players
    }
}

fn find<'a>(devices: &'a [Device], id: &str) -> Option<&'a Device> {
    devices.iter().find(|device| device.id == id)
}

/// What a device supports, through the endpoint in use.
fn features(device: &Device, member: &Member) -> Vec<&'static str> {
    let capabilities = &member.state.capabilities;
    let mut features = vec!["play_media"];
    if capabilities.mute {
        features.push("volume_mute");
    }
    if device
        .members
        .iter()
        .any(|m| m.state.capabilities.sync_group)
    {
        features.push("set_members");
    }
    if capabilities.volume {
        features.push("volume_set");
    }
    features
}

/// A device's endpoints, when it has more than one.
fn output_protocols(device: &Device) -> Vec<Value> {
    if device.members.len() < 2 {
        return Vec::new();
    }
    device
        .members
        .iter()
        .map(|member| {
            let protocol = member.state.provider;
            json!({
                "output_protocol_id": member.id,
                "name": member.state.name,
                "protocol_domain": protocol,
                "priority": PROTOCOLS.iter().position(|p| *p == protocol),
                "available": member.state.available,
            })
        })
        .collect()
}

/// What a queue command asks of the source a group plays, if it is for the source: transport
/// is, and shuffle, repeat and clearing stay with the queue.
fn source_control(command: &QueueCommand, state: &bopsie_sources::SourceState) -> Option<Control> {
    let position = |seconds: f64| (seconds.max(0.0) * 1_000.0) as u64;
    Some(match *command {
        QueueCommand::Play => Control::Play,
        QueueCommand::Pause | QueueCommand::Stop => Control::Pause,
        QueueCommand::PlayPause if state.playing => Control::Pause,
        QueueCommand::PlayPause => Control::Play,
        QueueCommand::Next => Control::Next,
        QueueCommand::Previous => Control::Previous,
        QueueCommand::Seek(seconds) => Control::Seek(position(seconds)),
        QueueCommand::Skip(seconds) => Control::Seek(position(
            state.position_at(clock::now()) as f64 / 1_000.0 + seconds,
        )),
        QueueCommand::Shuffle(_) | QueueCommand::Repeat(_) | QueueCommand::Clear => return None,
    })
}

fn now_playing(
    track: &Track,
    position: Duration,
    playing: bool,
    base_url: Option<&str>,
) -> NowPlaying {
    let artist = track.artists.first().map(|a| a.name.clone());
    NowPlaying {
        title: track.fields.name.clone(),
        artist: artist.clone(),
        album: track.album.as_ref().map(|a| a.name.clone()),
        album_artist: artist,
        year: track.album.as_ref().and_then(|a| a.year),
        track_number: (track.track_number > 0).then_some(track.track_number),
        artwork_url: model::artwork_url(track, base_url),
        duration: u64::from(track.duration) * 1_000,
        position: position.as_millis() as u64,
        playing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(uri: &str, playing: bool, seconds: u64, then: Instant) -> Reported {
        Reported {
            uri: uri.to_owned(),
            playing,
            rate: 1.0,
            ended: false,
            position: Duration::from_secs(seconds),
            duration: Duration::from_secs(600),
            at: then,
        }
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn progress_is_reported_from_what_changed() {
        let t0 = Instant::now();
        let t1 = t0 + secs(1);
        let book = "audiobookshelf://audiobook/a";
        // Starting, playing on, pausing, staying paused, playing again.
        assert_eq!(
            progress_between(None, Some(&at(book, true, 30, t0))),
            [(book.to_owned(), Progress::Playing(secs(30)))]
        );
        assert_eq!(
            progress_between(Some(&at(book, true, 30, t0)), Some(&at(book, true, 31, t1))),
            [(book.to_owned(), Progress::Playing(secs(31)))]
        );
        assert_eq!(
            progress_between(
                Some(&at(book, true, 31, t0)),
                Some(&at(book, false, 32, t1))
            ),
            [(book.to_owned(), Progress::Paused(secs(32)))]
        );
        assert_eq!(
            progress_between(
                Some(&at(book, false, 32, t0)),
                Some(&at(book, false, 32, t1))
            ),
            []
        );
        // A jump is a seek, playing or paused.
        assert_eq!(
            progress_between(
                Some(&at(book, true, 31, t0)),
                Some(&at(book, true, 200, t1))
            ),
            [(book.to_owned(), Progress::Seeked(secs(200)))]
        );
        assert_eq!(
            progress_between(
                Some(&at(book, false, 32, t0)),
                Some(&at(book, false, 10, t1))
            ),
            [(book.to_owned(), Progress::Seeked(secs(10)))]
        );
    }

    #[test]
    fn a_book_left_is_stopped_or_ended() {
        let t0 = Instant::now();
        let (a, b) = (
            "audiobookshelf://audiobook/a",
            "audiobookshelf://audiobook/b",
        );
        assert_eq!(
            progress_between(Some(&at(a, true, 100, t0)), Some(&at(b, true, 0, t0))),
            [
                (a.to_owned(), Progress::Stopped(secs(100))),
                (b.to_owned(), Progress::Playing(secs(0)))
            ]
        );
        assert_eq!(
            progress_between(Some(&at(a, true, 597, t0)), None),
            [(a.to_owned(), Progress::Ended)]
        );
        // The queue played it to its end: ended, once.
        let ended = Reported {
            ended: true,
            playing: false,
            ..at(a, false, 600, t0)
        };
        assert_eq!(
            progress_between(Some(&at(a, true, 599, t0)), Some(&ended)),
            [(a.to_owned(), Progress::Ended)]
        );
        assert_eq!(progress_between(Some(&ended), Some(&ended)), []);
        assert_eq!(progress_between(Some(&ended), None), []);
    }
}
