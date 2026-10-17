//! A decoder that runs on a thread of its own, keeping a few seconds decoded ahead, for audio
//! that comes over a network: the engine waits for it as for live input rather than for the
//! network, and a decoder that is slow to open opens there too.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::PcmFormat;
use crate::decode::{DecodeError, Decoder, Read};
use crate::loudness::Loudness;

/// How much decoded audio is kept ahead of what is read.
const AHEAD: Duration = Duration::from_secs(5);
/// How long a read waits for audio before it answers that there is none yet.
const READ_WAIT: Duration = Duration::from_millis(20);

#[derive(Default)]
struct State {
    samples: VecDeque<f32>,
    ended: bool,
    error: Option<String>,
    /// A seek asked for and not yet made.
    seek: Option<u64>,
    /// The reader let it go.
    closed: bool,
    loudness: Option<Loudness>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

/// Decodes what `open` gives on a thread of its own, in `format`, which is what `open`'s
/// decoder must produce.
pub struct Buffered {
    format: PcmFormat,
    shared: Arc<Shared>,
}

impl Buffered {
    pub fn start(
        format: PcmFormat,
        open: impl FnOnce() -> Result<Box<dyn Decoder>, DecodeError> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
        });
        let decoding = shared.clone();
        std::thread::Builder::new()
            .name("buffered-decoder".to_owned())
            .spawn(move || {
                let result = open().and_then(|decoder| decode(&decoding, format, decoder));
                let mut state = decoding.state.lock().unwrap();
                if let Err(error) = result {
                    state.error = Some(error.to_string());
                }
                drop(state);
                decoding.changed.notify_all();
            })
            .expect("spawning a thread");
        Self { format, shared }
    }
}

fn decode(
    shared: &Shared,
    format: PcmFormat,
    mut decoder: Box<dyn Decoder>,
) -> Result<(), DecodeError> {
    if decoder.format() != format {
        return Err(DecodeError::Unsupported(format!(
            "decoded as {:?}, not {format:?}",
            decoder.format()
        )));
    }
    shared.state.lock().unwrap().loudness = decoder.loudness();
    let room = (AHEAD.as_secs_f64() * f64::from(format.sample_rate)) as usize
        * usize::from(format.channels);
    let mut out = Vec::new();
    loop {
        let seek = {
            let mut state = shared
                .changed
                .wait_while(shared.state.lock().unwrap(), |state| {
                    !state.closed && state.seek.is_none() && state.samples.len() >= room
                })
                .unwrap();
            if state.closed {
                return Ok(());
            }
            state.seek.take()
        };
        if let Some(frame) = seek {
            decoder.seek(frame)?;
            continue;
        }
        out.clear();
        let read = decoder.read(&mut out)?;
        let mut state = shared.state.lock().unwrap();
        // Decoded before a seek that came meanwhile: not wanted.
        if state.seek.is_none() {
            state.samples.extend(&out);
        }
        drop(state);
        shared.changed.notify_all();
        if read == Read::End {
            // Done, unless the reader seeks back into it.
            let mut state = shared.state.lock().unwrap();
            state.ended = true;
            shared.changed.notify_all();
            let state = shared
                .changed
                .wait_while(state, |state| !state.closed && state.seek.is_none())
                .unwrap();
            if state.closed {
                return Ok(());
            }
        }
    }
}

impl Decoder for Buffered {
    fn format(&self) -> PcmFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError> {
        let shared = &*self.shared;
        let mut state = shared
            .changed
            .wait_timeout_while(shared.state.lock().unwrap(), READ_WAIT, |state| {
                state.samples.is_empty() && !state.ended && state.error.is_none()
            })
            .unwrap()
            .0;
        if state.samples.is_empty() {
            if let Some(error) = &state.error {
                return Err(DecodeError::Corrupt(error.clone()));
            }
            return Ok(if state.ended {
                Read::End
            } else {
                Read::Pending
            });
        }
        let take = state.samples.len().min(8_192);
        out.extend(state.samples.drain(..take));
        drop(state);
        shared.changed.notify_all();
        Ok(Read::Data)
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(error) = &state.error {
            return Err(DecodeError::Corrupt(error.clone()));
        }
        state.samples.clear();
        state.ended = false;
        state.seek = Some(frame);
        drop(state);
        self.shared.changed.notify_all();
        Ok(frame)
    }

    fn loudness(&self) -> Option<Loudness> {
        self.shared.state.lock().unwrap().loudness
    }
}

impl Drop for Buffered {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.changed.notify_all();
    }
}
