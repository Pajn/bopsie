//! One speaker as a Spotify Connect device: announced on the network under the speaker's name,
//! connected when someone picks it in the Spotify app, and a [`Source`] for the speaker's
//! group from then on.
//!
//! librespot decodes into [`ToLive`], which hands the audio to the group's live input, and asks
//! [`GroupVolume`] for the volume, which keeps the audio at full scale and tells the group what
//! the app's slider says instead.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use bopsie_audio::PcmFormat;
use bopsie_audio::decode::Decoder;
use bopsie_audio::live::{LiveWriter, live};
use bopsie_core::clock;
use bopsie_sources::{Control, Happened, Source, SourceState, Sources};
use futures::StreamExt;
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig};
use librespot_discovery::Discovery;
use librespot_playback::audio_backend::{Sink, SinkResult};
use librespot_playback::config::{NormalisationType, PlayerConfig};

use crate::media::SPOTIFY_REFERENCE_LUFS;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::{Mixer, MixerConfig};
use librespot_playback::player::Player;
use tokio::task::JoinHandle;

use crate::events;

/// What librespot decodes to.
pub const FORMAT: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};
/// The most decoded audio held for the group: librespot waits while it is full.
const BUFFER: Duration = Duration::from_millis(500);

/// A speaker's Spotify Connect device.
pub struct ConnectDevice {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    id: String,
    player: String,
    name: String,
    device_id: String,
    /// Where it is announced; empty for every address.
    addresses: Vec<IpAddr>,
    cache_dir: PathBuf,
    sources: Sources,
    state: Mutex<SourceState>,
    /// The loudness what the app plays is brought to, in LUFS; `None` plays it as it is.
    normalise: Option<f32>,
    /// Where decoded audio goes: the group's live input, once it took one.
    writer: Mutex<Option<Arc<LiveWriter>>>,
    spirc: Mutex<Option<Spirc>>,
    volume: VolumeState,
    task: Mutex<Option<JoinHandle<()>>>,
    /// Tells the announcing task to stop, so that it says goodbye on the network.
    stop: tokio::sync::Notify,
}

/// The app's volume, and what the group last told it, to tell an echo from a change.
struct VolumeState {
    /// librespot's scale, 0 to 65535.
    current: AtomicU16,
    told: Mutex<Option<u8>>,
}

fn to_percent(volume: u16) -> u8 {
    ((f64::from(volume) * 100.0 / f64::from(u16::MAX)).round() as u8).min(100)
}

fn from_percent(percent: u8) -> u16 {
    (f64::from(percent.min(100)) * f64::from(u16::MAX) / 100.0).round() as u16
}

/// How a device is announced and plays.
pub struct Setup {
    /// What Spotify knows the device by.
    pub device_id: String,
    /// The addresses it is announced at.
    pub addresses: Vec<IpAddr>,
    /// Where it keeps the credentials it was given.
    pub cache_dir: PathBuf,
    /// The loudness what the app plays is brought to, in LUFS; `None` plays it as it is.
    pub normalise: Option<f32>,
}

impl ConnectDevice {
    /// Announces a device named `name` for `player`, at the player's `volume`, and registers
    /// it as a source. Needs a Tokio runtime.
    pub fn start(
        player: &str,
        name: &str,
        volume: u8,
        setup: Setup,
        sources: &Sources,
    ) -> Arc<Self> {
        let inner = Arc::new(Inner {
            id: format!("spotify_connect--{player}"),
            player: player.to_owned(),
            name: name.to_owned(),
            device_id: setup.device_id,
            addresses: setup.addresses,
            cache_dir: setup.cache_dir,
            normalise: setup.normalise,
            sources: sources.clone(),
            state: Mutex::default(),
            writer: Mutex::default(),
            spirc: Mutex::default(),
            volume: VolumeState {
                current: AtomicU16::new(from_percent(volume)),
                told: Mutex::default(),
            },
            task: Mutex::default(),
            stop: tokio::sync::Notify::new(),
        });
        let device = Arc::new(Self {
            inner: inner.clone(),
        });
        sources.register(device.clone());
        let task = tokio::spawn(run(Arc::downgrade(&inner)));
        *inner.task.lock().unwrap() = Some(task);
        device
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Stops announcing the device and ends its session.
    pub fn shutdown(&self) {
        self.inner.stop.notify_one();
        self.inner.task.lock().unwrap().take();
        if let Some(spirc) = self.inner.spirc.lock().unwrap().take() {
            let _ = spirc.shutdown();
        }
        self.inner.writer.lock().unwrap().take();
        self.inner.sources.remove(&self.inner.id);
    }
}

impl Source for ConnectDevice {
    fn id(&self) -> &str {
        &self.inner.id
    }

