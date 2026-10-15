//! LinkPlay devices in playback, against stand-ins: grouped by their own multiroom, groups made
//! in the vendor's app shown as they are, and a follower whose AirPlay is withdrawn joining an
//! AirPlay group.

mod common;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::media::{ItemFields, MediaType, Metadata, Track};
use bopsie_fixtures::{noise, write_wav};
use bopsie_players::{
    Capabilities, DeviceInfo, Devices, Endpoint, EndpointState, Facts, NowPlaying, Players,
};
use bopsie_queues::{Config, Playback, QueueOption};
use common::*;

const AMP: &str = "0123456789ABCDEF01234567";
const KITCHEN: &str = "89ABCDEF0123456789ABCDEF";

/// An AirPlay endpoint that plays into nothing.
struct Airplay {
    id: String,
    mac: String,
    available: AtomicBool,
    players: Players,
}

impl Airplay {
    fn register(players: &Players, id: &str, mac: &str, available: bool) -> Arc<Self> {
        let endpoint = Arc::new(Self {
            id: id.into(),
            mac: mac.into(),
            available: AtomicBool::new(available),
            players: players.clone(),
        });
        players.register(endpoint.clone());
        endpoint
    }

    fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::SeqCst);
        self.players.changed(&self.id);
    }
}

struct Nowhere(OutputFormat);

impl Sink for Nowhere {
    fn format(&self) -> OutputFormat {
        self.0
    }
    fn write(&mut self, _: u64, _: &[u8]) -> io::Result<()> {
        std::thread::sleep(std::time::Duration::from_millis(5));
        Ok(())
    }
    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Endpoint for Airplay {
    fn id(&self) -> &str {
        &self.id
    }
    fn state(&self) -> EndpointState {
        EndpointState {
            name: self.id.clone(),
            provider: "airplay",
            available: self.available.load(Ordering::SeqCst),
            volume: Some(20),
            muted: Some(false),
            device: DeviceInfo {
                model: None,
                manufacturer: None,
                software_version: None,
            },
            app: false,
            owner_token: None,
            facts: Facts {
                mac: Some(self.mac.clone()),
                ..Facts::default()
            },
            capabilities: Capabilities {
                formats: vec![OutputFormat {
                    sample_rate: 44_100,
                    channels: 2,
                    bits: 16,
                }],
                lead: 200_000,
                sync_group: true,
                max_group_size: Some(8),
                volume: true,
                mute: true,
                pairing: Default::default(),
                native_group: None,
                withdrawn: Vec::new(),
            },
            quirks: Default::default(),
            native_leader: None,
        }
    }
    fn output(&self, format: OutputFormat, _: Timeline) -> Box<dyn Sink> {
        Box::new(Nowhere(format))
    }
    fn set_volume(&self, _: u8) {}
    fn set_muted(&self, _: bool) {}
    fn set_now_playing(&self, _: Option<&NowPlaying>) {}
    fn set_group(&self, _: bool, _: &str) {}
}

struct Setup {
    network: Network,
    provider: bopsie_players_linkplay::LinkplayProvider,
    players: Players,
    playback: Playback,
    dir: tempfile::TempDir,
}

async fn setup() -> Setup {
    let network = Network::default();
    let (provider, players) = provider().await;
    let devices = Devices::new(
        players.clone(),
        Database::open_in_memory(&[bopsie_players::devices::MIGRATIONS]).unwrap(),
    )
    .await
    .unwrap();
    let playback = Playback::start(devices, EventBus::default(), Config::default());
    Setup {
        network,
        provider,
        players,
        playback,
        dir: tempfile::tempdir().unwrap(),
    }
}

impl Setup {
    async fn device(&self, name: &str, ip: &str, uuid: &str) -> Fake {
        let fake = self.network.device(name, ip, uuid, true).await;
        fake.add_to(&self.provider);
        self.provider.endpoint(&fake.id()).unwrap().refresh().await;
        assert!(eventually(|| self.playback.player(&fake.id(), None).is_some()).await);
        fake
    }

    fn player(&self, id: &str) -> serde_json::Value {
        self.playback.player(id, None).unwrap()
    }

