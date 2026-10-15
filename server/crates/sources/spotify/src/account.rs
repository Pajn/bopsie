//! The Spotify account the server uses as a music provider. It signs in twice, for two
//! things Spotify keeps apart:
//!
//! - **Playback and playlists**, through librespot: picking a "Bopsie sign-in" device in the
//!   Spotify app hands over a login, kept in the data directory as Spotify Connect's are.
//! - **Search and the saved library**, through Spotify's Web API, as the listener's own
//!   developer app: "Log in with Spotify" with PKCE, its refresh token kept in the database.
//!   librespot's own access to the Web API is shared by every librespot app and used up.
//!
//! Web API calls are paced (at most one a second, after a burst of five), and a call Spotify
//! refuses for its rate limit waits as long as Spotify says and tries again.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_core::db::{self, Database};
use futures::StreamExt;
use librespot_core::cache::Cache;
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig};
use librespot_discovery::Discovery;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;

/// What the Web API login may do: read the saved library, followed artists and playlists.
const SCOPES: &str =
    "user-library-read user-follow-read playlist-read-private playlist-read-collaborative";
const CLIENT_ID: &str = "spotify.client_id";
const REFRESH_TOKEN: &str = "spotify.refresh_token";
const WEB_USER: &str = "spotify.web_user";
/// How many calls may go at once before pacing starts, and how often one may go after.
const BURST: f64 = 5.0;
const PACE: Duration = Duration::from_secs(1);
/// The longest a rate-limited call waits before it gives up.
const LONGEST_WAIT: Duration = Duration::from_secs(120);

/// Where Spotify is, so that tests can stand in for it.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub accounts: String,
    pub api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            accounts: "https://accounts.spotify.com".to_owned(),
            api: "https://api.spotify.com/v1".to_owned(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Spotify's Web API needs a developer app's client ID first")]
    NoClientId,
    #[error("not logged in to Spotify's Web API")]
    NotLoggedIn,
    #[error("no Spotify login is waiting for this answer")]
    UnexpectedCallback,
    #[error("not signed in to Spotify for playback")]
    NoPlayback,
    #[error("Spotify refused: {0}")]
    Refused(String),
    #[error("Spotify could not be reached: {0}")]
    Unreachable(String),
    #[error("Spotify kept refusing for its rate limit")]
    RateLimited,
    #[error(transparent)]
    Db(#[from] db::DbError),
}

/// How the account stands, for the web UI.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AccountStatus {
    /// The developer app's client ID, once given.
    pub client_id: Option<String>,
    /// Who the Web API login is, while there is one.
    pub web_user: Option<String>,
    /// Who the playback login is, while there is one.
    pub playback_user: Option<String>,
    /// Waiting to be picked in the Spotify app.
    pub pairing: bool,
    pub error: Option<String>,
}

struct Pending {
    state: String,
    verifier: String,
    redirect_uri: String,
}

#[derive(Default)]
struct Web {
    client_id: Option<String>,
    refresh_token: Option<String>,
    user: Option<String>,
    access: Option<(String, Instant)>,
    pending: Option<Pending>,
}

struct Throttle {
    tokens: f64,
    at: Instant,
}

impl Throttle {
    /// How long to wait before the next call, taking its turn.
    fn take(&mut self, now: Instant) -> Duration {
        let refilled = (now - self.at).as_secs_f64() / PACE.as_secs_f64();
        self.tokens = (self.tokens + refilled).min(BURST);
        self.at = now;
        self.tokens -= 1.0;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            PACE.mul_f64(-self.tokens)
        }
    }
}

#[derive(Clone)]
pub struct SpotifyAccount {
    inner: Arc<Inner>,
}

struct Inner {
    db: Database,
    dir: PathBuf,
    endpoints: Endpoints,
    http: reqwest::Client,
    web: Mutex<Web>,
    throttle: tokio::sync::Mutex<Throttle>,
    session: tokio::sync::Mutex<Option<Session>>,
    pairing: Mutex<Option<JoinHandle<()>>>,
    error: Mutex<Option<String>>,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

/// URL-safe base64 without padding, as PKCE uses.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn random_text() -> String {
    base64url(&rand::random::<[u8; 32]>())
}

fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub(crate) fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl SpotifyAccount {
    /// The account as `db` and `data_dir` keep it.
    pub async fn open(
        db: Database,
        data_dir: PathBuf,
        endpoints: Endpoints,
    ) -> Result<Self, AccountError> {
        let web = db
            .call(|conn| {
                Ok(Web {
                    client_id: db::setting(conn, CLIENT_ID)?,
                    refresh_token: db::setting(conn, REFRESH_TOKEN)?,
                    user: db::setting(conn, WEB_USER)?,
                    ..Web::default()
                })
            })
            .await?;
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                dir: data_dir.join("spotify").join("account"),
                endpoints,
                http: reqwest::Client::new(),
                web: Mutex::new(web),
                throttle: tokio::sync::Mutex::new(Throttle {
                    tokens: BURST,
                    at: Instant::now(),
                }),
                session: tokio::sync::Mutex::default(),
                pairing: Mutex::default(),
                error: Mutex::default(),
            }),
        })
    }

    /// Whether the Web API login is there, for search and the saved library.
    pub fn web_logged_in(&self) -> bool {
        let web = self.inner.web.lock().unwrap();
        web.client_id.is_some() && web.refresh_token.is_some()
    }

    pub async fn status(&self) -> AccountStatus {
        let playback_user = match self.session().await {
            Ok(session) => Some(session.username()),
            Err(_) => None,
        };
        let web = self.inner.web.lock().unwrap();
        AccountStatus {
            client_id: web.client_id.clone(),
            web_user: web.refresh_token.as_ref().and(web.user.clone()),
            playback_user,
            pairing: self
                .inner
                .pairing
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|task| !task.is_finished()),
            error: self.inner.error.lock().unwrap().clone(),
        }
    }

    async fn save(&self, key: &'static str, value: Option<String>) -> Result<(), AccountError> {
        self.inner
            .db
            .call(move |conn| Ok(db::set_setting(conn, key, value.as_deref())?))
            .await?;
        Ok(())
    }

    /// Uses the developer app with `client_id`, or none; either way the Web API login so far
    /// ends.
    pub async fn set_client_id(&self, client_id: Option<String>) -> Result<(), AccountError> {
        let client_id = client_id
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        self.save(CLIENT_ID, client_id.clone()).await?;
        self.web_sign_out().await?;
        self.inner.web.lock().unwrap().client_id = client_id;
        Ok(())
    }

    /// Where to send the listener to log in, and come back to `redirect_uri` from.
    pub fn authorize_url(&self, redirect_uri: &str) -> Result<String, AccountError> {
        let mut web = self.inner.web.lock().unwrap();
        let client_id = web.client_id.clone().ok_or(AccountError::NoClientId)?;
        let pending = Pending {
            state: random_text(),
            verifier: random_text(),
            redirect_uri: redirect_uri.to_owned(),
        };
        let challenge = base64url(&Sha256::digest(pending.verifier.as_bytes()));
        let url = format!(
            "{}/authorize?{}",
            self.inner.endpoints.accounts,
            query(&[
                ("client_id", &client_id),
                ("response_type", "code"),
                ("redirect_uri", redirect_uri),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
                ("state", &pending.state),
                ("scope", SCOPES),
            ])
        );
        web.pending = Some(pending);
        Ok(url)
    }

    /// Finishes a login Spotify sent back with `code` for `state`.
    pub async fn callback(&self, code: &str, state: &str) -> Result<(), AccountError> {
        let (pending, client_id) = {
            let mut web = self.inner.web.lock().unwrap();
            match web.pending.take() {
                Some(pending) if pending.state == state => (
                    pending,
                    web.client_id.clone().ok_or(AccountError::NoClientId)?,
                ),
                other => {
                    web.pending = other;
                    return Err(AccountError::UnexpectedCallback);
                }
            }
        };
        let answer = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &pending.redirect_uri),
                ("client_id", &client_id),
                ("code_verifier", &pending.verifier),
            ])
            .await?;
        let refresh = answer
            .refresh_token
            .clone()
            .ok_or_else(|| AccountError::Refused("no refresh token".to_owned()))?;
        self.save(REFRESH_TOKEN, Some(refresh.clone())).await?;
        {
            let mut web = self.inner.web.lock().unwrap();
            web.refresh_token = Some(refresh);
            web.access = Some((
                answer.access_token,
                Instant::now() + Duration::from_secs(answer.expires_in.saturating_sub(60)),
            ));
        }
        let me = self.get("me").await?;
        let user = me["display_name"]
            .as_str()
            .or(me["id"].as_str())
            .map(str::to_owned);
        self.save(WEB_USER, user.clone()).await?;
        self.inner.web.lock().unwrap().user = user;
        *self.inner.error.lock().unwrap() = None;
        tracing::info!("logged in to Spotify's Web API");
        Ok(())
    }

    async fn token(&self, form: &[(&str, &str)]) -> Result<TokenAnswer, AccountError> {
        let response = self
            .inner
            .http
            .post(format!("{}/api/token", self.inner.endpoints.accounts))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(query(form))
            .send()
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AccountError::Refused(body));
        }
        response
            .json()
            .await
            .map_err(|e| AccountError::Refused(e.to_string()))
    }

    /// An access token for the Web API, refreshed when it is about to run out.
    async fn access_token(&self, fresh: bool) -> Result<String, AccountError> {
        let (refresh, client_id) = {
            let web = self.inner.web.lock().unwrap();
            if !fresh
                && let Some((token, until)) = &web.access
                && Instant::now() < *until
            {
                return Ok(token.clone());
            }
            (
                web.refresh_token.clone().ok_or(AccountError::NotLoggedIn)?,
                web.client_id.clone().ok_or(AccountError::NoClientId)?,
            )
        };
        let answer = match self
            .token(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", &client_id),
            ])
            .await
        {
            Ok(answer) => answer,
            Err(error) => {
                *self.inner.error.lock().unwrap() = Some(error.to_string());
                return Err(error);
            }
        };
        if let Some(rotated) = &answer.refresh_token {
            self.save(REFRESH_TOKEN, Some(rotated.clone())).await?;
        }
        let mut web = self.inner.web.lock().unwrap();
        if let Some(rotated) = answer.refresh_token {
            web.refresh_token = Some(rotated);
        }
        web.access = Some((
            answer.access_token.clone(),
            Instant::now() + Duration::from_secs(answer.expires_in.saturating_sub(60)),
        ));
        Ok(answer.access_token)
    }

    /// GETs `path` from the Web API, such as `me/albums?limit=50`, or a whole `https://` URL
    /// Spotify gave, such as a page's `next`.
    pub async fn get(&self, path: &str) -> Result<Value, AccountError> {
        let url = if path.starts_with("https://") || path.starts_with("http://") {
            path.to_owned()
        } else {
            format!("{}/{path}", self.inner.endpoints.api)
        };
        let mut waited = Duration::ZERO;
        let mut refreshed = false;
        loop {
            let wait = self.inner.throttle.lock().await.take(Instant::now());
            tokio::time::sleep(wait).await;
            let token = self.access_token(false).await?;
            let response = self
                .inner
                .http
                .get(&url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| AccountError::Unreachable(e.to_string()))?;
            match response.status().as_u16() {
                200..=299 => {
                    return response
                        .json()
                        .await
                        .map_err(|e| AccountError::Refused(e.to_string()));
                }
                401 if !refreshed => {
                    refreshed = true;
                    self.access_token(true).await?;
                }
                429 => {
                    let after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map_or(Duration::from_secs(5), Duration::from_secs);
                    if waited + after > LONGEST_WAIT {
                        return Err(AccountError::RateLimited);
                    }
                    tracing::debug!(?after, url, "Spotify's rate limit; waiting");
                    waited += after;
                    tokio::time::sleep(after).await;
                }
                status => {
                    let body = response.text().await.unwrap_or_default();
                    return Err(AccountError::Refused(format!("{status}: {body}")));
                }
            }
        }
    }

    /// Every item of a paged Web API list, following each page's `next`. `items` picks a
    /// page's paging object out of its answer, for answers that wrap it.
    pub async fn get_all(
        &self,
        path: &str,
        items: impl Fn(&Value) -> &Value,
    ) -> Result<Vec<Value>, AccountError> {
        let mut all = Vec::new();
        let mut next = Some(path.to_owned());
        while let Some(path) = next.take() {
            let answer = self.get(&path).await?;
            let page = items(&answer);
            all.extend(page["items"].as_array().cloned().unwrap_or_default());
            next = page["next"].as_str().map(str::to_owned);
        }
        Ok(all)
    }

    /// Ends the Web API login.
    pub async fn web_sign_out(&self) -> Result<(), AccountError> {
        self.save(REFRESH_TOKEN, None).await?;
        self.save(WEB_USER, None).await?;
        let mut web = self.inner.web.lock().unwrap();
        web.refresh_token = None;
        web.user = None;
        web.access = None;
        Ok(())
    }

    fn cache(&self) -> Option<Cache> {
        Cache::new(Some(&self.inner.dir), None, None, None)
            .map_err(|error| tracing::warn!(%error, "cannot keep the Spotify login"))
            .ok()
    }

    /// The playback session, connecting with the kept login when there is none.
    pub async fn session(&self) -> Result<Session, AccountError> {
        let mut session = self.inner.session.lock().await;
        if let Some(current) = session.as_ref()
            && !current.is_invalid()
        {
            return Ok(current.clone());
        }
        let cache = self.cache();
        let credentials = cache
            .as_ref()
            .and_then(Cache::credentials)
            .ok_or(AccountError::NoPlayback)?;
        let connected = Session::new(SessionConfig::default(), cache);
        connected
            .connect(credentials, true)
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))?;
        *session = Some(connected.clone());
        Ok(connected)
    }

    /// Announces a "Bopsie sign-in" device until someone picks it in the Spotify app, and
    /// keeps the login it is given. Needs a Tokio runtime.
    pub fn pair(&self, name: &str, addresses: Vec<std::net::IpAddr>) {
        self.cancel_pairing();
        let account = self.clone();
        let name = name.to_owned();
        let task = tokio::spawn(async move {
            let config = SessionConfig::default();
            let discovery = Discovery::builder(config.device_id.clone(), config.client_id.clone())
                .name(name)
                .device_type(DeviceType::Computer)
                .zeroconf_ip(addresses)
                .launch();
            let mut discovery = match discovery {
                Ok(discovery) => discovery,
                Err(error) => {
                    *account.inner.error.lock().unwrap() = Some(error.to_string());
                    return;
                }
            };
            let Some(credentials) = discovery.next().await else {
                return;
            };
            discovery.shutdown().await;
            let session = Session::new(config, account.cache());
            match session.connect(credentials, true).await {
                Ok(()) => {
                    tracing::info!(
                        user = session.username(),
                        "signed in to Spotify for playback"
                    );
                    *account.inner.session.lock().await = Some(session);
                    *account.inner.error.lock().unwrap() = None;
                }
                Err(error) => *account.inner.error.lock().unwrap() = Some(error.to_string()),
            }
        });
        *self.inner.pairing.lock().unwrap() = Some(task);
    }

    pub fn cancel_pairing(&self) {
        if let Some(task) = self.inner.pairing.lock().unwrap().take() {
            task.abort();
        }
    }

    /// Ends the playback login and forgets it.
    pub async fn playback_sign_out(&self) {
        self.cancel_pairing();
        if let Some(session) = self.inner.session.lock().await.take() {
            session.shutdown();
        }
        let _ = std::fs::remove_dir_all(&self.inner.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_as_rfc_4648_says() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn calls_are_paced_after_a_burst() {
        let start = Instant::now();
        let mut throttle = Throttle {
            tokens: BURST,
            at: start,
        };
        for _ in 0..5 {
            assert_eq!(throttle.take(start), Duration::ZERO);
        }
        assert_eq!(throttle.take(start), PACE);
        assert_eq!(throttle.take(start), PACE * 2);
        // Waiting refills it.
        let later = start + PACE * 10;
        assert_eq!(throttle.take(later), Duration::ZERO);
    }
}
