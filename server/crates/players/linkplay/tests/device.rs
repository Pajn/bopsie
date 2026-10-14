//! LinkPlay devices through the provider, against stand-ins that speak their HTTP API: what a
//! device says of itself, its volume, playing a session to it, and its own multiroom.

mod common;

use std::collections::VecDeque;
use std::time::Duration;

use bopsie_audio::PcmFormat;
use bopsie_audio::engine::{self, Command, Event, Realtime, Settings, TrackRequest, TrackSource};
use bopsie_audio::timeline::Timeline;
use bopsie_core::clock;
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::{Endpoint, Remote};
use bopsie_players_linkplay::endpoint::FORMATS;
use common::*;

const LEAD: i64 = 1_000_000;
/// Long enough for the first start's `play` to go out as early as it is meant to.
const LONG_LEAD: i64 = 3_000_000;

struct Tracks(VecDeque<TrackRequest>);

impl TrackSource for Tracks {
    fn next(&mut self) -> Option<TrackRequest> {
        self.0.pop_front()
    }
}

#[tokio::test]
async fn a_device_says_what_it_is() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let leader = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    let follower = network
        .device("Kitchen", "10.0.0.3", "89ABCDEF0123456789ABCDEF", false)
        .await;
    follower.device.lock().unwrap().leader = Some("0123456789ABCDEF01234567".into());
    leader.add_to(&provider);
    follower.add_to(&provider);
    for fake in [&leader, &follower] {
        provider.endpoint(&fake.id()).unwrap().refresh().await;
    }

    let state = players.get(&leader.id()).unwrap().state();
    assert_eq!(state.name, "Living Room");
    assert_eq!(state.provider, "linkplay");
    assert!(state.available);
    assert_eq!(state.device.model.as_deref(), Some("Fake_Speaker"));
    assert_eq!(state.facts.mac.as_deref(), Some("aabbcc000067"));
    assert_eq!(state.capabilities.formats, FORMATS);
    assert!(
        !state.capabilities.sync_group,
        "it cannot follow the server's timeline"
    );
    let native = state.capabilities.native_group.unwrap();
    assert_eq!((native.family, native.version), ("linkplay", 4));
    assert_eq!(state.native_leader, None);
    assert_eq!(state.volume, Some(30));

    // Read over UPnP, since it lacks getPlayerStatus.
    let state = players.get(&follower.id()).unwrap().state();
    assert_eq!(state.native_leader, Some(leader.id()));
    assert_eq!(state.volume, Some(30));

    // What an admin reads of each.
    let leader_diagnostics = players.get(&leader.id()).unwrap().diagnostics();
    assert_eq!(leader_diagnostics["address"], "10.0.0.2");
    assert!(
        leader_diagnostics["api"].as_str().is_some(),
        "{leader_diagnostics}"
    );
    assert_eq!(leader_diagnostics["player_status"], "getPlayerStatus");
    assert_eq!(leader_diagnostics["status"]["wmrm_version"], "4.2");
    assert_eq!(leader_diagnostics["status"]["follower"], false);
    let follower_diagnostics = players.get(&follower.id()).unwrap().diagnostics();
    assert_eq!(follower_diagnostics["player_status"], "upnp");
    assert_eq!(follower_diagnostics["status"]["follower"], true);
    provider.shutdown();
}

#[tokio::test]
async fn volume_and_mute_reach_the_device() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let fake = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    endpoint.set_volume(7);
    endpoint.set_muted(true);
    assert!(
        eventually(|| {
            let d = fake.device.lock().unwrap();
            d.volume == 7 && d.muted
        })
        .await
    );
    assert_eq!(endpoint.state().volume, Some(7));
    provider.shutdown();
}