    fn track(&self) -> (Track, bopsie_audio::engine::Input) {
        let path = self.dir.path().join("a.wav");
        write_wav(&path, 44_100, 2, &noise(1, 44_100 * 2 * 5)).unwrap();
        let track = Track {
            fields: ItemFields {
                item_id: "a".into(),
                provider: "library".into(),
                name: "a".into(),
                version: String::new(),
                sort_name: "a".into(),
                uri: "library://track/a".into(),
                external_ids: Vec::new(),
                is_playable: true,
                media_type: MediaType::Track,
                provider_mappings: Vec::new(),
                metadata: Metadata::default(),
                favorite: false,
                position: None,
                date_added: None,
            },
            duration: 5,
            artists: Vec::new(),
            last_played: 0,
            album: None,
            disc_number: 0,
            track_number: 0,
            audio_metadata: None,
        };
        (track, path.into())
    }
}

fn has(fake: &Fake, command: &str) -> bool {
    fake.commands().iter().any(|c| c == command)
}

#[tokio::test(flavor = "multi_thread")]
async fn linkplay_devices_group_by_their_own_multiroom() {
    let s = setup().await;
    let amp = s.device("Living Room", "10.0.0.2", AMP).await;
    let kitchen = s.device("Kitchen", "10.0.0.3", KITCHEN).await;
    assert_eq!(
        s.player(&amp.id())["can_group_with"],
        serde_json::json!([kitchen.id()])
    );

    s.playback.group(&kitchen.id(), &amp.id()).unwrap();
    assert!(
        eventually(|| has(
            &kitchen,
            "ConnectMasterAp:JoinGroupMaster:eth10.0.0.2:wifi0.0.0.0"
        ))
        .await
    );
    assert_eq!(s.player(&kitchen.id())["synced_to"], amp.id());

    // Only the leader is streamed to; its multiroom carries the rest.
    s.playback
        .play_media(&amp.id(), vec![s.track()], QueueOption::Replace, None)
        .unwrap();
    assert!(eventually(|| amp.device.lock().unwrap().streams == 1).await);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(kitchen.device.lock().unwrap().streams, 0);
    assert_eq!(s.player(&amp.id())["active_output_protocol"], amp.id());

    s.playback.ungroup(&kitchen.id()).unwrap();
    assert!(eventually(|| has(&amp, "multiroom:SlaveKickout:10.0.0.3")).await);
    assert_eq!(
        s.player(&kitchen.id())["synced_to"],
        serde_json::Value::Null
    );
    // Not taken back for the vendor app's doing while the device catches up.
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        s.player(&kitchen.id())["synced_to"],
        serde_json::Value::Null
    );
    s.provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_group_made_in_the_vendors_app_is_shown_as_it_is() {
    let s = setup().await;
    let amp = s.device("Living Room", "10.0.0.2", AMP).await;
    let kitchen = s.device("Kitchen", "10.0.0.3", KITCHEN).await;

    kitchen.device.lock().unwrap().leader = Some(AMP.into());
    s.provider.endpoint(&kitchen.id()).unwrap().refresh().await;
    assert!(eventually(|| s.player(&kitchen.id())["synced_to"] == amp.id()).await);
    assert!(
        !has(
            &kitchen,
            "ConnectMasterAp:JoinGroupMaster:eth10.0.0.2:wifi0.0.0.0"
        ),
        "nothing sent"
    );
    assert_eq!(
        s.player(&amp.id())["group_members"],
        serde_json::json!([amp.id(), kitchen.id()])
    );

    kitchen.device.lock().unwrap().leader = None;
    s.provider.endpoint(&kitchen.id()).unwrap().refresh().await;
    assert!(eventually(|| s.player(&kitchen.id())["synced_to"].is_null()).await);
    s.provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_speaker_of_another_kind_cannot_join_a_linkplay_group() {
    let s = setup().await;
    let amp = s.device("Living Room", "10.0.0.2", AMP).await;
    Airplay::register(&s.players, "apsonos", "aabbcc00ff01", true);
    assert!(eventually(|| s.playback.player("apsonos", None).is_some()).await);
    let error = s.playback.group("apsonos", &amp.id()).unwrap_err();
    assert_eq!(
        error.details,
        "apsonos can't join Living Room: it can't join a LinkPlay group"
    );
    s.provider.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_follower_leaves_its_native_group_to_join_over_the_airplay_it_withdrew() {
    let s = setup().await;
    let amp = s.device("Living Room", "10.0.0.2", AMP).await;
    let kitchen_fake = s.network.device("Kitchen", "10.0.0.3", KITCHEN, true).await;
    kitchen_fake.device.lock().unwrap().leader = Some(AMP.into());
    // Its AirPlay, withdrawn while it follows: the same MAC, so one device.
    let mac = kitchen_fake
        .device
        .lock()
        .unwrap()
        .mac
        .replace(':', "")
        .to_lowercase();
    let kitchen_airplay = Airplay::register(&s.players, "apkitchen", &mac, false);
    kitchen_fake.add_to(&s.provider);
    s.provider
        .endpoint(&kitchen_fake.id())
        .unwrap()
        .refresh()
        .await;
    let sonos = Airplay::register(&s.players, "apsonos", "aabbcc00ff01", true);
    assert!(eventually(|| s.playback.player("apsonos", None).is_some()).await);
    // The device takes the AirPlay endpoint's id, which it was found by first.
    let kitchen = "apkitchen";
    assert!(
        eventually(|| s
            .playback
            .player(kitchen, None)
            .is_some_and(|p| p["synced_to"] == amp.id()))
        .await
    );
    assert!(
        s.player("apsonos")["can_group_with"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(kitchen)),
        "offered, since it can join once it leaves"
    );

    s.playback.group(kitchen, "apsonos").unwrap();
    assert!(eventually(|| has(&amp, "multiroom:SlaveKickout:10.0.0.3")).await);
    kitchen_airplay.set_available(true);
    assert!(eventually(|| s.player(kitchen)["synced_to"] == "apsonos").await);
    assert_eq!(s.player(&amp.id())["group_members"], serde_json::json!([]));
    drop(sonos);
    s.provider.shutdown();
}
