//! Playback across players: one queue per player, sync groups that a leader's queue plays to,
//! and what the API reports of both.
//!
//! A leader's queue session runs one engine, in the leader's format, with a sink for the
//! leader and for every member; a member that joins while it plays is given what is still to
//! play. Whatever a session reports is passed on as Music Assistant's events, and to the
//! players themselves as group state and what is playing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::engine::{Realtime, Settings};
use bopsie_audio::timeline::Timeline;
use bopsie_audio::{OutputFormat, PcmFormat};
use bopsie_core::clock;
use bopsie_core::events::{Event, EventBus, EventType};
use bopsie_core::media::Track;
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::{Change, Endpoint, NowPlaying, Players};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::{Value, json};

use crate::model::{self, Grouping, PlayerPlayback};
use crate::session::{Notice, QueueSession};
use crate::state::{Effect, PlaybackState, Queue, QueueError, QueueOption, RepeatMode};

/// Recent audio kept for members that join a playing group.
const RING_SECONDS: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub ffmpeg: Option<PathBuf>,
    /// Where clients reach the server, for artwork URLs sent to players.
    pub base_url: Option<String>,
}

#[derive(Clone)]
struct Entry {
    session: QueueSession,
    /// The leader this player follows.
    synced_to: Option<String>,
    /// The players following this one, in the order they joined.
    members: Vec<String>,
}

struct Inner {
    players: Players,
    events: EventBus,
    config: Config,
    entries: Mutex<BTreeMap<String, Entry>>,
    rng: Mutex<StdRng>,
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

impl Playback {
    /// Follows `players`, giving each a queue. Needs a Tokio runtime.
    pub fn start(players: Players, events: EventBus, config: Config) -> Self {
        let playback = Self {
            inner: Arc::new(Inner {
                players: players.clone(),
                events,
                config,
                entries: Mutex::default(),
                rng: Mutex::new(StdRng::from_rng(&mut rand::rng())),
            }),
        };
        let mut changes = players.subscribe();
        for endpoint in players.all() {
            playback.add_player(endpoint.id());
        }
        let watcher = playback.clone();
        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(Change::Added(id)) => watcher.add_player(&id),
                    Ok(Change::Updated(id)) => watcher.player_changed(&id),
                    // Endpoints stay registered once added.
                    Ok(Change::Removed(_)) => {}
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

    fn entry(&self, id: &str) -> Result<Entry> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }

    fn endpoint(&self, id: &str) -> Result<Arc<dyn Endpoint>> {
        self.inner.players.get(id).ok_or_else(|| not_found(id))
    }

    fn add_player(&self, id: &str) {
        let session = {
            let mut entries = self.inner.entries.lock().unwrap();
            if entries.contains_key(id) {
                drop(entries);
                self.player_changed(id);
                return;
            }
            let session = QueueSession::default();
            entries.insert(
                id.to_owned(),
                Entry {
                    session: session.clone(),
                    synced_to: None,
                    members: Vec::new(),
                },
            );
            session
        };
        let mut notices = session.notices();
        let follower = self.clone();
        let queue_id = id.to_owned();
        tokio::spawn(async move {
            while let Ok(notice) = notices.recv().await {
                follower.on_notice(&queue_id, notice);
            }
        });
        self.publish(EventType::PlayerAdded, id, self.player(id, None));
        self.publish(EventType::QueueAdded, id, self.queue(id));
    }

    fn player_changed(&self, id: &str) {
        // A player that reconnects gets a new sink in the group it plays in.
        if let (Ok(endpoint), Some(leader)) = (self.endpoint(id), self.leader_of(id))
            && endpoint.state().available
            && let Ok(entry) = self.entry(&leader)
            && let Some((format, Some(timeline))) = entry.session.engine()
            && let Some(output) = member_format(&endpoint, format)
        {
            entry
                .session
                .add_sink(id.to_owned(), endpoint.output(output, timeline));
        }
        self.publish(EventType::PlayerUpdated, id, self.player(id, None));
        self.publish(EventType::QueueUpdated, id, self.queue(id));
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
    }

    fn on_notice(&self, queue_id: &str, notice: Notice) {
        match notice {
            Notice::Started(_) | Notice::Seeked => self.announce(queue_id),
            Notice::Finished | Notice::Stopped => {
                for id in self.group_of(queue_id) {
                    if let Ok(endpoint) = self.endpoint(&id) {
                        endpoint.set_group(false, queue_id);
                        endpoint.set_now_playing(None);
                    }
                }
            }
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

    /// Tells the group's players what is playing.
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
            if let Ok(endpoint) = self.endpoint(&id) {
                endpoint.set_group(queue.state == PlaybackState::Playing, queue_id);
                endpoint.set_now_playing(Some(&now_playing));
            }
        }
    }

    fn tick(&self) {
        let playing: Vec<(String, f64)> = self
            .inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| entry.synced_to.is_none())
            .filter(|(_, entry)| entry.session.snapshot().state == PlaybackState::Playing)
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