    fn player(&self) -> &str {
        &self.inner.player
    }

    fn name(&self) -> String {
        "Spotify".to_owned()
    }

    fn state(&self) -> SourceState {
        self.inner.state.lock().unwrap().clone()
    }

    fn input(&self) -> Box<dyn Decoder> {
        let (writer, input) = live(FORMAT, BUFFER);
        *self.inner.writer.lock().unwrap() = Some(Arc::new(writer));
        Box::new(input)
    }

    fn control(&self, control: Control) {
        let spirc = self.inner.spirc.lock().unwrap();
        let Some(spirc) = spirc.as_ref() else {
            return;
        };
        let done = match control {
            Control::Play => spirc.play(),
            Control::Pause => spirc.pause(),
            Control::Next => spirc.next(),
            Control::Previous => spirc.prev(),
            Control::Seek(ms) => spirc.set_position_ms(u32::try_from(ms).unwrap_or(u32::MAX)),
            Control::Volume(percent) => {
                let current = to_percent(self.inner.volume.current.load(Ordering::SeqCst));
                if current.abs_diff(percent) <= 1 {
                    return;
                }
                *self.inner.volume.told.lock().unwrap() = Some(percent);
                spirc.set_volume(from_percent(percent))
            }
            Control::Release => {
                self.inner.writer.lock().unwrap().take();
                spirc.disconnect(true)
            }
        };
        if let Err(error) = done {
            tracing::warn!(device = self.inner.id, ?control, %error, "Spotify refused");
        }
    }

    fn unheard(&self, unheard: Duration) {
        let held = self
            .inner
            .writer
            .lock()
            .unwrap()
            .as_ref()
            .map(|writer| {
                let held = writer.buffered();
                writer.clear();
                held
            })
            .unwrap_or_default();
        let back = (unheard + held).as_millis() as u64;
        let position = self.state().position_at(clock::now()).saturating_sub(back);
        tracing::debug!(
            device = self.inner.id,
            back,
            position,
            "going back to what was heard"
        );
        if let Some(spirc) = self.inner.spirc.lock().unwrap().as_ref()
            && let Err(error) = spirc.set_position_ms(position as u32)
        {
            tracing::warn!(device = self.inner.id, %error, "cannot go back");
        }
    }
}

/// Announces the device, and connects with the credentials of whoever picks it, or with those
/// kept from last time.
async fn run(inner: Weak<Inner>) {
    let Some(device) = inner.upgrade() else {
        return;
    };
    let cache = Cache::new(Some(&device.cache_dir), None, None, None)
        .map_err(|error| tracing::warn!(device = device.id, %error, "cannot keep credentials"))
        .ok();
    let session_config = SessionConfig {
        device_id: device.device_id.clone(),
        ..SessionConfig::default()
    };
    let mut discovery = match Discovery::builder(
        session_config.device_id.clone(),
        session_config.client_id.clone(),
    )
    .name(device.name.clone())
    .device_type(DeviceType::Speaker)
    .zeroconf_ip(device.addresses.clone())
    .launch()
    {
        Ok(discovery) => discovery,
        Err(error) => {
            tracing::warn!(device = device.id, %error, "cannot announce the Spotify Connect device");
            return;
        }
    };
    tracing::info!(
        device = device.id,
        name = device.name,
        "announced for Spotify Connect"
    );
    if let Some(credentials) = cache.as_ref().and_then(Cache::credentials) {
        connect_apart(&device, &session_config, &cache, credentials).await;
    }
    loop {
        let credentials = tokio::select! {
            credentials = discovery.next() => credentials,
            () = device.stop.notified() => None,
        };
        let Some(credentials) = credentials else {
            break;
        };
        tracing::info!(device = device.id, "picked in the Spotify app");
        connect_apart(&device, &session_config, &cache, credentials).await;
    }
    tracing::info!(
        device = device.id,
        "no longer announced for Spotify Connect"
    );
    discovery.shutdown().await;
}

/// Connects on a task of its own, so that a panic in librespot does not end the announcement.
async fn connect_apart(
    device: &Arc<Inner>,
    session_config: &SessionConfig,
    cache: &Option<Cache>,
    credentials: Credentials,
) {
    let connecting = tokio::spawn({
        let (device, session_config, cache) =
            (device.clone(), session_config.clone(), cache.clone());
        async move { connect(&device, &session_config, cache, credentials).await }
    });
    if let Err(error) = connecting.await {
        tracing::warn!(device = device.id, %error, "connecting to Spotify failed");
    }
}

/// librespot's player, normalising to `normalise` LUFS as queues do: Spotify's gains are to
/// its own reference, and librespot picks the album's gain when an album plays in order.
fn player_config(normalise: Option<f32>) -> PlayerConfig {
    let mut config = PlayerConfig::default();
    if let Some(target) = normalise {
        config.normalisation = true;
        config.normalisation_type = NormalisationType::Auto;
        config.normalisation_pregain_db = f64::from(target - SPOTIFY_REFERENCE_LUFS);
    }
    config
}

async fn connect(
    device: &Arc<Inner>,
    session_config: &SessionConfig,
    cache: Option<Cache>,
    credentials: Credentials,
) {
    if let Some(old) = device.spirc.lock().unwrap().take() {
        let _ = old.shutdown();
    }
    let session = Session::new(session_config.clone(), cache);
    let mixer = Arc::new(GroupVolume {
        device: Arc::downgrade(device),
    });
    let sink_device = Arc::downgrade(device);
    let player = Player::new(
        player_config(device.normalise),
        session.clone(),
        mixer.get_soft_volume(),
        move || {
            Box::new(ToLive {
                device: sink_device,
                paced: None,
            })
        },
    );
    let mut player_events = player.get_player_event_channel();
    let watched = Arc::downgrade(device);
    tokio::spawn(async move {
        while let Some(event) = player_events.recv().await {
            let Some(device) = watched.upgrade() else {
                return;
            };
            let happened = events::apply(&mut device.state.lock().unwrap(), &event, clock::now());
            if let Some(happened) = happened {
                tracing::debug!(device = device.id, ?happened, "Spotify");
                device.sources.report(&device.id, happened);
            }
        }
    });
    let config = ConnectConfig {
        name: device.name.clone(),
        device_type: DeviceType::Speaker,
        initial_volume: device.volume.current.load(Ordering::SeqCst),
        ..ConnectConfig::default()
    };
    match Spirc::new(config, session, credentials, player, mixer).await {
        Ok((spirc, task)) => {
            tracing::info!(device = device.id, "connected to Spotify");
            *device.spirc.lock().unwrap() = Some(spirc);
            let ended = Arc::downgrade(device);
            tokio::spawn(async move {
                task.await;
                if let Some(device) = ended.upgrade() {
                    let was = std::mem::take(&mut device.state.lock().unwrap().connected);
                    device.state.lock().unwrap().playing = false;
                    if was {
                        device.sources.report(&device.id, Happened::Ended);
                    }
                }
            });
        }
        Err(error) => tracing::warn!(device = device.id, %error, "cannot connect to Spotify"),
    }
}

/// librespot's volume control: never turns the audio down, and tells the group what the app
/// asked for, unless it is the group's own volume coming back.
struct GroupVolume {
    device: Weak<Inner>,
}

impl Mixer for GroupVolume {
    fn open(_: MixerConfig) -> Result<Self, librespot_core::Error> {
        Ok(Self {
            device: Weak::new(),
        })
    }

