//! The server's TOML config file. Every key has a default, so a missing file means a server
//! with defaults; an unknown key is an error, so a typo does not silently do nothing.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::quirks::Quirks;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub library: LibraryConfig,
    pub sendspin: SendspinConfig,
    pub audio: AudioConfig,
    pub airplay: AirplayConfig,
    pub linkplay: LinkplayConfig,
    pub spotify: SpotifyConfig,
    pub web: WebConfig,
    /// Settings for one player, by player id.
    pub players: BTreeMap<String, PlayerConfig>,
    pub planner: PlannerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlannerConfig {
    /// The protocols a group may play over, in order of preference.
    pub protocols: Vec<String>,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            protocols: vec!["airplay".to_owned(), "sendspin".to_owned()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Address and port for the API, `/ws` and `/sendspin`. 8095 is Music Assistant's port.
    pub bind: SocketAddr,
    /// The name clients show for this server.
    pub name: String,
    /// The URL clients reach the server at, reported in server info. Unset, the server reports
    /// none rather than guess one from the bind address.
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Where the database lives. Relative paths are relative to the working directory.
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryConfig {
    /// Folders of music to scan. Each is a filesystem provider of its own.
    pub folders: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendspinConfig {
    /// Address and port for Sendspin clients that connect directly, without an API token.
    /// 8927 is the port Sendspin clients look for.
    pub bind: SocketAddr,
}

impl Default for SendspinConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8927)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    /// The ffmpeg binary. Unset, `ffmpeg` is used if it is on the path.
    pub ffmpeg: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AirplayConfig {
    /// The `cliairplay` binary. Unset, `cliairplay` is used if it is on the path; without it
    /// there is no AirPlay.
    pub binary: Option<PathBuf>,
    /// Run one shared PTP clock for AirPlay 2 groups. It needs UDP ports 319 and 320.
    pub ptp: bool,
}

impl Default for AirplayConfig {
    fn default() -> Self {
        Self {
            binary: None,
            ptp: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpotifyConfig {
    /// Show speakers in the Spotify app as Spotify Connect devices, each one that its settings
    /// do not turn off.
    pub enabled: bool,
}

impl Default for SpotifyConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LinkplayConfig {
    /// Find and play to LinkPlay speakers: WiiM, Audio Pro and others.
    pub enabled: bool,
    /// A PEM file with the client certificate and key to present to devices that ask for one,
    /// instead of the LinkPlay certificate the server carries.
    pub certificate: Option<PathBuf>,
}

impl Default for LinkplayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            certificate: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebConfig {
    /// Serve the web UI from this directory instead of the one built into the server.
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerConfig {
    /// The name shown, instead of the one the player gives itself.
    pub name: Option<String>,
    /// A disabled player is not shown and cannot be played to.
    pub enabled: bool,
    /// For speakers that ask for one.
    pub password: Option<String>,
    /// The loudest the player may be set, 0 to 100. The server holds it there whoever turns
    /// it up.
    pub max_volume: Option<u8>,
    /// Whether the player shows in the Spotify app as a Spotify Connect device; by default a
    /// speaker does.
    pub spotify_connect: Option<bool>,
    /// Over the shipped quirk rules.
    pub quirks: Quirks,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            name: None,
            enabled: true,
            password: None,
            max_volume: None,
            spotify_connect: None,
            quirks: Quirks::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8095)),
            name: "Bopsie".to_owned(),
            base_url: None,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Config {
    /// Reads the config file at `path`, or returns the defaults when there is none.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_the_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn keys_override_their_defaults_only() {
        let config = Config::parse(
            r#"
            [server]
            bind = "127.0.0.1:9000"
            base_url = "http://music.local:9000"
            "#,
        )
        .unwrap();
        assert_eq!(config.server.bind, SocketAddr::from(([127, 0, 0, 1], 9000)));
        assert_eq!(
            config.server.base_url.as_deref(),
            Some("http://music.local:9000")
        );
        assert_eq!(config.server.name, "Bopsie");
        assert_eq!(config.storage, StorageConfig::default());
        assert!(config.library.folders.is_empty());
    }

    #[test]
    fn the_container_config_parses() {
        let config = Config::parse(include_str!("../../../packaging/container.toml")).unwrap();
        assert_eq!(config.storage.data_dir, PathBuf::from("/data"));
        assert_eq!(config.library.folders, [PathBuf::from("/music")]);
    }

    #[test]
    fn the_example_config_is_the_defaults() {
        let example = include_str!("../../../config.example.toml");
        assert_eq!(Config::parse(example).unwrap(), Config::default());
    }

    #[test]
    fn library_folders() {
        let config = Config::parse("[library]\nfolders = [\"/music\", \"/more\"]\n").unwrap();
        assert_eq!(
            config.library.folders,
            [PathBuf::from("/music"), PathBuf::from("/more")]
        );
    }

    #[test]
    fn player_settings() {
        let config = Config::parse(
            r#"
            [airplay]
            ptp = false

            [players.apaabbcc001122]
            name = "Kitchen"
            password = "secret"

            [players.apaabbcc001122.quirks]
            sync_adjust_ms = 25
            timing = "ntp"
            "#,
        )
        .unwrap();
        assert!(!config.airplay.ptp);
        let kitchen = &config.players["apaabbcc001122"];
        assert_eq!(kitchen.name.as_deref(), Some("Kitchen"));
        assert!(kitchen.enabled, "enabled unless said otherwise");
        assert_eq!(kitchen.password.as_deref(), Some("secret"));
        assert_eq!(kitchen.quirks.sync_adjust_ms, Some(25));
        assert_eq!(kitchen.quirks.timing, Some(crate::quirks::Timing::Ntp));
        assert!(Config::parse("[players.x.quirks]\nunknown = 1\n").is_err());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Config::parse("[server]\nport = 9000\n").is_err());
        assert!(Config::parse("[stroage]\n").is_err());
    }

    #[test]
    fn no_path_means_defaults() {
        assert_eq!(Config::load(None).unwrap(), Config::default());
    }

    #[test]
    fn a_named_file_that_is_missing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Config::load(Some(&dir.path().join("missing.toml"))),
            Err(ConfigError::Read { .. })
        ));
    }
}
