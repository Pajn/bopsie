//! A queue playing through the stream engine to a WAV file.

use std::path::{Path, PathBuf};

use bopsie_audio::engine::Settings;
use bopsie_audio::sink::{Sink, WavSink};
use bopsie_audio::{OutputFormat, PcmFormat};
use bopsie_core::media::{ItemFields, MediaType, Metadata, Track};
use bopsie_fixtures::{noise, write_wav};
use bopsie_queues::{Notice, Queue, QueueOption, QueueSession, RepeatMode};
use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::broadcast;

const SESSION: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};
const OUTPUT: OutputFormat = OutputFormat {
    sample_rate: 44_100,
    channels: 2,
    bits: 16,
};

fn track(name: &str) -> Track {
    Track {
        fields: ItemFields {
            item_id: name.to_owned(),
            provider: "library".to_owned(),
            name: name.to_owned(),
            version: String::new(),
            sort_name: name.to_owned(),
            uri: format!("library://track/{name}"),
            external_ids: Vec::new(),
            is_playable: true,
            media_type: MediaType::Track,
            provider_mappings: Vec::new(),
            metadata: Metadata::default(),
            favorite: false,
            position: None,
            date_added: None,
        },
        duration: 0,
        artists: Vec::new(),
        last_played: 0,
        album: None,
        disc_number: 0,
        track_number: 0,
        audio_metadata: None,
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    samples: Vec<Vec<i16>>,
}

/// Tracks a, b, c… of odd lengths, as WAV files of deterministic noise.
fn fixture(lengths: &[usize]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let samples = lengths
        .iter()
        .enumerate()
        .map(|(i, &frames)| {
            let samples = noise(100 + i as u32, frames * 2);
            write_wav(&path(dir.path(), i), 44_100, 2, &samples).unwrap();
            samples
        })
        .collect();
    Fixture { dir, samples }
}

fn path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("{}.wav", (b'a' + index as u8) as char))
}

impl Fixture {
    fn tracks(&self) -> Vec<(Track, PathBuf)> {
        (0..self.samples.len())
            .map(|i| {
                let name = ((b'a' + i as u8) as char).to_string();
                (track(&name), path(self.dir.path(), i))
            })
            .collect()
    }

    fn session(&self) -> QueueSession {
        let out = self.dir.path().join("out.wav");
        let sinks: Vec<(String, Box<dyn Sink>)> = vec![(
            "out".to_owned(),
            Box::new(WavSink::create(&out, OUTPUT).unwrap()),
        )];
        let settings = Settings {
            format: SESSION,
            ffmpeg: None,
            ring_frames: 0,
            max_ahead: bopsie_audio::engine::QUEUE_AHEAD,
            realtime: None,
        };
        QueueSession::start(Queue::default(), settings, sinks)
    }

    fn played(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("out.wav")).unwrap()[44..].to_vec()
    }

    fn expected(&self, order: &[usize]) -> Vec<u8> {
        order
            .iter()
            .flat_map(|&i| self.samples[i].iter().flat_map(|s| s.to_le_bytes()))
            .collect()
    }
}

async fn until_finished(notices: &mut broadcast::Receiver<Notice>) -> Vec<Notice> {
    let mut seen = Vec::new();
    loop {
        let notice = notices.recv().await.unwrap();
        let done = notice == Notice::Finished;
        seen.push(notice);
        if done {
            return seen;
        }
    }
}

#[tokio::test]
async fn a_queue_plays_gaplessly_sample_for_sample() {
    let f = fixture(&[12_345, 777, 5_001]);
    let session = f.session();
    let mut notices = session.notices();
    let mut rng = StdRng::seed_from_u64(1);
    session.update(|queue| {
        (
            (),
            queue.enqueue(f.tracks(), QueueOption::Replace, &mut rng),
        )
    });

    let seen = until_finished(&mut notices).await;
    assert_eq!(
        seen,
        [
            Notice::Started(0),
            Notice::Started(1),
            Notice::Started(2),
            Notice::Finished
        ]
    );
    let queue = session.snapshot();
    assert!(queue.ended);
    assert_eq!(queue.current_index, Some(2));
    session.shutdown();

    let played = f.played();
    let expected = f.expected(&[0, 1, 2]);
    assert_eq!(played.len(), expected.len());
    assert!(
        played == expected,
        "the output differs from the queue back to back"
    );
}

#[tokio::test]
async fn playing_from_an_index_plays_from_there() {
    let f = fixture(&[1_001, 2_002, 3_003]);
    let session = f.session();
    let mut notices = session.notices();
    let mut rng = StdRng::seed_from_u64(1);
    // Staged without playing, then played from the second item.
    session.update(|queue| ((), queue.enqueue(f.tracks(), QueueOption::Add, &mut rng)));
    session.update(|queue| ((), queue.play_index(1)));

    let seen = until_finished(&mut notices).await;
    assert_eq!(
        seen,
        [Notice::Started(1), Notice::Started(2), Notice::Finished]
    );
    session.shutdown();
    assert!(f.played() == f.expected(&[1, 2]));
}

#[tokio::test]
async fn repeat_all_goes_round_until_stopped() {
    let f = fixture(&[501, 303]);
    let session = f.session();
    let mut notices = session.notices();
    let mut rng = StdRng::seed_from_u64(1);
    session.update(|queue| {
        queue.set_repeat(RepeatMode::All);
        (
            (),
            queue.enqueue(f.tracks(), QueueOption::Replace, &mut rng),
        )
    });

    let mut started = Vec::new();
    while started.len() < 5 {
        if let Notice::Started(index) = notices.recv().await.unwrap() {
            started.push(index);
        }
    }
    session.stop();
    assert_eq!(started, [0, 1, 0, 1, 0]);
    session.shutdown();
    // A track has been written in full by the time the next one starts, so four are complete;
    // the fifth may have been cut short by the stop. All of it follows the queue round and
    // round, without gaps.
    let played = f.played();
    assert!(played.len() >= f.expected(&[0, 1, 0, 1]).len());
    let round = f.expected(&[0, 1]);
    assert!(
        played
            .chunks(round.len())
            .all(|chunk| round.starts_with(chunk))
    );
}

#[tokio::test]
async fn a_held_start_plays_when_released_unless_stopped_first() {
    let f = fixture(&[1_001, 2_002]);
    let session = f.session();
    let mut notices = session.notices();
    let mut rng = StdRng::seed_from_u64(1);
    session.update(|queue| ((), queue.enqueue(f.tracks(), QueueOption::Add, &mut rng)));

    // Stopped while its players got ready: the start comes to nothing.
    let ((), held) = session.update_held(|queue| ((), queue.play_index(0)));
    let stale = held.expect("a start is held");
    session.stop();
    session.release(stale, None);

    let ((), held) = session.update_held(|queue| ((), queue.play_index(1)));
    session.release(held.expect("a start is held"), None);

    let seen = until_finished(&mut notices).await;
    assert_eq!(
        seen.into_iter()
            .filter(|n| *n != Notice::Stopped)
            .collect::<Vec<_>>(),
        [Notice::Started(1), Notice::Finished]
    );
    session.shutdown();
    assert!(f.played() == f.expected(&[1]));
}