    fn volume(&self) -> u16 {
        self.device
            .upgrade()
            .map_or(u16::MAX / 2, |d| d.volume.current.load(Ordering::SeqCst))
    }

    fn set_volume(&self, volume: u16) {
        let Some(device) = self.device.upgrade() else {
            return;
        };
        device.volume.current.store(volume, Ordering::SeqCst);
        let percent = to_percent(volume);
        let told = device.volume.told.lock().unwrap().take();
        if told.is_some_and(|told| told.abs_diff(percent) <= 1) {
            return;
        }
        device.sources.report(&device.id, Happened::Volume(percent));
    }
}

/// librespot's audio output: the group's live input, or, while the group has none, nowhere at
/// the pace it would play, so that librespot does not race through the listener's queue.
struct ToLive {
    device: Weak<Inner>,
    /// When unpaced audio began, and how many frames since.
    paced: Option<(Instant, u64)>,
}

impl Sink for ToLive {
    fn write(&mut self, packet: AudioPacket, _: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else {
            return Ok(());
        };
        let samples: Vec<f32> = samples.iter().map(|&s| s as f32).collect();
        let writer = self
            .device
            .upgrade()
            .and_then(|device| device.writer.lock().unwrap().clone());
        if let Some(writer) = writer
            && writer.write(&samples)
        {
            self.paced = None;
            return Ok(());
        }
        let frames = (samples.len() / usize::from(FORMAT.channels)) as u64;
        let (since, sent) = self.paced.get_or_insert((Instant::now(), 0));
        *sent += frames;
        let due = Duration::from_secs_f64(*sent as f64 / f64::from(FORMAT.sample_rate));
        std::thread::sleep(due.saturating_sub(since.elapsed()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_scales_both_ways() {
        assert_eq!(to_percent(u16::MAX), 100);
        assert_eq!(to_percent(0), 0);
        for percent in 0..=100 {
            assert_eq!(to_percent(from_percent(percent)), percent);
        }
    }
}
