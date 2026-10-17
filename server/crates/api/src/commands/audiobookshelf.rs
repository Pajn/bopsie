//! The Audiobookshelf server the server takes audiobooks from, from the web UI: signing in to
//! it, which keeps the tokens it gives and not the password, and signing out.

use bopsie_core::{Error, ErrorCode, Result};
use bopsie_sources_audiobookshelf::{Account, AccountError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::Scope;
use crate::command::{Access, Command, Context};
use crate::commands::auth::NoArgs;

/// Audiobookshelf's errors as the API reports them.
pub fn error(error: AccountError) -> Error {
    let code = match &error {
        AccountError::SignedOut => ErrorCode::AuthenticationRequired,
        AccountError::Refused(_) => ErrorCode::ProviderPermissionDenied,
        AccountError::Unreachable(_) => ErrorCode::ProviderUnavailable,
        AccountError::NotFound(_) => ErrorCode::MediaNotFound,
        AccountError::Db(_) => ErrorCode::Generic,
    };
    Error::new(code, error.to_string())
}

fn account(ctx: &Context) -> Result<&Account> {
    ctx.api.audiobookshelf.as_ref().ok_or_else(|| {
        Error::new(
            ErrorCode::ProviderUnavailable,
            "Audiobookshelf is not available on this server",
        )
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AudiobookshelfStatus {
    /// The server's address, once signed in to it.
    pub url: Option<String>,
    /// Who the server is signed in as, or was last.
    pub user: Option<String>,
    pub signed_in: bool,
}

pub struct Status;
impl Command for Status {
    const NAME: &'static str = "admin/audiobookshelf/status";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Which Audiobookshelf server the server takes audiobooks from, and who it is signed in as.";
    type Args = NoArgs;
    type Output = AudiobookshelfStatus;

    async fn handle(ctx: Context, _: NoArgs) -> Result<AudiobookshelfStatus> {
        let status = account(&ctx)?.status();
        Ok(AudiobookshelfStatus {
            url: status.url,
            user: status.user,
            signed_in: status.signed_in,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoginArgs {
    /// The server's address, such as `http://audiobooks.local:13378`.
    pub url: String,
    pub username: String,
    /// Used to sign in once and kept nowhere: the server keeps the tokens it is given.
    pub password: String,
}

pub struct Login;
impl Command for Login {
    const NAME: &'static str = "admin/audiobookshelf/login";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Sign in to an Audiobookshelf server, keeping the tokens it gives and not the password.";
    type Args = LoginArgs;
    type Output = AudiobookshelfStatus;

    async fn handle(ctx: Context, args: LoginArgs) -> Result<AudiobookshelfStatus> {
        let account = account(&ctx)?;
        account
            .sign_in(&args.url, &args.username, &args.password)
            .await
            .map_err(error)?;
        let status = account.status();
        tracing::info!(user = status.user, "signed in to Audiobookshelf");
        Ok(AudiobookshelfStatus {
            url: status.url,
            user: status.user,
            signed_in: status.signed_in,
        })
    }
}

pub struct SignOut;
impl Command for SignOut {
    const NAME: &'static str = "admin/audiobookshelf/sign_out";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Forget the Audiobookshelf sign-in.";
    type Args = NoArgs;
    type Output = ();

    async fn handle(ctx: Context, _: NoArgs) -> Result<()> {
        account(&ctx)?.sign_out().await.map_err(error)
    }
}
