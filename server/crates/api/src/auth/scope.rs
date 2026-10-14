//! What a user may do. Music Assistant checks a scope per command and derives scopes from the
//! user's role; only the scopes this server's commands need are listed.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    LibraryRead,
    PlayersRead,
    PlayersControl,
    QueuesRead,
    QueuesControl,
    UsersManage,
    /// What the server knows of itself and its players, for the web UI's admin pages.
    Admin,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::LibraryRead => "library.read",
            Scope::PlayersRead => "players.read",
            Scope::PlayersControl => "players.control",
            Scope::QueuesRead => "queues.read",
            Scope::QueuesControl => "queues.control",
            Scope::UsersManage => "users.manage",
            Scope::Admin => "admin",
        }
    }
}

pub const ADMIN: &str = "admin";
pub const USER: &str = "user";

/// Whether a user with `role` holds `scope`. Roles this server does not create, such as
/// Music Assistant's `guest` and `service`, hold nothing.
pub fn allows(role: &str, scope: Scope) -> bool {
    match role {
        ADMIN => true,
        USER => !matches!(scope, Scope::UsersManage | Scope::Admin),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles() {
        assert!(allows(ADMIN, Scope::UsersManage));
        assert!(allows(USER, Scope::QueuesControl));
        assert!(!allows(USER, Scope::UsersManage));
        assert!(allows(ADMIN, Scope::Admin));
        assert!(!allows(USER, Scope::Admin));
        assert!(!allows("guest", Scope::LibraryRead));
    }
}
