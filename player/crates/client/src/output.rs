//! Turns a stream's audio chunks into sound: decodes them and hands them to `sendspin`'s
//! `SyncedPlayer`, which plays each at the time the server stamped on it.
//!
//! The output device is opened on the first chunk of a stream and released when the stream ends,
//! so an idle player does not hold the device. A device error, such as the device going away,
//! drops the output and the next chunk opens the current default device.

use std::sync::Arc;

use parking_lot::Mutex;
use sendspin::audio::decode::{Decoder, PcmDecoder, PcmEndian};
use sendspin::audio::{AudioBuffer, AudioFormat, Codec, SyncedPlayer, SyncedPlayerConfig};
use sendspin::protocol::client::AudioChunk;
use sendspin::protocol::messages::{AudioFormatSpec, StreamPlayerConfig};
use sendspin::sync::ClockSync;

/// What the player can decode, in order of preference. The server sends PCM.
pub(crate) fn supported_formats() -> Vec<AudioFormatSpec> {
    [24, 16]
        .into_iter()
        .map(|bit_depth| AudioFormatSpec {
            codec: "pcm".to_owned(),
            channels: 2,
            sample_rate: 48_000,
            bit_depth,
        })
        .collect()
}

pub(crate) struct Output {
    clock: Arc<Mutex<ClockSync>>,
    stream: Option<(AudioFormat, PcmDecoder)>,
    player: Option<SyncedPlayer>,
    volume: u8,
    muted: bool,
    static_delay_ms: u16,
}

impl Output {
    pub(crate) fn new(clock: Arc<Mutex<ClockSync>>, volume: u8, muted: bool) -> Self {
        Self {
            clock,
            stream: None,
            player: None,
            volume,
            muted,
            static_delay_ms: 0,
        }
    }

    /// A new stream, or a new format for the current one.
    pub(crate) fn start(&mut self, config: &StreamPlayerConfig) {
        self.stream = None;
        if config.codec != "pcm" || !matches!(config.bit_depth, 16 | 24) {
            tracing::warn!(
                codec = config.codec,
                bit_depth = config.bit_depth,
                "the server started a stream in a format this player did not offer"
            );
            self.player = None;
            return;
        }
        let format = AudioFormat {
            codec: Codec::Pcm,
            sample_rate: config.sample_rate,
            channels: config.channels,
            bit_depth: config.bit_depth,
            codec_header: None,
        };
        match &self.player {
            Some(player) if player.format() == &format => player.clear(),
            _ => self.player = None,
        }
        let decoder = PcmDecoder::with_endian(format.bit_depth, PcmEndian::Little);
        self.stream = Some((format, decoder));
    }

    /// Drops what is buffered, for a seek or a skip; the stream goes on.
    pub(crate) fn clear(&mut self) {
        if let Some(player) = &self.player {
            player.clear();
        }
    }

    /// The stream ended: stop and release the device.
    pub(crate) fn end(&mut self) {
        self.stream = None;
        self.player = None;
    }

    pub(crate) fn push(&mut self, chunk: AudioChunk) {
        let Some((format, decoder)) = &self.stream else {
            return;
        };
        let samples = match decoder.decode(&chunk.data) {
            Ok(samples) => samples,
            Err(error) => {
                tracing::warn!(%error, "could not decode an audio chunk");
                return;
            }
        };
        if let Some(error) = self.player.as_ref().and_then(SyncedPlayer::take_error) {
            tracing::warn!(error, "the audio output failed; reopening it");
            self.player = None;
        }
        if self.player.is_none() {
            let config = SyncedPlayerConfig {
                volume: self.volume,
                muted: self.muted,
                ..SyncedPlayerConfig::default()
            };
            match SyncedPlayer::new(format.clone(), Arc::clone(&self.clock), config) {
                Ok(player) => {
                    player.set_static_delay(self.static_delay_ms);
                    self.player = Some(player);
                }
                Err(error) => {
                    tracing::warn!(%error, "could not open the audio output");
                    return;
                }
            }
        }
        if let Some(player) = &self.player {
            player.enqueue(AudioBuffer {
                timestamp: chunk.timestamp,
                samples,
                format: format.clone(),
            });
        }
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        if let Some(player) = &self.player {
            player.set_volume(self.volume);
        }
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(player) = &self.player {
            player.set_mute(muted);
        }
    }

    pub(crate) fn set_static_delay(&mut self, delay_ms: u16) {
        self.static_delay_ms = delay_ms;
        if let Some(player) = &self.player {
            player.set_static_delay(delay_ms);
        }
    }
}
