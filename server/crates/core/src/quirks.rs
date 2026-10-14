//! Where an endpoint deviates from what its protocol promises. Every field is optional: a set
//! of quirks says only what it changes, so quirk rules and config overrides can be layered.

use serde::{Deserialize, Serialize};

/// How an endpoint has to be paired before it plays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pairing {
    #[default]
    None,
    /// Paired afresh on every connection, without the user.
    Transient,
    /// Paired once with a PIN the speaker shows.
    Pin,
    /// A password on every connection.
    Password,
}

/// The best format an endpoint really plays, whatever it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleCap {
    pub sample_rate: u32,
    pub bits: u16,
}

/// The AirPlay route `cliairplay` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AirplayProtocol {
    /// Chosen from the speaker's records.
    Auto,
    Raop,
    Airplay2,
    /// AirPlay 2's authentication, then the RAOP flow.
    Airplay2Compat,
}

impl AirplayProtocol {
    /// The value of `cliairplay --protocol`.
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Raop => "raop",
            Self::Airplay2 => "airplay2",
            Self::Airplay2Compat => "airplay2-compat",
        }
    }
}

/// How an AirPlay session keeps time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    /// PTP when the speaker advertises it, else NTP.
    Auto,
    Ptp,
    /// For speakers that advertise PTP but never answer it.
    Ntp,
}

impl Timing {
    /// The value of `cliairplay --timing`.
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ptp => "ptp",
            Self::Ntp => "ntp",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Quirks {
    /// The best format to send, for endpoints that accept better and play silence.
    pub max_sample_format: Option<SampleCap>,
    /// A floor on how far ahead playback starts, above the protocol's own.
    pub min_lead_ms: Option<u32>,
    /// Added to when this endpoint is told to play, to line it up with the rest of a group.
    /// Positive plays later.
    pub sync_adjust_ms: Option<i32>,
    /// AirPlay: the route, instead of the one chosen from the speaker's records.
    pub protocol: Option<AirplayProtocol>,
    /// AirPlay: how the session keeps time.
    pub timing: Option<Timing>,
    /// How the endpoint pairs, when detection gets it wrong.
    pub pairing: Option<Pairing>,
    /// Ends the session after this long without playing, for speakers that drop it anyway.
    pub disconnect_on_idle_s: Option<u32>,
}

/// Which layer set a quirk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "layer", content = "name")]
pub enum Source {
    /// The protocol's own behaviour.
    Default,
    /// A shipped quirk rule, by name.
    Rule(String),
    /// The config file.
    Config,
}

macro_rules! each_quirk {
    ($apply:ident) => {
        $apply!(max_sample_format);
        $apply!(min_lead_ms);
        $apply!(sync_adjust_ms);
        $apply!(protocol);
        $apply!(timing);
        $apply!(pairing);
        $apply!(disconnect_on_idle_s);
    };
}

/// Quirks resolved from every layer, and the layer that set each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub quirks: Quirks,
    pub sources: Vec<(&'static str, Source)>,
}

impl Resolved {
    /// Sets every quirk `layer` sets, over what earlier layers set.
    pub fn apply(&mut self, layer: &Quirks, source: &Source) {
        macro_rules! apply {
            ($field:ident) => {
                if layer.$field.is_some() {
                    self.quirks.$field = layer.$field;
                    self.sources.retain(|(name, _)| *name != stringify!($field));
                    self.sources.push((stringify!($field), source.clone()));
                }
            };
        }
        each_quirk!(apply);
    }

    /// The layer that set `quirk`, if any did.
    pub fn source(&self, quirk: &str) -> Option<&Source> {
        self.sources
            .iter()
            .find(|(name, _)| *name == quirk)
            .map(|(_, source)| source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_layers_win_and_say_so() {
        let mut resolved = Resolved::default();
        resolved.apply(
            &Quirks {
                timing: Some(Timing::Auto),
                pairing: Some(Pairing::Transient),
                ..Quirks::default()
            },
            &Source::Default,
        );
        resolved.apply(
            &Quirks {
                timing: Some(Timing::Ntp),
                ..Quirks::default()
            },
            &Source::Rule("TVs".into()),
        );
        resolved.apply(
            &Quirks {
                sync_adjust_ms: Some(-20),
                ..Quirks::default()
            },
            &Source::Config,
        );
        assert_eq!(resolved.quirks.timing, Some(Timing::Ntp));
        assert_eq!(resolved.quirks.pairing, Some(Pairing::Transient));
        assert_eq!(resolved.quirks.sync_adjust_ms, Some(-20));
        assert_eq!(resolved.source("timing"), Some(&Source::Rule("TVs".into())));
        assert_eq!(resolved.source("pairing"), Some(&Source::Default));
        assert_eq!(resolved.source("sync_adjust_ms"), Some(&Source::Config));
        assert_eq!(resolved.source("min_lead_ms"), None);
    }

    #[test]
    fn parses_from_toml() {
        let quirks: Quirks = toml::from_str(
            "max_sample_format = { sample_rate = 44100, bits = 16 }\nprotocol = \"airplay2-compat\"\n",
        )
        .unwrap();
        assert_eq!(
            quirks.max_sample_format,
            Some(SampleCap {
                sample_rate: 44_100,
                bits: 16
            })
        );
        assert_eq!(quirks.protocol, Some(AirplayProtocol::Airplay2Compat));
        assert!(toml::from_str::<Quirks>("sync_ajdust_ms = 3\n").is_err());
    }
}
