//! The engine playing files to WAV sinks: tracks join without a gap, sample for sample.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bopsie_audio::engine::{self, Command, Event, Settings, TrackRequest, TrackSource};
use bopsie_audio::sink::{Sink, WavSink};
use bopsie_audio::{OutputFormat, PcmFormat};
use bopsie_fixtures::{noise, write_wav};

struct Tracks(VecDeque<TrackRequest>);

impl TrackSource for Tracks {
    fn next(&mut self) -> Option<TrackRequest> {
        self.0.pop_front()
    }
}

fn request(id: &str, path: &Path) -> TrackRequest {
    TrackRequest {
        id: id.to_owned(),
        path: path.to_owned(),
        start: Duration::ZERO,
    }
}

const SESSION: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};

fn output(bits: u16) -> OutputFormat {
    OutputFormat {
        sample_rate: SESSION.sample_rate,
        channels: SESSION.channels,
        bits,
    }
}

/// Plays `tracks` to WAV sinks of the given bit depths, until there is nothing left. Returns
/// the events and each sink's audio data.
fn play(dir: &Path, tracks: Vec<TrackRequest>, bits: &[u16]) -> (Vec<Event>, Vec<Vec<u8>>) {
    let paths: Vec<PathBuf> = bits
        .iter()
        .enumerate()
        .map(|(i, _)| dir.join(format!("out{i}.wav")))
        .collect();
    let sinks: Vec<(String, Box<dyn Sink>)> = paths
        .iter()
        .zip(bits)
        .map(|(path, &bits)| {
            let sink = Box::new(WavSink::create(path, output(bits)).unwrap()) as Box<dyn Sink>;
            (path.display().to_string(), sink)
        })
        .collect();
    let settings = Settings {
        format: SESSION,
        ffmpeg: None,
        ring_frames: 44_100,
        realtime: None,
    };
    let mut handle = engine::spawn(settings, Box::new(Tracks(tracks.into())), sinks);
    handle.send(Command::Restart { epoch: 1 });
    let mut events = Vec::new();
    loop {
        let event = handle
            .events
            .blocking_recv()
            .expect("the engine runs")
            .event;
        let idle = matches!(event, Event::Idle { .. });
        events.push(event);
        if idle {
            break;
        }
    }
    handle.shutdown();
    let data = paths
        .iter()
        .map(|path| std::fs::read(path).unwrap()[44..].to_vec())
        .collect();
    (events, data)
}

fn bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[test]
fn tracks_join_sample_for_sample() {
    let dir = tempfile::tempdir().unwrap();
    // Odd lengths, so nothing lines up with a packet or chunk size by accident.
    let a = noise(1, 12_345 * 2);
    let b = noise(2, 777 * 2);
    let mono = noise(3, 5_001);
    write_wav(&dir.path().join("a.wav"), 44_100, 2, &a).unwrap();
    write_wav(&dir.path().join("b.wav"), 44_100, 2, &b).unwrap();
    write_wav(&dir.path().join("c.wav"), 44_100, 1, &mono).unwrap();

    let (events, data) = play(
        dir.path(),
        vec![
            request("a", &dir.path().join("a.wav")),
            request("b", &dir.path().join("b.wav")),
            request("c", &dir.path().join("c.wav")),
        ],
        &[16],
    );

    let mut expected = bytes(&a);
    expected.extend(bytes(&b));
    // Mono plays on both channels.
    expected.extend(bytes(
        &mono.iter().flat_map(|&s| [s, s]).collect::<Vec<_>>(),
    ));
    assert_eq!(data[0].len(), expected.len());
    assert!(
        data[0] == expected,
        "the output differs from the tracks back to back"
    );

    let started: Vec<(String, u64)> = events
        .iter()
        .filter_map(|e| match e {
            Event::TrackStarted { id, at, .. } => Some((id.clone(), *at)),
            _ => None,
        })
        .collect();
    assert_eq!(
        started,
        [
            ("a".to_owned(), 0),
            ("b".to_owned(), 12_345),
            ("c".to_owned(), 13_122)
        ]
    );
    assert_eq!(events.last(), Some(&Event::Idle { at: 18_123 }));
}

