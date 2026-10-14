//! Finding speakers: `_airplay._tcp` and `_raop._tcp` services, merged into one speaker per
//! MAC address. A `_raop` instance is named `MAC@Name`; an `_airplay` instance carries its MAC
//! in the `deviceid` TXT record.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use crate::speaker::Speaker;
use crate::txt::{Records, Txt, player_id};

pub const AIRPLAY_SERVICE: &str = "_airplay._tcp.local.";
pub const RAOP_SERVICE: &str = "_raop._tcp.local.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Service {
    Airplay,
    Raop,
}

impl Service {
    pub fn from_type(service_type: &str) -> Option<Self> {
        match service_type {
            AIRPLAY_SERVICE => Some(Service::Airplay),
            RAOP_SERVICE => Some(Service::Raop),
            _ => None,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Service::Airplay => AIRPLAY_SERVICE,
            Service::Raop => RAOP_SERVICE,
        }
    }
}

/// One resolved service instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub service: Service,
    /// The full service name, such as `Kitchen._airplay._tcp.local.`.
    pub fullname: String,
    pub host: String,
    pub port: u16,
    pub address: Option<IpAddr>,
    pub txt: Txt,
}

/// The instance name: the full name without the service type.
fn instance(fullname: &str, service: Service) -> &str {
    fullname
        .strip_suffix(service.suffix())
        .map_or(fullname, |name| name.trim_end_matches('.'))
}

/// The player id and display name a sighting stands for.
fn identify(sighting: &Sighting) -> Option<(String, String)> {
    let instance = instance(&sighting.fullname, sighting.service);
    if let Some((mac, name)) = instance.split_once('@') {
        return Some((player_id(mac), name.to_owned()));
    }
    let mac = sighting.txt.get("deviceid")?;
    Some((player_id(mac), instance.to_owned()))
}

#[derive(Debug, Default)]
struct Partial {
    name: Option<String>,
    address: Option<IpAddr>,
    airplay: Option<(u16, String, Txt)>,
    raop: Option<(u16, String, Txt)>,
    /// Full names of the services currently seen, by service.
    present: BTreeSet<(Service, String)>,
}

/// Every speaker seen, merged across its services.
#[derive(Debug, Default)]
pub struct Directory {
    speakers: BTreeMap<String, Partial>,
    /// Which speaker each service instance belongs to.
    owners: BTreeMap<String, String>,
}

impl Directory {
    /// Records a sighting, and returns the speaker it describes once it has an address.
    pub fn see(&mut self, sighting: Sighting) -> Option<Speaker> {
        let (id, name) = identify(&sighting)?;
        self.owners.insert(sighting.fullname.clone(), id.clone());
        let partial = self.speakers.entry(id.clone()).or_default();
        // The AirPlay name is the one people gave the speaker; RAOP's is a fallback.
        if sighting.service == Service::Airplay || partial.name.is_none() {
            partial.name = Some(name);
        }
        if sighting.address.is_some() {
            partial.address = sighting.address;
        }
        partial
            .present
            .insert((sighting.service, sighting.fullname.clone()));
        let instance_or_host = match sighting.service {
            Service::Airplay => sighting.host.clone(),
            Service::Raop => instance(&sighting.fullname, Service::Raop).to_owned(),
        };
        let record = Some((sighting.port, instance_or_host, sighting.txt));
        match sighting.service {
            Service::Airplay => partial.airplay = record,
            Service::Raop => partial.raop = record,
        }
        Self::speaker(&id, partial)
    }

    fn speaker(id: &str, partial: &Partial) -> Option<Speaker> {
        Some(Speaker {
            id: id.to_owned(),
            name: partial.name.clone()?,
            address: partial.address?,
            airplay: partial
                .airplay
                .as_ref()
                .map(|(port, host, _)| (*port, host.clone())),
            raop: partial
                .raop
                .as_ref()
                .map(|(port, name, _)| (*port, name.clone())),
            records: Records {
                airplay: partial.airplay.as_ref().map(|(_, _, txt)| txt.clone()),
                raop: partial.raop.as_ref().map(|(_, _, txt)| txt.clone()),
            },
        })
    }

    /// Records that a service went away. Returns the speaker's id when none of its services
    /// remain.
    pub fn lose(&mut self, fullname: &str) -> Option<String> {
        let id = self.owners.remove(fullname)?;
        let partial = self.speakers.get_mut(&id)?;
        partial.present.retain(|(_, name)| name != fullname);
        partial.present.is_empty().then_some(id)
    }
}

