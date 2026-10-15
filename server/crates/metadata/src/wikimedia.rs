//! An artist's photo from Wikimedia Commons, by the Wikidata entity MusicBrainz links it to:
//! for artists Spotify has no photo of. Wikimedia asks software that calls it to name itself and
//! a contact, as MusicBrainz does. <https://meta.wikimedia.org/wiki/User-Agent_policy>

use std::sync::Arc;
use std::time::Duration;

use bopsie_core::pace::Pacer;
use serde_json::Value;

use crate::musicbrainz::MbError;

/// Wikidata's own server.
pub const WIKIDATA: &str = "https://www.wikidata.org";
/// Wikimedia Commons'.
pub const COMMONS: &str = "https://commons.wikimedia.org";
/// How wide a photo is fetched.
const WIDTH: u32 = 500;

#[derive(Clone)]
pub struct Wikimedia {
    http: reqwest::Client,
    wikidata: String,
    commons: String,
    pacer: Arc<tokio::sync::Mutex<Pacer>>,
}

/// `text` as a URL path segment.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl Wikimedia {
    /// A client of Wikidata at `wikidata` and Commons at `commons`. `contact` goes in the
    /// User-Agent, as Wikimedia asks.
    pub fn new(wikidata: &str, commons: &str, contact: Option<&str>) -> Self {
        let mut agent = format!("Bopsie/{}", env!("CARGO_PKG_VERSION"));
        if let Some(contact) = contact.map(str::trim).filter(|c| !c.is_empty()) {
            agent.push_str(&format!(" ( {contact} )"));
        }
        Self {
            http: reqwest::Client::builder()
                .user_agent(agent)
                .timeout(Duration::from_secs(30))
                .build()
                .expect("a client without TLS settings builds"),
            wikidata: wikidata.trim_end_matches('/').to_owned(),
            commons: commons.trim_end_matches('/').to_owned(),
            pacer: Arc::new(tokio::sync::Mutex::new(Pacer::new(
                Duration::from_millis(200),
                5,
            ))),
        }
    }

    async fn fetch(&self, url: &str) -> Result<Option<reqwest::Response>, MbError> {
        Pacer::wait(&self.pacer).await;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        Ok(response.status().is_success().then_some(response))
    }

    /// The photo Wikidata entity `entity`, such as `Q1299`, has, with its file extension: `None`
    /// when it has none, or it is not a JPEG or PNG.
    pub async fn photo(&self, entity: &str) -> Result<Option<(Vec<u8>, &'static str)>, MbError> {
        let url = format!(
            "{}/wiki/Special:EntityData/{}.json",
            self.wikidata,
            encode(entity)
        );
        let Some(response) = self.fetch(&url).await? else {
            return Ok(None);
        };
        let data: Value = response
            .json()
            .await
            .map_err(|e| MbError::Refused(e.to_string()))?;
        let file = data["entities"]
            .as_object()
            .and_then(|entities| entities.values().next())
            .and_then(|entity| {
                entity["claims"]["P18"][0]["mainsnak"]["datavalue"]["value"].as_str()
            });
        let Some(file) = file else {
            return Ok(None);
        };
        let url = format!(
            "{}/wiki/Special:FilePath/{}?width={WIDTH}",
            self.commons,
            encode(&file.replace(' ', "_"))
        );
        let Some(response) = self.fetch(&url).await? else {
            return Ok(None);
        };
        let extension = match response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
        {
            Some(t) if t.starts_with("image/jpeg") => "jpg",
            Some(t) if t.starts_with("image/png") => "png",
            _ => return Ok(None),
        };
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        Ok(Some((bytes.to_vec(), extension)))
    }
}
