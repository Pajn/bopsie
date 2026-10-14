//! Managing users and their sign-ins, as Music Assistant's `auth/user*` and `auth/tokens`
//! commands do. The server always keeps an enabled admin: the last one cannot be disabled,
//! deleted or made a user.

use bopsie_core::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::Serialize;
use time::OffsetDateTime;

use super::{
    Auth, USER_COLUMNS, User, hash_password, new_id, now, scope, user_by_id, user_from_row,
};

/// Music Assistant's `AuthToken`. This server keeps no hash of a token, so `token_hash` is
/// empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AuthToken {
    pub token_id: String,
    pub user_id: String,
    pub token_hash: String,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schemars(with = "Option<String>")]
    pub expires_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schemars(with = "Option<String>")]
    pub last_used_at: Option<OffsetDateTime>,
    pub is_long_lived: bool,
}

/// What `auth/user/update` changes; `None` leaves a field as it is.
#[derive(Debug, Default)]
pub struct UserChanges {
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub password: Option<String>,
    pub role: Option<String>,
}

/// Tokens listed for a user, newest first.
const TOKEN_LIST_LIMIT: i64 = 100;

fn invalid(details: &str) -> Error {
    Error::invalid_data(details)
}

fn valid_username(username: &str) -> Result<String> {
    let username = username.trim();
    if username.chars().count() < 2 {
        return Err(invalid("Username must be at least 2 characters"));
    }
    Ok(username.to_owned())
}

fn valid_password(password: &str) -> Result<()> {
    if password.chars().count() < 8 {
        return Err(invalid("Password must be at least 8 characters"));
    }
    Ok(())
}

fn valid_role(role: &str) -> Result<()> {
    if role != scope::ADMIN && role != scope::USER {
        return Err(invalid("Invalid role. Must be 'admin' or 'user'"));
    }
    Ok(())
}

fn time(unix: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(unix).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// Whether some enabled admin other than `user_id` is left.
fn other_admins(conn: &Connection, user_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM users WHERE role = ?1 AND enabled AND user_id != ?2)",
        (scope::ADMIN, user_id),
        |row| row.get(0),
    )
}

fn existing(conn: &Connection, user_id: &str) -> rusqlite::Result<Result<User>> {
    Ok(user_by_id(conn, user_id)?.ok_or_else(|| invalid("User not found")))
}

