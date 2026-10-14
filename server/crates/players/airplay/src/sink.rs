//! A session's audio to one AirPlay speaker, through its `cliairplay` process.
//!
//! The speaker plays on the session's timeline: a stream is commanded to be audible exactly
//! when the timeline puts its first frame. Until the speaker can do that — its process
//! connecting, its clock not yet following ours — the sink keeps the audio it is given, without
//! holding up the session, and then starts at the earliest frame the speaker can play on time,
//! dropping only what came before. A speaker joining a playing group is so in step from its
//! first sample, a moment after it is ready.

use std::io;
use std::time::{Duration, Instant};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_core::clock;

use crate::supervisor::{ClockStart, LATE_TOLERANCE_MS, Supervisor};

/// How long to wait for a speaker's clock to say when it will be ready before starting on the
/// timeline regardless.
pub const CLOCK_PATIENCE: Duration = Duration::from_secs(5);
/// How long a stream that has ended waits for its speaker to start what is kept.
const DRAIN_PATIENCE: Duration = Duration::from_secs(20);
/// The least lead a start gets: `cliairplay` moves anything under 250 ms later.
const MIN_LEAD_MS: u64 = 300;
/// The most audio kept while the speaker gets ready.
const MAX_PENDING_SECONDS: usize = 20;

pub struct AirplaySink {
    supervisor: Supervisor,
    format: OutputFormat,
    timeline: Timeline,
    /// A START was sent for the stream being written.
    started: bool,
    /// The speaker may be playing audio this sink sent.
    live: bool,
    /// Audio kept until the speaker can start, and the timeline frame it begins at.
    pending: Vec<u8>,
    pending_at: u64,
    /// When this stream began waiting for the speaker's clock.
    waiting_since: Option<Instant>,
    /// How much later than the timeline the speaker plays, from its `sync_adjust_ms` quirk.
    adjust_ms: i64,
}

impl AirplaySink {
    pub fn new(
        supervisor: Supervisor,
        format: OutputFormat,
        timeline: Timeline,
        adjust_ms: i32,
    ) -> Self {
        Self {
            supervisor,
            format,
            timeline,
            started: false,
            live: false,
            pending: Vec::new(),
            pending_at: 0,
            waiting_since: None,
            adjust_ms: i64::from(adjust_ms),
        }
    }

    fn frame_bytes(&self) -> usize {
        self.format.bytes_per_frame()
    }

    /// The first timeline frame audible at or after Unix time `unix_ms`.
    fn frame_at_unix(&self, unix_ms: u64) -> u64 {
        let now_ms = clock::unix_ms(clock::now());
        let micros = clock::now() + (unix_ms as i64 - now_ms as i64 - self.adjust_ms) * 1_000;
        self.timeline.frame_at(micros) + 1
    }

    /// When the speaker plays `frame`, in Unix milliseconds.
    fn unix_ms_of(&self, frame: u64) -> u64 {
        (clock::unix_ms(self.timeline.instant_of(frame)) as i64 + self.adjust_ms) as u64
    }

    /// Adds audio for frame `at` on to what is kept, as far back as it may reach.
    fn keep(&mut self, at: u64, data: &[u8]) {
        let frame_bytes = self.frame_bytes();
        let kept_frames = (self.pending.len() / frame_bytes) as u64;
        if self.pending.is_empty() || self.pending_at + kept_frames != at {
            self.pending.clear();
            self.pending_at = at;
        }
        self.pending.extend_from_slice(data);
        let max = MAX_PENDING_SECONDS * self.format.sample_rate as usize * frame_bytes;
        if self.pending.len() > max {
            let excess = (self.pending.len() - max) / frame_bytes * frame_bytes;
            self.pending.drain(..excess);
            self.pending_at += (excess / frame_bytes) as u64;
        }
    }

    /// Starts the stream from what is kept, if the speaker can play some of it on time.
    fn try_start(&mut self) -> io::Result<()> {
        if !self.supervisor.is_connected() {
            if !self.supervisor.is_connecting() {
                if let Some(failure) = self.supervisor.failure() {
                    return Err(failure.into());
                }
                self.supervisor.connect_in_background();
            }
            return Ok(());
        }
        let since = *self.waiting_since.get_or_insert_with(Instant::now);
        let mut earliest = clock::unix_ms(clock::now()) + MIN_LEAD_MS;
        match self.supervisor.clock_start() {
            ClockStart::Unknown if since.elapsed() < CLOCK_PATIENCE => return Ok(()),
            ClockStart::From(ms) => earliest = earliest.max(ms),
            ClockStart::Unknown | ClockStart::NotMeasured => {}
        }
        // From the first frame kept, unless the speaker cannot play it on time.
        let start = if earliest > self.unix_ms_of(self.pending_at) + LATE_TOLERANCE_MS {
            self.frame_at_unix(earliest)
        } else {
            self.pending_at
        };
        let frame_bytes = self.frame_bytes();
        let kept_frames = (self.pending.len() / frame_bytes) as u64;
        if start >= self.pending_at + kept_frames {
            // All of it is too soon; wait for more.
            self.pending.clear();
            return Ok(());
        }
        self.pending
            .drain(..(start - self.pending_at) as usize * frame_bytes);
        self.pending_at = start;

        if self.supervisor.is_streaming() {
            self.supervisor.flush()?;
        }
        self.supervisor.write_audio(&self.pending)?;
        self.pending.clear();
        let requested = self.unix_ms_of(start);
        let actual = self.supervisor.start(requested)?;
        self.live = true;
        self.waiting_since = None;
        if actual > requested + LATE_TOLERANCE_MS {
            // The speaker would play behind the timeline: start over from later audio.
            self.supervisor.flush()?;
            return Ok(());
        }
        self.started = true;
        Ok(())
    }

    fn reset(&mut self) {
        self.started = false;
        self.pending.clear();
        self.waiting_since = None;
    }
}

impl Sink for AirplaySink {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        if self.started {
            return self.supervisor.write_audio(data);
        }
        self.keep(at, data);
        self.try_start()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.reset();
        if std::mem::take(&mut self.live) {
            self.supervisor.flush()?;
        }
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        self.reset();
        if std::mem::take(&mut self.live) {
            self.supervisor.flush()?;
            self.supervisor.command(&["ACTION=STANDBY".into()])?;
        }
        Ok(())
    }

    fn drain(&mut self) -> io::Result<()> {
        // Audio still kept is all there is: the session has nothing more to do, so wait for
        // the speaker to be able to play it.
        let deadline = Instant::now() + DRAIN_PATIENCE;
        while !self.started && !self.pending.is_empty() && Instant::now() < deadline {
            self.try_start()?;
            std::thread::sleep(Duration::from_millis(20));
        }
        // The speaker plays out what it holds, which a stop still cuts short. The next stream
        // flushes before it starts.
        self.reset();
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        // The process outlives the session, so the next one starts warm.
        self.drain()
    }
}
