//! A discovered AirPlay speaker, and how `cliairplay` is told to reach it.

use std::net::IpAddr;
use std::path::Path;

use crate::txt::Records;

/// RTSP ports when a record names none.
const AIRPLAY_PORT: u16 = 7000;
const RAOP_PORT: u16 = 5000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// `ap` and the MAC address.
    pub id: String,
    pub name: String,
    pub address: IpAddr,
    /// The `_airplay._tcp` service's port and host name, when it advertises one.
    pub airplay: Option<(u16, String)>,
    /// The `_raop._tcp` service's port and instance name (`MAC@Name`), when it advertises one.
    pub raop: Option<(u16, String)>,
    pub records: Records,
}

/// What a stream needs beyond the speaker itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamOptions {
    /// The server's DACP id, which is also its pairing identity: it must not change once a
    /// speaker is paired.
    pub dacp_id: String,
    pub active_remote: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub volume: Option<u8>,
    /// Credentials from PIN pairing.
    pub credentials: Option<String>,
    pub password: Option<String>,
    /// Use the shared PTP daemon's clock.
    pub ptp_shared: bool,
}

impl Speaker {
    /// Whether the stream goes over AirPlay 2, which takes the `_airplay` service's port.
    fn uses_airplay2(&self) -> bool {
        self.airplay.is_some() && (self.records.airplay2() || self.raop.is_none())
    }

    /// The arguments for a `cliairplay` stream to this speaker, as Music Assistant builds them.
    pub fn stream_args(&self, options: &StreamOptions, cmdpipe: &Path) -> Vec<String> {
        // With no RAOP fallback, AirPlay 2 is forced: a receiver that advertises no features
        // cannot be recognised from them.
        let protocol = if self.raop.is_none() && self.airplay.is_some() {
            "airplay2"
        } else {
            "auto"
        };
        let mut args: Vec<String> = vec![
            "--protocol".into(),
            protocol.into(),
            "--dacp".into(),
            options.dacp_id.clone(),
            "--activeremote".into(),
            options.active_remote.clone(),
            "--cmdpipe".into(),
            cmdpipe.display().to_string(),
            "--samplerate".into(),
            options.sample_rate.to_string(),
            "--bitdepth".into(),
            options.bit_depth.to_string(),
        ];
        match (&self.airplay, &self.raop) {
            (Some((port, host)), _) if self.uses_airplay2() => {
                args.extend(["--port".into(), port_or(*port, AIRPLAY_PORT)]);
                args.extend(["--name".into(), self.name.clone()]);
                args.extend(["--hostname".into(), host.clone()]);
            }
            (_, Some((port, _))) => args.extend(["--port".into(), port_or(*port, RAOP_PORT)]),
            (Some((port, _)), None) => args.extend(["--port".into(), port_or(*port, AIRPLAY_PORT)]),
            (None, None) => {}
        }
        if let Some((_, instance)) = &self.raop {
            args.extend(["--udn".into(), instance.clone()]);
            if let Some(raop) = &self.records.raop {
                for key in ["et", "md", "am", "pk", "pw", "cn"] {
                    if let Some(value) = raop.get(key) {
                        args.extend([format!("--{key}"), value.clone()]);
                    }
                }
            }
        }
        if let Some(txt) = self.records.cli_txt() {
            args.extend(["--txt".into(), txt]);
        }
        if let Some(volume) = options.volume {
            args.extend(["--volume".into(), volume.to_string()]);
        }
        if let Some(credentials) = &options.credentials {
            args.extend(["--auth".into(), credentials.clone()]);
        }
        if let Some(password) = &options.password {
            args.extend(["--password".into(), password.clone()]);
        }
        if options.ptp_shared && self.uses_airplay2() {
            args.push("--ptp-shared".into());
        }
        args.push(self.address.to_string());
        args
    }
}

fn port_or(port: u16, default: u16) -> String {
    if port == 0 { default } else { port }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::txt::Txt;

    fn txt(pairs: &[(&str, &str)]) -> Txt {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn options() -> StreamOptions {
        StreamOptions {
            dacp_id: "A1B2C3D4E5F60718".into(),
            active_remote: "12345".into(),
            sample_rate: 44_100,
            bit_depth: 16,
            volume: Some(30),
            credentials: None,
            password: None,
            ptp_shared: true,
        }
    }

    #[test]
    fn an_airplay2_speaker_streams_on_its_airplay_port() {
        let speaker = Speaker {
            id: "apaabbcc001122".into(),
            name: "Kitchen".into(),
            address: "192.168.1.50".parse().unwrap(),
            airplay: Some((7000, "Kitchen.local.".into())),
            raop: Some((5000, "AABBCC001122@Kitchen".into())),
            records: Records {
                airplay: Some(txt(&[
                    ("features", "0x4A7FDFD5,0x3C155FDE"),
                    ("model", "Speaker1,1"),
                ])),
                raop: Some(txt(&[("et", "0,4"), ("cn", "0,1")])),
            },
        };
        let args = speaker.stream_args(&options(), Path::new("/tmp/kitchen.fifo"));
        assert_eq!(
            args,
            [
                "--protocol",
                "auto",
                "--dacp",
                "A1B2C3D4E5F60718",
                "--activeremote",
                "12345",
                "--cmdpipe",
                "/tmp/kitchen.fifo",
                "--samplerate",
                "44100",
                "--bitdepth",
                "16",
                "--port",
                "7000",
                "--name",
                "Kitchen",
                "--hostname",
                "Kitchen.local.",
                "--udn",
                "AABBCC001122@Kitchen",
                "--et",
                "0,4",
                "--cn",
                "0,1",
                "--txt",
                "features=0x4A7FDFD5,0x3C155FDE model=Speaker1,1",
                "--volume",
                "30",
                "--ptp-shared",
                "192.168.1.50",
            ]
        );
    }

    #[test]
    fn an_airplay1_speaker_streams_on_its_raop_port_without_a_shared_clock() {
        let speaker = Speaker {
            id: "ap001122334455".into(),
            name: "Old".into(),
            address: "192.168.1.60".parse().unwrap(),
            airplay: None,
            raop: Some((0, "001122334455@Old".into())),
            records: Records {
                airplay: None,
                raop: Some(txt(&[("ft", "0x445F8A00"), ("pw", "true")])),
            },
        };
        let mut options = options();
        options.password = Some("secret".into());
        let args = speaker.stream_args(&options, Path::new("/tmp/old.fifo"));
        let joined = args.join(" ");
        assert!(joined.contains("--port 5000"), "{joined}");
        assert!(joined.contains("--pw true"), "{joined}");
        assert!(joined.contains("--password secret"), "{joined}");
        assert!(!joined.contains("--ptp-shared"));
        assert!(joined.ends_with("192.168.1.60"));
    }

    #[test]
    fn an_airplay_only_speaker_is_forced_to_airplay2() {
        let speaker = Speaker {
            id: "ap1".into(),
            name: "New".into(),
            address: "10.0.0.2".parse().unwrap(),
            airplay: Some((7000, "New.local.".into())),
            raop: None,
            records: Records {
                airplay: Some(Txt::new()),
                raop: None,
            },
        };
        let args = speaker.stream_args(&options(), Path::new("/p"));
        assert_eq!(&args[..2], ["--protocol", "airplay2"]);
    }
}
