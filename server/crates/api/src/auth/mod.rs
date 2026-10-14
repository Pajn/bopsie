//! Users and tokens. Tokens are JWTs signed with a secret kept in the database, which is also the
//! source of truth for their expiry and revocation: a token is valid only while its row exists.

pub mod lifetime;
pub mod scope;
pub mod users;

use std::sync::Arc;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::{Error, ErrorCode, Result};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rusqlite::{Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::broadcast;

use lifetime::{Check, TokenKind, TokenTimes};
pub use scope::Scope;

pub const MIGRATIONS: Migrations = Migrations {
    component: "auth",
    steps: &["
        CREATE TABLE users (
            user_id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL,
            role TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            display_name TEXT,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE TABLE auth_tokens (
            token_id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users ON DELETE CASCADE,
            name TEXT NOT NULL,
            long_lived INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            last_used_at INTEGER
        ) STRICT;
    "],
};

/// Music Assistant's `User`. Fields this server has no value for are present and empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct User {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub preferences: serde_json::Map<String, serde_json::Value>,
    pub provider_filter: Vec<String>,
    pub player_filter: Vec<String>,
}

/// An authenticated caller.
#[derive(Debug, Clone)]
pub struct Session {
    pub user: User,
    pub token_id: String,
}

impl Session {
    pub fn allows(&self, scope: Scope) -> bool {
        scope::allows(&self.user.role, scope)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    jti: String,
    iat: i64,
    exp: i64,
}

struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

#[derive(Clone)]
pub struct Auth {
    db: Database,
    keys: Arc<Keys>,
    revoked: broadcast::Sender<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SetupError {
    AlreadyDone,
    Invalid(&'static str),
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn invalid_token() -> Error {
    Error::new(ErrorCode::InvalidToken, "Invalid or expired token")
}

const USER_COLUMNS: &str = "user_id, username, role, enabled, display_name, created_at";

fn user_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let created_at: i64 = row.get(5)?;
    Ok(User {
        user_id: row.get(0)?,
        username: row.get(1)?,
        role: row.get(2)?,
        enabled: row.get(3)?,
        display_name: row.get(4)?,
        created_at: OffsetDateTime::from_unix_timestamp(created_at)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        avatar_url: None,
        preferences: serde_json::Map::new(),
        provider_filter: Vec::new(),
        player_filter: Vec::new(),
    })
}

fn user_by_id(conn: &Connection, user_id: &str) -> rusqlite::Result<Option<User>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE user_id = ?1"),
        [user_id],
        user_from_row,
    )
    .optional()
}

async fn hash_password(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|_| Error::unexpected())?
    .map_err(|error| {
        tracing::error!(%error, "hashing a password");
        Error::unexpected()
    })
}

async fn password_matches(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

impl Auth {
    /// Loads the signing secret, generating it on first start.
    pub async fn open(db: Database) -> Result<Self> {
        let secret = db
            .call(|conn| {
                Ok(db::setting_or_init(conn, "jwt_secret", || {
                    let bytes: [u8; 32] = rand::random();
                    bytes.iter().map(|b| format!("{b:02x}")).collect()
                })?)
            })
            .await?;
        let (revoked, _) = broadcast::channel(16);
        Ok(Self {
            db,
            keys: Arc::new(Keys {
                encoding: EncodingKey::from_secret(secret.as_bytes()),
                decoding: DecodingKey::from_secret(secret.as_bytes()),
            }),
            revoked,
        })
    }

    pub async fn has_users(&self) -> Result<bool> {
        Ok(self
            .db
            .call(|conn| {
                Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM users)", [], |row| row.get(0))?)
            })
            .await?)
    }

    /// Creates the first user, an admin. Refused once any user exists.
    pub async fn create_first_admin(
        &self,
        username: &str,
        password: &str,
        display_name: Option<String>,
    ) -> Result<std::result::Result<User, SetupError>> {
        let username = username.trim().to_owned();
        if username.chars().count() < 2 {
            return Ok(Err(SetupError::Invalid(
                "Username must be at least 2 characters",
            )));
        }
        if password.chars().count() < 8 {
            return Ok(Err(SetupError::Invalid(
                "Password must be at least 8 characters",
            )));
        }
        let password_hash = hash_password(password.to_owned()).await?;
        Ok(self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let exists: bool =
                    tx.query_row("SELECT EXISTS (SELECT 1 FROM users)", [], |row| row.get(0))?;
                if exists {
                    return Ok(Err(SetupError::AlreadyDone));
                }
                let user_id = new_id();
                tx.execute(
                    "INSERT INTO users (user_id, username, password_hash, role, display_name, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&user_id, &username, &password_hash, scope::ADMIN, &display_name, now()),
                )?;
                let user = user_by_id(&tx, &user_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                tx.commit()?;
                Ok(Ok(user))
            })
            .await?)
    }

    /// The user with these credentials, if they are right and the user is enabled.
    pub async fn verify_login(&self, username: &str, password: &str) -> Result<Option<User>> {
        let username = username.trim().to_owned();
        let found = self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        &format!(
                            "SELECT {USER_COLUMNS}, password_hash FROM users WHERE username = ?1"
                        ),
                        [&username],
                        |row| Ok((user_from_row(row)?, row.get::<_, String>(6)?)),
                    )
                    .optional()?)
            })
            .await?;
        let Some((user, hash)) = found else {
            return Ok(None);
        };
        let matches = password_matches(password.to_owned(), hash).await;
        Ok((matches && user.enabled).then_some(user))
    }

    /// Issues a token for `user_id`, named after the device or purpose it is for.
    pub async fn issue(&self, user_id: &str, name: &str, kind: TokenKind) -> Result<String> {
        let token_id = new_id();
        let issued_at = now();
        let (expires_at, exp) = lifetime::initial(kind, issued_at);
        let claims = Claims {
            sub: user_id.to_owned(),
            jti: token_id.clone(),
            iat: issued_at,
            exp,
        };
        let token =
            jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.keys.encoding)
                .map_err(|error| {
                    tracing::error!(%error, "signing a token");
                    Error::unexpected()
                })?;
        let (user_id, name) = (user_id.to_owned(), name.to_owned());
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO auth_tokens (token_id, user_id, name, long_lived, created_at, expires_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&token_id, &user_id, &name, kind == TokenKind::LongLived, issued_at, expires_at),
                )?;
                Ok(())
            })
            .await?;
        Ok(token)
    }

    /// The session a token stands for. Records its use, which renews a session token.
    pub async fn authenticate(&self, token: &str) -> Result<Session> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub", "jti"]);
        let claims = jsonwebtoken::decode::<Claims>(token, &self.keys.decoding, &validation)
            .map_err(|_| invalid_token())?
            .claims;
        let now = now();
        let found = self
            .db
            .call(move |conn| {
                let row = conn
                    .query_row(
                        "SELECT user_id, long_lived, created_at, expires_at, last_used_at
                         FROM auth_tokens WHERE token_id = ?1",
                        [&claims.jti],
                        |row| {
                            let kind = if row.get(1)? {
                                TokenKind::LongLived
                            } else {
                                TokenKind::Session
                            };
                            Ok((
                                row.get::<_, String>(0)?,
                                TokenTimes {
                                    kind,
                                    created_at: row.get(2)?,
                                    expires_at: row.get(3)?,
                                    last_used_at: row.get(4)?,
                                },
                            ))
                        },
                    )
                    .optional()?;
                let Some((user_id, times)) = row else {
                    return Ok(None);
                };
                if user_id != claims.sub {
                    return Ok(None);
                }
                match lifetime::check(&times, now) {
                    Check::Expired => {
                        conn.execute("DELETE FROM auth_tokens WHERE token_id = ?1", [&claims.jti])?;
                        return Ok(None);
                    }
                    Check::Valid { touch: Some(expires_at) } => {
                        conn.execute(
                            "UPDATE auth_tokens SET last_used_at = ?2, expires_at = ?3 WHERE token_id = ?1",
                            (&claims.jti, now, expires_at),
                        )?;
                    }
                    Check::Valid { touch: None } => {}
                }
                Ok(user_by_id(conn, &user_id)?.map(|user| Session {
                    user,
                    token_id: claims.jti,
                }))
            })
            .await?;
        match found {
            Some(session) if session.user.enabled => Ok(session),
            Some(_) => Err(Error::new(
                ErrorCode::AuthenticationFailed,
                "User account is disabled",
            )),
            None => Err(invalid_token()),
        }
    }

    /// Revokes a token and tells every connection using it.
    pub async fn revoke(&self, token_id: &str) -> Result<()> {
        let id = token_id.to_owned();
        self.db
            .call(move |conn| {
                conn.execute("DELETE FROM auth_tokens WHERE token_id = ?1", [&id])?;
                Ok(())
            })
            .await?;
        let _ = self.revoked.send(token_id.to_owned());
        Ok(())
    }

    /// Ids of tokens as they are revoked.
    pub fn revocations(&self) -> broadcast::Receiver<String> {
        self.revoked.subscribe()
    }

    pub async fn user(&self, user_id: &str) -> Result<Option<User>> {
        let id = user_id.to_owned();
        Ok(self.db.call(move |conn| Ok(user_by_id(conn, &id)?)).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn auth() -> Auth {
        Auth::open(Database::open_in_memory(&[MIGRATIONS]).unwrap())
            .await
            .unwrap()
    }

    async fn with_admin() -> (Auth, User) {
        let auth = auth().await;
        let user = auth
            .create_first_admin("alice", "correct horse", None)
            .await
            .unwrap()
            .unwrap();
        (auth, user)
    }

    #[tokio::test]
    async fn setup_creates_one_admin_only() {
        let auth = auth().await;
        assert!(!auth.has_users().await.unwrap());
        assert_eq!(
            auth.create_first_admin("a", "correct horse", None)
                .await
                .unwrap(),
            Err(SetupError::Invalid(
                "Username must be at least 2 characters"
            ))
        );
        assert_eq!(
            auth.create_first_admin("alice", "short", None)
                .await
                .unwrap(),
            Err(SetupError::Invalid(
                "Password must be at least 8 characters"
            ))
        );
        let user = auth
            .create_first_admin("alice", "correct horse", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.role, "admin");
        assert!(auth.has_users().await.unwrap());
        assert_eq!(
            auth.create_first_admin("bob", "correct horse", None)
                .await
                .unwrap(),
            Err(SetupError::AlreadyDone)
        );
    }

    #[tokio::test]
    async fn login_checks_the_password() {
        let (auth, user) = with_admin().await;
        assert_eq!(
            auth.verify_login("alice", "correct horse").await.unwrap(),
            Some(user.clone())
        );
        assert_eq!(
            auth.verify_login("ALICE", "correct horse").await.unwrap(),
            Some(user)
        );
        assert_eq!(
            auth.verify_login("alice", "wrong horse").await.unwrap(),
            None
        );
        assert_eq!(
            auth.verify_login("nobody", "correct horse").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn tokens_authenticate_until_revoked() {
        let (auth, user) = with_admin().await;
        let token = auth
            .issue(&user.user_id, "phone", TokenKind::Session)
            .await
            .unwrap();
        let session = auth.authenticate(&token).await.unwrap();
        assert_eq!(session.user, user);

        let mut revocations = auth.revocations();
        auth.revoke(&session.token_id).await.unwrap();
        assert_eq!(revocations.recv().await.unwrap(), session.token_id);
        assert_eq!(
            auth.authenticate(&token).await.unwrap_err().code,
            ErrorCode::InvalidToken
        );
    }

    #[tokio::test]
    async fn tokens_from_another_secret_or_garbage_are_invalid() {
        let (auth, user) = with_admin().await;
        let (other, other_user) = with_admin().await;
        let foreign = other
            .issue(&other_user.user_id, "phone", TokenKind::LongLived)
            .await
            .unwrap();
        for token in [foreign.as_str(), "not a token", ""] {
            assert_eq!(
                auth.authenticate(token).await.unwrap_err().code,
                ErrorCode::InvalidToken
            );
        }
        let own = auth
            .issue(&user.user_id, "phone", TokenKind::LongLived)
            .await
            .unwrap();
        assert!(auth.authenticate(&own).await.is_ok());
    }
}
