//! The AirPlay provider: speakers found become players, pairing stores credentials, and the
//! PTP daemon gives streams a shared clock. `cliairplay` is the fake in this crate.

use std::path::PathBuf;
use std::time::Duration;

use bopsie_core::db::Database;
use bopsie_players::Players;
use bopsie_players_airplay::discovery::{Service, Sighting};
use bopsie_players_airplay::txt::Txt;
use bopsie_players_airplay::{AirplayProvider, Config, MIGRATIONS};

struct Setup {
    dir: tempfile::TempDir,
    db: Database,
    players: Players,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("test.db"), &[MIGRATIONS]).unwrap();
    Setup {
        dir,
        db,
        players: Players::default(),
    }
}

impl Setup {
    async fn provider(&self, ptp: bool) -> AirplayProvider {
        self.provider_with(ptp, Duration::ZERO).await
    }

    async fn provider_with(&self, ptp: bool, lost_grace: Duration) -> AirplayProvider {
        AirplayProvider::new(
            self.players.clone(),
            self.db.clone(),
            Config {
                binary: PathBuf::from(env!("CARGO_BIN_EXE_fake-cliairplay")),
                passwords: Default::default(),
                env: vec![(
                    "FAKE_CLIAIRPLAY_DIR".into(),
                    self.dir.path().join("capture").display().to_string(),
                )],
                ptp,
                pipes: self.dir.path().join("pipes"),
                lost_grace,
            },
        )
        .await
        .unwrap()
    }
}

