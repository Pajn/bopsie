//! The server's TOML config file. Every key has a default, so a missing file means a server
//! with defaults; an unknown key is an error, so a typo does not silently do nothing.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub library: LibraryConfig,
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
