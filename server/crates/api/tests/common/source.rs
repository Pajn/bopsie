//! A source for tests: audio that a test pushes in as it pleases, with what the group asked of
//! it recorded.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::PcmFormat;
use bopsie_audio::decode::Decoder;
use bopsie_audio::live::{LiveWriter, live};
use bopsie_core::clock;
use bopsie_sources::{Control, Happened, Source, SourceMedia, SourceState, Sources};

pub const FORMAT: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};

pub struct FakeSource {
    pub id: String,
    pub player: String,
    pub sources: Sources,
    pub state: Mutex<SourceState>,
    pub writer: Mutex<Option<LiveWriter>>,
    /// What the group asked of it, in order.
    pub controls: Mutex<Vec<Control>>,
    /// What it was told went unheard.
    pub unheard: Mutex<Vec<Duration>>,
}

impl FakeSource {
    pub fn register(sources: &Sources, player: &str) -> Arc<Self> {
        let source = Arc::new(Self {
            id: format!("fake--{player}"),
            player: player.to_owned(),
            sources: sources.clone(),
            state: Mutex::default(),
            writer: Mutex::default(),
            controls: Mutex::default(),
            unheard: Mutex::default(),
        });
        sources.register(source.clone());
        source
    }

    /// Starts playing `title`, `duration_ms` long, as if someone picked it in the source's app.
    pub fn start(&self, title: &str, duration_ms: u64) {
        *self.state.lock().unwrap() = SourceState {
            connected: true,
            playing: true,
            media: Some(SourceMedia {
                title: title.to_owned(),
                artist: Some("Fake Artist".to_owned()),
                duration_ms,
                ..SourceMedia::default()
            }),
            position_ms: 0,
            at: clock::now(),
        };
        self.sources.report(&self.id, Happened::Playing);
    }

    pub fn happen(&self, happened: Happened) {
        {
            let mut state = self.state.lock().unwrap();
            match happened {
                Happened::Paused => {
                    state.position_ms = state.position_at(clock::now());
                    state.at = clock::now();
                    state.playing = false;
                }
                Happened::Playing => {
                    state.at = clock::now();
                    state.playing = true;
                }
                Happened::Ended => state.connected = false,
                _ => {}
            }
        }
        self.sources.report(&self.id, happened);
    }

    /// Pushes `ms` of audio, if the group took the input.
    pub fn push(&self, ms: u64) -> bool {
        let frames = FORMAT.sample_rate as u64 * ms / 1_000;
        let samples = vec![0.1f32; frames as usize * 2];
        match self.writer.lock().unwrap().as_ref() {
            Some(writer) => writer.write(&samples),
            None => false,
        }
    }

    pub fn controls(&self) -> Vec<Control> {
        self.controls.lock().unwrap().clone()
    }
}

impl Source for FakeSource {
    fn id(&self) -> &str {
        &self.id
    }
    fn player(&self) -> &str {
        &self.player
    }
    fn name(&self) -> String {
        "Fake".to_owned()
    }
    fn state(&self) -> SourceState {
        self.state.lock().unwrap().clone()
    }
    fn input(&self) -> Box<dyn Decoder> {
        let (writer, input) = live(FORMAT, Duration::from_millis(500));
        *self.writer.lock().unwrap() = Some(writer);
        Box::new(input)
    }
    fn control(&self, control: Control) {
        self.controls.lock().unwrap().push(control);
    }
    fn unheard(&self, unheard: Duration) {
        self.unheard.lock().unwrap().push(unheard);
    }
}