#[test]
fn other_rates_are_converted_to_the_session_rate_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let fast = noise(4, 4_801 * 2);
    let a = noise(5, 1_000 * 2);
    write_wav(&dir.path().join("fast.wav"), 48_000, 2, &fast).unwrap();
    write_wav(&dir.path().join("a.wav"), 44_100, 2, &a).unwrap();

    let (events, data) = play(
        dir.path(),
        vec![
            request("fast", &dir.path().join("fast.wav")),
            request("a", &dir.path().join("a.wav")),
        ],
        &[16, 24],
    );
    // 4801 frames at 48 kHz are 4411.0 frames at 44.1 kHz, rounded.
    let converted = (4_801f64 * 44_100.0 / 48_000.0).round() as u64;
    assert!(events.contains(&Event::TrackStarted {
        id: "a".to_owned(),
        at: converted,
        start: Duration::ZERO,
    }));
    assert_eq!(data[0].len() as u64, (converted + 1_000) * 4);
    assert_eq!(data[1].len() as u64, (converted + 1_000) * 6, "24-bit");
    // The track at the session rate follows untouched.
    assert!(data[0].ends_with(&bytes(&a)));
}

#[test]
fn a_track_can_start_partway_through() {
    let dir = tempfile::tempdir().unwrap();
    let a = noise(6, 44_100 * 2);
    write_wav(&dir.path().join("a.wav"), 44_100, 2, &a).unwrap();
    let mut halfway = request("a", &dir.path().join("a.wav"));
    halfway.start = Duration::from_millis(500);

    let (_, data) = play(dir.path(), vec![halfway], &[16]);
    assert!(
        data[0] == bytes(&a[22_050 * 2..]),
        "starts exactly at 0.5 s"
    );
}

#[test]
fn unplayable_tracks_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let a = noise(7, 100 * 2);
    write_wav(&dir.path().join("a.wav"), 44_100, 2, &a).unwrap();
    std::fs::write(dir.path().join("broken.wav"), b"not audio").unwrap();

    let (events, data) = play(
        dir.path(),
        vec![
            request("missing", &dir.path().join("missing.flac")),
            request("broken", &dir.path().join("broken.wav")),
            request("a", &dir.path().join("a.wav")),
        ],
        &[16],
    );
    let failed: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::TrackFailed { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(failed, ["missing", "broken"]);
    assert!(data[0] == bytes(&a));
}

#[test]
fn stopping_and_restarting() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(&dir.path().join("a.wav"), 44_100, 2, &noise(8, 100 * 2)).unwrap();
    let settings = Settings {
        format: SESSION,
        ffmpeg: None,
        ring_frames: 0,
        realtime: None,
    };
    let source = Tracks(vec![request("a", &dir.path().join("a.wav")); 1].into());
    let mut handle = engine::spawn(settings, Box::new(source), Vec::new());
    handle.send(Command::Restart { epoch: 1 });
    // With nothing left the engine idles and waits for commands.
    loop {
        if let Event::Idle { .. } = handle.events.blocking_recv().unwrap().event {
            break;
        }
    }
    handle.send(Command::Stop { epoch: 2 });
    let stopped = handle.events.blocking_recv().unwrap();
    assert_eq!(stopped.epoch, 2, "events carry the epoch of the last stop");
    assert!(matches!(stopped.event, Event::Idle { .. }));
    handle.send(Command::Restart { epoch: 3 });
    // The source is empty, so a restart idles again at once.
    let restarted = handle.events.blocking_recv().unwrap();
    assert_eq!(restarted.epoch, 3);
    assert!(matches!(restarted.event, Event::Idle { .. }));
    handle.shutdown();
}

mod realtime {
    use std::sync::{Arc, Mutex};

    use bopsie_audio::engine::Realtime;
    use bopsie_audio::timeline::{Micros, Timeline};

    use super::*;

