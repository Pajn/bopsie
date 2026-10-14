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

fn manage_required(what: &str) -> Error {
    Error::new(
        ErrorCode::InsufficientPermissions,
        format!("{what} requires the users.manage scope"),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UserIdArgs {
    pub user_id: String,
}

pub struct Users;
impl Command for Users {
    const NAME: &'static str = "auth/users";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "Every user.";
    type Args = NoArgs;
    type Output = Vec<User>;

    async fn handle(ctx: Context, _: NoArgs) -> Result<Vec<User>> {
        ctx.api.auth.users().await
    }
}

pub struct GetUser;
impl Command for GetUser {
    const NAME: &'static str = "auth/user";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "One user, or null.";
    type Args = UserIdArgs;
    type Output = Option<User>;

    async fn handle(ctx: Context, args: UserIdArgs) -> Result<Option<User>> {
        ctx.api.auth.user(&args.user_id).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateUserArgs {
    pub username: String,
    /// At least 8 characters.
    pub password: String,
    /// `admin` or `user`.
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

fn default_role() -> String {
    "user".to_owned()
}

pub struct CreateUser;
impl Command for CreateUser {
    const NAME: &'static str = "auth/user/create";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "Create a user who signs in with a username and password.";
    type Args = CreateUserArgs;
    type Output = User;

    async fn handle(ctx: Context, args: CreateUserArgs) -> Result<User> {
        ctx.api
            .auth
            .create_user(
                &args.username,
                &args.password,
                &args.role,
                args.display_name,
            )
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateUserArgs {
    /// Whose profile; yourself unless given. Another user's needs the `users.manage` scope.
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// `admin` or `user`. Needs the `users.manage` scope. An admin made a user is signed out.
    #[serde(default)]
    pub role: Option<String>,
}

pub struct UpdateUser;
impl Command for UpdateUser {
    const NAME: &'static str = "auth/user/update";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str = "Change a user's name, display name, password or role.";
    type Args = UpdateUserArgs;
    type Output = User;

    async fn handle(ctx: Context, args: UpdateUserArgs) -> Result<User> {
        let session = ctx.session()?;
        let manage = session.allows(Scope::UsersManage);
        let user_id = match args.user_id {
            Some(user_id) if user_id != session.user.user_id => {
                if !manage {
                    return Err(manage_required("Updating another user"));
                }
                user_id
            }
            _ => session.user.user_id.clone(),
        };
        if args.role.is_some() && !manage {
            return Err(manage_required("Changing a role"));
        }
        ctx.api
            .auth
            .update_user(
                &user_id,
                crate::auth::users::UserChanges {
                    username: args.username,
                    display_name: args.display_name,
                    password: args.password.filter(|p| !p.is_empty()),
                    role: args.role,
                },
            )
            .await
    }
}

/// Refuses an admin acting on their own account where Music Assistant does too.
fn not_yourself(ctx: &Context, user_id: &str, what: &str) -> Result<()> {
    if ctx.session()?.user.user_id == user_id {
        return Err(Error::invalid_data(format!(
            "Cannot {what} your own account"
        )));
    }
    Ok(())
}

pub struct EnableUser;
impl Command for EnableUser {
    const NAME: &'static str = "auth/user/enable";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "Let a user sign in again.";
    type Args = UserIdArgs;
    type Output = ();

    async fn handle(ctx: Context, args: UserIdArgs) -> Result<()> {
        ctx.api.auth.set_enabled(&args.user_id, true).await
    }
}

pub struct DisableUser;
impl Command for DisableUser {
    const NAME: &'static str = "auth/user/disable";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "Stop a user signing in, and sign them out everywhere.";
    type Args = UserIdArgs;
    type Output = ();

    async fn handle(ctx: Context, args: UserIdArgs) -> Result<()> {
        not_yourself(&ctx, &args.user_id, "disable")?;
        ctx.api.auth.set_enabled(&args.user_id, false).await
    }
}

pub struct DeleteUser;
impl Command for DeleteUser {
    const NAME: &'static str = "auth/user/delete";
    const ACCESS: Access = Access::Scope(Scope::UsersManage);
    const SUMMARY: &'static str = "Delete a user, signing them out everywhere.";
    type Args = UserIdArgs;
    type Output = ();

    async fn handle(ctx: Context, args: UserIdArgs) -> Result<()> {
        not_yourself(&ctx, &args.user_id, "delete")?;
        ctx.api.auth.delete_user(&args.user_id).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TokensArgs {
    /// Whose sign-ins; yours unless given. Another user's need the `users.manage` scope.
    #[serde(default)]
    pub user_id: Option<String>,
}

pub struct Tokens;
impl Command for Tokens {
    const NAME: &'static str = "auth/tokens";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str = "A user's sign-ins, newest first.";
    type Args = TokensArgs;
    type Output = Vec<crate::auth::users::AuthToken>;

    async fn handle(ctx: Context, args: TokensArgs) -> Result<Vec<crate::auth::users::AuthToken>> {
        let session = ctx.session()?;
        let user_id = match args.user_id {
            // As Music Assistant: another user's, without the scope, is an empty list.
            Some(user_id) if user_id != session.user.user_id => {
                if !session.allows(Scope::UsersManage) {
                    return Ok(Vec::new());
                }
                user_id
            }
            _ => session.user.user_id.clone(),
        };
        ctx.api.auth.tokens(&user_id).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RevokeTokenArgs {
    pub token_id: String,
}

pub struct RevokeToken;
impl Command for RevokeToken {
    const NAME: &'static str = "auth/token/revoke";
    const ACCESS: Access = Access::Authenticated;
    const SUMMARY: &'static str = "Revoke a sign-in, closing every connection using it. Another user's needs the users.manage scope.";
    type Args = RevokeTokenArgs;
    type Output = ();

    async fn handle(ctx: Context, args: RevokeTokenArgs) -> Result<()> {
        let session = ctx.session()?;
        let owner = ctx
            .api
            .auth
            .token_owner(&args.token_id)
            .await?
            .ok_or_else(|| Error::invalid_data("Token not found"))?;
        if owner != session.user.user_id && !session.allows(Scope::UsersManage) {
            return Err(Error::new(
                ErrorCode::InsufficientPermissions,
                "You can only revoke your own tokens",
            ));
        }
        ctx.api.auth.revoke(&args.token_id).await
    }
}