/// Plays `samples` (44.1 kHz stereo) to `endpoint` in a real-time session.
fn session(
    endpoint: &dyn Endpoint,
    dir: &std::path::Path,
    samples: &[i16],
    lead: i64,
) -> (engine::EngineHandle, Timeline) {
    let path = dir.join("a.wav");
    write_wav(&path, 44_100, 2, samples).unwrap();
    let timeline = Timeline::new(44_100, clock::now());
    let settings = Settings {
        format: PcmFormat {
            sample_rate: 44_100,
            channels: 2,
        },
        ffmpeg: None,
        ring_frames: 0,
        max_ahead: bopsie_audio::engine::QUEUE_AHEAD,
        realtime: Some(Realtime {
            timeline,
            lead,
            clock: clock::now,
        }),
    };
    let source = Tracks(
        vec![TrackRequest {
            id: "a".into(),
            input: path.into(),
            start: Duration::ZERO,
        }]
        .into(),
    );
    let sinks = vec![("lp".to_owned(), endpoint.output(FORMATS[0], timeline))];
    let handle = engine::spawn(settings, Box::new(source), sinks);
    handle.send(Command::Restart {
        epoch: 1,
        not_before: None,
    });
    (handle, timeline)
}

fn bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_streams_to_the_device_and_learns_its_start_delay() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let fake = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(3, 44_100 * 2);
    let (mut handle, _) =
        tokio::task::block_in_place(|| session(endpoint.as_ref(), dir.path(), &samples, LONG_LEAD));
    loop {
        let event = tokio::task::block_in_place(|| handle.events.blocking_recv())
            .unwrap()
            .event;
        if matches!(event, Event::Idle { .. }) {
            break;
        }
    }
    // The device pulls a WAV of no stated end, and in it the session's audio unchanged.
    assert!(
        eventually(|| fake.device.lock().unwrap().pulled.len() == 44 + samples.len() * 2).await
    );
    let pulled = fake.device.lock().unwrap().pulled.clone();
    assert_eq!(&pulled[..4], b"RIFF");
    assert!(pulled[44..] == bytes(&samples));
    let played = fake
        .commands()
        .into_iter()
        .find(|c| c.starts_with("setPlayerCmd:play:"))
        .unwrap();
    assert!(
        played.contains("/linkplay/stream/") && played.ends_with(".wav"),
        "{played}"
    );

    // It was told to play as long as a first start is assumed to take, 2.7 s, before it was
    // due; it took 0.6 s, so it will be told later next time.
    let endpoint = provider.endpoint(&fake.id()).unwrap();
    assert!(
        eventually(|| endpoint.start_delay_ms() < 2_000).await,
        "{}",
        endpoint.start_delay_ms()
    );
    tokio::task::block_in_place(|| handle.shutdown());
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seek_stops_the_device_and_starts_a_new_stream() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let fake = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(4, 44_100 * 2 * 4);
    let (handle, _) =
        tokio::task::block_in_place(|| session(endpoint.as_ref(), dir.path(), &samples, LEAD));
    assert!(eventually(|| fake.device.lock().unwrap().streams == 1).await);
    handle.send(Command::Seek(Duration::from_secs(2)));
    assert!(eventually(|| fake.device.lock().unwrap().streams == 2).await);
    let commands = fake.commands();
    let actions: Vec<&String> = commands
        .iter()
        .filter(|c| c.starts_with("setPlayerCmd:"))
        .collect();
    assert!(actions[0].starts_with("setPlayerCmd:play:"));
    assert_eq!(actions[1], "setPlayerCmd:stop");
    assert!(actions[2].starts_with("setPlayerCmd:play:"));
    assert_ne!(actions[0], actions[2], "a new stream");
    tokio::task::block_in_place(|| handle.shutdown());
    provider.shutdown();
}

