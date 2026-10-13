//! The command table. Every API command is a type implementing [`Command`]; the [`Registry`]
//! dispatches to them by name, checks access, and describes them for `/api-docs/commands.json`,
//! so the published list is always the implemented one.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use bopsie_core::{Error, ErrorCode, Result};
use futures::future::BoxFuture;
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::Api;
use crate::auth::{Scope, Session};

/// Who may run a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anyone, before authenticating.
    Public,
    /// Any authenticated user.
    Authenticated,
    /// An authenticated user holding this scope.
    Scope(Scope),
}

/// What a handler gets to work with.
#[derive(Clone)]
pub struct Context {
    pub api: Arc<Api>,
    /// The caller, unless the command is [`Access::Public`] and the caller has not authenticated.
    pub session: Option<Session>,
}

impl Context {
    /// The caller. Always present for commands that are not public.
    pub fn session(&self) -> Result<&Session> {
        self.session.as_ref().ok_or_else(authentication_required)
    }
}

pub trait Command: 'static {
    /// The command name on the wire, such as `auth/me`.
    const NAME: &'static str;
    const ACCESS: Access;
    /// One line for the API docs.
    const SUMMARY: &'static str;
    /// The arguments, with Music Assistant's field names. Field doc comments become parameter
    /// descriptions in the API docs.
    type Args: DeserializeOwned + JsonSchema + Send;
    type Output: Serialize + JsonSchema;

    fn handle(
        ctx: Context,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'static;
}

pub fn authentication_required() -> Error {
    Error::new(
        ErrorCode::AuthenticationRequired,
        "Authentication required. Please send auth command first.",
    )
}

pub fn invalid_command() -> Error {
    Error::new(ErrorCode::InvalidCommand, "Invalid or unsupported command.")
}

