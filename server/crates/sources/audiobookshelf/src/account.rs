//! Signing in to an Audiobookshelf server and staying signed in.
//!
//! A username and password are exchanged once for an access token and a refresh token, which
//! are kept in the settings table with the server's URL and the user's name; the password is
//! used for that one request and kept nowhere. An access token that has expired is refreshed
//! with the refresh token, and the new pair kept; a refused refresh signs out.

use std::sync::{Arc, Mutex};

use bopsie_core::db::{self, Database};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const URL: &str = "audiobookshelf.url";
const USER: &str = "audiobookshelf.user";
const ACCESS_TOKEN: &str = "audiobookshelf.access_token";
const REFRESH_TOKEN: &str = "audiobookshelf.refresh_token";

#[derive(Debug, Clone, thiserror::Error)]
pub enum AccountError {
    #[error("Not signed in to Audiobookshelf")]
    SignedOut,
    #[error("Audiobookshelf refused: {0}")]
    Refused(String),
    #[error("Audiobookshelf cannot be reached: {0}")]
    Unreachable(String),
    #[error("Audiobookshelf has no {0}")]
    NotFound(String),
    #[error("database: {0}")]
    Db(String),
}

impl From<db::DbError> for AccountError {
    fn from(error: db::DbError) -> Self {
        AccountError::Db(error.to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Session {
    url: Option<String>,
    user: Option<String>,
    access: Option<String>,
    refresh: Option<String>,
}

/// Who the server is signed in to Audiobookshelf as, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub url: Option<String>,
    pub user: Option<String>,
    pub signed_in: bool,
}

#[derive(Clone)]
pub struct Account {
    inner: Arc<Inner>,
}

struct Inner {
    db: Database,
    http: reqwest::Client,
    session: Mutex<Session>,
    /// One refresh at a time, so two requests that find the token expired refresh it once.
    refreshing: tokio::sync::Mutex<()>,
}

fn trimmed(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

impl Account {
    /// The account as `db` keeps it.
    pub async fn open(db: Database) -> Result<Self, AccountError> {
        let session = db
            .call(|conn| {
                Ok(Session {
                    url: db::setting(conn, URL)?,
                    user: db::setting(conn, USER)?,
                    access: db::setting(conn, ACCESS_TOKEN)?,
                    refresh: db::setting(conn, REFRESH_TOKEN)?,
                })
            })
            .await?;
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                http: reqwest::Client::new(),
                session: Mutex::new(session),
                refreshing: tokio::sync::Mutex::default(),
            }),
        })
    }

    pub fn status(&self) -> Status {
        let session = self.inner.session.lock().unwrap();
        Status {
            url: session.url.clone(),
            user: session.user.clone(),
            signed_in: session.access.is_some(),
        }
    }

    pub fn signed_in(&self) -> bool {
        self.inner.session.lock().unwrap().access.is_some()
    }

    /// The server's URL, once given.
    pub fn url(&self) -> Option<String> {
        self.inner.session.lock().unwrap().url.clone()
    }

    /// Signs in to the server at `url` as `username`, keeping the tokens it gives, not the
    /// password.
    pub async fn sign_in(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<(), AccountError> {
        let url = trimmed(url);
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AccountError::Refused(
                "the address must start with http:// or https://".to_owned(),
            ));
        }
        let response = self
            .inner
            .http
            .post(format!("{url}/login"))
            // The refresh token in the answer, rather than in a cookie.
            .header("x-return-tokens", "true")
            .json(&json!({"username": username, "password": password}))
            .send()
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(AccountError::Refused(
                "wrong username or password".to_owned(),
            ));
        }
        let answer = answer(response).await?;
        let (access, refresh) = tokens(&answer)?;
        let user = answer["user"]["username"]
            .as_str()
            .unwrap_or(username)
            .to_owned();
        self.keep(Session {
            url: Some(url),
            user: Some(user),
            access: Some(access),
            refresh,
        })
        .await
    }

    /// Forgets the tokens, keeping where and as whom to sign in again.
    pub async fn sign_out(&self) -> Result<(), AccountError> {
        let mut session = self.inner.session.lock().unwrap().clone();
        session.access = None;
        session.refresh = None;
        self.keep(session).await
    }

    async fn keep(&self, session: Session) -> Result<(), AccountError> {
        let kept = session.clone();
        self.inner
            .db
            .call(move |conn| {
                db::set_setting(conn, URL, kept.url.as_deref())?;
                db::set_setting(conn, USER, kept.user.as_deref())?;
                db::set_setting(conn, ACCESS_TOKEN, kept.access.as_deref())?;
                db::set_setting(conn, REFRESH_TOKEN, kept.refresh.as_deref())?;
                Ok(())
            })
            .await?;
        *self.inner.session.lock().unwrap() = session;
        Ok(())
    }

    /// The JSON the server answers `path` with.
    pub async fn get(&self, path: &str) -> Result<Value, AccountError> {
        self.request(Method::GET, path, None).await
    }

    /// The JSON the server answers posting `body` to `path` with.
    pub async fn post(&self, path: &str, body: Value) -> Result<Value, AccountError> {
        self.request(Method::POST, path, Some(body)).await
    }

    /// The bytes the server answers `path` with, such as a cover.
    pub async fn bytes(&self, path: &str) -> Result<Vec<u8>, AccountError> {
        let response = self.send(Method::GET, path, None).await?;
        let response = ok(response).await?;
        Ok(response
            .bytes()
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))?
            .to_vec())
    }

    /// The URL of `path` on the server, with the access token, for a stream fetched without
    /// headers, such as by ffmpeg.
    pub fn signed_url(&self, path: &str) -> Result<String, AccountError> {
        let session = self.inner.session.lock().unwrap();
        let (Some(url), Some(token)) = (&session.url, &session.access) else {
            return Err(AccountError::SignedOut);
        };
        let separator = if path.contains('?') { '&' } else { '?' };
        Ok(format!("{url}{path}{separator}token={token}"))
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, AccountError> {
        let response = self.send(method, path, body).await?;
        answer(response).await
    }

    /// Sends a request with the access token, refreshing it once if the server says it has
    /// expired.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, AccountError> {
        let (url, token) = self.credentials()?;
        let response = self
            .attempt(&method, &url, path, &token, body.as_ref())
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let token = self.refresh(&token).await?;
        self.attempt(&method, &url, path, &token, body.as_ref())
            .await
    }

    fn credentials(&self) -> Result<(String, String), AccountError> {
        let session = self.inner.session.lock().unwrap();
        match (&session.url, &session.access) {
            (Some(url), Some(token)) => Ok((url.clone(), token.clone())),
            _ => Err(AccountError::SignedOut),
        }
    }

    async fn attempt(
        &self,
        method: &Method,
        url: &str,
        path: &str,
        token: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::Response, AccountError> {
        let mut request = self
            .inner
            .http
            .request(method.clone(), format!("{url}{path}"))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        }
        request
            .send()
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))
    }

    /// A new access token for the one that expired, `stale`; signs out if the server refuses.
    async fn refresh(&self, stale: &str) -> Result<String, AccountError> {
        let _refreshing = self.inner.refreshing.lock().await;
        let session = self.inner.session.lock().unwrap().clone();
        // Refreshed meanwhile by another request.
        if let Some(access) = &session.access
            && access != stale
        {
            return Ok(access.clone());
        }
        let (Some(url), Some(refresh)) = (&session.url, &session.refresh) else {
            self.sign_out().await?;
            return Err(AccountError::SignedOut);
        };
        let response = self
            .inner
            .http
            .post(format!("{url}/auth/refresh"))
            .header("x-refresh-token", refresh)
            .send()
            .await
            .map_err(|e| AccountError::Unreachable(e.to_string()))?;
        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            tracing::warn!("Audiobookshelf refused to refresh the sign-in; signed out");
            self.sign_out().await?;
            return Err(AccountError::SignedOut);
        }
        let answer = answer(response).await?;
        let (access, new_refresh) = tokens(&answer)?;
        self.keep(Session {
            access: Some(access.clone()),
            refresh: new_refresh.or(session.refresh.clone()),
            ..session
        })
        .await?;
        Ok(access)
    }
}

/// The access token and refresh token in a sign-in or refresh answer. Servers before refresh
/// tokens give one lasting `token`.
fn tokens(answer: &Value) -> Result<(String, Option<String>), AccountError> {
    let user = &answer["user"];
    let access = user["accessToken"]
        .as_str()
        .or_else(|| user["token"].as_str())
        .ok_or_else(|| AccountError::Refused("no token in the answer".to_owned()))?;
    Ok((
        access.to_owned(),
        user["refreshToken"].as_str().map(str::to_owned),
    ))
}

async fn ok(response: reqwest::Response) -> Result<reqwest::Response, AccountError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let detail = response.text().await.unwrap_or_default();
    Err(match status {
        StatusCode::NOT_FOUND => AccountError::NotFound(detail),
        StatusCode::UNAUTHORIZED => AccountError::SignedOut,
        _ => AccountError::Refused(format!("{status}: {detail}")),
    })
}

async fn answer(response: reqwest::Response) -> Result<Value, AccountError> {
    let response = ok(response).await?;
    let text = response
        .text()
        .await
        .map_err(|e| AccountError::Unreachable(e.to_string()))?;
    if text.trim().is_empty() || text.trim() == "OK" {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| AccountError::Refused(format!("not JSON: {e}")))
}
