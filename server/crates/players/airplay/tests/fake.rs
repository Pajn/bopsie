//! AirPlay sessions through the `cliairplay` supervisor, against the fake binary that speaks
//! its process contract.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_audio::PcmFormat;
use bopsie_audio::engine::{self, Command, Event, Realtime, Settings, TrackRequest, TrackSource};
use bopsie_audio::timeline::Timeline;
use bopsie_core::clock;
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::{Endpoint, NowPlaying, Players};
use bopsie_players_airplay::endpoint::{AirplayEndpoint, FORMAT, Launcher};
use bopsie_players_airplay::speaker::Speaker;
use bopsie_players_airplay::txt::{Records, Txt};

const LEAD: i64 = 400_000;

struct Fake {
    dir: tempfile::TempDir,
    endpoint: Arc<AirplayEndpoint>,
}

fn speaker(flags: &str) -> Speaker {
    let txt: Txt = [
        ("features", "0x4A7FDFD5,0x3C155FDE"),
        ("flags", flags),
        ("model", "Fake1,1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    Speaker {
        id: "apaabbcc001122".into(),
        name: "Kitchen".into(),
        address: "127.0.0.1".parse().unwrap(),
        airplay: Some((7000, "Kitchen.local.".into())),
        raop: None,
        records: Records {
            airplay: Some(txt),
            raop: None,
        },
    }
}

fn fake_with(env: &[(&str, &str)], flags: &str) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let mut all_env = vec![(
        "FAKE_CLIAIRPLAY_DIR".to_owned(),
        dir.path().join("capture").display().to_string(),
    )];
    all_env.extend(env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    let launcher = Launcher {
        binary: PathBuf::from(env!("CARGO_BIN_EXE_fake-cliairplay")),
        pipes: dir.path().to_owned(),
        dacp_id: "A1B2C3D4E5F60718".into(),
        env: all_env,
        ptp_shared: Arc::new(Mutex::new(false)),
    };
    let endpoint = AirplayEndpoint::new(speaker(flags), launcher, None, None, Players::default());
    Fake { dir, endpoint }
}

fn fake() -> Fake {
    fake_with(&[], "0x4")
}

impl Fake {
    fn capture(&self, name: &str) -> PathBuf {
        self.dir.path().join("capture").join(name)
    }

    fn audio(&self) -> Vec<u8> {
        std::fs::read(self.capture("audio.pcm")).unwrap_or_default()
    }

    /// Command lines, each with the audio bytes received before it.
    fn commands(&self) -> Vec<(u64, String)> {
        std::fs::read_to_string(self.capture("commands"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (bytes, command) = line.split_once(' ')?;
                Some((bytes.parse().ok()?, command.to_owned()))
            })
            .collect()
    }

    fn actions(&self) -> Vec<String> {
        self.commands()
            .into_iter()
            .map(|(_, c)| c)
            .filter(|c| c.starts_with("ACTION="))
            .collect()
    }
}

/// Waits up to five seconds for `holds`.
fn eventually(mut holds: impl FnMut() -> bool) -> bool {
    for _ in 0..250 {
        if holds() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

struct Tracks(VecDeque<TrackRequest>);

impl TrackSource for Tracks {
    fn next(&mut self) -> Option<TrackRequest> {
        self.0.pop_front()
    }
}

/// Plays `samples` (44.1 kHz stereo) to the fake speaker in a real-time session.
fn session(fake: &Fake, samples: &[i16]) -> (engine::EngineHandle, Timeline) {
    let path = fake.dir.path().join(format!("track-{}.wav", samples.len()));
    write_wav(&path, 44_100, 2, samples).unwrap();
    let timeline = Timeline::new(44_100, clock::now());
    let settings = Settings {
        format: PcmFormat {
            sample_rate: 44_100,
            channels: 2,
        },
        ffmpeg: None,
        ring_frames: 0,
        realtime: Some(Realtime {
            timeline,
            lead: LEAD,
            clock: clock::now,
        }),
    };
    let source = Tracks(
        vec![TrackRequest {
            id: "a".into(),
            path,
            start: Duration::ZERO,
        }]
        .into(),
    );
    let sinks = vec![("kitchen".to_owned(), fake.endpoint.output(FORMAT, timeline))];
    let handle = engine::spawn(settings, Box::new(source), sinks);
    handle.send(Command::Restart { epoch: 1 });
    (handle, timeline)
}

fn until_idle(handle: &mut engine::EngineHandle) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = handle.events.blocking_recv().unwrap().event;
        let idle = matches!(event, Event::Idle { .. });
        events.push(event);
        if idle {
            return events;
        }
    }
}

fn bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[test]
fn a_session_streams_to_the_process_and_starts_on_the_timeline() {
    let fake = fake();
    let samples = noise(1, 22_050 * 2);
    let began = clock::now();
    let (mut handle, timeline) = session(&fake, &samples);
    let events = until_idle(&mut handle);
    let Event::TrackStarted { at, .. } = events[0] else {
        panic!("{events:?}")
    };
    handle.shutdown();
    assert!(eventually(|| fake.audio().len() == samples.len() * 2));
    assert!(
        fake.audio() == bytes(&samples),
        "the audio reaches the process unchanged"
    );

    let commands = fake.commands();
    let start_ms: u64 = commands
        .iter()
        .find_map(|(_, c)| c.strip_prefix("START_UNIX_MS="))
        .unwrap()
        .parse()
        .unwrap();
    let expected = clock::unix_ms(timeline.instant_of(at));
    assert!(start_ms.abs_diff(expected) <= 2, "{start_ms} vs {expected}");
    assert!(
        start_ms.abs_diff(clock::unix_ms(began + LEAD)) <= 50,
        "one lead after the session began"
    );
    // START follows the audio it starts.
    let (before_start, _) = commands.iter().find(|(_, c)| c == "ACTION=START").unwrap();
    assert!(*before_start > 0);

    let args = std::fs::read_to_string(fake.capture("args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(args.windows(2).any(|w| w == ["--volume", "30"]));
    assert!(args.windows(2).any(|w| w == ["--samplerate", "44100"]));
    assert_eq!(args.last(), Some(&"127.0.0.1"));
    assert!(fake.endpoint.supervisor().signals().connected);
}

#[test]
fn a_seek_flushes_and_starts_again() {
    let fake = fake();
    let samples = noise(2, 44_100 * 2 * 2);
    let (mut handle, _) = session(&fake, &samples);
    assert!(matches!(
        handle.events.blocking_recv().unwrap().event,
        Event::TrackStarted { .. }
    ));
    assert!(eventually(|| fake
        .actions()
        .contains(&"ACTION=START".to_owned())));
    handle.send(Command::Seek(Duration::from_millis(500)));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert_eq!(
        fake.actions(),
        ["ACTION=START", "ACTION=FLUSH", "ACTION=START"]
    );
}

#[test]
fn stopping_flushes_and_parks_the_speaker() {
    let fake = fake();
    let samples = noise(3, 44_100 * 2 * 3);
    let (handle, _) = session(&fake, &samples);
    assert!(eventually(|| fake
        .actions()
        .contains(&"ACTION=START".to_owned())));
    handle.send(Command::Stop { epoch: 2 });
    assert!(eventually(|| fake
        .actions()
        .contains(&"ACTION=STANDBY".to_owned())));
    handle.shutdown();
    assert_eq!(
        fake.actions(),
        ["ACTION=START", "ACTION=FLUSH", "ACTION=STANDBY"]
    );
}

#[test]
fn volume_and_metadata_go_to_a_running_process() {
    let fake = fake();
    let samples = noise(4, 44_100 * 2 * 2);
    let (mut handle, _) = session(&fake, &samples);
    assert!(eventually(|| fake
        .actions()
        .contains(&"ACTION=START".to_owned())));
    fake.endpoint.set_volume(55);
    fake.endpoint.set_now_playing(Some(&NowPlaying {
        title: "Opening\nNight".into(),
        artist: Some("Test Artist".into()),
        album: Some("First Album".into()),
        album_artist: None,
        year: None,
        track_number: None,
        artwork_url: None,
        duration: 30_000,
        position: 2_000,
        playing: true,
    }));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert!(eventually(|| fake
        .actions()
        .contains(&"ACTION=SENDMETA".to_owned())));
    let commands: Vec<String> = fake.commands().into_iter().map(|(_, c)| c).collect();
    for expected in [
        "VOLUME=55",
        "TITLE=Opening Night",
        "ARTIST=Test Artist",
        "DURATION=30",
        "PROGRESS=2",
    ] {
        assert!(
            commands.contains(&expected.to_owned()),
            "{expected} in {commands:?}"
        );
    }
    assert_eq!(fake.endpoint.state().volume, Some(55));
}

#[test]
fn a_late_start_is_counted() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_START_LATE_MS", "300")], "0x4");
    let (mut handle, _) = session(&fake, &noise(5, 4_410 * 2));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert_eq!(fake.endpoint.supervisor().signals().corrections, 1);
}

#[test]
fn an_authentication_failure_makes_the_speaker_unavailable() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_FAIL", "auth_required")], "0x4");
    assert!(fake.endpoint.state().available);
    let error = fake.endpoint.supervisor().ensure_connected().unwrap_err();
    assert!(error.to_string().starts_with("auth_required"), "{error}");
    assert!(eventually(|| !fake.endpoint.state().available));
}

#[test]
fn a_speaker_that_needs_a_pin_waits_for_pairing() {
    let fake = fake_with(&[], "0x8");
    assert!(!fake.endpoint.state().available);
    fake.endpoint.set_credentials("ab".repeat(96));
    assert!(fake.endpoint.state().available);
}

#[test]
fn a_process_that_ends_is_started_again() {
    let fake = fake();
    let (mut handle, _) = session(&fake, &noise(6, 4_410 * 2));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    fake.endpoint.supervisor().kill();
    assert!(eventually(|| !fake.endpoint.supervisor().is_running()));
    std::fs::remove_file(fake.capture("args")).unwrap();

    let (mut handle, _) = session(&fake, &noise(7, 4_410 * 2));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert!(Path::new(&fake.capture("args")).exists(), "a new process");
}
