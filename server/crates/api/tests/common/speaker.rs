//! Speakers for tests: endpoints that count the audio they are given, and can be made to fail.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_players::{
    Capabilities, Controls, DeviceInfo, Endpoint, EndpointState, Facts, NativeGroup, NowPlaying,
    Players,
};

pub const CD: OutputFormat = OutputFormat {
    sample_rate: 44_100,
    channels: 2,
    bits: 16,
};
pub const DAT: OutputFormat = OutputFormat {
    sample_rate: 48_000,
    channels: 2,
    bits: 16,
};

/// A speaker that counts the audio it is given.
pub struct Speaker {
    pub id: String,
    pub name: String,
    pub provider: &'static str,
    pub formats: Vec<OutputFormat>,
    pub mac: Option<String>,
    pub volume: Mutex<u8>,
    pub muted: Mutex<bool>,
    /// What it was last told of its group's controls.
    pub controls: Mutex<Option<Controls>>,
    /// Frames received, by sample rate.
    pub frames: Arc<Mutex<Vec<(u32, u64)>>>,
    /// Outputs made for it.
    pub outputs: AtomicU64,
    pub trouble: Arc<Trouble>,
    pub available: AtomicBool,
    pub players: Players,
    /// It groups by its own multiroom, as a LinkPlay speaker does.
    pub native: AtomicBool,
    /// It has to be launched first, as a Cast receiver does.
    pub launched: AtomicBool,
    /// How often it was woken, and when it last was, and last was prepared.
    pub woken: AtomicU64,
    pub woken_at: Mutex<Option<std::time::Instant>>,
    pub prepared_at: Mutex<Option<std::time::Instant>>,
}

/// What the next write to a speaker's output does wrong.
#[derive(Default)]
pub struct Trouble {
    pub fail: AtomicBool,
    pub panic: AtomicBool,
}

impl Speaker {
    pub fn register(
        players: &Players,
        id: &str,
        provider: &'static str,
        formats: &[OutputFormat],
        mac: Option<&str>,
    ) -> Arc<Self> {
        let speaker = Arc::new(Self {
            id: id.to_owned(),
            name: id[..1].to_uppercase() + &id[1..],
            provider,
            formats: formats.to_vec(),
            mac: mac.map(str::to_owned),
            volume: Mutex::new(40),
            muted: Mutex::new(false),
            controls: Mutex::new(None),
            frames: Arc::default(),
            outputs: AtomicU64::new(0),
            trouble: Arc::default(),
            available: AtomicBool::new(true),
            players: players.clone(),
            native: AtomicBool::new(false),
            launched: AtomicBool::new(false),
            woken: AtomicU64::new(0),
            woken_at: Mutex::default(),
            prepared_at: Mutex::default(),
        });
        players.register(speaker.clone());
        speaker
    }

    pub fn frames(&self) -> u64 {
        self.frames.lock().unwrap().iter().map(|(_, n)| n).sum()
    }

    pub fn frames_at(&self, rate: u32) -> u64 {
        self.frames
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, _)| *r == rate)
            .map(|(_, n)| n)
            .sum()
    }
}

/// Takes audio as a speaker would: no more than half a second ahead of playing it.
struct Counting {
    format: OutputFormat,
    trouble: Arc<Trouble>,
    begun: std::time::Instant,
    taken: u64,
    frames: Arc<Mutex<Vec<(u32, u64)>>>,
    written: Arc<AtomicU64>,
}

impl Sink for Counting {
    fn format(&self) -> OutputFormat {
        self.format
    }
    fn write(&mut self, _at: u64, data: &[u8]) -> std::io::Result<()> {
        if self.trouble.fail.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        }
        assert!(
            !self.trouble.panic.swap(false, Ordering::SeqCst),
            "an output that panics"
        );
        let frames = (data.len() / self.format.bytes_per_frame()) as u64;
        self.taken += frames;
        let ahead = Duration::from_secs_f64(self.taken as f64 / f64::from(self.format.sample_rate))
            .saturating_sub(self.begun.elapsed());
        std::thread::sleep(ahead.saturating_sub(Duration::from_millis(500)));
        self.written.fetch_add(frames, Ordering::SeqCst);
        let mut all = self.frames.lock().unwrap();
        match all.iter_mut().find(|(r, _)| *r == self.format.sample_rate) {
            Some((_, n)) => *n += frames,
            None => all.push((self.format.sample_rate, frames)),
        }
        Ok(())
    }
    fn finish(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Endpoint for Speaker {
    fn id(&self) -> &str {
        &self.id
    }
    fn state(&self) -> EndpointState {
        let mut state = EndpointState {
            name: self.name.clone(),
            provider: self.provider,
            available: self.available.load(Ordering::SeqCst),
            volume: Some(*self.volume.lock().unwrap()),
            muted: Some(*self.muted.lock().unwrap()),
            device: DeviceInfo {
                model: None,
                manufacturer: None,
                software_version: None,
            },
            app: false,
            owner_token: None,
            facts: Facts {
                mac: self.mac.clone(),
                ..Facts::default()
            },
            capabilities: Capabilities {
                formats: self.formats.clone(),
                lead: 200_000,
                sync_group: true,
                max_group_size: None,
                volume: true,
                mute: true,
                pairing: Default::default(),
                native_group: self.native.load(Ordering::SeqCst).then_some(NativeGroup {
                    family: "linkplay",
                    version: 4,
                }),
                launched: self.launched.load(Ordering::SeqCst),
                withdrawn: Vec::new(),
            },
            quirks: Default::default(),
            native_leader: None,
        };
        state.quirks = self
            .players
            .resolve_quirks(&self.id, &state, &Default::default());
        state
    }
    fn output(&self, format: OutputFormat, _timeline: Timeline) -> Box<dyn Sink> {
        self.outputs.fetch_add(1, Ordering::SeqCst);
        Box::new(Counting {
            format,
            trouble: self.trouble.clone(),
            begun: std::time::Instant::now(),
            taken: 0,
            frames: self.frames.clone(),
            written: Arc::default(),
        })
    }
    fn set_volume(&self, volume: u8) {
        *self.volume.lock().unwrap() = volume;
        self.players.changed(&self.id);
    }
    fn set_muted(&self, muted: bool) {
        *self.muted.lock().unwrap() = muted;
        self.players.changed(&self.id);
    }
    fn set_now_playing(&self, _: Option<&NowPlaying>) {}
    fn set_group(&self, _: bool, _: &str) {}
    fn set_controls(&self, controls: &Controls) {
        *self.controls.lock().unwrap() = Some(*controls);
    }
    fn wake(&self) {
        // Long enough that what comes after it is seen to.
        std::thread::sleep(Duration::from_millis(20));
        self.woken.fetch_add(1, Ordering::SeqCst);
        *self.woken_at.lock().unwrap() = Some(std::time::Instant::now());
    }
    fn prepare(&self) -> Option<bopsie_audio::timeline::Micros> {
        *self.prepared_at.lock().unwrap() = Some(std::time::Instant::now());
        None
    }
}
