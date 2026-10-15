//! An artist's backgrounds, logo, banner and photo from fanart.tv, by its MusicBrainz id: what
//! an app can be styled with on an artist's page. It needs a personal API key from fanart.tv.
//! <https://fanart.tv/api-docs/music-api/>

use std::sync::Arc;
use std::time::Duration;

use bopsie_core::pace::Pacer;
use serde::Deserialize;

use crate::musicbrainz::MbError;

/// fanart.tv's own server.
pub const BASE: &str = "https://webservice.fanart.tv/v3";
/// How many backgrounds of an artist are kept.
const BACKGROUNDS: usize = 3;

#[derive(Clone)]
pub struct Fanart {
    http: reqwest::Client,
    base: String,
    key: String,
    pacer: Arc<tokio::sync::Mutex<Pacer>>,
}

#[derive(Debug, Deserialize)]
struct Picture {
    url: String,
    #[serde(default)]
    likes: String,
}

#[derive(Debug, Default, Deserialize)]
struct Artist {
    #[serde(default)]
    artistthumb: Vec<Picture>,
    #[serde(default)]
    artistbackground: Vec<Picture>,
    #[serde(default)]
    hdmusiclogo: Vec<Picture>,
    #[serde(default)]
    musiclogo: Vec<Picture>,
    #[serde(default)]
    musicbanner: Vec<Picture>,
}

/// The most liked first.
fn liked(mut pictures: Vec<Picture>) -> Vec<Picture> {
    pictures.sort_by_key(|p| std::cmp::Reverse(p.likes.parse::<u32>().unwrap_or(0)));
    pictures
}

impl Fanart {
    /// A client of the service at `base` with API key `key`.
    pub fn new(base: &str, key: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(format!("Bopsie/{}", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("a client without TLS settings builds"),
            base: base.trim_end_matches('/').to_owned(),
            key,
            pacer: Arc::new(tokio::sync::Mutex::new(Pacer::new(
                Duration::from_millis(500),
                2,
            ))),
        }
    }

    async fn get(&self, url: &str) -> Result<Option<reqwest::Response>, MbError> {
        Pacer::wait(&self.pacer).await;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        match response.status().as_u16() {
            200..=299 => Ok(Some(response)),
            404 => Ok(None),
            401 | 403 => Err(MbError::Refused("fanart.tv refused the API key".to_owned())),
            status => Err(MbError::Refused(format!("fanart.tv: {status}"))),
        }
    }

    async fn download(&self, url: &str) -> Result<Option<(Vec<u8>, &'static str)>, MbError> {
        let Some(response) = self.get(url).await? else {
            return Ok(None);
        };
        let from_type = match response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
        {
            Some(t) if t.starts_with("image/jpeg") => Some("jpg"),
            Some(t) if t.starts_with("image/png") => Some("png"),
            _ => None,
        };
        let lower = url.to_ascii_lowercase();
        let extension = from_type.or(if lower.ends_with(".png") {
            Some("png")
        } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            Some("jpg")
        } else {
            None
        });
        let Some(extension) = extension else {
            return Ok(None);
        };
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MbError::Unreachable(e.to_string()))?;
        Ok(Some((bytes.to_vec(), extension)))
    }

    /// The images fanart.tv has of the artist with MusicBrainz id `mbid`, each with its kind
    /// (`thumb`, `fanart`, `logo`, `banner`) and file extension: none for one it does not know.
    pub async fn artist(
        &self,
        mbid: &str,
    ) -> Result<Vec<(String, Vec<u8>, &'static str)>, MbError> {
        let url = format!("{}/music/{mbid}?api_key={}", self.base, self.key);
        let Some(response) = self.get(&url).await? else {
            return Ok(Vec::new());
        };
        let artist: Artist = response.json().await.unwrap_or_default();
        let logos = if artist.hdmusiclogo.is_empty() {
            artist.musiclogo
        } else {
            artist.hdmusiclogo
        };
        let wanted: Vec<(&str, String)> = liked(artist.artistthumb)
            .into_iter()
            .take(1)
            .map(|p| ("thumb", p.url))
            .chain(
                liked(artist.artistbackground)
                    .into_iter()
                    .take(BACKGROUNDS)
                    .map(|p| ("fanart", p.url)),
            )
            .chain(liked(logos).into_iter().take(1).map(|p| ("logo", p.url)))
            .chain(
                liked(artist.musicbanner)
                    .into_iter()
                    .take(1)
                    .map(|p| ("banner", p.url)),
            )
            .collect();
        let mut images = Vec::with_capacity(wanted.len());
        for (kind, url) in wanted {
            if let Some((bytes, extension)) = self.download(&url).await? {
                images.push((kind.to_owned(), bytes, extension));
            }
        }
        Ok(images)
    }
}
