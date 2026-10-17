//! Decoders: compressed audio in, interleaved `f32` PCM out.
//!
//! Symphonia decodes in process. ffmpeg, when installed, runs as a subprocess for what
//! Symphonia cannot decode gaplessly or at all: AAC and ALAC in MP4 containers, and Opus. It is
//! asked for the session's format directly, so its output needs no further conversion.

use std::fs::File;
use std::io::{BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::format::PcmFormat;
use crate::loudness::Loudness;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unsupported(String),
    #[error("cannot decode: {0}")]
    Corrupt(String),
}

impl From<SymphoniaError> for DecodeError {
    fn from(error: SymphoniaError) -> Self {
        match error {
            SymphoniaError::IoError(error) => DecodeError::Io(error),
            SymphoniaError::Unsupported(what) => DecodeError::Unsupported(what.to_owned()),
            other => DecodeError::Corrupt(other.to_string()),
        }
    }
}

/// What a [`Decoder::read`] gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// Samples were appended.
    Data,
    /// Nothing yet, from a source that produces as it goes; ask again.
    Pending,
    /// The end of the stream; nothing was appended.
    End,
}

pub trait Decoder: Send {
    /// The format of what [`Decoder::read`] produces.
    fn format(&self) -> PcmFormat;
    /// Appends the next decoded interleaved samples to `out`. A live source waits a little for
    /// them, and answers [`Read::Pending`] rather than hold up the session for longer.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError>;
    /// Moves to `frame` frames from the start, exactly. Returns the frame it moved to, which
    /// is earlier only when the stream is shorter.
    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError>;
    /// How loud the audio is, for a source that knows once it is open, such as a streaming
    /// service's file.
    fn loudness(&self) -> Option<Loudness> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderKind {
    Symphonia,
    Ffmpeg,
}

/// Which decoder plays a file, from its extension. Files that want ffmpeg fall back to
/// Symphonia without it, which plays MP4 AAC and ALAC but not gaplessly. `None` means nothing
/// here can play the file.
pub fn choose(path: &Path, ffmpeg_available: bool) -> Option<DecoderKind> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    match extension.as_str() {
        "flac" | "wav" | "mp3" | "ogg" | "oga" | "aif" | "aiff" | "aifc" => {
            Some(DecoderKind::Symphonia)
        }
        "m4a" | "mp4" | "aac" | "alac" if ffmpeg_available => Some(DecoderKind::Ffmpeg),
        "m4a" | "mp4" | "aac" | "alac" => Some(DecoderKind::Symphonia),
        "opus" | "wv" | "ape" if ffmpeg_available => Some(DecoderKind::Ffmpeg),
        _ => None,
    }
}

pub struct SymphoniaDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    format: PcmFormat,
    /// Frames still to drop after an accurate seek landed early.
    skip: u64,
}

impl SymphoniaDecoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let extension = path.extension().and_then(|e| e.to_str());
        Self::from_source(Box::new(File::open(path)?), extension)
    }

    /// Decodes what `source` holds, such as a stream fetched as it is read, which should seek
    /// for seeks to work. `extension` hints at its format.
    pub fn from_source(
        source: Box<dyn symphonia::core::io::MediaSource>,
        extension: Option<&str>,
    ) -> Result<Self, DecodeError> {
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let source = MediaSourceStream::new(source, Default::default());
        let reader = symphonia::default::get_probe().probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::Unsupported("no audio track".to_owned()))?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
            return Err(DecodeError::Unsupported("no audio codec".to_owned()));
        };
        let format = PcmFormat {
            sample_rate: params
                .sample_rate
                .ok_or_else(|| DecodeError::Unsupported("unknown sample rate".to_owned()))?,
            channels: params
                .channels
                .as_ref()
                .map_or(2, |channels| channels.count() as u16),
        };
        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(true))?;
        Ok(Self {
            reader,
            decoder,
            track_id,
            format,
            skip: 0,
        })
    }
}

impl Decoder for SymphoniaDecoder {
    fn format(&self) -> PcmFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(Read::End),
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(Read::End);
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A damaged packet is skipped, as players do.
                Err(SymphoniaError::DecodeError(error)) => {
                    tracing::warn!(error, "skipping an undecodable packet");
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let mut samples = Vec::new();
            decoded.copy_to_vec_interleaved(&mut samples);
            let channels = usize::from(self.format.channels);
            let frames = (samples.len() / channels) as u64;
            if self.skip >= frames {
                self.skip -= frames;
                continue;
            }
            let start = (self.skip as usize) * channels;
            self.skip = 0;
            out.extend_from_slice(&samples[start..]);
            return Ok(Read::Data);
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let nanos = frame as i128 * 1_000_000_000 / i128::from(self.format.sample_rate);
        let time = Time::try_from_nanos_i128(nanos)
            .ok_or_else(|| DecodeError::Unsupported("seek position out of range".to_owned()))?;
        let seeked = match self.reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        ) {
            Ok(seeked) => seeked,
            Err(SymphoniaError::SeekError(_)) => return Ok(frame),
            Err(error) => return Err(error.into()),
        };
        self.decoder.reset();
        // Timestamps are in frames for every audio format Symphonia reads.
        let required = seeked.required_ts.get().max(0) as u64;
        let actual = seeked.actual_ts.get().max(0) as u64;
        self.skip = required.saturating_sub(actual);
        Ok(required)
    }
}