    /// Starts the leader's engine if it has none, in the leader's format, with a sink for
    /// every member.
    fn ensure_engine(&self, leader: &str) -> Result<()> {
        let entry = self.entry(leader)?;
        let endpoint = self.endpoint(leader)?;
        let state = endpoint.state();
        if !state.available {
            return Err(unavailable());
        }
        let output = *state.capabilities.formats.first().ok_or_else(unavailable)?;
        let format = PcmFormat {
            sample_rate: output.sample_rate,
            channels: output.channels,
        };
        if entry
            .session
            .engine()
            .is_some_and(|(running, _)| running == format)
        {
            return Ok(());
        }
        let members: Vec<Arc<dyn Endpoint>> = entry
            .members
            .iter()
            .filter_map(|id| self.endpoint(id).ok())
            .filter(|member| member.state().available)
            .collect();
        let lead = members
            .iter()
            .map(|m| m.state().capabilities.lead)
            .fold(state.capabilities.lead, i64::max);
        let timeline = Timeline::new(format.sample_rate, clock::now());
        let mut sinks = vec![(leader.to_owned(), endpoint.output(output, timeline))];
        for member in &members {
            if let Some(member_output) = member_format(member, format) {
                sinks.push((
                    member.id().to_owned(),
                    member.output(member_output, timeline),
                ));
            }
        }
        let settings = Settings {
            format,
            ffmpeg: self.inner.config.ffmpeg.clone(),
            ring_frames: format.sample_rate as usize * RING_SECONDS,
            realtime: Some(Realtime {
                timeline,
                lead,
                clock: clock::now,
            }),
        };
        entry.session.attach(settings, sinks);
        Ok(())
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
        let session = self.entry(&leader)?.session;
        let value = session.update(|queue| f(queue, &mut self.inner.rng.lock().unwrap()));
        self.changed(&leader);
        Ok(value)
    }

