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
    provider.see(sighting("0x4"));
    // A stream starts the speaker's process; its arguments show the shared clock.
    let endpoint = s.players.get(ID).unwrap();
    tokio::task::spawn_blocking(move || {
        let mut sink = endpoint.output(
            bopsie_players_airplay::endpoint::FORMAT,
            bopsie_audio::timeline::Timeline::new(44_100, bopsie_core::clock::now()),
        );
        sink.write(0, &[0u8; 4 * 4410]).unwrap();
        sink.stop().unwrap();
    })
    .await
    .unwrap();
    let args = std::fs::read_to_string(s.dir.path().join("capture/args")).unwrap();
    assert!(args.lines().any(|a| a == "--ptp-shared"), "{args}");
    provider.shutdown();
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
