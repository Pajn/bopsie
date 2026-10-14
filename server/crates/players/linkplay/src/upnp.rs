//! A LinkPlay device's UPnP renderer, for the status its HTTP API does not give on some
//! firmware: Audio Pro's answers `getPlayerStatus` with `unknown command`, but its AVTransport
//! and RenderingControl answer as any renderer's do. The control URLs are LinkPlay's fixed ones.

use std::net::IpAddr;
use std::time::Duration;

use crate::status::Transport;
use crate::transport::ApiError;

/// Where LinkPlay devices serve UPnP.
pub const PORT: u16 = 49152;
const AV_TRANSPORT: (&str, &str) = ("/upnp/control/rendertransport1", "AVTransport");
const RENDERING_CONTROL: (&str, &str) = ("/upnp/control/rendercontrol1", "RenderingControl");
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Upnp {
    client: reqwest::Client,
    base: String,
}

/// The text of the first `<name>` element in `xml`.
pub fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find("</")? + start;
    Some(xml[start..end].trim().to_owned())
}

/// `HH:MM:SS` as milliseconds; `NOT_IMPLEMENTED` and the like as none.
pub fn hms_ms(text: &str) -> Option<i64> {
    let mut parts = text.trim().split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    let seconds: f64 = s.parse().ok()?;
    Some(
        (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60) * 1000
            + (seconds * 1000.0) as i64,
    )
}

pub fn transport_state(text: &str) -> Option<Transport> {
    match text {
        "PLAYING" => Some(Transport::Playing),
        "PAUSED_PLAYBACK" => Some(Transport::Paused),
        "STOPPED" | "NO_MEDIA_PRESENT" => Some(Transport::Stopped),
        "TRANSITIONING" => Some(Transport::Loading),
        _ => None,
    }
}

impl Upnp {
    pub fn new(host: IpAddr, port: u16) -> Self {
        let base = match host {
            IpAddr::V4(ip) => format!("http://{ip}:{port}"),
            IpAddr::V6(ip) => format!("http://[{ip}]:{port}"),
        };
        Self {
            client: reqwest::Client::new(),
            base,
        }
    }

    async fn call(
        &self,
        (path, service): (&str, &str),
        action: &str,
        arguments: &str,
    ) -> Result<String, ApiError> {
        let body = format!(
            r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="urn:schemas-upnp-org:service:{service}:1"><InstanceID>0</InstanceID>{arguments}</u:{action}></s:Body></s:Envelope>"#
        );
        let response = self
            .client
            .post(format!("{}{path}", self.base))
            .header("Content-Type", r#"text/xml; charset="utf-8""#)
            .header(
                "SOAPACTION",
                format!(r#""urn:schemas-upnp-org:service:{service}:1#{action}""#),
            )
            .body(body)
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|e| ApiError::Request(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ApiError::Request(format!(
                "UPnP {action}: HTTP {}",
                response.status()
            )));
        }
        response
            .text()
            .await
            .map_err(|e| ApiError::Request(e.to_string()))
    }

    pub async fn transport(&self) -> Result<Option<Transport>, ApiError> {
        let xml = self.call(AV_TRANSPORT, "GetTransportInfo", "").await?;
        Ok(tag(&xml, "CurrentTransportState").and_then(|s| transport_state(&s)))
    }

    pub async fn position_ms(&self) -> Result<Option<i64>, ApiError> {
        let xml = self.call(AV_TRANSPORT, "GetPositionInfo", "").await?;
        Ok(tag(&xml, "RelTime").and_then(|t| hms_ms(&t)))
    }

    pub async fn volume(&self) -> Result<Option<u8>, ApiError> {
        let xml = self
            .call(RENDERING_CONTROL, "GetVolume", "<Channel>Master</Channel>")
            .await?;
        Ok(tag(&xml, "CurrentVolume").and_then(|v| v.parse::<u8>().ok().map(|v| v.min(100))))
    }

    pub async fn muted(&self) -> Result<Option<bool>, ApiError> {
        let xml = self
            .call(RENDERING_CONTROL, "GetMute", "<Channel>Master</Channel>")
            .await?;
        Ok(tag(&xml, "CurrentMute").map(|m| m == "1" || m.eq_ignore_ascii_case("true")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers() {
        let xml = r#"<s:Envelope><s:Body><u:GetTransportInfoResponse><CurrentTransportState>NO_MEDIA_PRESENT</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus></u:GetTransportInfoResponse></s:Body></s:Envelope>"#;
        assert_eq!(
            tag(xml, "CurrentTransportState").and_then(|s| transport_state(&s)),
            Some(Transport::Stopped)
        );
        assert_eq!(tag(xml, "Missing"), None);
        assert_eq!(hms_ms("00:01:02"), Some(62_000));
        assert_eq!(hms_ms("1:00:00.5"), Some(3_600_500));
        assert_eq!(hms_ms("NOT_IMPLEMENTED"), None);
        assert_eq!(transport_state("TRANSITIONING"), Some(Transport::Loading));
    }
}