    /// Runs `f`, which may start playback, on the queue `queue_id` plays from. The queue
    /// changes at once; the group's players get ready and playback starts in the background,
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
        if !self.endpoint(&leader)?.state().available {
            return Err(unavailable());
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
                Ok(()) => session.release(epoch, not_before),
                Err(error) => {
                    tracing::warn!(queue = leader, %error, "cannot start playback");
                    session.abandon(epoch);
                    playback.changed(&leader);
                }
            }
        });
        Ok(())
    }

    /// Gets every player in the group ready, together, and returns when the slowest can
    /// start, if any needs longer than its lead.
    async fn prepare(&self, leader: &str) -> Option<bopsie_audio::timeline::Micros> {
        let endpoints: Vec<Arc<dyn Endpoint>> = self
            .group_of(leader)
            .iter()
            .filter_map(|id| self.endpoint(id).ok())
            .filter(|endpoint| endpoint.state().available)
            .collect();
        let preparing: Vec<_> = endpoints
            .into_iter()
            .map(|endpoint| tokio::task::spawn_blocking(move || endpoint.prepare()))
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
        tracks: Vec<(Track, PathBuf)>,
        option: QueueOption,
        start_at: Option<usize>,
    ) -> Result<()> {
        let enqueue = move |queue: &mut Queue, rng: &mut StdRng| {
            let effect = queue.enqueue(tracks, option, rng);
            match (effect, start_at) {
                (Effect::PlayFrom(first), Some(offset)) if offset > 0 => {
                    ((), queue.play_index(first + offset))
                }
                _ => ((), effect),
            }
        };
        if matches!(option, QueueOption::Play | QueueOption::Replace) {
            self.begin(queue_id, enqueue)
        } else {
            self.update(queue_id, enqueue)
        }
    }

    pub fn command(&self, queue_id: &str, command: QueueCommand) -> Result<()> {
        let leader = self
            .leader_of(queue_id)
            .ok_or_else(|| not_found(queue_id))?;
        let session = self.entry(&leader)?.session;
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
            QueueCommand::Next => self.begin(&leader, |q, _| ((), q.next_track()))?,
            QueueCommand::Previous => {
                let elapsed = session.position(clock::now()).as_secs_f64();
                self.begin(&leader, move |q, _| ((), q.previous_track(elapsed)))?;
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

    /// Moves a queue, its position and its settings to another player, which plays on if the
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

    /// Adds `player` to the group `target` plays in. The player stops what it was playing.
    pub fn group(&self, player: &str, target: &str) -> Result<()> {
        let leader = self.leader_of(target).ok_or_else(|| not_found(target))?;
        if leader == player {
            return Ok(());
        }
        let endpoint = self.endpoint(player)?;
        if self.entry(player)?.synced_to.is_some() {
            self.ungroup(player)?;
        }
        let own = self.entry(player)?;
        if own.session.snapshot().state != PlaybackState::Idle {
            own.session.stop();
        }
        {
            let mut entries = self.inner.entries.lock().unwrap();
            if let Some(entry) = entries.get_mut(player) {
                entry.synced_to = Some(leader.clone());
            }
            if let Some(entry) = entries.get_mut(&leader)
                && !entry.members.iter().any(|m| m == player)
            {
                entry.members.push(player.to_owned());
            }
        }
        let leading = self.entry(&leader)?;
        if let Some((format, Some(timeline))) = leading.session.engine()
            && let Some(output) = member_format(&endpoint, format)
        {
            leading
                .session
                .add_sink(player.to_owned(), endpoint.output(output, timeline));
        }
        if leading.session.snapshot().state == PlaybackState::Playing {
            self.announce(&leader);
        }
        self.changed(&leader);
        Ok(())
    }

    /// Takes `player` out of its group. A leader hands its queue, and its followers, to the
    /// first of them.
    pub fn ungroup(&self, player: &str) -> Result<()> {
        let entry = self.entry(player)?;
        if let Some(leader) = entry.synced_to {
            {
                let mut entries = self.inner.entries.lock().unwrap();
                if let Some(leading) = entries.get_mut(&leader) {
                    leading.members.retain(|m| m != player);
                }
                if let Some(own) = entries.get_mut(player) {
                    own.synced_to = None;
                }
            }
            self.entry(&leader)?.session.remove_sink(player);
            if let Ok(endpoint) = self.endpoint(player) {
                endpoint.set_group(false, player);
                endpoint.set_now_playing(None);
            }
            self.changed(&leader);
            self.changed(player);
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

    pub fn volume_set(&self, player: &str, volume: u8) -> Result<()> {
        self.endpoint(player)?.set_volume(volume.min(100));
        Ok(())
    }

    pub fn volume_mute(&self, player: &str, muted: bool) -> Result<()> {
        self.endpoint(player)?.set_muted(muted);
        Ok(())
    }

    /// Moves every member's volume by the same proportion, so that the loudest ends at
    /// `volume`.
    pub fn group_volume(&self, player: &str, volume: u8) -> Result<()> {
        let leader = self.leader_of(player).ok_or_else(|| not_found(player))?;
        let members: Vec<Arc<dyn Endpoint>> = self
            .group_of(&leader)
            .iter()
            .filter_map(|id| self.endpoint(id).ok())
            .collect();
        let loudest = members
            .iter()
            .filter_map(|m| m.state().volume)
            .max()
            .unwrap_or(0);
        for member in members {
            let current = member.state().volume.unwrap_or(loudest);
            let scaled = if loudest == 0 {
                volume
            } else {
                ((f64::from(current) * f64::from(volume) / f64::from(loudest)).round() as u8)
                    .min(100)
            };
            member.set_volume(scaled);
        }
        Ok(())
    }

    /// The queue `player` plays from: its leader's.
    pub fn active_queue(&self, player: &str) -> Option<String> {
        self.leader_of(player)
    }

    pub fn queue(&self, queue_id: &str) -> Option<Value> {
        let entry = self.entry(queue_id).ok()?;
        let state = self.endpoint(queue_id).ok()?.state();
        let queue = entry.session.snapshot();
        Some(model::queue(
            queue_id,
            &state.name,
            state.available,
            &queue,
            entry.session.position(clock::now()),
        ))
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
        self.inner
            .players
            .get(player)
            .and_then(|endpoint| endpoint.state().owner_token)
            .is_some_and(|owner| owner == token_id)
    }

    /// The player as the holder of `viewer_token` sees it.
    pub fn player(&self, id: &str, viewer_token: Option<&str>) -> Option<Value> {
        let endpoint = self.endpoint(id).ok()?;
        let state = endpoint.state();
        let entry = self.entry(id).ok()?;
        let leader = entry.synced_to.clone().unwrap_or_else(|| id.to_owned());
        let leading = self.entry(&leader).ok()?;
        let queue = leading.session.snapshot();
        let elapsed = leading.session.position(clock::now());
        let group_volume = (!entry.members.is_empty())
            .then(|| {
                self.group_of(id)
                    .iter()
                    .filter_map(|m| self.endpoint(m).ok()?.state().volume)
                    .max()
            })
            .flatten();
        let can_group_with = self
            .inner
            .players
            .all()
            .iter()
            .filter(|other| other.id() != id && other.state().available)
            .map(|other| other.id().to_owned())
            .collect();
        let viewer_owns =
            viewer_token.is_some_and(|token| state.owner_token.as_deref() == Some(token));
        Some(model::player(
            id,
            &state,
            &Grouping {
                synced_to: entry.synced_to.as_deref(),
                members: &entry.members,
                can_group_with,
                group_volume,
            },
            &PlayerPlayback {
                state: queue.state,
                queue_id: &leader,
                current: queue.current(),
                elapsed,
            },
            viewer_owns,
            self.inner.config.base_url.as_deref(),
        ))
    }

    pub fn players(&self, viewer_token: Option<&str>) -> Vec<Value> {
        self.inner
            .players
            .all()
            .iter()
            .filter_map(|endpoint| self.player(endpoint.id(), viewer_token))
            .collect()
    }
}

/// The member's format for a session in `format`: the same rate and channels, at the bit
/// depth it prefers.
fn member_format(member: &Arc<dyn Endpoint>, format: PcmFormat) -> Option<OutputFormat> {
    member
        .state()
        .capabilities
        .formats
        .into_iter()
        .find(|f| f.sample_rate == format.sample_rate && f.channels == format.channels)
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