fn sighting(flags: &str) -> Sighting {
    let txt: Txt = [
        ("deviceid", "AA:BB:CC:00:11:22"),
        ("features", "0x4A7FDFD5,0x3C155FDE"),
        ("flags", flags),
        ("model", "HomePod"),
        ("pi", "2e388006-13ba-4041-9a67-25dd4a43d536"),
        ("fv", "p20.1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    Sighting {
        service: Service::Airplay,
        fullname: "Living Room._airplay._tcp.local.".into(),
        host: "Living-Room.local.".into(),
        port: 7000,
        address: Some("127.0.0.1".parse().unwrap()),
        txt,
    }
}

const ID: &str = "apaabbcc001122";

#[tokio::test]
async fn a_speaker_found_becomes_a_player_and_goes_when_it_leaves() {
    let s = setup();
    let provider = s.provider(false).await;
    provider.see(sighting("0x4"));
    let endpoint = s.players.get(ID).expect("registered");
    let state = endpoint.state();
    assert_eq!(state.name, "Living Room");
    assert_eq!(state.provider, "airplay");
    assert!(state.available);
    assert_eq!(state.device.model.as_deref(), Some("HomePod"));
    assert_eq!(state.device.software_version.as_deref(), Some("p20.1"));
    assert_eq!(state.facts.mac.as_deref(), Some("aabbcc001122"));
    assert_eq!(
        state.facts.uuid.as_deref(),
        Some("2e388006-13ba-4041-9a67-25dd4a43d536")
    );
    assert_eq!(state.facts.address, Some("127.0.0.1".parse().unwrap()));
    assert_eq!(state.facts.status_flags, Some(0x4));
    let capabilities = &state.capabilities;
    assert_eq!(
        capabilities.formats,
        [bopsie_players_airplay::endpoint::FORMAT]
    );
    assert!(capabilities.sync_group && capabilities.volume);
    assert_eq!(capabilities.max_group_size, Some(8));
    assert_eq!(capabilities.pairing, bopsie_players::Pairing::Transient);

    provider.lose("Living Room._airplay._tcp.local.");
    assert!(!s.players.get(ID).unwrap().state().available);
    provider.see(sighting("0x4"));
    assert!(s.players.get(ID).unwrap().state().available, "back again");
}

#[tokio::test]
async fn pin_pairing_stores_credentials_that_outlive_a_restart() {
    let s = setup();
    let provider = s.provider(false).await;
    provider.see(sighting("0x8"));
    assert!(
        !s.players.get(ID).unwrap().state().available,
        "needs pairing"
    );

    provider.pair_start(ID).unwrap();
    let wrong = provider.pair_finish(ID, "0000").await.unwrap_err();
    assert_eq!(wrong.code, bopsie_core::ErrorCode::AuthenticationFailed);
    provider.pair_start(ID).unwrap();
    provider.pair_finish(ID, " 1234 ").await.unwrap();
    assert!(s.players.get(ID).unwrap().state().available);

    // A new provider over the same database knows the credentials.
    let players = Players::default();
    let again = AirplayProvider::new(
        players.clone(),
        s.db.clone(),
        Config {
            binary: PathBuf::from(env!("CARGO_BIN_EXE_fake-cliairplay")),
            pipes: s.dir.path().join("pipes"),
            ..Config::default()
        },
    )
    .await
    .unwrap();
    again.see(sighting("0x8"));
    assert!(players.get(ID).unwrap().state().available);
}

#[tokio::test]
async fn finishing_a_pairing_that_was_not_started_fails() {
    let s = setup();
    let provider = s.provider(false).await;
    provider.see(sighting("0x8"));
    assert!(provider.pair_finish(ID, "1234").await.is_err());
}

#[tokio::test]
async fn streams_share_the_ptp_daemon_when_it_runs() {
    let s = setup();
    let provider = s.provider(true).await;
    provider.discover().unwrap();
    // The daemon starts in the background; streams share it once it serves.
    wait_for(|| provider.ptp_daemon()).await;
    provider.see(sighting("0x4"));
    // Getting a speaker ready starts its process; its arguments show the shared clock.
    let endpoint = s.players.get(ID).unwrap();
    tokio::task::spawn_blocking(move || endpoint.prepare())
        .await
        .unwrap();
    let args = std::fs::read_to_string(s.dir.path().join("capture/args")).unwrap();
    assert!(args.lines().any(|a| a == "--ptp-shared"), "{args}");
    tokio::task::spawn_blocking(move || provider.shutdown())
        .await
        .unwrap();
}

/// A speaker announced over mDNS on this machine is found. Needs multicast, which some CI
/// networks and sandboxes do not allow.
#[tokio::test]
#[ignore = "needs multicast on the local network"]
async fn a_speaker_announced_over_mdns_is_found() {
    let s = setup();
    let provider = s.provider(false).await;
    provider.discover().unwrap();

    let announcer = mdns_sd::ServiceDaemon::new().unwrap();
    let properties = [
        ("deviceid", "AA:BB:CC:00:11:22"),
        ("features", "0x4A7FDFD5,0x3C155FDE"),
        ("model", "Fake1,1"),
    ];
    let info = mdns_sd::ServiceInfo::new(
        "_airplay._tcp.local.",
        "Announced Speaker",
        "announced-speaker.local.",
        "127.0.0.1",
        7000,
        &properties[..],
    )
    .unwrap();
    announcer.register(info).unwrap();

    let mut found = false;
    for _ in 0..100 {
        if s.players.get(ID).is_some() {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = announcer.shutdown();
    provider.shutdown();
    assert!(found, "the speaker was not discovered");
    assert_eq!(s.players.get(ID).unwrap().state().name, "Announced Speaker");
}

#[tokio::test]
async fn the_config_file_overrides_how_a_speaker_says_it_pairs() {
    let mut s = setup();
    let settings = bopsie_core::config::PlayerConfig {
        quirks: bopsie_players::quirks::Quirks {
            pairing: Some(bopsie_players::Pairing::Transient),
            ..Default::default()
        },
        ..Default::default()
    };
    s.players = Players::new([(ID.to_owned(), settings)].into());
    let provider = s.provider(false).await;
    provider.see(sighting("0x8"));
    let state = s.players.get(ID).unwrap().state();
    assert!(state.available, "no PIN asked for");
    assert_eq!(
        state.quirks.source("pairing"),
        Some(&bopsie_players::quirks::Source::Config)
    );
}

#[tokio::test]
async fn a_speaker_that_comes_back_within_the_grace_period_was_never_gone() {
    let s = setup();
    let provider = s.provider_with(false, Duration::from_millis(300)).await;
    provider.see(sighting("0x4"));
    provider.lose("Living Room._airplay._tcp.local.");
    provider.see(sighting("0x4"));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(s.players.get(ID).unwrap().state().available);

    provider.lose("Living Room._airplay._tcp.local.");
    assert!(s.players.get(ID).unwrap().state().available, "not yet");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!s.players.get(ID).unwrap().state().available);
}

#[tokio::test]
async fn the_ptp_daemon_is_started_again_when_it_ends() {
    let s = setup();
    let provider = s.provider(true).await;
    provider.discover().unwrap();
    let first = wait_for(|| provider.ptp_daemon()).await;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(first as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    let second = wait_for(|| provider.ptp_daemon().filter(|pid| *pid != first)).await;
    assert_ne!(first, second);
    let pidfile = s.dir.path().join("pipes/ptp-daemon.pid");
    assert_eq!(
        std::fs::read_to_string(&pidfile).unwrap(),
        second.to_string()
    );

    tokio::task::spawn_blocking(move || provider.shutdown())
        .await
        .unwrap();
    assert!(!pidfile.exists(), "shutdown ends it");
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(second as i32), None).is_err(),
        "and reaps it"
    );
}

#[tokio::test]
async fn processes_an_earlier_run_left_are_ended() {
    let s = setup();
    let pipes = s.dir.path().join("pipes");
    std::fs::create_dir_all(&pipes).unwrap();
    let mut leftover = std::process::Command::new(env!("CARGO_BIN_EXE_fake-cliairplay"))
        .arg("--ptp-daemon")
        .spawn()
        .unwrap();
    std::fs::write(pipes.join("ptp-daemon.pid"), leftover.id().to_string()).unwrap();
    // A pid that is no longer cliairplay is left alone.
    std::fs::write(pipes.join("other.pid"), std::process::id().to_string()).unwrap();

    let _provider = s.provider(false).await;
    let status = tokio::task::spawn_blocking(move || leftover.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success(), "ended by a signal");
    assert!(!pipes.join("ptp-daemon.pid").exists());
}

/// Polls `found` for up to five seconds.
async fn wait_for<T>(mut found: impl FnMut() -> Option<T>) -> T {
    for _ in 0..250 {
        if let Some(value) = found() {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never found");
}