impl Auth {
    /// Every user, oldest first.
    pub async fn users(&self) -> Result<Vec<User>> {
        Ok(self
            .db
            .call(|conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {USER_COLUMNS} FROM users ORDER BY created_at, username"
                ))?;
                let users = statement
                    .query_map([], user_from_row)?
                    .collect::<rusqlite::Result<Vec<User>>>()?;
                Ok(users)
            })
            .await?)
    }

    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
        display_name: Option<String>,
    ) -> Result<User> {
        let username = valid_username(username)?;
        valid_password(password)?;
        valid_role(role)?;
        let password_hash = hash_password(password.to_owned()).await?;
        let role = role.to_owned();
        self.db
            .call(move |conn| {
                let taken: bool = conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM users WHERE username = ?1)",
                    [&username],
                    |row| row.get(0),
                )?;
                if taken {
                    return Ok(Err(invalid("That username is taken")));
                }
                let user_id = new_id();
                conn.execute(
                    "INSERT INTO users (user_id, username, password_hash, role, display_name, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&user_id, &username, &password_hash, &role, &display_name, now()),
                )?;
                Ok(Ok(user_by_id(conn, &user_id)?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?))
            })
            .await?
    }

    /// Changes a user. Making the last enabled admin a user is refused; an admin made a user
    /// is signed out everywhere, so their open connections lose what they could do.
    pub async fn update_user(&self, user_id: &str, changes: UserChanges) -> Result<User> {
        let username = changes
            .username
            .as_deref()
            .map(valid_username)
            .transpose()?;
        if let Some(password) = &changes.password {
            valid_password(password)?;
        }
        if let Some(role) = &changes.role {
            valid_role(role)?;
        }
        let password_hash = match changes.password {
            Some(password) => Some(hash_password(password).await?),
            None => None,
        };
        let (id, role, display_name) = (user_id.to_owned(), changes.role, changes.display_name);
        let (user, demoted) = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let before = match existing(&tx, &id)? {
                    Ok(user) => user,
                    Err(error) => return Ok(Err(error)),
                };
                let demoted = before.role == scope::ADMIN
                    && role.as_deref().is_some_and(|r| r != scope::ADMIN);
                if demoted && !other_admins(&tx, &id)? {
                    return Ok(Err(invalid("The last admin cannot be made a user")));
                }
                if let Some(username) = &username {
                    let taken: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM users WHERE username = ?1 AND user_id != ?2)",
                        (username, &id),
                        |row| row.get(0),
                    )?;
                    if taken {
                        return Ok(Err(invalid("That username is taken")));
                    }
                    tx.execute(
                        "UPDATE users SET username = ?2 WHERE user_id = ?1",
                        (&id, username),
                    )?;
                }
                if let Some(display_name) = &display_name {
                    let display_name = Some(display_name.trim()).filter(|n| !n.is_empty());
                    tx.execute(
                        "UPDATE users SET display_name = ?2 WHERE user_id = ?1",
                        (&id, display_name),
                    )?;
                }
                if let Some(role) = &role {
                    tx.execute("UPDATE users SET role = ?2 WHERE user_id = ?1", (&id, role))?;
                }
                if let Some(hash) = &password_hash {
                    tx.execute(
                        "UPDATE users SET password_hash = ?2 WHERE user_id = ?1",
                        (&id, hash),
                    )?;
                }
                let user = existing(&tx, &id)?;
                tx.commit()?;
                Ok(user.map(|user| (user, demoted)))
            })
            .await??;
        if demoted {
            self.revoke_all(&user.user_id).await?;
        }
        Ok(user)
    }

    /// Enables or disables a user. A disabled user is signed out everywhere.
    pub async fn set_enabled(&self, user_id: &str, enabled: bool) -> Result<()> {
        let id = user_id.to_owned();
        self.db
            .call(move |conn| {
                let user = match existing(conn, &id)? {
                    Ok(user) => user,
                    Err(error) => return Ok(Err(error)),
                };
                if !enabled && user.role == scope::ADMIN && !other_admins(conn, &id)? {
                    return Ok(Err(invalid("The last admin cannot be disabled")));
                }
                conn.execute(
                    "UPDATE users SET enabled = ?2 WHERE user_id = ?1",
                    (&id, enabled),
                )?;
                Ok(Ok(()))
            })
            .await??;
        if !enabled {
            self.revoke_all(user_id).await?;
        }
        Ok(())
    }

    /// Deletes a user, signing them out everywhere.
    pub async fn delete_user(&self, user_id: &str) -> Result<()> {
        let id = user_id.to_owned();
        let tokens = self
            .db
            .call(move |conn| {
                let user = match existing(conn, &id)? {
                    Ok(user) => user,
                    Err(error) => return Ok(Err(error)),
                };
                if user.role == scope::ADMIN && !other_admins(conn, &id)? {
                    return Ok(Err(invalid("The last admin cannot be deleted")));
                }
                let tokens = token_ids(conn, &id)?;
                // Its tokens go with it.
                conn.execute("DELETE FROM users WHERE user_id = ?1", [&id])?;
                Ok(Ok(tokens))
            })
            .await??;
        for token in tokens {
            let _ = self.revoked.send(token);
        }
        Ok(())
    }

    /// `user_id`'s sign-ins, newest first.
    pub async fn tokens(&self, user_id: &str) -> Result<Vec<AuthToken>> {
        let id = user_id.to_owned();
        Ok(self
            .db
            .call(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT token_id, user_id, name, long_lived, created_at, expires_at, last_used_at
                     FROM auth_tokens WHERE user_id = ?1
                     ORDER BY created_at DESC, token_id LIMIT ?2",
                )?;
                let tokens = statement
                    .query_map((&id, TOKEN_LIST_LIMIT), |row| {
                        Ok(AuthToken {
                            token_id: row.get(0)?,
                            user_id: row.get(1)?,
                            token_hash: String::new(),
                            name: row.get(2)?,
                            is_long_lived: row.get(3)?,
                            created_at: time(row.get(4)?),
                            expires_at: Some(time(row.get(5)?)),
                            last_used_at: row.get::<_, Option<i64>>(6)?.map(time),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<AuthToken>>>()?;
                Ok(tokens)
            })
            .await?)
    }

    /// The user a token belongs to, if it exists.
    pub async fn token_owner(&self, token_id: &str) -> Result<Option<String>> {
        let id = token_id.to_owned();
        Ok(self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT user_id FROM auth_tokens WHERE token_id = ?1",
                        [&id],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await?)
    }

    /// Revokes every token of `user_id`, closing the connections that use them.
    pub async fn revoke_all(&self, user_id: &str) -> Result<()> {
        let id = user_id.to_owned();
        let tokens = self
            .db
            .call(move |conn| {
                let tokens = token_ids(conn, &id)?;
                conn.execute("DELETE FROM auth_tokens WHERE user_id = ?1", [&id])?;
                Ok(tokens)
            })
            .await?;
        for token in tokens {
            let _ = self.revoked.send(token);
        }
        Ok(())
    }
}

fn token_ids(conn: &Connection, user_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT token_id FROM auth_tokens WHERE user_id = ?1")?;
    statement.query_map([user_id], |row| row.get(0))?.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MIGRATIONS;
    use crate::auth::lifetime::TokenKind;
    use bopsie_core::db::Database;

    async fn with_admin() -> (Auth, User) {
        let auth = Auth::open(Database::open_in_memory(&[MIGRATIONS]).unwrap())
            .await
            .unwrap();
        let admin = auth
            .create_first_admin("alice", "correct horse", None)
            .await
            .unwrap()
            .unwrap();
        (auth, admin)
    }

    #[tokio::test]
    async fn users_are_created_changed_and_deleted() {
        let (auth, admin) = with_admin().await;
        let bob = auth
            .create_user("bob", "battery staple", scope::USER, Some("Bob".into()))
            .await
            .unwrap();
        assert_eq!(
            (bob.role.as_str(), bob.display_name.as_deref()),
            ("user", Some("Bob"))
        );
        assert!(
            auth.create_user("BOB", "battery staple", "user", None)
                .await
                .is_err()
        );
        assert!(
            auth.create_user("x", "battery staple", "user", None)
                .await
                .is_err()
        );
        assert!(
            auth.create_user("carol", "short", "user", None)
                .await
                .is_err()
        );
        assert!(
            auth.create_user("carol", "long enough", "guest", None)
                .await
                .is_err()
        );

        let bob = auth
            .update_user(
                &bob.user_id,
                UserChanges {
                    username: Some("robert".into()),
                    password: Some("new password".into()),
                    ..UserChanges::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(bob.username, "robert");
        assert!(
            auth.verify_login("robert", "new password")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            auth.verify_login("robert", "battery staple")
                .await
                .unwrap()
                .is_none()
        );

        let names: Vec<String> = auth
            .users()
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.username)
            .collect();
        assert_eq!(names, ["alice", "robert"]);
        auth.delete_user(&bob.user_id).await.unwrap();
        assert_eq!(auth.users().await.unwrap(), [admin]);
    }

    #[tokio::test]
    async fn an_enabled_admin_is_always_left() {
        let (auth, admin) = with_admin().await;
        let only = |result: Result<_>| result.unwrap_err().details;
        assert_eq!(
            only(auth.set_enabled(&admin.user_id, false).await),
            "The last admin cannot be disabled"
        );
        assert_eq!(
            only(auth.delete_user(&admin.user_id).await),
            "The last admin cannot be deleted"
        );
        assert_eq!(
            only(
                auth.update_user(
                    &admin.user_id,
                    UserChanges {
                        role: Some("user".into()),
                        ..UserChanges::default()
                    }
                )
                .await
                .map(|_| ())
            ),
            "The last admin cannot be made a user"
        );
        // With a second admin, the first may go.
        auth.create_user("bob", "battery staple", scope::ADMIN, None)
            .await
            .unwrap();
        auth.set_enabled(&admin.user_id, false).await.unwrap();
    }

    #[tokio::test]
    async fn disabling_a_user_revokes_their_sign_ins() {
        let (auth, _) = with_admin().await;
        let bob = auth
            .create_user("bob", "battery staple", scope::USER, None)
            .await
            .unwrap();
        let mut revoked = auth.revocations();
        let token = auth
            .issue(&bob.user_id, "Bopsie", TokenKind::LongLived)
            .await
            .unwrap();
        let tokens = auth.tokens(&bob.user_id).await.unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            (tokens[0].name.as_str(), tokens[0].is_long_lived),
            ("Bopsie", true)
        );
        assert_eq!(
            auth.token_owner(&tokens[0].token_id)
                .await
                .unwrap()
                .as_deref(),
            Some(bob.user_id.as_str())
        );

        auth.set_enabled(&bob.user_id, false).await.unwrap();
        assert_eq!(revoked.recv().await.unwrap(), tokens[0].token_id);
        assert!(auth.tokens(&bob.user_id).await.unwrap().is_empty());
        assert!(auth.authenticate(&token).await.is_err());
        assert!(
            auth.verify_login("bob", "battery staple")
                .await
                .unwrap()
                .is_none()
        );
    }
}
