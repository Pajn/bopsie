//! Audio that arrives as it is produced, such as a Spotify Connect stream: a writer that the
//! producer pushes samples into, and a [`Decoder`] the session reads them from.
//!
//! The buffer between them is bounded. A producer that is ahead waits for the session, which is
//! how a producer that decodes faster than real time is paced; one that has waited too long, as
//! when no session reads, has its oldest audio dropped instead, so that it never stalls.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::decode::{DecodeError, Decoder, Read};
use crate::format::PcmFormat;

/// How long a read waits for audio before it answers [`Read::Pending`].
const READ_WAIT: Duration = Duration::from_millis(20);
/// How long a write waits for room before it drops the oldest audio.
const WRITE_WAIT: Duration = Duration::from_secs(1);

struct State {
    samples: VecDeque<f32>,
    closed: bool,
}

struct Shared {
    format: PcmFormat,
    /// In samples.
    capacity: usize,
    state: Mutex<State>,
    changed: Condvar,
}

/// A live stream in `format`, holding at most `buffer` of audio between writer and reader.
pub fn live(format: PcmFormat, buffer: Duration) -> (LiveWriter, LiveInput) {
    let frames = (buffer.as_secs_f64() * f64::from(format.sample_rate)).ceil() as usize;
    let shared = Arc::new(Shared {
        format,
        capacity: frames.max(1) * usize::from(format.channels),
        state: Mutex::new(State {
            samples: VecDeque::new(),
            closed: false,
        }),
        changed: Condvar::new(),
    });
    (
        LiveWriter {
            shared: shared.clone(),
        },
        LiveInput { shared },
    )
}

/// The producer's end. Dropping it ends the stream once the reader has what was written.
pub struct LiveWriter {
    shared: Arc<Shared>,
}

impl LiveWriter {
    pub fn format(&self) -> PcmFormat {
        self.shared.format
    }

    /// Appends interleaved samples, waiting for room when the buffer is full. Returns `false`
    /// once the reader is gone.
    pub fn write(&self, samples: &[f32]) -> bool {
        let shared = &*self.shared;
        let mut state = shared.state.lock().unwrap();
        if Arc::strong_count(&self.shared) == 1 {
            return false;
        }
        let room = |state: &State| state.samples.len() + samples.len() <= shared.capacity;
        if !room(&state) {
            state = shared
                .changed
                .wait_timeout_while(state, WRITE_WAIT, |state| !room(state))
                .unwrap()
                .0;
        }
        let held = state.samples.len();
        let excess = (held + samples.len()).saturating_sub(shared.capacity);
        state.samples.drain(..excess.min(held));
        let keep = samples.len().min(shared.capacity);
        state.samples.extend(&samples[samples.len() - keep..]);
        drop(state);
        shared.changed.notify_all();
        true
    }

    /// How much audio is written and not yet taken.
    pub fn buffered(&self) -> Duration {
        let samples = self.shared.state.lock().unwrap().samples.len();
        let frames = samples / usize::from(self.shared.format.channels);
        Duration::from_secs_f64(frames as f64 / f64::from(self.shared.format.sample_rate))
    }

    /// Drops what the reader has not taken yet, as when the producer jumps elsewhere.
    pub fn clear(&self) {
        self.shared.state.lock().unwrap().samples.clear();
        self.shared.changed.notify_all();
    }
}

impl Drop for LiveWriter {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.changed.notify_all();
    }
}

/// The session's end: a [`Decoder`] that cannot seek.
pub struct LiveInput {
    shared: Arc<Shared>,
}

impl Decoder for LiveInput {
    fn format(&self) -> PcmFormat {
        self.shared.format
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError> {
        let shared = &*self.shared;
        let mut state = shared
            .changed
            .wait_timeout_while(shared.state.lock().unwrap(), READ_WAIT, |state| {
                state.samples.is_empty() && !state.closed
            })
            .unwrap()
            .0;
        if state.samples.is_empty() {
            return Ok(if state.closed {
                Read::End
            } else {
                Read::Pending
            });
        }
        // Whole frames only.
        let channels = usize::from(shared.format.channels);
        let take = state.samples.len() - state.samples.len() % channels;
        if take == 0 {
            return Ok(Read::Pending);
        }
        out.extend(state.samples.drain(..take));
        drop(state);
        shared.changed.notify_all();
        Ok(Read::Data)
    }

    fn seek(&mut self, _frame: u64) -> Result<u64, DecodeError> {
        Err(DecodeError::Unsupported(
            "a live stream cannot seek".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEREO: PcmFormat = PcmFormat {
        sample_rate: 1_000,
        channels: 2,
    };

    #[test]
    fn what_is_written_is_read_and_then_it_ends() {
        let (writer, mut input) = live(STEREO, Duration::from_secs(1));
        let mut out = Vec::new();
        assert_eq!(input.read(&mut out).unwrap(), Read::Pending);
        assert!(writer.write(&[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(writer.buffered(), Duration::from_millis(2));
        assert_eq!(input.read(&mut out).unwrap(), Read::Data);
        assert_eq!(writer.buffered(), Duration::ZERO);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
        drop(writer);
        assert_eq!(input.read(&mut out).unwrap(), Read::End);
        assert!(input.seek(0).is_err());
    }

    #[test]
    fn a_full_buffer_paces_the_writer_then_drops_the_oldest() {
        // Room for 10 frames, 20 samples.
        let (writer, mut input) = live(STEREO, Duration::from_millis(10));
        assert!(writer.write(&[0.0; 20]));
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut out = Vec::new();
            input.read(&mut out).unwrap();
            (out.len(), input)
        });
        // Waits until the reader takes what is there.
        let started = std::time::Instant::now();
        assert!(writer.write(&[1.0; 4]));
        assert!(started.elapsed() >= Duration::from_millis(40));
        let (taken, mut input) = reader.join().unwrap();
        assert_eq!(taken, 20);

        // With nobody reading, the oldest audio goes after the wait.
        let samples: Vec<f32> = (0..20).map(|n| n as f32).collect();
        writer.write(&samples);
        writer.write(&[100.0, 101.0]);
        let mut out = Vec::new();
        input.read(&mut out).unwrap();
        assert_eq!(out.len(), 20);
        assert_eq!(&out[out.len() - 2..], [100.0, 101.0]);
    }

    #[test]
    fn a_writer_learns_the_reader_left() {
        let (writer, input) = live(STEREO, Duration::from_secs(1));
        drop(input);
        assert!(!writer.write(&[0.0, 0.0]));
    }
}
