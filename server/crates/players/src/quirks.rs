//! Quirk rules, and resolving an endpoint's quirks in layers, last wins: its protocol's
//! defaults, then the rules in `quirks.toml` that match it, then the config file.

use std::sync::LazyLock;

pub use bopsie_core::quirks::{
    AirplayProtocol, Pairing, Quirks, Resolved, SampleCap, Source, Timing,
};
use serde::Deserialize;

use crate::{DeviceInfo, Facts};

/// A kind of endpoint and how it deviates.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    /// Why, and where that was found.
    pub cause: String,
    #[serde(rename = "match")]
    pub when: Match,
    pub quirks: Quirks,
}

/// What an endpoint must be for a rule to apply. Unset keys match anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Match {
    pub provider: Option<String>,
    /// Exact, ignoring case.
    pub manufacturer: Option<String>,
    /// Ignoring case, with `*` for any run of characters.
    pub model: Option<String>,
    pub firmware: Option<String>,
    /// AirPlay feature bits that must all be set.
    pub features: Vec<u8>,
    /// AirPlay status flag bits that must all be set.
    pub status_flags: Vec<u8>,
}

/// The endpoint a rule is matched against.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    pub provider: &'a str,
    pub device: &'a DeviceInfo,
    pub facts: &'a Facts,
}

impl Match {
    pub fn matches(&self, subject: &Subject<'_>) -> bool {
        let text = |pattern: &Option<String>, value: &Option<String>, glob: bool| match pattern {
            None => true,
            Some(pattern) => value.as_deref().is_some_and(|value| {
                let (pattern, value) = (pattern.to_lowercase(), value.to_lowercase());
                if glob {
                    glob_matches(&pattern, &value)
                } else {
                    pattern == value
                }
            }),
        };
        let bits = |wanted: &[u8], value: Option<u64>| {
            wanted.is_empty()
                || value.is_some_and(|value| {
                    wanted
                        .iter()
                        .all(|&bit| bit < 64 && value & (1 << bit) != 0)
                })
        };
        self.provider
            .as_deref()
            .is_none_or(|provider| provider == subject.provider)
            && text(&self.manufacturer, &subject.device.manufacturer, false)
            && text(&self.model, &subject.device.model, true)
            && text(&self.firmware, &subject.device.software_version, true)
            && bits(&self.features, subject.facts.features)
            && bits(&self.status_flags, subject.facts.status_flags)
    }
}

/// Whether `value` matches `pattern`, where `*` matches any run of characters.
fn glob_matches(pattern: &str, value: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == value,
        Some((prefix, rest)) => {
            let Some(value) = value.strip_prefix(prefix) else {
                return false;
            };
            (0..=value.len())
                .filter(|&i| value.is_char_boundary(i))
                .any(|i| glob_matches(rest, &value[i..]))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    rule: Vec<Rule>,
}

/// The rules that ship with the server.
pub fn shipped() -> &'static [Rule] {
    static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
        toml::from_str::<RuleFile>(include_str!("../quirks.toml"))
            .expect("quirks.toml is checked by the tests")
            .rule
    });
    &RULES
}