/// The ffmpeg binary to use, if one is installed: `path` if given, else `ffmpeg` on `PATH`.
pub fn find_ffmpeg(path: Option<&Path>) -> Option<PathBuf> {
    let candidate = path.map_or_else(|| PathBuf::from("ffmpeg"), Path::to_path_buf);
    let works = Command::new(&candidate)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    works.then_some(candidate)
}

pub struct FfmpegDecoder {
    ffmpeg: PathBuf,
    path: PathBuf,
    format: PcmFormat,
    /// A stream, such as a URL, that seeks by jumping rather than decoding up to the position.
    stream: bool,
    child: Child,
    stdout: BufReader<ChildStdout>,
}

/// Bytes read from ffmpeg at a time: about 50 ms of stereo audio at 48 kHz.
const FFMPEG_CHUNK: usize = 4096 * 4;

impl FfmpegDecoder {
    /// Decodes `path` with `ffmpeg`, converted to `format`.
    pub fn open(ffmpeg: &Path, path: &Path, format: PcmFormat) -> Result<Self, DecodeError> {
        let (child, stdout) = Self::spawn(ffmpeg, path, format, 0, false)?;
        Ok(Self {
            ffmpeg: ffmpeg.to_owned(),
            path: path.to_owned(),
            format,
            stream: false,
            child,
            stdout,
        })
    }

    /// Decodes the stream at `url`, such as a file a server serves, from `start_frame`,
    /// converted to `format`. It seeks by jumping to the position, to within a packet, rather
    /// than by reading everything before it, which for a long file over a network would take
    /// minutes.
    pub fn open_stream(
        ffmpeg: &Path,
        url: &str,
        format: PcmFormat,
        start_frame: u64,
    ) -> Result<Self, DecodeError> {
        let path = PathBuf::from(url);
        let (child, stdout) = Self::spawn(ffmpeg, &path, format, start_frame, true)?;
        Ok(Self {
            ffmpeg: ffmpeg.to_owned(),
            path,
            format,
            stream: true,
            child,
            stdout,
        })
    }

    fn spawn(
        ffmpeg: &Path,
        path: &Path,
        format: PcmFormat,
        start_frame: u64,
        stream: bool,
    ) -> Result<(Child, BufReader<ChildStdout>), DecodeError> {
        let mut command = Command::new(ffmpeg);
        command.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
        let seconds = format!("{:.9}", start_frame as f64 / f64::from(format.sample_rate));
        if stream && start_frame > 0 {
            // Before -i, ffmpeg jumps to the position in the input.
            command.args(["-ss", &seconds]);
        }
        command.arg("-i").arg(path);
        if !stream && start_frame > 0 {
            // After -i, ffmpeg decodes and discards up to the position: exact, not keyframe.
            command.args(["-ss", &seconds]);
        }
        command.args([
            "-map",
            "0:a:0",
            "-f",
            "f32le",
            "-ac",
            &format.channels.to_string(),
            "-ar",
            &format.sample_rate.to_string(),
            "pipe:1",
        ]);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        Ok((child, BufReader::with_capacity(FFMPEG_CHUNK, stdout)))
    }
}

impl Decoder for FfmpegDecoder {
    fn format(&self) -> PcmFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError> {
        let mut bytes = vec![0u8; FFMPEG_CHUNK];
        let mut filled = 0;
        // Fill whole samples; a read can end mid-sample.
        while filled < bytes.len() {
            let read = self.stdout.read(&mut bytes[filled..])?;
            if read == 0 {
                break;
            }
            filled += read;
            if filled % 4 == 0 {
                break;
            }
        }
        if filled == 0 {
            let status = self.child.wait()?;
            if !status.success() {
                return Err(DecodeError::Corrupt(format!("ffmpeg exited with {status}")));
            }
            return Ok(Read::End);
        }
        out.extend(
            bytes[..filled - filled % 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_le_bytes(b)),
        );
        Ok(Read::Data)
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (child, stdout) =
            Self::spawn(&self.ffmpeg, &self.path, self.format, frame, self.stream)?;
        self.child = child;
        self.stdout = stdout;
        Ok(frame)
    }
}

impl Drop for FfmpegDecoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_choice() {
        let choose = |name: &str, ffmpeg| choose(Path::new(name), ffmpeg);
        assert_eq!(choose("a.FLAC", false), Some(DecoderKind::Symphonia));
        assert_eq!(choose("a.mp3", true), Some(DecoderKind::Symphonia));
        assert_eq!(choose("a.m4a", true), Some(DecoderKind::Ffmpeg));
        assert_eq!(choose("a.m4a", false), Some(DecoderKind::Symphonia));
        assert_eq!(choose("a.opus", true), Some(DecoderKind::Ffmpeg));
        assert_eq!(choose("a.opus", false), None);
        assert_eq!(choose("a.txt", true), None);
        assert_eq!(choose("noextension", true), None);
    }
}
