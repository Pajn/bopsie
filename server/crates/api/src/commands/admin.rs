//! What the server knows, for the web UI's admin pages: every device and endpoint as the
//! server sees it, how each group plays, and the server itself. Admins only, and this
//! server's own: Music Assistant has none of these.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use bopsie_audio::OutputFormat;
use bopsie_core::Result;
use bopsie_core::config::Config;
use bopsie_core::media::MediaType;
use bopsie_library::ListQuery;
use bopsie_players::planner::{Plan, Strategy};
use bopsie_players::settings::Sourced;
use bopsie_players::{Capabilities, Device, Facts, Member};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::Scope;
use crate::command::{Access, Command, Context};
use crate::commands::auth::NoArgs;

const ADMIN: Access = Access::Scope(Scope::Admin);

/// Telemetry events kept per endpoint in an answer, newest first.
const HISTORY: usize = 50;

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub available: bool,
    pub max_volume: Option<u8>,
    /// What it is called in the Spotify app: its own Spotify name, else the config file's
    /// pattern applied to its name.
    pub spotify_name: String,
    /// What it is called in the Spotify app without a Spotify name of its own.
    pub spotify_default_name: String,
    /// Each setting and the layer it comes from: `default`, `config` or `saved`.
    pub settings: DeviceSettingsView,
    /// Its endpoints, in order of protocol preference.
    pub members: Vec<EndpointView>,
    /// The sources announced for it, such as its Spotify Connect device.
    pub sources: Vec<SourceView>,
}

/// A source announced for a device, as it stands.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SourceView {
    pub id: String,
    pub name: String,
    /// Someone picked it, as in the Spotify app, and it has a session.
    pub connected: bool,
    pub playing: bool,
    /// What it plays: title, and artist when known.
    pub title: Option<String>,
    pub artist: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceSettingsView {
    pub name: SourcedView<Option<String>>,
    pub enabled: SourcedView<bool>,
    pub max_volume: SourcedView<Option<u8>>,
    pub spotify_connect: SourcedView<bool>,
    pub spotify_name: SourcedView<Option<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EndpointSettingsView {
    /// Whether the device may play over this endpoint.
    pub enabled: SourcedView<bool>,
    /// Whether a password is set; it is never shown.
    pub password: SourcedView<bool>,
}

/// A setting's value and the layer it comes from: `default`, `config` or `saved`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SourcedView<T> {
    pub value: T,
    pub layer: String,
}

