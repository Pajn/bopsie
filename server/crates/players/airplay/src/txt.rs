//! What a speaker's mDNS TXT records say about it. The bit and flag tests mirror Music
//! Assistant's (`providers/airplay/helpers.py`, `player.py`), which mirror `cliairplay`'s own
//! route selection and pyatv's pairing rules.

use std::collections::BTreeMap;

/// Feature bits.
const SUPPORTS_UNIFIED_MEDIA_CONTROL: u32 = 38;
const SUPPORTS_PTP: u32 = 41;
const SUPPORTS_CORE_UTILS_PAIRING: u32 = 48;

/// Status flags.
const PIN_REQUIRED: u64 = 0x8;
const PASSWORD_REQUIRED: u64 = 0x80;
const LEGACY_PAIRING: u64 = 0x200;

pub type Txt = BTreeMap<String, String>;

/// A features value, `0xLOW` or `0xLOW,0xHIGH`, as a 64-bit mask. Zero when malformed.
pub fn features(value: Option<&str>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    let mut parts = value.split(',');
    let parse = |part: Option<&str>| -> Option<u64> {
        let part = part?.trim();
        u64::from_str_radix(part.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
    };
    let Some(low) = parse(parts.next()) else {
        return 0;
    };
    match parts.next() {
        Some(high) => match parse(Some(high)) {
            Some(high) => low | (high << 32),
            None => 0,
        },
        None => low,
    }
}

fn flags(value: Option<&str>) -> u64 {
    value
        .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

fn bit(mask: u64, bit: u32) -> bool {
    mask >> bit & 1 == 1
}

/// A speaker as its `_airplay._tcp` and `_raop._tcp` records describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Records {
    pub airplay: Option<Txt>,
    pub raop: Option<Txt>,
}

impl Records {
    /// The features mask: the `_airplay` service's, else the `_raop` service's `ft`.
    pub fn features(&self) -> u64 {
        let airplay = self
            .airplay
            .as_ref()
            .and_then(|t| t.get("features").or(t.get("ft")));
        let raop = self.raop.as_ref().and_then(|t| t.get("ft"));
        features(airplay.or(raop).map(String::as_str))
    }

    pub fn airplay2(&self) -> bool {
        let features = self.features();
        bit(features, SUPPORTS_UNIFIED_MEDIA_CONTROL) || bit(features, SUPPORTS_CORE_UTILS_PAIRING)
    }

    pub fn ptp(&self) -> bool {
        bit(self.features(), SUPPORTS_PTP)
    }

    /// Status flags from both services: the legacy pairing bit is often only on `_raop`.
    fn status_flags(&self) -> u64 {
        [&self.raop, &self.airplay]
            .into_iter()
            .flatten()
            .map(|txt| flags(txt.get("sf").or(txt.get("flags")).map(String::as_str)))
            .fold(0, |all, f| all | f)
    }

    pub fn requires_pin(&self) -> bool {
        self.status_flags() & (PIN_REQUIRED | LEGACY_PAIRING) != 0
    }

    pub fn requires_password(&self) -> bool {
        self.status_flags() & PASSWORD_REQUIRED != 0
            || self
                .raop
                .as_ref()
                .and_then(|t| t.get("pw"))
                .is_some_and(|pw| pw == "true")
            || self
                .airplay
                .as_ref()
                .and_then(|t| t.get("pw"))
                .is_some_and(|pw| pw == "true")
    }

    pub fn model(&self) -> Option<&str> {
        self.airplay
            .as_ref()
            .and_then(|t| t.get("model"))
            .or_else(|| self.raop.as_ref().and_then(|t| t.get("am")))
            .map(String::as_str)
    }

    pub fn manufacturer(&self) -> Option<&str> {
        self.airplay
            .as_ref()
            .and_then(|t| t.get("manufacturer"))
            .map(String::as_str)
    }

    /// The `_airplay` records as `cliairplay --txt` takes them, with the `_raop` features
    /// added when only that service carries them.
    pub fn cli_txt(&self) -> Option<String> {
        let mut txt = self.airplay.clone()?;
        if !txt.contains_key("features")
            && !txt.contains_key("ft")
            && let Some(ft) = self.raop.as_ref().and_then(|t| t.get("ft"))
        {
            txt.insert("ft".to_owned(), ft.clone());
        }
        Some(
            txt.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

/// A player id from a MAC address: `ap` and the address in lower-case hex, as Music
/// Assistant names AirPlay players.
pub fn player_id(mac: &str) -> String {
    format!("ap{}", mac.replace([':', '-'], "").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txt(pairs: &[(&str, &str)]) -> Txt {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn features_masks() {
        assert_eq!(features(Some("0x5A7FFFF7,0x1E")), 0x5A7FFFF7 | (0x1E << 32));
        assert_eq!(features(Some("0x445F8A00")), 0x445F8A00);
        assert_eq!(features(Some("nonsense")), 0);
        assert_eq!(features(Some("0x1,zz")), 0);
        assert_eq!(features(None), 0);
    }

    #[test]
    fn airplay2_and_ptp_from_feature_bits() {
        // A HomePod-like mask: bits 38, 41 and 48 set.
        let ap2 = Records {
            airplay: Some(txt(&[("features", "0x4A7FDFD5,0x3C155FDE")])),
            raop: None,
        };
        assert!(ap2.airplay2());
        assert!(ap2.ptp());
        // An AirPlay 1 receiver.
        let raop = Records {
            airplay: None,
            raop: Some(txt(&[("ft", "0x445F8A00")])),
        };
        assert!(!raop.airplay2());
        assert!(!raop.ptp());
        // Features only on _raop still count.
        // Bit 38 (0x40 in the high word) set on _raop only.
        let split = Records {
            airplay: Some(txt(&[("model", "X")])),
            raop: Some(txt(&[("ft", "0x0,0x40")])),
        };
        assert!(split.airplay2());
        assert!(!split.ptp());
        assert_eq!(split.cli_txt().as_deref(), Some("ft=0x0,0x40 model=X"));
    }

    #[test]
    fn pairing_and_password_flags_from_either_service() {
        let pin = Records {
            airplay: Some(txt(&[("flags", "0x8")])),
            raop: None,
        };
        assert!(pin.requires_pin());
        let legacy = Records {
            airplay: Some(txt(&[("flags", "0x4")])),
            raop: Some(txt(&[("sf", "0x204")])),
        };
        assert!(legacy.requires_pin());
        let password = Records {
            airplay: None,
            raop: Some(txt(&[("pw", "true")])),
        };
        assert!(password.requires_password() && !password.requires_pin());
        let plain = Records {
            airplay: Some(txt(&[("flags", "0x4")])),
            raop: None,
        };
        assert!(!plain.requires_pin() && !plain.requires_password());
    }

    #[test]
    fn player_ids() {
        assert_eq!(player_id("AA:BB:CC:00:11:22"), "apaabbcc001122");
        assert_eq!(player_id("AABBCC001122"), "apaabbcc001122");
    }
}
