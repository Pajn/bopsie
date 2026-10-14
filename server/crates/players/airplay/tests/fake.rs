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
use bopsie_core::config::PlayerConfig;
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::health::{Event as HealthEvent, Level};
use bopsie_players::quirks::{AirplayProtocol, Quirks, Timing};
use bopsie_players::{Endpoint, NowPlaying, Players};
use bopsie_players_airplay::endpoint::{AirplayEndpoint, FORMAT, Launcher};
use bopsie_players_airplay::speaker::Speaker;
use bopsie_players_airplay::txt::{Records, Txt};

const LEAD: i64 = 400_000;

struct Fake {
    dir: tempfile::TempDir,
    endpoint: Arc<AirplayEndpoint>,
    players: Players,
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
    fake_in(env, flags, Players::default())
}

/// A fake speaker whose quirks the config file sets.
fn fake_quirked(quirks: Quirks) -> Fake {
    let settings = PlayerConfig {
        quirks,
        ..PlayerConfig::default()
    };
    fake_in(
        &[],
        "0x4",
        Players::new([("apaabbcc001122".to_owned(), settings)].into()),
    )
}

fn fake_in(env: &[(&str, &str)], flags: &str, players: Players) -> Fake {
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
    let endpoint = AirplayEndpoint::new(speaker(flags), launcher, None, None, players.clone());
    Fake {
        dir,
        endpoint,
        players,
    }
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

/// Plays `samples` (44.1 kHz stereo) to the fake speaker in a real-time session, prepared as
/// playback prepares a session's speakers.
fn session(fake: &Fake, samples: &[i16]) -> (engine::EngineHandle, Timeline) {
    session_with(fake, samples, true)
}

/// As [`session`]; unprepared, the speaker gets ready while the session plays, as one that
/// joins a playing group does.
fn session_with(fake: &Fake, samples: &[i16], prepare: bool) -> (engine::EngineHandle, Timeline) {
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
    // As playback does: the speaker gets ready, and playback starts when it can.
    let not_before = if prepare {
        fake.endpoint.prepare()
    } else {
        None
    };
    let sinks = vec![("kitchen".to_owned(), fake.endpoint.output(FORMAT, timeline))];
    let handle = engine::spawn(settings, Box::new(source), sinks);
    handle.send(Command::Restart {
        epoch: 1,
        not_before,
    });
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
fn a_session_starts_when_the_speakers_clock_is_ready() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_CLOCK_READY_MS", "1500")], "0x4");
    let connected = clock::unix_ms(clock::now());
    let samples = noise(5, 4_410 * 2);
    let (mut handle, timeline) = session(&fake, &samples);
    let events = until_idle(&mut handle);
    let Event::TrackStarted { at: first, .. } = events[0] else {
        panic!("{events:?}")
    };
    handle.shutdown();
    let start_ms: u64 = fake
        .commands()
        .iter()
        .find_map(|(_, c)| c.strip_prefix("START_UNIX_MS=")?.parse().ok())
        .unwrap();
    assert!(
        start_ms >= connected + 1_500 + 500,
        "after the clock is ready"
    );
    assert!(
        start_ms.abs_diff(clock::unix_ms(timeline.instant_of(first))) <= 2,
        "and from the first frame: nothing is skipped"
    );
    assert!(eventually(|| fake.audio() == bytes(&samples)));
}

#[test]
fn a_speaker_whose_clock_is_not_ready_joins_the_timeline_later() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_CLOCK_READY_MS", "1500")], "0x4");
    let samples = noise(5, 44_100 * 4 * 2);
    let connected = clock::unix_ms(clock::now());
    let (mut handle, timeline) = session_with(&fake, &samples, false);
    let events = until_idle(&mut handle);
    let Event::TrackStarted { at: first, .. } = events[0] else {
        panic!("{events:?}")
    };
    handle.shutdown();

    let start_ms: u64 = fake
        .commands()
        .iter()
        .find_map(|(_, c)| c.strip_prefix("START_UNIX_MS=")?.parse().ok())
        .expect("a start was commanded");
    assert!(
        start_ms >= connected + 1_500 + 500,
        "after the clock is ready, with a margin: {} ms after connecting",
        start_ms - connected
    );
    // What reached the speaker is the timeline from that instant on, sample for sample: the
    // audio before it was skipped, not delayed. The fake takes audio at real-time speed, so
    // compare what it has so far.
    let all = bytes(&samples);
    assert!(eventually(|| fake.audio().len() > 44_100 * 4));
    let audio = fake.audio();
    let offset = all
        .windows(64)
        .position(|window| window == &audio[..64])
        .expect("the audio comes from the track");
    assert!(
        all[offset..].starts_with(&audio),
        "contiguous from where it joined"
    );
    let late_ms = start_ms - clock::unix_ms(timeline.instant_of(first));
    let expected_frames = late_ms * 441 / 10;
    assert!(
        (offset as i64 / 4 - expected_frames as i64).abs() <= 50,
        "skipped {} frames for a start {late_ms} ms late",
        offset / 4
    );
}

