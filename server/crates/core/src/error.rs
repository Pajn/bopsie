//! Errors as the API reports them: a Music Assistant error code and a human-readable detail.

/// Music Assistant's error codes, from `music_assistant_models/errors.py`. Clients branch on the
/// number, so the values are part of the wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ErrorCode {
    Generic = 0,
    ProviderUnavailable = 1,
    MediaNotFound = 2,
    InvalidData = 3,
    AlreadyRegistered = 4,
    SetupFailed = 5,
    LoginFailed = 6,
    Audio = 7,
    QueueEmpty = 8,
    UnsupportedFeature = 9,
    PlayerUnavailable = 10,
    PlayerCommandFailed = 11,
    InvalidCommand = 12,
    UnplayableMedia = 13,
    InvalidProviderUri = 14,
    InvalidProviderId = 15,
    RetriesExhausted = 16,
    ResourceTemporarilyUnavailable = 17,
    ProviderPermissionDenied = 18,
    ActionUnavailable = 19,
    AuthenticationRequired = 20,
    AuthenticationFailed = 21,
    InsufficientPermissions = 22,
    InvalidToken = 23,
    ResourceBusy = 24,
    RateLimited = 25,
    UnsupportedSystem = 26,
    UserNotFound = 27,
    /// What Music Assistant reports for exceptions it did not anticipate.
    Unexpected = 999,
}

impl ErrorCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// An error that reaches a client.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{details}")]
pub struct Error {
    pub code: ErrorCode,
    pub details: String,
}

impl Error {
    pub fn new(code: ErrorCode, details: impl Into<String>) -> Self {
        Self {
            code,
            details: details.into(),
        }
    }

    pub fn invalid_data(details: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidData, details)
    }

    /// A failure the client cannot act on. The cause belongs in the log, not in `details`.
    pub fn unexpected() -> Self {
        Self::new(ErrorCode::Unexpected, "Internal server error")
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