type Handler = Box<dyn Fn(Context, Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

struct Entry {
    access: Access,
    handler: Handler,
    doc: CommandDoc,
}

/// One entry of `/api-docs/commands.json`, in Music Assistant's format.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandDoc {
    pub command: &'static str,
    pub category: String,
    pub summary: &'static str,
    pub description: String,
    pub parameters: Vec<ParameterDoc>,
    pub return_type: String,
    pub authenticated: bool,
    pub required_scope: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterDoc {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub required: bool,
    pub description: String,
}

#[derive(Default)]
pub struct Registry {
    entries: BTreeMap<&'static str, Entry>,
}

impl Registry {
    pub fn add<C: Command>(&mut self) -> &mut Self {
        let handler: Handler = Box::new(|ctx, args| {
            Box::pin(async move {
                // A command sent without `args` gets its defaults.
                let args = if args.is_null() {
                    Value::Object(Map::new())
                } else {
                    args
                };
                let args: C::Args = serde_json::from_value(args)
                    .map_err(|e| Error::invalid_data(format!("Invalid arguments: {e}")))?;
                let output = C::handle(ctx, args).await?;
                serde_json::to_value(output).map_err(|error| {
                    tracing::error!(%error, command = C::NAME, "serializing a result");
                    Error::unexpected()
                })
            })
        });
        let previous = self.entries.insert(
            C::NAME,
            Entry {
                access: C::ACCESS,
                handler,
                doc: describe::<C>(),
            },
        );
        assert!(previous.is_none(), "{} is registered twice", C::NAME);
        self
    }

    /// Runs `command` for the caller in `ctx`, after checking it exists and the caller may run
    /// it.
    pub async fn call(&self, ctx: Context, command: &str, args: Value) -> Result<Value> {
        let entry = self.entries.get(command).ok_or_else(invalid_command)?;
        match (entry.access, &ctx.session) {
            (Access::Public, _) => {}
            (_, None) => return Err(authentication_required()),
            (Access::Authenticated, Some(_)) => {}
            (Access::Scope(scope), Some(session)) => {
                if !session.allows(scope) {
                    return Err(Error::new(
                        ErrorCode::InsufficientPermissions,
                        format!("This command requires the {} scope", scope.as_str()),
                    ));
                }
            }
        }
        (entry.handler)(ctx, args).await
    }

    /// Who may run `command`, if it exists.
    pub fn access(&self, command: &str) -> Option<Access> {
        self.entries.get(command).map(|entry| entry.access)
    }

    /// Every command, sorted by name.
    pub fn docs(&self) -> Vec<CommandDoc> {
        self.entries
            .values()
            .map(|entry| entry.doc.clone())
            .collect()
    }
}

fn schema_for<T: JsonSchema>() -> Schema {
    SchemaSettings::default()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<T>()
}

fn describe<C: Command>() -> CommandDoc {
    let args = schema_for::<C::Args>();
    let required: Vec<&str> = args
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let parameters = args
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, schema)| ParameterDoc {
                    name: name.clone(),
                    kind: type_name(schema),
                    required: required.contains(&name.as_str()),
                    description: schema
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let category = C::NAME
        .split_once('/')
        .map_or("general", |(category, _)| category)
        .to_owned();
    CommandDoc {
        command: C::NAME,
        category,
        summary: C::SUMMARY,
        description: String::new(),
        parameters,
        return_type: type_name(schema_for::<C::Output>().as_value()),
        authenticated: C::ACCESS != Access::Public,
        required_scope: match C::ACCESS {
            Access::Scope(scope) => Some(scope.as_str()),
            _ => None,
        },
    }
}

/// A short type name for a JSON schema, such as `string`, `integer | null` or `any`.
fn type_name(schema: &Value) -> String {
    let names: Vec<String> = match schema.get("type") {
        Some(Value::String(name)) => vec![name.clone()],
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => match schema.get("anyOf").or_else(|| schema.get("oneOf")) {
            Some(Value::Array(variants)) => variants.iter().map(type_name).collect(),
            _ if schema.get("enum").is_some() || schema.get("const").is_some() => {
                vec!["string".to_owned()]
            }
            _ => vec!["any".to_owned()],
        },
    };
    names.join(" | ")
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::auth::lifetime::TokenKind;

    #[derive(Deserialize, JsonSchema)]
    struct EchoArgs {
        /// What to say back.
        text: String,
        /// How many times.
        #[serde(default)]
        times: Option<u32>,
    }

    struct Echo;
    impl Command for Echo {
        const NAME: &'static str = "test/echo";
        const ACCESS: Access = Access::Public;
        const SUMMARY: &'static str = "Says it back.";
        type Args = EchoArgs;
        type Output = String;
        async fn handle(_: Context, args: EchoArgs) -> Result<String> {
            Ok(args.text.repeat(args.times.unwrap_or(1) as usize))
        }
    }

    #[derive(Deserialize, JsonSchema)]
    struct NoArgs {}

    struct Whoami;
    impl Command for Whoami {
        const NAME: &'static str = "whoami";
        const ACCESS: Access = Access::Authenticated;
        const SUMMARY: &'static str = "";
        type Args = NoArgs;
        type Output = Option<String>;
        async fn handle(ctx: Context, _: NoArgs) -> Result<Option<String>> {
            Ok(Some(ctx.session()?.user.username.clone()))
        }
    }

    struct Manage;
    impl Command for Manage {
        const NAME: &'static str = "users/manage";
        const ACCESS: Access = Access::Scope(Scope::UsersManage);
        const SUMMARY: &'static str = "";
        type Args = NoArgs;
        type Output = ();
        async fn handle(_: Context, _: NoArgs) -> Result<()> {
            Ok(())
        }
    }

    fn registry() -> Registry {
        let mut registry = Registry::default();
        registry.add::<Echo>().add::<Whoami>().add::<Manage>();
        registry
    }

    async fn contexts() -> (Context, Context) {
        let api = crate::tests::api().await;
        let user = api
            .auth
            .create_first_admin("alice", "correct horse", None)
            .await
            .unwrap()
            .unwrap();
        let token = api
            .auth
            .issue(&user.user_id, "test", TokenKind::Session)
            .await
            .unwrap();
        let mut session = api.auth.authenticate(&token).await.unwrap();
        let admin = Context {
            api: api.clone(),
            session: Some(session.clone()),
        };
        session.user.role = "user".to_owned();
        let user = Context {
            api,
            session: Some(session),
        };
        (admin, user)
    }

    #[tokio::test]
    async fn dispatches_by_name_with_typed_args() {
        let (admin, _) = contexts().await;
        let registry = registry();
        assert_eq!(
            registry
                .call(
                    admin.clone(),
                    "test/echo",
                    json!({"text": "hi", "times": 2})
                )
                .await,
            Ok(json!("hihi"))
        );
        assert_eq!(
            registry.call(admin.clone(), "whoami", Value::Null).await,
            Ok(json!("alice"))
        );
        assert_eq!(
            registry.call(admin, "users/manage", json!({})).await,
            Ok(Value::Null)
        );
    }

    #[tokio::test]
    async fn rejects_unknown_commands_bad_args_and_missing_access() {
        let (admin, user) = contexts().await;
        let anonymous = Context {
            session: None,
            ..admin.clone()
        };
        let registry = registry();
        let code = |result: Result<Value>| result.unwrap_err().code;

        assert_eq!(
            code(registry.call(admin.clone(), "nope", json!({})).await),
            ErrorCode::InvalidCommand
        );
        assert_eq!(
            code(registry.call(admin, "test/echo", json!({"times": 2})).await),
            ErrorCode::InvalidData
        );
        assert_eq!(
            code(registry.call(anonymous.clone(), "whoami", json!({})).await),
            ErrorCode::AuthenticationRequired
        );
        assert_eq!(
            registry
                .call(anonymous, "test/echo", json!({"text": "a"}))
                .await,
            Ok(json!("a"))
        );
        let refused = registry
            .call(user, "users/manage", json!({}))
            .await
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::InsufficientPermissions);
        assert_eq!(
            refused.details,
            "This command requires the users.manage scope"
        );
    }

    #[test]
    fn describes_commands_in_music_assistants_format() {
        let docs = serde_json::to_value(registry().docs()).unwrap();
        assert_eq!(
            docs,
            json!([
                {
                    "command": "test/echo",
                    "category": "test",
                    "summary": "Says it back.",
                    "description": "",
                    "parameters": [
                        {"name": "text", "type": "string", "required": true, "description": "What to say back."},
                        {"name": "times", "type": "integer | null", "required": false, "description": "How many times."},
                    ],
                    "return_type": "string",
                    "authenticated": false,
                    "required_scope": null,
                },
                {
                    "command": "users/manage",
                    "category": "users",
                    "summary": "",
                    "description": "",
                    "parameters": [],
                    "return_type": "null",
                    "authenticated": true,
                    "required_scope": "users.manage",
                },
                {
                    "command": "whoami",
                    "category": "general",
                    "summary": "",
                    "description": "",
                    "parameters": [],
                    "return_type": "string | null",
                    "authenticated": true,
                    "required_scope": null,
                },
            ])
        );
    }
}