#[test]
fn a_start_the_speaker_moves_later_is_started_again_in_step() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_START_LATE_MS", "300")], "0x4");
    let samples = noise(5, 44_100 * 3 * 2);
    let (mut handle, _) = session(&fake, &samples);
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert!(fake.endpoint.supervisor().signals().corrections >= 1);
    let health = fake.players.health("apaabbcc001122");
    assert!(health.counts[&HealthEvent::LateStart] >= 1);
    let actions = fake.actions();
    assert_eq!(
        &actions[..3],
        ["ACTION=START", "ACTION=FLUSH", "ACTION=START"],
        "{actions:?}"
    );
}

#[test]
fn an_authentication_failure_makes_the_speaker_unavailable() {
    let fake = fake_with(&[("FAKE_CLIAIRPLAY_FAIL", "auth_required")], "0x4");
    assert!(fake.endpoint.state().available);
    let error = fake.endpoint.supervisor().ensure_connected().unwrap_err();
    assert!(error.to_string().starts_with("auth_required"), "{error}");
    assert!(eventually(|| !fake.endpoint.state().available));
    let health = fake.players.health("apaabbcc001122");
    assert_eq!(health.level, Level::Failing);
    assert_eq!(health.counts[&HealthEvent::ConnectFailed], 1);
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

#[test]
fn quirks_choose_the_route_and_timing() {
    let quirked = fake_quirked(Quirks {
        protocol: Some(AirplayProtocol::Airplay2Compat),
        timing: Some(Timing::Ntp),
        ..Quirks::default()
    });
    quirked.endpoint.prepare();
    let args = std::fs::read_to_string(quirked.capture("args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["--protocol", "airplay2-compat"])
    );
    assert!(args.windows(2).any(|w| w == ["--timing", "ntp"]));

    let plain = fake();
    plain.endpoint.prepare();
    let args = std::fs::read_to_string(plain.capture("args")).unwrap();
    assert!(
        !args.contains("--timing"),
        "cliairplay's own choice: {args}"
    );
}

#[test]
fn a_sync_adjustment_moves_when_the_speaker_plays() {
    let fake = fake_quirked(Quirks {
        sync_adjust_ms: Some(150),
        ..Quirks::default()
    });
    let samples = noise(1, 22_050 * 2);
    let (mut handle, timeline) = session(&fake, &samples);
    let events = until_idle(&mut handle);
    let Event::TrackStarted { at, .. } = events[0] else {
        panic!("{events:?}")
    };
    handle.shutdown();
    let start_ms: u64 = fake
        .commands()
        .iter()
        .find_map(|(_, c)| c.strip_prefix("START_UNIX_MS=")?.parse().ok())
        .expect("a start was commanded");
    let on_timeline = clock::unix_ms(timeline.instant_of(at));
    assert!(
        start_ms.abs_diff(on_timeline + 150) <= 2,
        "{} ms after the timeline",
        start_ms as i64 - on_timeline as i64
    );
}

#[test]
fn an_idle_speaker_is_disconnected_when_its_quirk_says_so() {
    let quirked = fake_quirked(Quirks {
        disconnect_on_idle_s: Some(1),
        ..Quirks::default()
    });
    quirked.endpoint.prepare();
    assert!(quirked.endpoint.supervisor().is_running());
    assert!(eventually(|| !quirked.endpoint.supervisor().is_running()));
    assert!(quirked.actions().contains(&"ACTION=DISCONNECT".to_owned()));

    // Without the quirk the session stays.
    let plain = fake();
    plain.endpoint.prepare();
    std::thread::sleep(Duration::from_millis(2_500));
    assert!(plain.endpoint.supervisor().is_running());
}

#[test]
fn health_follows_what_the_speaker_does() {
    let fake = fake();
    let health = || fake.players.health("apaabbcc001122");
    let (mut handle, _) = session(&fake, &noise(8, 4_410 * 2));
    let _ = until_idle(&mut handle);
    handle.shutdown();
    assert_eq!(health().level, Level::Good);
    assert_eq!(health().counts[&HealthEvent::Connected], 1);
    assert_eq!(health().counts[&HealthEvent::Started], 1);

    // Ended by the server: not a failure.
    fake.endpoint.supervisor().kill();
    assert!(eventually(|| !fake.endpoint.supervisor().is_running()));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!health().counts.contains_key(&HealthEvent::Exited));

    // Ended on its own: failing until it works again.
    fake.endpoint.prepare();
    fake.endpoint
        .supervisor()
        .command(&["ACTION=STOP".into()])
        .unwrap();
    assert!(eventually(|| health().level == Level::Failing));
    assert_eq!(health().counts[&HealthEvent::Exited], 1);
    fake.endpoint.prepare();
    assert!(eventually(|| health().level == Level::Good));
}
