//! Player settings: the config file's, with what was saved from the web UI over them. A saved
//! value wins; clearing it falls back to the config file, then to the default.
//!
//! A device's settings (name, enabled, maximum volume) are kept apart from an endpoint's
//! (whether its protocol is on, its password, its quirks), since a device's id is often one of
//! its endpoints' ids too.

use std::collections::BTreeMap;

use bopsie_core::config::PlayerConfig;
use bopsie_core::quirks::{Quirks, Source};
use serde::{Deserialize, Serialize};

/// Which layer a setting comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Default,
    Config,
    Saved,
}

/// A device's settings as saved from the web UI. `None` is not set here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedDevice {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub max_volume: Option<u8>,
    pub spotify_connect: Option<bool>,
    pub spotify_name: Option<String>,
}

/// An endpoint's settings as saved from the web UI. `None` is not set here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedEndpoint {
    /// Whether the device may play over this endpoint's protocol.
    pub enabled: Option<bool>,
    pub password: Option<String>,
    pub quirks: Quirks,
}

/// A setting and the layer it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    pub layer: Layer,
}

/// A device's settings, layered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSettings {
    /// The name shown, instead of the one the device gives itself.
    pub name: Sourced<Option<String>>,
    pub enabled: Sourced<bool>,
    pub max_volume: Sourced<Option<u8>>,
    /// Whether it shows in the Spotify app, if it is a speaker.
    pub spotify_connect: Sourced<bool>,
    /// What it is called in the Spotify app, instead of the config file's pattern.
    pub spotify_name: Sourced<Option<String>>,
}

/// An endpoint's settings, layered. Quirks are layered by [`crate::quirks::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointSettings {
    pub enabled: Sourced<bool>,
    pub password: Sourced<Option<String>>,
    /// The quirk layers over the shipped rules: the config file's, then the saved ones.
    pub quirks: Vec<(Quirks, Source)>,
}

/// The config file's settings and the saved ones.
#[derive(Debug, Default)]
pub struct Layers {
    pub file: BTreeMap<String, PlayerConfig>,
    pub devices: BTreeMap<String, SavedDevice>,
    pub endpoints: BTreeMap<String, SavedEndpoint>,
}

fn layered<T>(saved: Option<T>, file: Option<T>, default: T) -> Sourced<T> {
    match (saved, file) {
        (Some(value), _) => Sourced {
            value,
            layer: Layer::Saved,
        },
        (None, Some(value)) => Sourced {
            value,
            layer: Layer::Config,
        },
        (None, None) => Sourced {
            value: default,
            layer: Layer::Default,
        },
    }
}

impl Layers {
    pub fn device(&self, id: &str) -> DeviceSettings {
        let file = self.file.get(id);
        let saved = self.devices.get(id).cloned().unwrap_or_default();
        DeviceSettings {
            name: layered(
                saved.name.map(Some),
                file.and_then(|f| f.name.clone()).map(Some),
                None,
            ),
            // The config file's `enabled` defaults to true, so only a `false` there is said.
            enabled: layered(
                saved.enabled,
                file.filter(|f| !f.enabled).map(|_| false),
                true,
            ),
            max_volume: layered(
                saved.max_volume.map(Some),
                file.and_then(|f| f.max_volume).map(Some),
                None,
            ),
            spotify_connect: layered(
                saved.spotify_connect,
                file.and_then(|f| f.spotify_connect),
                true,
            ),
            spotify_name: layered(
                saved.spotify_name.map(Some),
                file.and_then(|f| f.spotify_name.clone()).map(Some),
                None,
            ),
        }
    }

    pub fn endpoint(&self, id: &str) -> EndpointSettings {
        let file = self.file.get(id);
        let saved = self.endpoints.get(id).cloned().unwrap_or_default();
        let mut quirks = Vec::new();
        if let Some(file) = file {
            quirks.push((file.quirks.clone(), Source::Config));
        }
        quirks.push((saved.quirks, Source::Saved));
        EndpointSettings {
            enabled: layered(saved.enabled, None, true),
            password: layered(
                saved.password.map(Some),
                file.and_then(|f| f.password.clone()).map(Some),
                None,
            ),
            quirks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers() -> Layers {
        let mut file = BTreeMap::new();
        file.insert(
            "kitchen".to_owned(),
            PlayerConfig {
                name: Some("Kitchen".into()),
                enabled: false,
                password: Some("from file".into()),
                max_volume: Some(60),
                ..PlayerConfig::default()
            },
        );
        Layers {
            file,
            ..Layers::default()
        }
    }

    #[test]
    fn the_config_file_is_under_what_was_saved() {
        let mut layers = layers();
        let device = layers.device("kitchen");
        assert_eq!(device.name.value.as_deref(), Some("Kitchen"));
        assert_eq!(device.name.layer, Layer::Config);
        assert_eq!(
            (device.enabled.value, device.enabled.layer),
            (false, Layer::Config)
        );
        assert_eq!(device.max_volume.value, Some(60));

        layers.devices.insert(
            "kitchen".into(),
            SavedDevice {
                name: Some("Cooking".into()),
                enabled: Some(true),
                max_volume: None,
                spotify_connect: Some(false),
                spotify_name: Some("Cooking speaker".into()),
            },
        );
        let device = layers.device("kitchen");
        assert_eq!(
            (device.spotify_connect.value, device.spotify_connect.layer),
            (false, Layer::Saved)
        );
        assert_eq!(
            device.spotify_name.value.as_deref(),
            Some("Cooking speaker")
        );
        assert_eq!(device.name.value.as_deref(), Some("Cooking"));
        assert_eq!(device.name.layer, Layer::Saved);
        assert_eq!(
            (device.enabled.value, device.enabled.layer),
            (true, Layer::Saved)
        );
        assert_eq!(
            device.max_volume.layer,
            Layer::Config,
            "not saved: the file's"
        );
    }

    #[test]
    fn what_neither_sets_is_the_default() {
        let layers = layers();
        let device = layers.device("other");
        assert_eq!(device.name.value, None);
        assert_eq!(
            (device.enabled.value, device.enabled.layer),
            (true, Layer::Default)
        );
        let endpoint = layers.endpoint("other");
        assert!(endpoint.enabled.value);
        assert_eq!(endpoint.password.layer, Layer::Default);
    }

    #[test]
    fn endpoint_settings_layer_too() {
        let mut layers = layers();
        assert_eq!(
            layers.endpoint("kitchen").password.value.as_deref(),
            Some("from file")
        );
        layers.endpoints.insert(
            "kitchen".into(),
            SavedEndpoint {
                enabled: Some(false),
                password: Some("saved".into()),
                quirks: Quirks {
                    sync_adjust_ms: Some(20),
                    ..Quirks::default()
                },
            },
        );
        let endpoint = layers.endpoint("kitchen");
        assert_eq!(
            (endpoint.enabled.value, endpoint.enabled.layer),
            (false, Layer::Saved)
        );
        assert_eq!(endpoint.password.value.as_deref(), Some("saved"));
        let sources: Vec<&Source> = endpoint.quirks.iter().map(|(_, s)| s).collect();
        assert_eq!(sources, [&Source::Config, &Source::Saved]);
    }
}
