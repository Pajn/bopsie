//! Finding LinkPlay devices: they announce `_linkplay._tcp` over mDNS, with their MAC and UPnP
//! UUID in the TXT record.

use std::collections::HashMap;
use std::net::IpAddr;

pub const SERVICE: &str = "_linkplay._tcp.local.";

/// One device as mDNS announced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub fullname: String,
    /// The instance name, which LinkPlay sets to the device's name.
    pub name: String,
    pub address: IpAddr,
    /// As `AA:BB:CC:DD:EE:FF`.
    pub mac: Option<String>,
    /// The 24 hex digits the HTTP API calls `uuid`.
    pub uuid: Option<String>,
}

impl Sighting {
    /// The player id: `lp` and the device's UUID in lower case, else its MAC.
    pub fn id(&self) -> Option<String> {
        if let Some(uuid) = &self.uuid {
            return Some(format!("lp{}", uuid.to_lowercase()));
        }
        self.mac
            .as_ref()
            .map(|mac| format!("lp{}", mac.replace([':', '-'], "").to_lowercase()))
    }
}

/// The player id of the device whose HTTP API `uuid` is `uuid`.
pub fn id_of_uuid(uuid: &str) -> String {
    format!("lp{}", uuid.to_lowercase())
}

/// The API's 24-hex `uuid` from a UDN such as `uuid:01234567-89AB-CDEF-0123-456701234567`: its
/// hex digits, less the last eight, which repeat the first.
pub fn uuid_of_udn(udn: &str) -> Option<String> {
    let hex: String = udn
        .trim_start_matches("uuid:")
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect();
    (hex.len() >= 24).then(|| hex[..24].to_uppercase())
}

/// What a resolved `_linkplay._tcp` service says.
pub fn sighting(
    fullname: &str,
    addresses: impl IntoIterator<Item = IpAddr>,
    txt: &HashMap<String, String>,
) -> Option<Sighting> {
    let address = addresses.into_iter().filter(IpAddr::is_ipv4).min()?;
    let name = fullname
        .strip_suffix(&format!(".{SERVICE}"))
        .unwrap_or(fullname)
        .replace("\\032", " ")
        .replace('\\', "");
    Some(Sighting {
        fullname: fullname.to_owned(),
        name,
        address,
        mac: txt.get("MAC").filter(|m| !m.is_empty()).cloned(),
        uuid: txt.get("uuid").and_then(|u| uuid_of_udn(u)),
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
        .name("linkplay-discovery".into())
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
                        if let Some(sighting) = sighting(&resolved.fullname, addresses, &txt) {
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

    #[test]
    fn a_sighting_from_the_txt_record() {
        let txt: HashMap<String, String> = [
            ("uuid", "uuid:0123ABCD-0123-4567-89AB-CDEF01230123ABCD"),
            ("MAC", "AA:BB:CC:00:11:22"),
            ("security", "https 3.0"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        let sighting = sighting(
            "Living\\032Room._linkplay._tcp.local.",
            ["10.0.0.9".parse().unwrap(), "10.0.0.2".parse().unwrap()],
            &txt,
        )
        .unwrap();
        assert_eq!(sighting.name, "Living Room");
        assert_eq!(sighting.address, "10.0.0.2".parse::<IpAddr>().unwrap());
        assert_eq!(sighting.uuid.as_deref(), Some("0123ABCD0123456789ABCDEF"));
        assert_eq!(sighting.id().as_deref(), Some("lp0123abcd0123456789abcdef"));
        assert_eq!(sighting.mac.as_deref(), Some("AA:BB:CC:00:11:22"));
        assert_eq!(
            id_of_uuid("0123ABCD0123456789ABCDEF"),
            "lp0123abcd0123456789abcdef"
        );
    }
}
