//! A session's audio to one Sendspin client: cut into 25 ms chunks, each stamped with the
//! server time it must play at, and sent no faster than the client's buffer allows.

use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::{Micros, Timeline};
use bopsie_core::clock;

use crate::connection::{Outgoing, Shared};
use crate::pacer::Pacer;
use crate::wire::{self, AUDIO_HEADER_BYTES, Codec, StreamFormat};

/// Chunks per second: 25 ms each, as Music Assistant sends them.
const CHUNKS_PER_SECOND: u32 = 40;
/// Audio a fresh stream may send at once, beyond its lead.
const BURST_BEYOND_LEAD: Micros = 250_000;
/// The longest the engine waits between looks at the client's buffer.
const POLL: Micros = 20_000;

pub struct SendspinSink {
    shared: Arc<Shared>,
    format: OutputFormat,
    timeline: Timeline,
    pacer: Pacer,
    chunk_frames: u64,
    /// Audio not yet a whole chunk, and the frame it starts at.
    pending: Vec<u8>,
    pending_at: u64,
    started: bool,
    generation: u64,
    /// When the last chunk sent ends.
    last_end: Micros,
}

impl SendspinSink {
    pub(crate) fn new(shared: Arc<Shared>, format: OutputFormat, timeline: Timeline) -> Self {
        let (capacity, delay, lead) = {
            let info = shared.info.lock().unwrap();
            (
                info.buffer_capacity(),
                Micros::from(info.static_delay_ms) * 1_000,
                info.lead(),
            )
        };
        Self {
            shared,
            format,
            timeline,
            pacer: Pacer::new(capacity, delay, lead + BURST_BEYOND_LEAD),
            chunk_frames: u64::from(format.sample_rate / CHUNKS_PER_SECOND),
            pending: Vec::new(),
            pending_at: 0,
            started: false,
            generation: 0,
            last_end: 0,
        }
    }

    fn frame_bytes(&self) -> usize {
        self.format.bytes_per_frame()
    }

    fn start(&mut self) {
        self.generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let format = StreamFormat {
            codec: Codec::Pcm,
            sample_rate: self.format.sample_rate,
            channels: self.format.channels,
            bit_depth: self.format.bits,
        };
        self.shared.send(wire::stream_start(clock::now(), format));
        self.pacer.reset();
        self.started = true;
    }

    fn send_chunk(&mut self, at: u64, audio: &[u8]) -> io::Result<()> {
        let frames = (audio.len() / self.frame_bytes()) as u64;
        let timestamp = self.timeline.instant_of(at);
        let duration = self.timeline.instant_of(at + frames) - timestamp;
        let bytes = (AUDIO_HEADER_BYTES + audio.len()) as u64;
        loop {
            if self.shared.out.is_closed() {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            let now = clock::now();
            match self.pacer.ready_at(now, timestamp, duration, bytes) {
                None => break,
                Some(ready) => {
                    let wait = (ready - now).clamp(1, POLL) as u64;
                    std::thread::sleep(Duration::from_micros(wait));
                }
            }
        }
        if timestamp + duration <= clock::now() {
            // Too late to play; the client would drop it.
            tracing::debug!(client = self.shared.id, "dropping a late chunk");
            return Ok(());
        }
        let _ = self
            .shared
            .out
            .send(Outgoing::Binary(wire::audio_chunk(timestamp, audio)));
        self.pacer.sent(timestamp, duration, bytes);
        self.last_end = timestamp + duration;
        Ok(())
    }

    fn send_pending(&mut self, whole_chunks_only: bool) -> io::Result<()> {
        let chunk_bytes = self.chunk_frames as usize * self.frame_bytes();
        while self.pending.len() >= chunk_bytes || (!whole_chunks_only && !self.pending.is_empty())
        {
            let take = chunk_bytes.min(self.pending.len());
            let audio: Vec<u8> = self.pending.drain(..take).collect();
            let at = self.pending_at;
            self.pending_at += (take / self.frame_bytes()) as u64;
            self.send_chunk(at, &audio)?;
        }
        Ok(())
    }
}

impl Sink for SendspinSink {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        if !self.started {
            self.start();
            self.pending.clear();
        }
        let pending_frames = (self.pending.len() / self.frame_bytes()) as u64;
        if self.pending.is_empty() || self.pending_at + pending_frames != at {
            // A new stretch of audio: what was pending from before cannot follow on.
            self.pending.clear();
            self.pending_at = at;
        }
        self.pending.extend_from_slice(data);
        self.send_pending(true)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.pending.clear();
        if self.started {
            self.shared.send(wire::stream_clear(clock::now()));
            self.pacer.reset();
        }
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        self.pending.clear();
        if self.started {
            self.shared.send(wire::stream_end(clock::now()));
            self.started = false;
            self.pacer.reset();
        }
        Ok(())
    }

    fn drain(&mut self) -> io::Result<()> {
        self.send_pending(false)?;
        if !self.started {
            return Ok(());
        }
        self.started = false;
        self.pacer.reset();
        // End the stream once the client has played what it holds, unless a new stream has
        // begun by then.
        let shared = self.shared.clone();
        let generation = self.generation;
        let delay = (self.last_end - clock::now()).max(0) as u64;
        self.shared.runtime.spawn(async move {
            tokio::time::sleep(Duration::from_micros(delay)).await;
            if shared.generation.load(Ordering::SeqCst) == generation {
                shared.send(wire::stream_end(clock::now()));
            }
        });
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.stop()
    }
}
