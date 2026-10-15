//! An AirPlay speaker in a playing queue, through playback, against the fake `cliairplay`:
//! one whose process dies mid-song is started again and rejoins.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::media::{ItemFields, MediaType, Metadata, Track};
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::{Devices, Players};
use bopsie_players_airplay::endpoint::{AirplayEndpoint, Launcher};
use bopsie_players_airplay::speaker::Speaker;
use bopsie_players_airplay::txt::{Records, Txt};
use bopsie_queues::{Config, Playback, QueueOption};

const ID: &str = "apaabbcc001122";

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

fn speaker() -> Speaker {
    let txt: Txt = [("features", "0x4A7FDFD5,0x3C155FDE"), ("flags", "0x4")]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Speaker {
        id: ID.into(),
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

/// How many processes the fake has started: each writes its arguments afresh.
fn starts(capture: &std::path::Path) -> usize {
    std::fs::read_to_string(capture.join("commands"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.ends_with("ACTION=START"))
        .count()
}

async fn eventually(mut holds: impl FnMut() -> bool) {
    for _ in 0..500 {
        if holds() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never held");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_speaker_whose_process_dies_mid_song_rejoins() {
    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture");
    let path = dir.path().join("a.wav");
    write_wav(&path, 44_100, 2, &noise(1, 44_100 * 2 * 8)).unwrap();

    let players = Players::default();
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[bopsie_players::devices::MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    let playback = Playback::start(devices, EventBus::default(), Config::default());
    let launcher = Launcher {
        binary: PathBuf::from(env!("CARGO_BIN_EXE_fake-cliairplay")),
        pipes: dir.path().to_owned(),
        dacp_id: "A1B2C3D4E5F60718".into(),
        env: vec![
            ("FAKE_CLIAIRPLAY_DIR".into(), capture.display().to_string()),
            ("FAKE_CLIAIRPLAY_EXIT_ONCE_MS".into(), "1500".into()),
        ],
        ptp_shared: Arc::new(Mutex::new(false)),
    };
    let endpoint = AirplayEndpoint::new(speaker(), launcher, None, players.clone());
    players.register(endpoint.clone());
    eventually(|| playback.player(ID, None).is_some()).await;

    playback
        .play_media(
            ID,
            vec![(track("a"), path.into())],
            QueueOption::Replace,
            None,
        )
        .unwrap();
    eventually(|| starts(&capture) == 1).await;
    eventually(|| capture.join("exited").exists()).await;
    // A new process, started on the timeline again.
    eventually(|| starts(&capture) == 2).await;
    assert!(endpoint.supervisor().is_running());
    let queue = playback.queue(ID).unwrap();
    assert_eq!(queue["state"], "playing");
}