fn sourced<T, U>(setting: Sourced<T>, map: impl FnOnce(T) -> U) -> SourcedView<U> {
    SourcedView {
        layer: to_name(&setting.layer),
        value: map(setting.value),
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EndpointView {
    pub id: String,
    /// The protocol, such as `airplay`.
    pub provider: String,
    pub name: String,
    pub available: bool,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub software_version: Option<String>,
    /// An app's own player.
    pub app: bool,
    /// The endpoint it follows in its vendor's multiroom.
    pub native_leader: Option<String>,
    pub facts: FactsView,
    pub capabilities: CapabilitiesView,
    /// Each quirk's value.
    pub quirks: Value,
    /// The layer that set each quirk: `default`, a shipped `rule` by name, or `config`.
    pub quirk_sources: Value,
    pub health: HealthView,
    /// What the protocol knows beyond the above, in its own shape.
    pub diagnostics: Value,
    /// Turned off in the device's settings.
    pub disabled: bool,
    pub settings: EndpointSettingsView,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FactsView {
    pub mac: Option<String>,
    pub serial: Option<String>,
    pub uuid: Option<String>,
    pub address: Option<String>,
    /// AirPlay's feature bits, in hex.
    pub features: Option<String>,
    /// AirPlay's status flags, in hex.
    pub status_flags: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CapabilitiesView {
    pub formats: Vec<FormatView>,
    pub lead_ms: i64,
    pub sync_group: bool,
    pub max_group_size: Option<usize>,
    pub volume: bool,
    pub mute: bool,
    pub pairing: String,
    /// The vendor multiroom it groups by, as `family` and `version`.
    pub native_group: Option<NativeGroupView>,
    /// Protocols it withdraws while it follows another natively.
    pub withdrawn: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NativeGroupView {
    pub family: String,
    pub version: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FormatView {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits: u16,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HealthView {
    /// `good`, `degraded` or `failing`.
    pub level: String,
    /// How often each event happened since the server started.
    pub counts: Value,
    /// The latest events, newest first.
    pub recent: Vec<HealthEvent>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HealthEvent {
    pub event: String,
    /// How long ago, in seconds.
    pub ago_s: f64,
}

fn format(format: &OutputFormat) -> FormatView {
    FormatView {
        sample_rate: format.sample_rate,
        channels: format.channels,
        bits: format.bits,
    }
}

fn facts(facts: &Facts) -> FactsView {
    FactsView {
        mac: facts.mac.clone(),
        serial: facts.serial.clone(),
        uuid: facts.uuid.clone(),
        address: facts.address.map(|address| address.to_string()),
        features: facts.features.map(|bits| format!("{bits:#x}")),
        status_flags: facts.status_flags.map(|bits| format!("{bits:#x}")),
    }
}

fn capabilities(capabilities: &Capabilities) -> CapabilitiesView {
    CapabilitiesView {
        formats: capabilities.formats.iter().map(format).collect(),
        lead_ms: capabilities.lead / 1_000,
        sync_group: capabilities.sync_group,
        max_group_size: capabilities.max_group_size,
        volume: capabilities.volume,
        mute: capabilities.mute,
        pairing: to_name(&capabilities.pairing),
        native_group: capabilities.native_group.map(|group| NativeGroupView {
            family: group.family.to_owned(),
            version: group.version,
        }),
        withdrawn: capabilities
            .withdrawn
            .iter()
            .map(|p| (*p).to_owned())
            .collect(),
    }
}

/// A unit enum's serialized name, such as `good`.
fn to_name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(name)) => name,
        _ => String::new(),
    }
}

fn endpoint(ctx: &Context, member: &Member) -> EndpointView {
    let state = &member.state;
    let players = ctx.api.playback.devices().players().clone();
    let recent = players
        .history(&member.id)
        .into_iter()
        .take(HISTORY)
        .map(|(ago, event)| HealthEvent {
            event: to_name(&event),
            ago_s: ago.as_secs_f64(),
        })
        .collect();
    let sources: serde_json::Map<String, Value> = state
        .quirks
        .sources
        .iter()
        .map(|(quirk, source)| ((*quirk).to_owned(), json!(source)))
        .collect();
    EndpointView {
        id: member.id.clone(),
        provider: state.provider.to_owned(),
        name: state.name.clone(),
        available: state.available,
        volume: state.volume,
        muted: state.muted,
        model: state.device.model.clone(),
        manufacturer: state.device.manufacturer.clone(),
        software_version: state.device.software_version.clone(),
        app: state.app,
        native_leader: state.native_leader.clone(),
        facts: facts(&state.facts),
        capabilities: capabilities(&state.capabilities),
        quirks: json!(state.quirks.quirks),
        quirk_sources: Value::Object(sources),
        health: HealthView {
            level: to_name(&member.health.level),
            counts: json!(member.health.counts),
            recent,
        },
        diagnostics: member.endpoint.diagnostics(),
        disabled: member.disabled,
        settings: {
            let settings = players.endpoint_settings(&member.id);
            EndpointSettingsView {
                enabled: sourced(settings.enabled, |v| v),
                password: sourced(settings.password, |p| p.is_some()),
            }
        },
    }
}

fn device(ctx: &Context, device: &Device) -> DeviceView {
    DeviceView {
        id: device.id.clone(),
        name: device.name.clone(),
        enabled: device.enabled,
        available: device.available(),
        max_volume: device.max_volume,
        spotify_name: device
            .spotify_name
            .clone()
            .unwrap_or_else(|| ctx.api.config.spotify.device_name(&device.name)),
        spotify_default_name: ctx.api.config.spotify.device_name(&device.name),
        settings: {
            let settings = ctx
                .api
                .playback
                .devices()
                .players()
                .device_settings(&device.id);
            DeviceSettingsView {
                name: sourced(settings.name, |v| v),
                enabled: sourced(settings.enabled, |v| v),
                max_volume: sourced(settings.max_volume, |v| v),
                spotify_connect: sourced(settings.spotify_connect, |v| v),
                spotify_name: sourced(settings.spotify_name, |v| v),
            }
        },
        members: device
            .members
            .iter()
            .map(|member| endpoint(ctx, member))
            .collect(),
        sources: ctx
            .api
            .sources
            .of_player(&device.id)
            .iter()
            .map(|source| {
                let state = source.state();
                SourceView {
                    id: source.id().to_owned(),
                    name: source.name(),
                    connected: state.connected,
                    playing: state.playing,
                    title: state.media.as_ref().map(|m| m.title.clone()),
                    artist: state.media.and_then(|m| m.artist),
                }
            })
            .collect(),
    }
}

pub struct Devices;
impl Command for Devices {
    const NAME: &'static str = "admin/devices";
    const ACCESS: Access = ADMIN;
    const SUMMARY: &'static str = "Every device and its endpoints, with all the server knows of each: facts, capabilities, quirks and where each came from, health, and the protocol's own state.";
    type Args = NoArgs;
    type Output = Vec<DeviceView>;

    async fn handle(ctx: Context, _: NoArgs) -> Result<Vec<DeviceView>> {
        let devices = ctx.api.playback.devices().all();
        Ok(devices.iter().map(|d| device(&ctx, d)).collect())
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GroupView {
    /// The device leading the group, whose queue it plays.
    pub leader: String,
    /// The devices following it, in the order they joined.
    pub members: Vec<String>,
    /// How it plays, while it has an engine.
    pub plan: Option<PlanView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlanView {
    /// `airplay_direct`, `sendspin_direct` or `native`.
    pub strategy: String,
    /// The endpoints that carry the audio, the leader's first.
    pub carriers: Vec<CarrierView>,
    pub sample_rate: u32,
    pub channels: u16,
    /// How far ahead of now playback starts.
    pub lead_ms: i64,
    /// Devices left out, and why.
    pub dropped: Vec<DroppedView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CarrierView {
    pub device: String,
    pub endpoint: String,
    pub format: FormatView,
    /// Carried by the leader's own multiroom rather than streamed to.
    pub native: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DroppedView {
    pub device: String,
    pub name: String,
    /// What to tell the user.
    pub message: String,
}

fn plan(plan: &Plan, leader: &str) -> PlanView {
    PlanView {
        strategy: match plan.strategy {
            Strategy::AirplayDirect => "airplay_direct",
            Strategy::SendspinDirect => "sendspin_direct",
            Strategy::Native => "native",
        }
        .to_owned(),
        carriers: plan
            .carriers
            .iter()
            .map(|carrier| CarrierView {
                device: carrier.device.clone(),
                endpoint: carrier.endpoint.clone(),
                format: format(&carrier.format),
                native: carrier.native,
            })
            .collect(),
        sample_rate: plan.sample_rate,
        channels: plan.channels,
        lead_ms: plan.lead / 1_000,
        dropped: plan
            .dropped
            .iter()
            .map(|dropped| DroppedView {
                device: dropped.device.clone(),
                name: dropped.name.clone(),
                message: dropped.message(leader),
            })
            .collect(),
    }
}

pub struct Groups;
impl Command for Groups {
    const NAME: &'static str = "admin/groups";
    const ACCESS: Access = ADMIN;
    const SUMMARY: &'static str =
        "Each device that leads a group or plays, with its members and the plan it plays by.";
    type Args = NoArgs;
    type Output = Vec<GroupView>;

    async fn handle(ctx: Context, _: NoArgs) -> Result<Vec<GroupView>> {
        let devices = ctx.api.playback.devices();
        Ok(ctx
            .api
            .playback
            .groups()
            .into_iter()
            .map(|group| {
                let name = devices
                    .get(&group.leader)
                    .map_or_else(|| group.leader.clone(), |device| device.name);
                GroupView {
                    plan: group.plan.as_ref().map(|p| plan(p, &name)),
                    leader: group.leader,
                    members: group.members,
                }
            })
            .collect())
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ServerView {
    pub version: String,
    /// Seconds since the server started.
    pub uptime_s: u64,
    /// Which player protocols run.
    pub providers: ProvidersView,
    pub ffmpeg: bool,
    pub library: LibraryView,
    /// The config file as the server read it, defaults filled in, without passwords.
    pub config: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProvidersView {
    pub airplay: bool,
    /// The shared PTP daemon's process id, while it runs.
    pub airplay_ptp_daemon: Option<u32>,
    pub linkplay: bool,
    pub sendspin: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryView {
    /// Each music provider's instance id.
    pub providers: Vec<String>,
    pub artists: u32,
    pub albums: u32,
    pub tracks: u32,
    pub playlists: u32,
    pub syncing: bool,
    /// The last sync since the server started.
    pub last_sync: Option<SyncView>,
    /// Looking albums up on MusicBrainz.
    pub metadata: MetadataView,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MetadataView {
    /// Whether albums are looked up at all, as `[metadata] musicbrainz` says.
    pub enabled: bool,
    /// A run is under way.
    pub running: bool,
    pub albums: u32,
    /// Albums looked up since they last changed or were refreshed.
    pub checked: u32,
    /// Albums identified as a release.
    pub identified: u32,
    /// Albums with a release suggested, awaiting a person.
    pub suggested: u32,
    /// The last run that finished since the server started.
    pub last_run: Option<MetadataRunView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MetadataRunView {
    /// Unix seconds.
    pub finished: i64,
    pub matched: usize,
    pub suggested: usize,
    pub unmatched: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncView {
    /// Unix seconds.
    pub started: i64,
    pub finished: i64,
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    /// Providers whose scan failed, with why.
    pub failed: Vec<SyncFailure>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncFailure {
    pub provider: String,
    pub error: String,
}

/// The config, with what must not be shown left out: player passwords say only that one is
/// set.
pub fn redacted(config: &Config) -> Value {
    let players: serde_json::Map<String, Value> = config
        .players
        .iter()
        .map(|(id, player)| {
            (
                id.clone(),
                json!({
                    "name": player.name,
                    "enabled": player.enabled,
                    "password": player.password.is_some(),
                    "quirks": player.quirks,
                }),
            )
        })
        .collect();
    json!({
        "server": {
            "bind": config.server.bind.to_string(),
            "name": config.server.name,
            "base_url": config.server.base_url,
        },
        "storage": {"data_dir": config.storage.data_dir},
        "library": {"folders": config.library.folders},
        "sendspin": {"bind": config.sendspin.bind.to_string()},
        "audio": {"ffmpeg": config.audio.ffmpeg},
        "airplay": {"binary": config.airplay.binary, "ptp": config.airplay.ptp},
        "linkplay": {
            "enabled": config.linkplay.enabled,
            "certificate": config.linkplay.certificate,
        },
        "web": {"dir": config.web.dir},
        "planner": {"protocols": config.planner.protocols},
        "players": players,
    })
}

pub struct Server;
impl Command for Server {
    const NAME: &'static str = "admin/server";
    const ACCESS: Access = ADMIN;
    const SUMMARY: &'static str = "The server itself: version, uptime, what runs, the library and its last sync, and the config.";
    type Args = NoArgs;
    type Output = ServerView;

    async fn handle(ctx: Context, _: NoArgs) -> Result<ServerView> {
        let api = &ctx.api;
        let count = |media_type| api.library.count(media_type, ListQuery::default());
        let last_sync = api.library.last_sync().map(|sync| SyncView {
            started: sync.started,
            finished: sync.finished,
            added: sync.added,
            updated: sync.updated,
            deleted: sync.deleted,
            failed: sync
                .failed
                .into_iter()
                .map(|(provider, error)| SyncFailure { provider, error })
                .collect(),
        });
        let (run, status) = api.library.metadata_status().await?;
        let metadata = MetadataView {
            enabled: api.config.metadata.musicbrainz,
            running: run.running,
            albums: status.albums,
            checked: status.checked,
            identified: status.identified,
            suggested: status.suggested,
            last_run: run.last.map(|last| MetadataRunView {
                finished: last.finished,
                matched: last.matched,
                suggested: last.suggested,
                unmatched: last.unmatched,
                failed: last.failed,
            }),
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(ServerView {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            uptime_s: now.saturating_sub(api.started),
            providers: ProvidersView {
                airplay: api.airplay.is_some(),
                airplay_ptp_daemon: api.airplay.as_ref().and_then(|a| a.ptp_daemon()),
                linkplay: api.linkplay.is_some(),
                sendspin: true,
            },
            ffmpeg: api.ffmpeg,
            library: LibraryView {
                providers: api.library.providers(),
                artists: count(MediaType::Artist).await?,
                albums: count(MediaType::Album).await?,
                tracks: count(MediaType::Track).await?,
                playlists: count(MediaType::Playlist).await?,
                syncing: api.library.syncing(),
                last_sync,
                metadata,
            },
            config: redacted(&api.config),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsArgs {
    /// Only lines after this one, by `seq`. Omitted, the latest lines.
    #[serde(default)]
    pub after: Option<u64>,
    /// At most this many of the newest lines. 500 unless given.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LogLine {
    /// Counts up, so a reader asks for what came after the last it has.
    pub seq: u64,
    pub unix_ms: u64,
    pub level: String,
    /// The module that logged it.
    pub target: String,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

pub struct Logs;
impl Command for Logs {
    const NAME: &'static str = "admin/logs";
    const ACCESS: Access = ADMIN;
    const SUMMARY: &'static str = "The log's latest lines, oldest first: what it prints, from the last 2000 lines the server keeps.";
    type Args = LogsArgs;
    type Output = Vec<LogLine>;

    async fn handle(ctx: Context, args: LogsArgs) -> Result<Vec<LogLine>> {
        Ok(ctx
            .api
            .logs
            .after(args.after.unwrap_or(0), args.limit.unwrap_or(500))
            .into_iter()
            .map(|line| LogLine {
                seq: line.seq,
                unix_ms: line.unix_ms,
                level: line.level,
                target: line.target,
                message: line.message,
                fields: line.fields,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::lifetime::TokenKind;
    use bopsie_core::ErrorCode;

    #[test]
    fn the_config_shows_no_passwords() {
        let config = Config::parse(
            "[players.ap1]\nname = \"Kitchen\"\npassword = \"hunter22\"\n[players.ap2]\n",
        )
        .unwrap();
        let view = redacted(&config);
        assert_eq!(view["players"]["ap1"]["name"], "Kitchen");
        assert_eq!(view["players"]["ap1"]["password"], true);
        assert_eq!(view["players"]["ap2"]["password"], false);
        assert!(!view.to_string().contains("hunter22"));
    }

    #[tokio::test]
    async fn only_admins_may_read_them() {
        let api = crate::tests::api().await;
        let mut registry = crate::command::Registry::default();
        registry
            .add::<Devices>()
            .add::<Groups>()
            .add::<Server>()
            .add::<Logs>();
        let user = api
            .auth
            .create_first_admin("alice", "correct horse", None)
            .await
            .unwrap()
            .unwrap();
        let token = api
            .auth
            .issue(&user.user_id, "test", TokenKind::Session)
            .await
            .unwrap();
        let mut session = api.auth.authenticate(&token).await.unwrap();
        for command in [
            "admin/devices",
            "admin/groups",
            "admin/server",
            "admin/logs",
        ] {
            let admin = Context {
                api: api.clone(),
                session: Some(session.clone()),
            };
            assert!(registry.call(admin, command, json!({})).await.is_ok());
        }
        session.user.role = "user".to_owned();
        for command in [
            "admin/devices",
            "admin/groups",
            "admin/server",
            "admin/logs",
        ] {
            let user = Context {
                api: api.clone(),
                session: Some(session.clone()),
            };
            let error = registry.call(user, command, json!({})).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::InsufficientPermissions, "{command}");
            assert_eq!(error.details, "This command requires the admin scope");
        }
    }

    #[tokio::test]
    async fn the_log_is_read_after_the_last_line_seen() {
        use tracing_subscriber::layer::SubscriberExt;

        let api = crate::tests::api().await;
        let subscriber = tracing_subscriber::registry().with(api.logs.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(speaker = "kitchen", "first");
            tracing::warn!("second");
        });
        let ctx = || Context {
            api: api.clone(),
            session: None,
        };
        let all = Logs::handle(
            ctx(),
            LogsArgs {
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].message, "first");
        assert_eq!(all[0].fields["speaker"], "kitchen");
        assert_eq!(all[1].level, "WARN");
        let after = Logs::handle(
            ctx(),
            LogsArgs {
                after: Some(all[0].seq),
                limit: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].message, "second");
    }
}