/// `subject`'s quirks: `defaults`, then each of `rules` that matches, then each of `layers`
/// in order, such as the config file's and then the saved settings.
pub fn resolve(
    defaults: &Quirks,
    rules: &[Rule],
    subject: &Subject<'_>,
    layers: &[(&Quirks, Source)],
) -> Resolved {
    let mut resolved = Resolved::default();
    resolved.apply(defaults, &Source::Default);
    for rule in rules.iter().filter(|rule| rule.when.matches(subject)) {
        resolved.apply(&rule.quirks, &Source::Rule(rule.name.clone()));
    }
    for (layer, source) in layers {
        resolved.apply(layer, source);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(manufacturer: &str, model: &str, firmware: &str) -> DeviceInfo {
        DeviceInfo {
            manufacturer: Some(manufacturer.into()),
            model: Some(model.into()),
            software_version: Some(firmware.into()),
        }
    }

    fn rule(name: &str, when: Match, quirks: Quirks) -> Rule {
        Rule {
            name: name.into(),
            cause: String::new(),
            when,
            quirks,
        }
    }

    #[test]
    fn the_shipped_rules_parse_and_each_says_why() {
        assert!(!shipped().is_empty());
        for rule in shipped() {
            assert!(!rule.cause.trim().is_empty(), "{} has no cause", rule.name);
        }
    }

    #[test]
    fn sonos_over_airplay_is_capped_at_cd_quality() {
        let sonos = device("Sonos", "One", "p20.97.1-80312");
        let facts = Facts::default();
        let resolved = resolve(
            &Quirks::default(),
            shipped(),
            &Subject {
                provider: "airplay",
                device: &sonos,
                facts: &facts,
            },
            &[],
        );
        assert_eq!(
            resolved.quirks.max_sample_format,
            Some(SampleCap {
                sample_rate: 44_100,
                bits: 16
            })
        );
        assert_eq!(
            resolved.source("max_sample_format"),
            Some(&Source::Rule("Sonos over AirPlay".into()))
        );
        let other = device("Bluesound", "POWERNODE", "p20.4.16.6");
        let resolved = resolve(
            &Quirks::default(),
            shipped(),
            &Subject {
                provider: "airplay",
                device: &other,
                facts: &facts,
            },
            &[],
        );
        assert_eq!(resolved.quirks.max_sample_format, None);
    }

    #[test]
    fn matching() {
        let tv = device("Samsung", "QE55Q80T", "1.2.3");
        let facts = Facts {
            features: Some(1 << 41 | 1 << 9),
            status_flags: Some(0x4),
            ..Facts::default()
        };
        let subject = Subject {
            provider: "airplay",
            device: &tv,
            facts: &facts,
        };
        let cases = [
            (Match::default(), true),
            (
                Match {
                    provider: Some("sendspin".into()),
                    ..Match::default()
                },
                false,
            ),
            (
                Match {
                    manufacturer: Some("samsung".into()),
                    ..Match::default()
                },
                true,
            ),
            (
                Match {
                    manufacturer: Some("Sams*".into()),
                    ..Match::default()
                },
                false,
            ),
            (
                Match {
                    model: Some("qe*q80*".into()),
                    ..Match::default()
                },
                true,
            ),
            (
                Match {
                    model: Some("QE*Q90*".into()),
                    ..Match::default()
                },
                false,
            ),
            (
                Match {
                    firmware: Some("1.*".into()),
                    ..Match::default()
                },
                true,
            ),
            (
                Match {
                    features: vec![41, 9],
                    ..Match::default()
                },
                true,
            ),
            (
                Match {
                    features: vec![41, 38],
                    ..Match::default()
                },
                false,
            ),
            (
                Match {
                    status_flags: vec![2],
                    ..Match::default()
                },
                true,
            ),
        ];
        for (i, (when, expected)) in cases.into_iter().enumerate() {
            assert_eq!(when.matches(&subject), expected, "case {i}: {when:?}");
        }
        let unknown = DeviceInfo {
            manufacturer: None,
            model: None,
            software_version: None,
        };
        let bare = Subject {
            provider: "airplay",
            device: &unknown,
            facts: &Facts::default(),
        };
        let wants_model = Match {
            model: Some("*".into()),
            ..Match::default()
        };
        assert!(
            !wants_model.matches(&bare),
            "an unknown model matches no pattern"
        );
    }

    #[test]
    fn layers_apply_in_order() {
        let tv = device("Samsung", "QE55Q80T", "1.2.3");
        let facts = Facts::default();
        let subject = Subject {
            provider: "airplay",
            device: &tv,
            facts: &facts,
        };
        let rules = [
            rule(
                "AirPlay TVs",
                Match {
                    manufacturer: Some("Samsung".into()),
                    ..Match::default()
                },
                Quirks {
                    timing: Some(Timing::Ntp),
                    min_lead_ms: Some(3000),
                    ..Quirks::default()
                },
            ),
            rule(
                "This model",
                Match {
                    model: Some("QE55*".into()),
                    ..Match::default()
                },
                Quirks {
                    min_lead_ms: Some(4000),
                    ..Quirks::default()
                },
            ),
            rule(
                "Not this one",
                Match {
                    model: Some("HW-*".into()),
                    ..Match::default()
                },
                Quirks {
                    sync_adjust_ms: Some(99),
                    ..Quirks::default()
                },
            ),
        ];
        let defaults = Quirks {
            timing: Some(Timing::Auto),
            protocol: Some(AirplayProtocol::Auto),
            ..Quirks::default()
        };
        let config = Quirks {
            timing: Some(Timing::Ptp),
            ..Quirks::default()
        };
        let saved = Quirks {
            sync_adjust_ms: Some(12),
            ..Quirks::default()
        };
        let resolved = resolve(
            &defaults,
            &rules,
            &subject,
            &[(&config, Source::Config), (&saved, Source::Saved)],
        );
        assert_eq!(resolved.quirks.protocol, Some(AirplayProtocol::Auto));
        assert_eq!(resolved.quirks.min_lead_ms, Some(4000));
        assert_eq!(resolved.quirks.timing, Some(Timing::Ptp));
        assert_eq!(resolved.quirks.sync_adjust_ms, Some(12));
        assert_eq!(resolved.source("sync_adjust_ms"), Some(&Source::Saved));
        assert_eq!(resolved.source("protocol"), Some(&Source::Default));
        assert_eq!(
            resolved.source("min_lead_ms"),
            Some(&Source::Rule("This model".into()))
        );
        assert_eq!(resolved.source("timing"), Some(&Source::Config));
    }

    #[test]
    fn globs() {
        assert!(glob_matches("*", ""));
        assert!(glob_matches("a*c", "abbbc"));
        assert!(glob_matches("a*", "a"));
        assert!(!glob_matches("a*c", "abcd"));
        assert!(glob_matches("*é*", "café au lait"));
    }
}
