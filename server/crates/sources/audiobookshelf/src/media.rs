//! A book as the engine plays it: its files one after another, as one stream, from the server.
//!
//! Opening a book asks the server to play it, which answers with the book's files in order;
//! they are decoded through ffmpeg over HTTP, on a thread of their own. A position in the book
//! falls in the file whose span holds it, the spans laid end to end by the files' durations,
//! as Audiobookshelf's own apps lay them.

use std::path::{Path, PathBuf};

use bopsie_audio::PcmFormat;
use bopsie_audio::buffered::Buffered;
use bopsie_audio::decode::{DecodeError, Decoder, FfmpegDecoder, Read};
use bopsie_audio::engine::Media;
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::account::Account;
use crate::progress::Sessions;

/// What the server is told can be played as it is: everything ffmpeg decodes.
const SUPPORTED: &[&str] = &[
    "audio/flac",
    "audio/mpeg",
    "audio/mp4",
    "audio/ogg",
    "audio/aac",
    "audio/webm",
    "audio/wav",
    "audio/x-m4b",
];

/// One of a book's files: where to fetch it, and how long it plays.
#[derive(Debug, Clone, PartialEq)]
pub struct BookFile {
    pub url: String,
    pub seconds: f64,
}

/// What the server answered asking to play a book.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub files: Vec<BookFile>,
}

/// Asks the server to play book `id`, as the client Bopsie: a session, and the book's files.
pub async fn start_session(account: &Account, id: &str) -> Result<Session, DecodeError> {
    let unavailable = |why: String| DecodeError::Unsupported(format!("audiobook {id}: {why}"));
    let answer = account
        .post(
            &format!("/api/items/{id}/play"),
            json!({
                "deviceInfo": {"clientName": "Bopsie", "deviceName": "Bopsie"},
                "mediaPlayer": "bopsie",
                "supportedMimeTypes": SUPPORTED,
                "forceDirectPlay": true,
            }),
        )
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    session(account, &answer).ok_or_else(|| unavailable("no files to play".to_owned()))
}

fn session(account: &Account, answer: &Value) -> Option<Session> {
    let id = answer["id"].as_str()?.to_owned();
    let mut tracks: Vec<&Value> = answer["audioTracks"].as_array()?.iter().collect();
    tracks.sort_by_key(|track| track["index"].as_u64().unwrap_or(0));
    let files: Vec<BookFile> = tracks
        .into_iter()
        .filter_map(|track| {
            Some(BookFile {
                url: account.signed_url(track["contentUrl"].as_str()?).ok()?,
                seconds: track["duration"].as_f64()?,
            })
        })
        .collect();
    (!files.is_empty()).then_some(Session { id, files })
}

/// Book `id` of `account`'s server, opened as the engine opens it.
pub struct BookMedia {
    account: Account,
    id: String,
    ffmpeg: PathBuf,
    runtime: Handle,
    /// Where the session it opens is kept, for its progress to be synced through.
    sessions: Sessions,
}

impl BookMedia {
    pub fn new(
        account: Account,
        id: String,
        ffmpeg: PathBuf,
        runtime: Handle,
        sessions: Sessions,
    ) -> Self {
        Self {
            account,
            id,
            ffmpeg,
            runtime,
            sessions,
        }
    }
}

impl Media for BookMedia {
    fn open(&self, format: PcmFormat) -> Result<Box<dyn Decoder>, DecodeError> {
        let (account, id, ffmpeg, runtime, sessions) = (
            self.account.clone(),
            self.id.clone(),
            self.ffmpeg.clone(),
            self.runtime.clone(),
            self.sessions.clone(),
        );
        Ok(Box::new(Buffered::start(format, move || {
            let session = runtime.block_on(start_session(&account, &id))?;
            let seconds = session.files.iter().map(|file| file.seconds).sum();
            sessions.opened(&id, &session.id, seconds);
            tracing::debug!(
                book = id,
                session = session.id,
                files = session.files.len(),
                "playing from Audiobookshelf"
            );
            Ok(Box::new(BookDecoder::new(ffmpeg, session.files, format)) as Box<dyn Decoder>)
        })))
    }
}

/// A book's files decoded one after another, as one stream.
pub struct BookDecoder {
    ffmpeg: PathBuf,
    files: Vec<BookFile>,
    format: PcmFormat,
    /// The file playing, and its decoder; `None` before the first read and after the last file.
    current: Option<(usize, FfmpegDecoder)>,
    /// The file the next read opens, from where, in frames, when none is open.
    next: Option<(usize, u64)>,
}

impl BookDecoder {
    pub fn new(ffmpeg: PathBuf, files: Vec<BookFile>, format: PcmFormat) -> Self {
        Self {
            ffmpeg,
            files,
            format,
            current: None,
            next: Some((0, 0)),
        }
    }

    /// The file `frame` falls in, and how far into it, in frames. Past the end, the end of the
    /// last file.
    fn locate(&self, frame: u64) -> (usize, u64) {
        let rate = f64::from(self.format.sample_rate);
        let mut start = 0.0;
        for (index, file) in self.files.iter().enumerate() {
            let end = start + file.seconds;
            if (frame as f64) < end * rate || index + 1 == self.files.len() {
                let within = (frame as f64 - start * rate).max(0.0);
                return (index, within.round() as u64);
            }
            start = end;
        }
        (0, 0)
    }

    fn open(&mut self, index: usize, frame: u64) -> Result<(), DecodeError> {
        let decoder = FfmpegDecoder::open_stream(
            Path::new(&self.ffmpeg),
            &self.files[index].url,
            self.format,
            frame,
        )?;
        self.current = Some((index, decoder));
        Ok(())
    }
}

impl Decoder for BookDecoder {
    fn format(&self) -> PcmFormat {
        self.format
    }

    fn read(&mut self, out: &mut Vec<f32>) -> Result<Read, DecodeError> {
        loop {
            if self.current.is_none() {
                let Some((index, frame)) = self.next.take() else {
                    return Ok(Read::End);
                };
                self.open(index, frame)?;
            }
            let (index, decoder) = self.current.as_mut().expect("opened above");
            match decoder.read(out)? {
                Read::End => {
                    let following = *index + 1;
                    self.current = None;
                    self.next = (following < self.files.len()).then_some((following, 0));
                }
                read => return Ok(read),
            }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        let (index, within) = self.locate(frame);
        self.current = None;
        self.next = Some((index, within));
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoder(seconds: &[f64]) -> BookDecoder {
        let files = seconds
            .iter()
            .map(|&seconds| BookFile {
                url: String::new(),
                seconds,
            })
            .collect();
        BookDecoder::new(
            PathBuf::from("ffmpeg"),
            files,
            PcmFormat {
                sample_rate: 1_000,
                channels: 2,
            },
        )
    }

    #[test]
    fn a_position_falls_in_the_file_whose_span_holds_it() {
        let book = decoder(&[10.0, 2.5, 30.0]);
        assert_eq!(book.locate(0), (0, 0));
        assert_eq!(book.locate(9_999), (0, 9_999));
        assert_eq!(book.locate(10_000), (1, 0));
        assert_eq!(book.locate(12_499), (1, 2_499));
        assert_eq!(book.locate(12_500), (2, 0));
        // Past the end: the last file, as far as asked; ffmpeg ends it at once.
        assert_eq!(book.locate(50_000), (2, 37_500));
    }
}
