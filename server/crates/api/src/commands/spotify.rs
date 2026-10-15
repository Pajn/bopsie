//! The Spotify account the server uses as a music provider, from the web UI: its developer
//! app for the Web API, logging in to it, and pairing for playback.

use bopsie_core::{Error, ErrorCode, Result};
use bopsie_sources_spotify::{AccountError, SpotifyAccount};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::Scope;
use crate::command::{Access, Command, Context};
use crate::commands::auth::NoArgs;

/// What the Spotify device to pick for playback is called.
pub const SIGN_IN_NAME: &str = "Bopsie sign-in";

/// Spotify's errors as the API reports them.
pub fn error(error: AccountError) -> Error {
    let code = match &error {
        AccountError::NoClientId | AccountError::NotLoggedIn | AccountError::NoPlayback => {
            ErrorCode::AuthenticationRequired
        }
        AccountError::UnexpectedCallback => ErrorCode::InvalidData,
        AccountError::RateLimited => ErrorCode::RateLimited,
        AccountError::Refused(_) => ErrorCode::ProviderPermissionDenied,
        AccountError::Unreachable(_) => ErrorCode::ProviderUnavailable,
        AccountError::Db(_) => ErrorCode::Generic,
    };
    Error::new(code, error.to_string())
}

fn account(ctx: &Context) -> Result<&SpotifyAccount> {
    ctx.api.spotify.as_ref().ok_or_else(|| {
        Error::new(
            ErrorCode::ProviderUnavailable,
            "Spotify is turned off in the config file",
        )
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SpotifyStatus {
    /// Whether the config file turns Spotify on.
    pub enabled: bool,
    /// The developer app's client ID, once given.
    pub client_id: Option<String>,
    /// Who the Web API login is: search and the saved library.
    pub web_user: Option<String>,
    /// Who the playback login is: playing and playlists.
    pub playback_user: Option<String>,
    /// Waiting to be picked in the Spotify app, under `sign_in_name`.
    pub pairing: bool,
    pub sign_in_name: String,
    pub error: Option<String>,
}

pub struct Status;
impl Command for Status {
    const NAME: &'static str = "admin/spotify/status";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "How the Spotify account stands: its developer app, and who it is logged in as for the Web API and for playback.";
    type Args = NoArgs;
    type Output = SpotifyStatus;

    async fn handle(ctx: Context, _: NoArgs) -> Result<SpotifyStatus> {
        let Some(account) = ctx.api.spotify.as_ref() else {
            return Ok(SpotifyStatus {
                enabled: false,
                client_id: None,
                web_user: None,
                playback_user: None,
                pairing: false,
                sign_in_name: SIGN_IN_NAME.to_owned(),
                error: None,
            });
        };
        let status = account.status().await;
        Ok(SpotifyStatus {
            enabled: true,
            client_id: status.client_id,
            web_user: status.web_user,
            playback_user: status.playback_user,
            pairing: status.pairing,
            sign_in_name: SIGN_IN_NAME.to_owned(),
            error: status.error,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetClientIdArgs {
    /// The developer app's client ID; empty or `null` forgets it. Either ends the Web API login.
    pub client_id: Option<String>,
}

pub struct SetClientId;
impl Command for SetClientId {
    const NAME: &'static str = "admin/spotify/set_client_id";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Use a Spotify developer app for the Web API, by its client ID.";
    type Args = SetClientIdArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SetClientIdArgs) -> Result<()> {
        account(&ctx)?
            .set_client_id(args.client_id)
            .await
            .map_err(error)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoginArgs {
    /// Where Spotify sends the listener back to: this server's `/spotify/callback`, as the web
    /// UI reaches it, which must be among the developer app's redirect URIs.
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LoginStart {
    /// Spotify's login page, to open.
    pub url: String,
}

pub struct Login;
impl Command for Login {
    const NAME: &'static str = "admin/spotify/login";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Start logging in to Spotify's Web API: the page to open, which comes back to redirect_uri.";
    type Args = LoginArgs;
    type Output = LoginStart;

    async fn handle(ctx: Context, args: LoginArgs) -> Result<LoginStart> {
        if !args.redirect_uri.ends_with("/spotify/callback") {
            return Err(Error::invalid_data(
                "redirect_uri is this server's /spotify/callback",
            ));
        }
        let url = account(&ctx)?
            .authorize_url(&args.redirect_uri)
            .map_err(error)?;
        Ok(LoginStart { url })
    }
}

pub struct PairStart;
impl Command for PairStart {
    const NAME: &'static str = "admin/spotify/pair_start";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Sign in to Spotify for playback: announces a device to pick in the Spotify app, until it is picked.";
    type Args = NoArgs;
    type Output = ();

    async fn handle(ctx: Context, _: NoArgs) -> Result<()> {
        let addresses = if ctx.api.config.spotify.addresses.is_empty() {
            bopsie_sources_spotify::local_addresses()
        } else {
            ctx.api.config.spotify.addresses.clone()
        };
        account(&ctx)?.pair(SIGN_IN_NAME, addresses);
        Ok(())
    }
}

pub struct PairCancel;
impl Command for PairCancel {
    const NAME: &'static str = "admin/spotify/pair_cancel";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Stop waiting to be picked in the Spotify app.";
    type Args = NoArgs;
    type Output = ();

    async fn handle(ctx: Context, _: NoArgs) -> Result<()> {
        account(&ctx)?.cancel_pairing();
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpotifyLogin {
    /// The Web API login: search and the saved library.
    Web,
    /// The playback login: playing and playlists.
    Playback,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignOutArgs {
    pub login: SpotifyLogin,
}

pub struct SignOut;
impl Command for SignOut {
    const NAME: &'static str = "admin/spotify/sign_out";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "End and forget one of the Spotify logins: web or playback.";
    type Args = SignOutArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SignOutArgs) -> Result<()> {
        let account = account(&ctx)?;
        match args.login {
            SpotifyLogin::Web => account.web_sign_out().await.map_err(error),
            SpotifyLogin::Playback => {
                account.playback_sign_out().await;
                Ok(())
            }
        }
    }
}
