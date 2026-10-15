//! Finding Cast devices: they announce `_googlecast._tcp` over mDNS, with their id, name and
//! model in the TXT record.

use std::collections::HashMap;
use std::net::IpAddr;

pub const SERVICE: &str = "_googlecast._tcp.local.";

/// The model a speaker group announces itself as: its members are played to, not the group.
const GROUP_MODEL: &str = "Google Cast Group";

/// One device as mDNS announced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub fullname: String,
    /// The 32 hex digits of the device's id, in lower case.
    pub uuid: String,
    /// The name its owner gave it.
    pub name: String,
    pub model: Option<String>,
    pub address: IpAddr,
    pub port: u16,
}

impl Sighting {
    /// The player id: `cast` and the device's id.
    pub fn id(&self) -> String {
        format!("cast{}", self.uuid)
    }
}

/// What a resolved `_googlecast._tcp` service says, unless it is a speaker group.
pub fn sighting(
    fullname: &str,
    addresses: impl IntoIterator<Item = IpAddr>,
    port: u16,
    txt: &HashMap<String, String>,
) -> Option<Sighting> {
    let address = addresses.into_iter().filter(IpAddr::is_ipv4).min()?;
    let uuid: String = txt
        .get("id")?
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_lowercase();
    if uuid.len() != 32 {
        return None;
    }
    let model = txt.get("md").filter(|m| !m.is_empty()).cloned();
    if model.as_deref() == Some(GROUP_MODEL) {
        return None;
    }
    Some(Sighting {
        fullname: fullname.to_owned(),
        uuid,
        name: txt
            .get("fn")
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| model.clone().unwrap_or_else(|| "Cast device".to_owned())),
        model,
        address,
        port,
    })
}

/// Browses for devices on their own thread, reporting each sighting and loss. Returns the thread,
/// which ends when the daemon stops answering.
pub fn browse(
    daemon: &mdns_sd::ServiceDaemon,
    on_sighting: impl Fn(Sighting) + Send + 'static,
    on_loss: impl Fn(&str) + Send + 'static,
) -> Result<std::thread::JoinHandle<()>, mdns_sd::Error> {
    let events = daemon.browse(SERVICE)?;
    Ok(std::thread::Builder::new()
        .name("cast-discovery".into())
        .spawn(move || {
            while let Ok(event) = events.recv() {
                match event {
                    mdns_sd::ServiceEvent::ServiceResolved(resolved) => {
                        let txt: HashMap<String, String> = resolved
                            .txt_properties
                            .iter()
                            .map(|p| (p.key().to_owned(), p.val_str().to_owned()))
                            .collect();
                        let addresses = resolved
                            .get_addresses_v4()
                            .into_iter()
                            .map(IpAddr::V4)
                            .collect::<Vec<_>>();
                        if let Some(sighting) =
                            sighting(&resolved.fullname, addresses, resolved.get_port(), &txt)
                        {
                            on_sighting(sighting);
                        }
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => on_loss(&fullname),
                    _ => {}
                }
            }
        })
        .expect("spawning the discovery thread"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txt(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_sighting_from_the_txt_record() {
        let sighting = sighting(
            "WiiM-AMP-0123._googlecast._tcp.local.",
            ["10.0.0.9".parse().unwrap(), "10.0.0.4".parse().unwrap()],
            8009,
            &txt(&[
                ("id", "0123456789ABCDEF0123456789abcdef"),
                ("fn", "Living Room"),
                ("md", "WiiM AMP"),
                ("ca", "199172"),
            ]),
        )
        .unwrap();
        assert_eq!(sighting.name, "Living Room");
        assert_eq!(sighting.model.as_deref(), Some("WiiM AMP"));
        assert_eq!(sighting.address, "10.0.0.4".parse::<IpAddr>().unwrap());
        assert_eq!(sighting.port, 8009);
        assert_eq!(sighting.id(), "cast0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn groups_and_records_without_an_id_are_not_devices() {
        let addresses = || ["10.0.0.4".parse::<IpAddr>().unwrap()];
        let group = txt(&[
            ("id", "0123456789abcdef0123456789abcdef"),
            ("fn", "Downstairs"),
            ("md", "Google Cast Group"),
        ]);
        assert!(sighting("g", addresses(), 32187, &group).is_none());
        assert!(sighting("x", addresses(), 8009, &txt(&[("fn", "Nameless")])).is_none());
    }
}