/// Browses for speakers, one thread per service type, reporting each sighting and loss. The
/// threads end when the daemon stops answering.
pub fn browse(
    daemon: &mdns_sd::ServiceDaemon,
    on_sighting: impl Fn(Sighting) + Send + Sync + 'static,
    on_loss: impl Fn(&str) + Send + Sync + 'static,
) -> Result<Vec<std::thread::JoinHandle<()>>, mdns_sd::Error> {
    let on_sighting = std::sync::Arc::new(on_sighting);
    let on_loss = std::sync::Arc::new(on_loss);
    let mut threads = Vec::new();
    for service_type in [AIRPLAY_SERVICE, RAOP_SERVICE] {
        let events = daemon.browse(service_type)?;
        let (on_sighting, on_loss) = (on_sighting.clone(), on_loss.clone());
        let thread = std::thread::Builder::new()
            .name("airplay-discovery".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    match event {
                        mdns_sd::ServiceEvent::ServiceResolved(resolved) => {
                            let Some(service) = Service::from_type(&resolved.ty_domain) else {
                                continue;
                            };
                            let txt = resolved
                                .txt_properties
                                .iter()
                                .map(|p| (p.key().to_owned(), p.val_str().to_owned()))
                                .collect();
                            on_sighting(Sighting {
                                service,
                                fullname: resolved.fullname.clone(),
                                host: resolved.host.clone(),
                                port: resolved.port,
                                address: resolved
                                    .get_addresses_v4()
                                    .into_iter()
                                    .min()
                                    .map(IpAddr::V4),
                                txt,
                            });
                        }
                        mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => on_loss(&fullname),
                        _ => {}
                    }
                }
            })
            .expect("spawning the discovery thread");
        threads.push(thread);
    }
    Ok(threads)
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

    fn airplay() -> Sighting {
        Sighting {
            service: Service::Airplay,
            fullname: "Kitchen._airplay._tcp.local.".into(),
            host: "Kitchen.local.".into(),
            port: 7000,
            address: Some("192.168.1.50".parse().unwrap()),
            txt: txt(&[
                ("deviceid", "AA:BB:CC:00:11:22"),
                ("features", "0x4A7FDFD5,0x3C155FDE"),
            ]),
        }
    }

    fn raop() -> Sighting {
        Sighting {
            service: Service::Raop,
            fullname: "AABBCC001122@Kitchen Speaker._raop._tcp.local.".into(),
            host: "Kitchen.local.".into(),
            port: 5000,
            address: Some("192.168.1.50".parse().unwrap()),
            txt: txt(&[("et", "0,4")]),
        }
    }

    #[test]
    fn both_services_merge_into_one_speaker() {
        let mut directory = Directory::default();
        let first = directory.see(raop()).unwrap();
        assert_eq!(first.id, "apaabbcc001122");
        assert_eq!(first.name, "Kitchen Speaker");
        assert!(first.airplay.is_none());

        let merged = directory.see(airplay()).unwrap();
        assert_eq!(merged.id, "apaabbcc001122");
        assert_eq!(merged.name, "Kitchen", "the AirPlay name wins");
        assert_eq!(merged.airplay, Some((7000, "Kitchen.local.".into())));
        assert_eq!(
            merged.raop,
            Some((5000, "AABBCC001122@Kitchen Speaker".into()))
        );
        assert!(merged.records.airplay2());
    }

    #[test]
    fn a_speaker_is_lost_when_its_last_service_goes() {
        let mut directory = Directory::default();
        directory.see(airplay());
        directory.see(raop());
        assert_eq!(directory.lose("Kitchen._airplay._tcp.local."), None);
        assert_eq!(
            directory.lose("AABBCC001122@Kitchen Speaker._raop._tcp.local."),
            Some("apaabbcc001122".into())
        );
        assert_eq!(directory.lose("Unknown._raop._tcp.local."), None);
    }

    #[test]
    fn unidentifiable_or_unaddressed_services_wait() {
        let mut directory = Directory::default();
        let mut anonymous = airplay();
        anonymous.txt.remove("deviceid");
        assert_eq!(directory.see(anonymous), None);
        let mut no_address = airplay();
        no_address.address = None;
        assert_eq!(directory.see(no_address), None);
    }
}
