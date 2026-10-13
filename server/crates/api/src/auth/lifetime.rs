//! When a token expires, following Music Assistant: a session token lives 30 days from its last
//! use but never beyond 90 days from creation; a long-lived token lives 365 days, unrenewed.
//! Times are unix seconds.

const DAY: i64 = 24 * 60 * 60;
pub const SESSION: i64 = 30 * DAY;
pub const SESSION_CAP: i64 = 90 * DAY;
pub const LONG_LIVED: i64 = 365 * DAY;
/// Use is recorded at most this often, so that every command does not write to the database.
pub const TOUCH_INTERVAL: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Session,
    LongLived,
}

/// The expiry stored in the database, which is authoritative, and the `exp` put in the JWT,
/// which is only the outer bound.
pub fn initial(kind: TokenKind, now: i64) -> (i64, i64) {
    match kind {
        TokenKind::Session => (now + SESSION, now + SESSION_CAP),
        TokenKind::LongLived => (now + LONG_LIVED, now + LONG_LIVED),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenTimes {
    pub kind: TokenKind,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Expired,
    /// Valid. When `touch` is set, record the use: store `now` as the last use and the new
    /// expiry.
    Valid {
        touch: Option<i64>,
    },
}

pub fn check(token: &TokenTimes, now: i64) -> Check {
    if now >= token.expires_at {
        return Check::Expired;
    }
    let due = token
        .last_used_at
        .is_none_or(|last| now - last >= TOUCH_INTERVAL);
    let touch = match token.kind {
        TokenKind::Session => {
            if now >= token.created_at + SESSION_CAP {
                return Check::Expired;
            }
            due.then(|| (now + SESSION).min(token.created_at + SESSION_CAP))
        }
        TokenKind::LongLived => due.then_some(token.expires_at),
    };
    Check::Valid { touch }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    fn session(expires_at: i64, last_used_at: Option<i64>) -> TokenTimes {
        TokenTimes {
            kind: TokenKind::Session,
            created_at: T0,
            expires_at,
            last_used_at,
        }
    }

    #[test]
    fn new_tokens() {
        assert_eq!(
            initial(TokenKind::Session, T0),
            (T0 + 30 * DAY, T0 + 90 * DAY)
        );
        assert_eq!(
            initial(TokenKind::LongLived, T0),
            (T0 + 365 * DAY, T0 + 365 * DAY)
        );
    }

    #[test]
    fn session_tokens() {
        let fresh = T0 + 30 * DAY;
        let cases = [
            // Used for the first time: renewed from now.
            (
                session(fresh, None),
                T0 + 10,
                Check::Valid {
                    touch: Some(T0 + 10 + SESSION),
                },
            ),
            // Used again within the hour: nothing written.
            (
                session(fresh, Some(T0)),
                T0 + 59 * 60,
                Check::Valid { touch: None },
            ),
            // Used after an hour: renewed.
            (
                session(fresh, Some(T0)),
                T0 + 60 * 60,
                Check::Valid {
                    touch: Some(T0 + 60 * 60 + SESSION),
                },
            ),
            // Renewal stops at the 90-day cap.
            (
                session(T0 + 89 * DAY, Some(T0 + 70 * DAY)),
                T0 + 80 * DAY,
                Check::Valid {
                    touch: Some(T0 + 90 * DAY),
                },
            ),
            // Unused for its 30 days.
            (session(fresh, Some(T0)), fresh, Check::Expired),
            // Past the cap, whatever the stored expiry says.
            (
                session(T0 + 100 * DAY, Some(T0 + 89 * DAY)),
                T0 + 90 * DAY,
                Check::Expired,
            ),
        ];
        for (token, now, expected) in cases {
            assert_eq!(check(&token, now), expected, "{token:?} at {now}");
        }
    }

    #[test]
    fn long_lived_tokens_are_never_renewed() {
        let token = TokenTimes {
            kind: TokenKind::LongLived,
            created_at: T0,
            expires_at: T0 + LONG_LIVED,
            last_used_at: None,
        };
        assert_eq!(
            check(&token, T0 + 200 * DAY),
            Check::Valid {
                touch: Some(T0 + LONG_LIVED)
            }
        );
        assert_eq!(check(&token, T0 + LONG_LIVED), Check::Expired);
    }
}