    /// The clock stands still, so frame positions are exact.
    fn clock() -> Micros {
        1_000_000
    }

    const LEAD: Micros = 300_000;

    fn settings() -> Settings {
        Settings {
            format: SESSION,
            ffmpeg: None,
            ring_frames: 44_100 * 10,
            realtime: Some(Realtime {
                timeline: Timeline::new(44_100, 0),
                lead: LEAD,
                clock,
            }),
        }
    }

    /// Where each write landed, and how many frames it held.
    type Writes = Arc<Mutex<Vec<(u64, usize)>>>;

    /// Records where each write lands, slowly, so the session is still playing when another
    /// sink joins.
    struct Recorder {
        writes: Writes,
        delay: Duration,
    }

    impl Sink for Recorder {
        fn format(&self) -> OutputFormat {
            output(16)
        }

        fn write(&mut self, at: u64, data: &[u8]) -> std::io::Result<()> {
            self.writes.lock().unwrap().push((at, data.len() / 4));
            std::thread::sleep(self.delay);
            Ok(())
        }

        fn finish(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn recorder(delay: Duration) -> (Box<dyn Sink>, Writes) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let sink = Recorder {
            writes: writes.clone(),
            delay,
        };
        (Box::new(sink), writes)
    }

    #[test]
    fn playback_starts_one_lead_ahead_of_now() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("a.wav"), 44_100, 2, &noise(9, 1_000 * 2)).unwrap();
        let (sink, writes) = recorder(Duration::ZERO);
        let source = Tracks(vec![request("a", &dir.path().join("a.wav"))].into());
        let mut handle = engine::spawn(settings(), Box::new(source), vec![("a".to_owned(), sink)]);
        handle.send(Command::Restart { epoch: 1 });

        let due = Timeline::new(44_100, 0).frame_at(clock() + LEAD);
        let started = handle.events.blocking_recv().unwrap().event;
        assert_eq!(
            started,
            Event::TrackStarted {
                id: "a".to_owned(),
                at: due,
                start: Duration::ZERO
            }
        );
        while !matches!(
            handle.events.blocking_recv().unwrap().event,
            Event::Idle { .. }
        ) {}
        handle.shutdown();
        let writes = writes.lock().unwrap();
        assert_eq!(writes[0].0, due);
        // Contiguous from there.
        for pair in writes.windows(2) {
            assert_eq!(pair[0].0 + pair[0].1 as u64, pair[1].0);
        }
    }

    #[test]
    fn a_sink_that_joins_is_given_what_is_still_to_play() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(
            &dir.path().join("a.wav"),
            44_100,
            2,
            &noise(10, 44_100 * 5 * 2),
        )
        .unwrap();
        let (first, _) = recorder(Duration::from_millis(2));
        let (late, late_writes) = recorder(Duration::ZERO);
        let source = Tracks(vec![request("a", &dir.path().join("a.wav"))].into());
        let mut handle = engine::spawn(
            settings(),
            Box::new(source),
            vec![("first".to_owned(), first)],
        );
        handle.send(Command::Restart { epoch: 1 });
        assert!(matches!(
            handle.events.blocking_recv().unwrap().event,
            Event::TrackStarted { .. }
        ));
        std::thread::sleep(Duration::from_millis(30));
        handle.send(Command::AddSink {
            key: "late".to_owned(),
            sink: late,
        });
        while !matches!(
            handle.events.blocking_recv().unwrap().event,
            Event::Idle { .. }
        ) {}
        handle.shutdown();

        let writes = late_writes.lock().unwrap();
        let due = Timeline::new(44_100, 0).frame_at(clock() + LEAD);
        assert_eq!(writes[0].0, due, "replay starts one lead from now");
        for pair in writes.windows(2) {
            assert_eq!(
                pair[0].0 + pair[0].1 as u64,
                pair[1].0,
                "and runs on without a gap"
            );
        }
        let end = writes.last().map(|(at, len)| at + *len as u64).unwrap();
        assert_eq!(end, due + 44_100 * 5);
    }
}