async fn heard(remotes: &mut tokio::sync::broadcast::Receiver<(String, Remote)>) -> Option<Remote> {
    tokio::time::timeout(Duration::from_secs(5), remotes.recv())
        .await
        .ok()
        .map(|heard| heard.unwrap().1)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_devices_own_pause_and_play_are_heard() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let mut remotes = players.remotes();
    let fake = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(5, 44_100 * 2 * 20);
    let (handle, _) =
        tokio::task::block_in_place(|| session(endpoint.as_ref(), dir.path(), &samples, LEAD));
    assert!(eventually(|| fake.device.lock().unwrap().streams == 1).await);
    let linkplay = provider.endpoint(&fake.id()).unwrap();
    assert!(eventually(|| linkplay.diagnostics()["playing"] == true).await);

    // Its pause button.
    fake.device.lock().unwrap().paused = true;
    assert_eq!(heard(&mut remotes).await, Some(Remote::Pause));
    // The group pauses, which does not stop the device: it stays paused.
    handle.send(Command::Stop { epoch: 1 });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !fake.commands().iter().any(|c| c == "setPlayerCmd:stop"),
        "{:?}",
        fake.commands()
    );
    // Its play button resumes the group.
    fake.device.lock().unwrap().paused = false;
    assert_eq!(heard(&mut remotes).await, Some(Remote::Play));
    tokio::task::block_in_place(|| handle.shutdown());
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_switched_to_another_input_is_released_and_left_on_it() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let mut remotes = players.remotes();
    let fake = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(6, 44_100 * 2 * 20);
    let (handle, _) =
        tokio::task::block_in_place(|| session(endpoint.as_ref(), dir.path(), &samples, LEAD));
    assert!(eventually(|| fake.device.lock().unwrap().streams == 1).await);
    let linkplay = provider.endpoint(&fake.id()).unwrap();
    assert!(eventually(|| linkplay.diagnostics()["mode"] == "10").await);

    fake.device.lock().unwrap().mode = "49".to_owned();
    assert_eq!(heard(&mut remotes).await, Some(Remote::Released));
    handle.send(Command::Stop { epoch: 1 });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !fake.commands().iter().any(|c| c == "setPlayerCmd:stop"),
        "stopping it would stop the input it went to"
    );
    tokio::task::block_in_place(|| handle.shutdown());
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_join_and_leave_each_others_groups() {
    let network = Network::default();
    let (provider, players) = provider().await;
    let leader = network
        .device("Living Room", "10.0.0.2", "0123456789ABCDEF01234567", true)
        .await;
    let follower = network
        .device("Kitchen", "10.0.0.3", "89ABCDEF0123456789ABCDEF", false)
        .await;
    leader.add_to(&provider);
    follower.add_to(&provider);
    for fake in [&leader, &follower] {
        provider.endpoint(&fake.id()).unwrap().refresh().await;
    }
    let endpoint = players.get(&follower.id()).unwrap();
    let leader_id = leader.id();
    let joining = endpoint.clone();
    tokio::task::spawn_blocking(move || joining.join_native(&leader_id))
        .await
        .unwrap()
        .unwrap();
    assert!(
        follower
            .commands()
            .contains(&"ConnectMasterAp:JoinGroupMaster:eth10.0.0.2:wifi0.0.0.0".to_owned())
    );
    assert_eq!(endpoint.state().native_leader, Some(leader.id()));

    let leaving = endpoint.clone();
    tokio::task::spawn_blocking(move || leaving.leave_native())
        .await
        .unwrap()
        .unwrap();
    assert!(
        leader
            .commands()
            .contains(&"multiroom:SlaveKickout:10.0.0.3".to_owned()),
        "by its leader"
    );
    assert_eq!(endpoint.state().native_leader, None);
    provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_without_a_countdown_is_timed_by_its_first_second() {
    let network = Network::default();
    let (provider, players) = provider().await;
    // As Audio Pro's firmware: no getPlayerStatus, only UPnP's whole-second position.
    let fake = network
        .device("Kitchen", "10.0.0.3", "89ABCDEF0123456789ABCDEF", false)
        .await;
    fake.add_to(&provider);
    let endpoint = players.get(&fake.id()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(5, 44_100 * 2 * 3);
    let (handle, _) =
        tokio::task::block_in_place(|| session(endpoint.as_ref(), dir.path(), &samples, LONG_LEAD));
    let endpoint = provider.endpoint(&fake.id()).unwrap();
    // Told to play 2.7 s ahead, it took 0.6 s: the start delay comes down.
    assert!(
        eventually(|| endpoint.start_delay_ms() < 2_000).await,
        "{}",
        endpoint.start_delay_ms()
    );
    tokio::task::block_in_place(|| handle.shutdown());
    provider.shutdown();
}
