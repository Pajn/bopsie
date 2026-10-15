//! Endpoints linked into devices, whose ids outlive a restart.

use std::sync::{Arc, Mutex};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_core::config::PlayerConfig;
use bopsie_core::db::Database;
use bopsie_players::devices::MIGRATIONS;
use bopsie_players::settings::{SavedDevice, SavedEndpoint};
use bopsie_players::{
    Capabilities, Change, DeviceInfo, Devices, Endpoint, EndpointState, Facts, NowPlaying, Players,
};

struct Fake {
    id: String,
    state: Mutex<EndpointState>,
}

impl Fake {
    fn new(id: &str, provider: &'static str, facts: Facts) -> Arc<Self> {
        Arc::new(Self {
            id: id.to_owned(),
            state: Mutex::new(EndpointState {
                name: format!("{id} name"),
                provider,
                available: true,
                volume: None,
                muted: None,
                device: DeviceInfo {
                    model: None,
                    manufacturer: None,
                    software_version: None,
                },
                app: false,
                owner_token: None,
                facts,
                capabilities: Capabilities {
                    formats: Vec::new(),
                    lead: 0,
                    sync_group: true,
                    max_group_size: None,
                    volume: true,
                    mute: true,
                    pairing: Default::default(),
                    native_group: None,
                    launched: false,
                    withdrawn: Vec::new(),
                },
                quirks: Default::default(),
                native_leader: None,
            }),
        })
    }
}

impl Endpoint for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn state(&self) -> EndpointState {
        self.state.lock().unwrap().clone()
    }
    fn output(&self, _: OutputFormat, _: Timeline) -> Box<dyn Sink> {
        unimplemented!("not played to")
    }
    fn set_volume(&self, _: u8) {}
    fn set_muted(&self, _: bool) {}
    fn set_now_playing(&self, _: Option<&NowPlaying>) {}
    fn set_group(&self, _: bool, _: &str) {}
}

fn mac(mac: &str) -> Facts {
    Facts {
        mac: Some(mac.to_owned()),
        ..Facts::default()
    }
}

fn ids(devices: &Devices) -> Vec<(String, Vec<String>)> {
    devices
        .all()
        .into_iter()
        .map(|d| (d.id, d.members.into_iter().map(|m| m.id).collect()))
        .collect()
}

fn owned(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    pairs
        .iter()
        .map(|(d, es)| {
            (
                (*d).to_owned(),
                es.iter().map(|e| (*e).to_owned()).collect(),
            )
        })
        .collect()
}

