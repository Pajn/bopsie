//! `auth/*` commands. The `auth` command itself authenticates a connection, so the `/ws`
//! connection handles it rather than the table.

use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::lifetime::TokenKind;
use crate::auth::{Scope, User};
use crate::command::{Access, Command, Context};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoginArgs {
    pub username: String,
    pub password: String,
    /// The authentication provider. Only `builtin` exists.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Names the session, so the user can tell their sessions apart.
    #[serde(default)]
    pub device_name: Option<String>,
}

/// The user as `auth/login` reports it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LoginUser {
    pub user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
}

/// A failed login is a result, not an error, as in Music Assistant: clients check `success`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LoginResult {
    Success {
        success: bool,
        access_token: String,
        user: LoginUser,
    },
    Failure {
        success: bool,
        error: String,
    },
}

pub const LOGIN_FAILED: &str = "Invalid username or password";

/// The name sessions get when the client does not name its device.
pub const UNNAMED_DEVICE: &str = "Unknown device";

pub struct Login;
impl Command for Login {
    const NAME: &'static str = "auth/login";
    const ACCESS: Access = Access::Public;
    const SUMMARY: &'static str = "Log in with a username and password and get a session token. Does not authenticate the connection.";
    type Args = LoginArgs;
    type Output = LoginResult;

    async fn handle(ctx: Context, args: LoginArgs) -> Result<LoginResult> {
        let failure = |error: &str| LoginResult::Failure {
            success: false,
            error: error.to_owned(),
        };
        if args
            .provider_id
            .as_deref()
            .is_some_and(|id| id != "builtin")
        {
            return Ok(failure("Unknown authentication provider"));
        }
        let auth = &ctx.api.auth;
        let Some(user) = auth.verify_login(&args.username, &args.password).await? else {
            return Ok(failure(LOGIN_FAILED));
        };
        let device = args.device_name.as_deref().unwrap_or(UNNAMED_DEVICE);
        let access_token = auth
            .issue(&user.user_id, device, TokenKind::Session)
            .await?;
        Ok(LoginResult::Success {
            success: true,
            access_token,
            user: LoginUser {
                user_id: user.user_id,
                username: user.username,
                display_name: user.display_name,
                role: user.role,
            },
        })
    }
}

pub struct Me;
impl Command for Me {
    const NAME: &'static str = "auth/me";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str = "The authenticated user.";
    type Args = NoArgs;
    type Output = User;

    async fn handle(ctx: Context, _: NoArgs) -> Result<User> {
        let session = ctx.session()?;
        ctx.api
            .auth
            .user(&session.user.user_id)
            .await?
            .ok_or_else(|| Error::new(ErrorCode::UserNotFound, "User not found"))
    }
}

pub struct Logout;
impl Command for Logout {
    const NAME: &'static str = "auth/logout";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str =
        "Revoke the token this connection authenticated with, and close every connection using it.";
    type Args = NoArgs;
    type Output = ();

    async fn handle(ctx: Context, _: NoArgs) -> Result<()> {
        let token_id = ctx.session()?.token_id.clone();
        ctx.api.auth.revoke(&token_id).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTokenArgs {
    /// What the token is for.
    pub name: String,
    /// Whose token it is. Creating a token for another user needs the `users.manage` scope.
    #[serde(default)]
    pub user_id: Option<String>,
}

pub struct CreateToken;
impl Command for CreateToken {
    const NAME: &'static str = "auth/token/create";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str = "Create a long-lived token, valid for a year.";
    type Args = CreateTokenArgs;
    type Output = String;

    async fn handle(ctx: Context, args: CreateTokenArgs) -> Result<String> {
        let session = ctx.session()?;
        let user_id = match args.user_id {
            Some(user_id) if user_id != session.user.user_id => {
                if !session.allows(Scope::UsersManage) {
                    return Err(Error::new(
                        ErrorCode::InsufficientPermissions,
                        "Creating a token for another user requires the users.manage scope",
                    ));
                }
                if ctx.api.auth.user(&user_id).await?.is_none() {
                    return Err(Error::new(ErrorCode::UserNotFound, "User not found"));
                }
                user_id
            }
            _ => session.user.user_id.clone(),
        };
        ctx.api
            .auth
            .issue(&user_id, &args.name, TokenKind::LongLived)
            .await
    }
}
