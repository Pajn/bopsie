//! Checks that a Spotify account can play through librespot: announces one Connect device and
//! logs what it is given, without playing it anywhere. Pick it in the Spotify app and play a
//! track; it logs the track and the audio it receives, or librespot's errors.
//!
//! ```sh
//! cargo run -p bopsie-sources-spotify --example spotify-probe -- "Bopsie probe"
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::time::Instant;

use futures::StreamExt;
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig};
use librespot_discovery::Discovery;
use librespot_playback::audio_backend::{Sink, SinkResult};
use librespot_playback::config::PlayerConfig;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::{Mixer, MixerConfig};
use librespot_playback::player::{Player, PlayerEvent};

struct Volume(AtomicU16);

impl Mixer for Volume {
    fn open(_: MixerConfig) -> Result<Self, librespot_core::Error> {
        Ok(Self(AtomicU16::new(u16::MAX / 2)))
    }
    fn volume(&self) -> u16 {
        self.0.load(Ordering::SeqCst)
    }
    fn set_volume(&self, volume: u16) {
        tracing::info!(volume, "volume from the app");
        self.0.store(volume, Ordering::SeqCst);
    }
}

/// Counts the samples it is given, taking them no faster than they would play, so that it
/// does not race through the listener's queue.
struct Counting {
    samples: Arc<AtomicU64>,
    since: Option<(Instant, u64)>,
}

impl Sink for Counting {
    fn start(&mut self) -> SinkResult<()> {
        self.since = None;
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, _: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(samples) = packet {
            let total = self
                .samples
                .fetch_add(samples.len() as u64, Ordering::SeqCst)
                + samples.len() as u64;
            let (at, from) = *self.since.get_or_insert((Instant::now(), total));
            let due = std::time::Duration::from_secs_f64((total - from) as f64 / 2.0 / 44_100.0);
            std::thread::sleep(due.saturating_sub(at.elapsed()));
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,librespot=debug".into()),
        )
        .init();
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Bopsie probe".to_owned());
    let session_config = SessionConfig::default();
    let mut discovery = Discovery::builder(
        session_config.device_id.clone(),
        session_config.client_id.clone(),
    )
    .name(name.clone())
    .device_type(DeviceType::Speaker)
    .launch()
    .expect("cannot announce the device");
    tracing::info!(name, "announced; pick it in the Spotify app");

    let samples = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let counted = samples.clone();
    tokio::spawn(async move {
        let mut last = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let now = counted.load(Ordering::SeqCst);
            if now != last {
                // Interleaved stereo at 44.1 kHz.
                let seconds = (now - last) as f64 / 2.0 / 44_100.0;
                tracing::info!(
                    audio_seconds = format!("{seconds:.1}"),
                    total_seconds = format!("{:.1}", now as f64 / 2.0 / 44_100.0),
                    elapsed = started.elapsed().as_secs(),
                    "audio received"
                );
                last = now;
            }
        }
    });

    let mut running: Option<Spirc> = None;
    while let Some(credentials) = discovery.next().await {
        tracing::info!("credentials from the app");
        if let Some(old) = running.take() {
            let _ = old.shutdown();
        }
        let session = Session::new(session_config.clone(), None);
        let sink_samples = samples.clone();
        let mixer = Arc::new(Volume::open(MixerConfig::default()).unwrap());
        let player = Player::new(
            PlayerConfig::default(),
            session.clone(),
            mixer.get_soft_volume(),
            move || {
                Box::new(Counting {
                    samples: sink_samples,
                    since: None,
                })
            },
        );
        let mut events = player.get_player_event_channel();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    PlayerEvent::TrackChanged { audio_item } => tracing::info!(
                        title = audio_item.name,
                        uri = audio_item.uri,
                        duration_ms = audio_item.duration_ms,
                        "track"
                    ),
                    other => tracing::info!(event = ?other, "player"),
                }
            }
        });
        let config = ConnectConfig {
            name: name.clone(),
            device_type: DeviceType::Speaker,
            ..ConnectConfig::default()
        };
        match Spirc::new(config, session, credentials, player, mixer).await {
            Ok((spirc, task)) => {
                tokio::spawn(task);
                running = Some(spirc);
            }
            Err(error) => tracing::error!(%error, "cannot connect"),
        }
    }
}
