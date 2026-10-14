//! A LinkPlay device's HTTP API: `GET /httpapi.asp?command=<command>`.
//!
//! Where it answers depends on the device: WiiM on HTTPS at 443 with a self-signed certificate,
//! Audio Pro on HTTPS at 4443 only to a client presenting LinkPlay's certificate, older devices
//! on plain HTTP at 80. Each device is probed in that order once, and the first that answers is
//! kept until it stops answering.

use std::net::IpAddr;
use std::sync::Mutex;

use std::time::Duration;

/// LinkPlay's client certificate and key: see the crate's NOTICE.
pub const CLIENT_CERTIFICATE: &[u8] = include_bytes!("../linkplay-client.pem");

/// Where the API may answer, in the order tried.
pub const PROBES: &[(&str, u16)] = &[
    ("https", 443),
    ("https", 4443),
    ("https", 8443),
    ("http", 80),
];
/// Mutual TLS can take seconds on these devices.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("the device's HTTP API cannot be reached")]
    Unreachable,
    #[error("the device does not know {0}")]
    UnknownCommand(String),
    #[error("the request failed: {0}")]
    Request(String),
    #[error("the client certificate is not usable: {0}")]
    Certificate(String),
}

/// One device's API.
pub struct Api {
    host: IpAddr,
    client: reqwest::Client,
    /// The scheme and port that answered, once known.
    base: Mutex<Option<String>>,
    /// Where to look, in order.
    probes: Vec<(&'static str, u16)>,
}

impl Api {
    /// The API of the device at `host`, presenting `certificate` (PEM, key and certificate) to
    /// devices that ask for one.
    pub fn new(host: IpAddr, certificate: &[u8]) -> Result<Self, ApiError> {
        Self::with_probes(host, certificate, PROBES)
    }

    /// As [`Api::new`], looking only at `probes`, in order.
    pub fn with_probes(
        host: IpAddr,
        certificate: &[u8],
        probes: &[(&'static str, u16)],
    ) -> Result<Self, ApiError> {
        let tls = crate::tls::client_config(certificate).map_err(ApiError::Certificate)?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(PROBE_TIMEOUT)
            .build()
            .map_err(|e| ApiError::Certificate(e.to_string()))?;
        Ok(Self {
            host,
            client,
            base: Mutex::new(None),
            probes: probes.to_vec(),
        })
    }

    pub fn host(&self) -> IpAddr {
        self.host
    }

    /// Where the API answered, as `https://<host>:<port>`, if it has.
    pub fn base(&self) -> Option<String> {
        self.base.lock().unwrap().clone()
    }

    fn url(&self, base: &str, command: &str) -> String {
        format!("{base}/httpapi.asp?command={command}")
    }

    async fn get(&self, base: &str, command: &str, timeout: Duration) -> Result<String, ApiError> {
        let response = self
            .client
            .get(self.url(base, command))
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| ApiError::Request(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ApiError::Request(format!("HTTP {}", response.status())));
        }
        let body = response
            .text()
            .await
            .map_err(|e| ApiError::Request(e.to_string()))?;
        if body.trim().eq_ignore_ascii_case("unknown command") {
            return Err(ApiError::UnknownCommand(command.to_owned()));
        }
        Ok(body)
    }

    /// Finds where the API answers, trying each scheme and port in turn.
    pub async fn probe(&self) -> Result<String, ApiError> {
        for (scheme, port) in &self.probes {
            let base = match self.host {
                IpAddr::V4(ip) => format!("{scheme}://{ip}:{port}"),
                IpAddr::V6(ip) => format!("{scheme}://[{ip}]:{port}"),
            };
            match self.get(&base, "getStatusEx", PROBE_TIMEOUT).await {
                Ok(_) => {
                    tracing::debug!(host = %self.host, base, "LinkPlay API found");
                    *self.base.lock().unwrap() = Some(base.clone());
                    return Ok(base);
                }
                Err(error) => {
                    tracing::trace!(host = %self.host, base, %error, "no LinkPlay API here");
                }
            }
        }
        Err(ApiError::Unreachable)
    }

    /// Sends `command` and returns the answer: JSON for status commands, `OK` for most others.
    /// Probes first if the API has not been found, and again if it stops answering.
    pub async fn command(&self, command: &str) -> Result<String, ApiError> {
        let base = match self.base() {
            Some(base) => base,
            None => self.probe().await?,
        };
        match self.get(&base, command, COMMAND_TIMEOUT).await {
            Err(ApiError::Request(_)) => {
                // It may have moved, as after a firmware update; look again once.
                *self.base.lock().unwrap() = None;
                let base = self.probe().await?;
                self.get(&base, command, COMMAND_TIMEOUT).await
            }
            other => other,
        }
    }
}