/// Lets writes to the database land.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_speaker_over_two_protocols_is_one_device_whose_id_outlives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let players = Players::default();
        let devices = Devices::new(
            players.clone(),
            Database::open(&path, &[MIGRATIONS]).unwrap(),
        )
        .await
        .unwrap();
        players.register(Fake::new("speaker", "sendspin", mac("aabbcc001122")));
        players.register(Fake::new("phone", "sendspin", Facts::default()));
        assert_eq!(
            ids(&devices),
            owned(&[("phone", &["phone"]), ("speaker", &["speaker"])])
        );
        // Its AirPlay endpoint is found: the device keeps its id, preferring AirPlay.
        players.register(Fake::new("apaabbcc001122", "airplay", mac("AABBCC001122")));
        assert_eq!(
            ids(&devices),
            owned(&[
                ("phone", &["phone"]),
                ("speaker", &["apaabbcc001122", "speaker"])
            ])
        );
        let device = devices.get("speaker").unwrap();
        assert_eq!(device.primary().id, "apaabbcc001122");
        assert_eq!(device.name, "apaabbcc001122 name");
        assert_eq!(devices.device_of("apaabbcc001122").unwrap().id, "speaker");
        settle().await;
    }
    // After a restart the AirPlay endpoint is found first, and is still that device.
    let players = Players::default();
    let devices = Devices::new(
        players.clone(),
        Database::open(&path, &[MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    players.register(Fake::new("apaabbcc001122", "airplay", mac("aabbcc001122")));
    assert_eq!(ids(&devices), owned(&[("speaker", &["apaabbcc001122"])]));
}

#[tokio::test]
async fn a_receiver_that_has_to_be_launched_is_a_devices_last_resort() {
    let players = Players::default();
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    let address = Facts {
        address: Some("10.0.0.4".parse().unwrap()),
        ..Facts::default()
    };
    // A WiiM: a LinkPlay speaker, and a Cast device whose Sendspin receiver is launched.
    let cast = Fake::new("cast1", "sendspin", address.clone());
    cast.state.lock().unwrap().capabilities.launched = true;
    players.register(cast);
    players.register(Fake::new("lp1", "linkplay", address));
    settle().await;
    let device = devices.all().into_iter().next().unwrap();
    assert_eq!(
        device
            .members
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["lp1", "cast1"]
    );
    assert_eq!(
        device.primary().id,
        "lp1",
        "it is shown, and its volume set, over LinkPlay"
    );
}

#[tokio::test]
async fn devices_are_announced_as_they_link() {
    let players = Players::default();
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    let mut changes = devices.subscribe();
    players.register(Fake::new("ap1", "airplay", mac("aa")));
    assert_eq!(changes.recv().await.unwrap(), Change::Added("ap1".into()));
    players.register(Fake::new("ss1", "sendspin", mac("bb")));
    assert_eq!(changes.recv().await.unwrap(), Change::Added("ss1".into()));
    players.changed("ap1");
    assert_eq!(changes.recv().await.unwrap(), Change::Updated("ap1".into()));

    // ss1 turns out to be the same speaker as ap1, which ss1 found first.
    players.register(Fake::new("ss1", "sendspin", mac("aa")));
    let mut seen = vec![changes.recv().await.unwrap(), changes.recv().await.unwrap()];
    seen.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(
        seen,
        [Change::Removed("ss1".into()), Change::Updated("ap1".into())]
    );
}

#[tokio::test]
async fn the_config_file_names_and_disables_devices() {
    let players = Players::new(
        [
            (
                "ap1".to_owned(),
                PlayerConfig {
                    name: Some("Kitchen".into()),
                    ..PlayerConfig::default()
                },
            ),
            (
                "ap2".to_owned(),
                PlayerConfig {
                    enabled: false,
                    ..PlayerConfig::default()
                },
            ),
        ]
        .into(),
    );
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    players.register(Fake::new("ap1", "airplay", mac("aa")));
    players.register(Fake::new("ap2", "airplay", mac("bb")));
    let kitchen = devices.get("ap1").unwrap();
    assert_eq!(kitchen.name, "Kitchen");
    assert!(kitchen.available());
    let disabled = devices.get("ap2").unwrap();
    assert!(!disabled.enabled && !disabled.available());
}

#[tokio::test]
async fn saved_settings_are_over_the_config_file_and_outlive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let file = || -> std::collections::BTreeMap<String, PlayerConfig> {
        [(
            "ap1".to_owned(),
            PlayerConfig {
                name: Some("Kitchen".into()),
                ..PlayerConfig::default()
            },
        )]
        .into()
    };
    {
        let players = Players::new(file());
        let db = Database::open(&path, &[MIGRATIONS]).unwrap();
        players.load(db.clone()).await.unwrap();
        let devices = Devices::new(players.clone(), db).await.unwrap();
        players.register(Fake::new("ap1", "airplay", mac("aa")));
        players.register(Fake::new("ss1", "sendspin", mac("aa")));
        players
            .save_device(
                "ap1",
                SavedDevice {
                    name: Some("Cooking".into()),
                    max_volume: Some(40),
                    ..SavedDevice::default()
                },
            )
            .await
            .unwrap();
        players
            .save_endpoint(
                "ss1",
                SavedEndpoint {
                    enabled: Some(false),
                    ..SavedEndpoint::default()
                },
            )
            .await
            .unwrap();
        let device = devices.get("ap1").unwrap();
        assert_eq!(device.name, "Cooking");
        assert_eq!(device.max_volume, Some(40));
        let sendspin = device.member("ss1").unwrap();
        assert!(sendspin.disabled && !sendspin.state.available);
        assert!(device.available(), "AirPlay still plays");
    }
    // After a restart the saved settings are read back.
    let players = Players::new(file());
    let db = Database::open(&path, &[MIGRATIONS]).unwrap();
    players.load(db.clone()).await.unwrap();
    let devices = Devices::new(players.clone(), db).await.unwrap();
    players.register(Fake::new("ap1", "airplay", mac("aa")));
    players.register(Fake::new("ss1", "sendspin", mac("aa")));
    let device = devices.get("ap1").unwrap();
    assert_eq!(device.name, "Cooking");
    assert!(device.member("ss1").unwrap().disabled);

    // Cleared, the config file's name is back.
    players
        .save_device("ap1", SavedDevice::default())
        .await
        .unwrap();
    assert_eq!(devices.get("ap1").unwrap().name, "Kitchen");
    // Turning off every endpoint leaves the device unable to play.
    players
        .save_endpoint(
            "ap1",
            SavedEndpoint {
                enabled: Some(false),
                ..SavedEndpoint::default()
            },
        )
        .await
        .unwrap();
    assert!(!devices.get("ap1").unwrap().available());
}
